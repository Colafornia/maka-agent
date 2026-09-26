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

import type { CSSProperties } from 'react';

/* The appFrame publishes both column widths because the titlebar's first grid
   track is a calc() on --maka-sidenav-width. Every value here must stay a
   <length>: a unitless 0 makes that calc() invalid at computed-value time and
   drops the whole grid-template-columns — the strip falls back to implicit
   columns and the rail parks mid-window instead of beside the traffic lights.
   Shared with the Storybook frame so the story cannot drift from this write.

   Each column gets two numbers. Its width is the user's (0 when collapsed) and
   is what eases open and shut. Its `-max-width` is the room the frame leaves
   it — the frame is an inline-size container (shell-layout.css) — so the
   conversation keeps its minimum first, the sidebar takes what is left and the
   Workbar what the sidebar leaves. The cap is a separate property because it
   must not ease: it follows a window resize, and an eased cap trails the
   window edge by the whole transition.

   The sidebar needs three numbers because the same var cannot be both the
   eased target and the drawn readout. `--maka-sidenav-user-width` is what the
   rail's width transition chases — the uncapped preference, so a window resize
   never restarts the ease. `--maka-sidenav-max-width` clamps the drawn box
   instantly. `--maka-sidenav-width` stays the drawn width, capped, because
   the titlebar, the shell floor and the Workbar's own cap all read it — and
   they must see the settled width, not a target the rail is still easing
   toward. */
const CONVERSATION_FLOOR = 'var(--maka-conversation-min-width)';
const SEAM = 'var(--agents-content-area-gap)';

export function appShellFrameStyle(input: {
  sessionListCollapsed: boolean;
  sessionListWidth: number;
  workbarRightWidth: number;
}): CSSProperties {
  const sidenavMax = `max(0px, 100cqi - ${CONVERSATION_FLOOR} - ${SEAM})`;
  const sidenavUser = input.sessionListCollapsed ? '0px' : `${input.sessionListWidth}px`;
  return {
    '--maka-session-workbar-width': `${input.workbarRightWidth}px`,
    '--maka-session-workbar-max-width':
      `max(0px, 100cqi - var(--maka-sidenav-width) - ${CONVERSATION_FLOOR} - 2 * ${SEAM})`,
    // Keep the motion target inside the current frame. A persisted wide rail
    // otherwise makes AppShell's LayoutPanel wider than the space available
    // on a narrow window before its child can reflow.
    '--maka-sidenav-max-width': sidenavMax,
    '--maka-sidenav-user-width': sidenavUser,
    '--maka-sidenav-width': input.sessionListCollapsed
      ? '0px'
      : `min(${sidenavUser}, ${sidenavMax})`,
  } as CSSProperties;
}
