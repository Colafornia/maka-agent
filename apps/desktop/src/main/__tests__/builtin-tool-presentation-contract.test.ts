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

import assert from 'node:assert/strict';
import test from 'node:test';
import { createDefaultRuntimePolicy } from '@maka/core/runtime-policy';
import { buildComputerUseTools } from '@maka/runtime/computer-use-tools';
import { createInteractiveRunComposer } from '@maka/runtime-host/test-only/interactive-run-composer';
import { BUILTIN_TOOL_LABELS } from '@maka/ui';
import { buildBrowserTools } from '../browser/browser-tools.js';
import { buildClientSettingsTools } from '../client-settings-tools.js';
import { buildRiveWorkflowTool } from '../rive-workflow-tool.js';

test('every default Runtime Host tool has localized Desktop presentation', () => {
  const composer = createInteractiveRunComposer({
    runtimePolicy: { revision: 0, policy: createDefaultRuntimePolicy() },
    skills: {
      readCanonicalModelInventory: async () => ({ inventory: [] }),
    } as never,
    memory: {} as never,
    sessionTodo: {} as never,
    builtinTools: {},
  });

  assert.deepEqual(
    composer.tools
      .map(({ name }) => name)
      .filter((name) => !Object.hasOwn(BUILTIN_TOOL_LABELS, name)),
    [],
  );
});

test('every Desktop-owned tool has localized presentation', () => {
  const tools = [
    ...buildBrowserTools(),
    ...buildComputerUseTools({ backend: {} as never }),
    ...buildClientSettingsTools({} as never),
    buildRiveWorkflowTool(),
  ];

  assert.deepEqual(
    tools.map(({ name }) => name).filter((name) => !Object.hasOwn(BUILTIN_TOOL_LABELS, name)),
    [],
  );
});
