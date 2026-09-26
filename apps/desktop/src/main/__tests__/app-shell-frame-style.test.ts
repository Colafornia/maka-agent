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
import { test } from 'node:test';
import { appShellFrameStyle } from '../../renderer/shell/frame-style.js';

test('every published width stays a <length>, collapsed included', () => {
  const collapsed = appShellFrameStyle({
    sessionListCollapsed: true,
    sessionListWidth: 260,
    workbarRightWidth: 480,
  }) as Record<string, string>;
  const expanded = appShellFrameStyle({
    sessionListCollapsed: false,
    sessionListWidth: 291,
    workbarRightWidth: 480,
  }) as Record<string, string>;

  assert.equal(collapsed['--maka-sidenav-width'], '0px');
  // The drawn sidebar width is the user's width capped by the frame: still a
  // <length> because every branch of the min() is one.
  assert.match(expanded['--maka-sidenav-width']!, /^min\(291px, max\(0px, 100cqi - /);
  // The eased motion target is the uncapped preference: a window resize must
  // never restart the rail's width transition.
  assert.equal(collapsed['--maka-sidenav-user-width'], '0px');
  assert.equal(expanded['--maka-sidenav-user-width'], '291px');
  assert.equal(collapsed['--maka-session-workbar-width'], '480px');
});

test('the caps leave the conversation its minimum and are not the eased width', () => {
  const style = appShellFrameStyle({
    sessionListCollapsed: false,
    sessionListWidth: 260,
    workbarRightWidth: 480,
  }) as Record<string, string>;

  for (const cap of [style['--maka-sidenav-max-width']!, style['--maka-session-workbar-max-width']!]) {
    assert.ok(cap.includes('var(--maka-conversation-min-width)'), cap);
    assert.ok(cap.includes('100cqi'), cap);
  }
  // The Workbar sits beside the sidebar, so its cap subtracts the drawn rail.
  assert.ok(style['--maka-session-workbar-max-width']!.includes('var(--maka-sidenav-width)'));
  // The drawn width the readers see stays capped and instant; only the eased
  // targets carry the uncapped preference, so a stored wide rail cannot make
  // LayoutPanel wider than the narrow frame.
  assert.ok(style['--maka-sidenav-width']!.includes('cqi'));
  assert.ok(!style['--maka-sidenav-user-width']!.includes('cqi'));
  assert.ok(!style['--maka-session-workbar-width']!.includes('cqi'));
});
