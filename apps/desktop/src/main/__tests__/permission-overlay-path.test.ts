import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pathToFileURL } from 'node:url';

import { resolvePermissionOverlayAssetDir } from '../permission-overlay/permission-overlay-path.js';

test('permission overlay assets resolve from bundled and tsc layouts', () => {
  const dist = '/workspace/apps/desktop/dist';
  for (const modulePath of [
    `${dist}/main/main.js`,
    `${dist}/main/permission-overlay/permission-overlay-main.js`,
  ]) {
    assert.equal(resolvePermissionOverlayAssetDir(pathToFileURL(modulePath).href), `${dist}/overlay`);
  }
});
