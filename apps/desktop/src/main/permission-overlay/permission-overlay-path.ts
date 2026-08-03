import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export function resolvePermissionOverlayAssetDir(moduleUrl = import.meta.url): string {
  const start = dirname(fileURLToPath(moduleUrl));
  let dir = start;
  for (let i = 0; i < 6; i++) {
    if (basename(dir) === 'dist') return join(dir, 'overlay');
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return join(start, '..', '..', 'overlay');
}
