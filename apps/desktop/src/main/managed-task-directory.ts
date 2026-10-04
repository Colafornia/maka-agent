/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

import { randomUUID } from 'node:crypto';
import { lstat, mkdir, realpath, rmdir, stat } from 'node:fs/promises';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';

/**
 * How a Session cwd relates to the managed task-directory root:
 * - `managed`: inside the root — a directory this authority allocated.
 * - `suspicious`: a binding that cannot plausibly be deliberate — the
 *   filesystem root itself, or inside a reserved location (application state,
 *   install files). Such sessions are offered a one-click correction.
 * - `other`: any other user-owned directory, including explicit picks.
 */
export type ManagedTaskDirectoryClass = 'managed' | 'suspicious' | 'other';

export interface ManagedTaskDirectoryAuthority {
  /**
   * Creates a fresh, empty task directory under the managed root and returns
   * its canonical path. Each call is a distinct directory — separate tasks
   * never share an output root. Throws when the root cannot be provisioned or
   * validated; the caller must not fall back to an implicit directory.
   */
  allocate(): Promise<string>;
  /**
   * Removes an allocation whose binding was never committed (e.g. the Session
   * create or relocate failed). Best-effort and empty-directory only — a
   * directory that somehow gained contents is left alone.
   */
  release(path: string): Promise<void>;
  classify(path: string): Promise<ManagedTaskDirectoryClass>;
}

export interface ManagedTaskDirectoryDeps {
  /** The managed root, e.g. `~/Maka/tasks`. */
  readonly root: string;
  /**
   * Locations the managed root must not resolve into: application state,
   * credentials, and installation files. Evaluated per call so callers can
   * report lazily-initialized paths.
   */
  readonly reservedRoots?: () => readonly string[];
  readonly newId?: () => string;
  /** Owner check; defaults to `process.getuid()` on POSIX. */
  readonly ownerUid?: () => number | undefined;
}

export function createManagedTaskDirectoryAuthority(
  deps: ManagedTaskDirectoryDeps,
): ManagedTaskDirectoryAuthority {
  const newId = deps.newId ?? randomUUID;
  const ownerUid =
    deps.ownerUid ?? (() => (typeof process.getuid === 'function' ? process.getuid() : undefined));

  async function prepareRoot(): Promise<string> {
    const root = resolve(deps.root);
    await mkdir(root, { recursive: true });
    const rootStat = await lstat(root);
    if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) {
      throw unmanagedTaskDirectoryError(`managed task directory root is not a real directory: ${root}`);
    }
    const owner = ownerUid();
    if (owner !== undefined && rootStat.uid !== owner) {
      throw unmanagedTaskDirectoryError(`managed task directory root is owned by another user: ${root}`);
    }
    const canonicalRoot = await realpath(root);
    for (const reserved of deps.reservedRoots?.() ?? []) {
      const canonicalReserved = await realpath(reserved).catch(() => resolve(reserved));
      if (isInsideOrSamePath(canonicalReserved, canonicalRoot)) {
        throw unmanagedTaskDirectoryError(
          `managed task directory root resolves into a protected location: ${canonicalRoot}`,
        );
      }
    }
    return canonicalRoot;
  }

  return {
    async allocate() {
      const canonicalRoot = await prepareRoot();
      for (const suffix of [newId().slice(0, 8), newId()]) {
        const name = `task-${suffix}`;
        const candidate = join(canonicalRoot, name);
        try {
          await mkdir(candidate, { mode: 0o700 });
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code === 'EEXIST') continue;
          throw error;
        }
        const candidateStat = await lstat(candidate);
        if (!candidateStat.isDirectory() || candidateStat.isSymbolicLink()) {
          throw unmanagedTaskDirectoryError(`managed task directory is not a real directory: ${candidate}`);
        }
        const canonical = await realpath(candidate);
        if (!isInsideOrSamePath(canonicalRoot, canonical) || basename(canonical) !== name) {
          throw unmanagedTaskDirectoryError(`managed task directory escaped its root: ${canonical}`);
        }
        return canonical;
      }
      throw unmanagedTaskDirectoryError('managed task directory could not allocate a unique name');
    },

    async release(path) {
      // `rmdir` fails on a non-empty directory by design: this only ever
      // removes the empty directory the authority just created, never task
      // output.
      await rmdir(path).catch(() => undefined);
    },

    async classify(path) {
      const resolved = resolve(path);
      const canonical = await realpath(resolved).catch(() => resolved);
      const canonicalRoot = await realpath(resolve(deps.root)).catch(() => undefined);
      if (canonicalRoot && isInsideOrSamePath(canonicalRoot, canonical)) return 'managed';
      if (dirname(canonical) === canonical) return 'suspicious';
      for (const reserved of deps.reservedRoots?.() ?? []) {
        const canonicalReserved = await realpath(reserved).catch(() => resolve(reserved));
        if (isInsideOrSamePath(canonicalReserved, canonical)) return 'suspicious';
      }
      return 'other';
    },
  };
}

function unmanagedTaskDirectoryError(message: string): Error {
  return new Error(`Refusing to use a managed task directory: ${message}`);
}

function isInsideOrSamePath(root: string, target: string): boolean {
  if (target === root) return true;
  const rel = relative(root, target);
  return (
    rel !== '' &&
    rel !== '..' &&
    !rel.startsWith(`..${sep}`) &&
    !rel.startsWith(sep)
  );
}
