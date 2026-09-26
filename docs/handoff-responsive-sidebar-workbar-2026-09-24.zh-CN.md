<!--
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# Handoff：响应式侧边栏与 Workbar 调整

日期：2026-09-24
仓库：`/Users/yilin.wang/Developer/github/maka-agent`
分支：`fix/narrow-composer-layout`

## 目标

本轮工作的目标是改善窄窗口下的 Desktop shell 响应式行为：

- 窗口变窄时自动隐藏 session sidebar；
- 窗口恢复足够宽度时恢复 sidebar；
- Workbar 在窄屏下能够自动隐藏或恢复；
- 侧边栏和 Workbar 的展开、收起、拖拽宽度行为保持稳定；
- 原生窗口必须保留有效的最小宽度，不能被 renderer 的 CSS 布局下限替代。

## 当前状态

- 当前工作区是正确的 `maka-agent`，不是同级的 `maka` 仓库。
- 当前分支是 `fix/narrow-composer-layout`。
- 备份分支：`bak/0924-narrow-composer-layout`。
- 当前实现保留原有 sidebar / Workbar 的简单状态逻辑；没有继续保留 `activeSurface` takeover 方案。
- 当前未提交的实现改动只有原生窗口最小宽度相关的两处文件：
  - `apps/desktop/src/main/main-window.ts`
  - `apps/desktop/src/main/window-state.ts`

## 已确认的窗口最小宽度问题

之前 renderer 中的 `.maka-shell-astryx { min-width: ... }` 只能限制网页内部布局，不能阻止 Electron 原生窗口被拖得更窄。

当前已恢复：

```ts
export const SAFE_MIN_WIDTH = 560;
```

并在 `BrowserWindow` 创建参数中加入：

```ts
minWidth: SAFE_MIN_WIDTH,
```

这样恢复窗口尺寸的 `sanitizeBounds` 和运行时原生拖拽使用同一个宽度下限。桌面端 TypeScript 检查已通过。

## 尝试过的方案

### 1. 语义类选择器覆盖 sidebar 行内宽度

问题表现：

- sidebar 的 `<nav>` 带有运行时行内 `style="width: ..."`；
- 直接写 `.maka-session-panel` 没有生效，因为 inline style 和 Astryx/StyleX 的规则优先级更高；
- 通过更稳定的语义类和必要的 `!important` 覆盖 nav 宽度，是比依赖大量生成类名更合理的方向。

结论：

- 语义类选择器本身不脆；
- 但不应把生成的 `x...` 类名当作长期 API；
- 最终应让外层稳定 wrapper 成为宽度 authority，nav 在 wrapper 内使用 `width: 100%`。

### 2. `activeSurface` takeover

尝试在 `app-shell.tsx` 中增加：

```ts
activeSurface: 'agent' | 'session-list' | 'workbar'
```

然后让这个状态同时控制：

- main column 是否显示；
- sidebar 是否显示；
- Workbar 是否接管窗口；
- AppShell grid；
- titlebar 与 Workbar 的宽度预留；
- session selection 和 usage/inspector 的打开入口。

观察到的问题：

- Workbar 点击后闪烁；
- `activeSurface` 最终一直回到 `agent`；
- Workbar 的 `getBoundingClientRect()` 宽度为 `0`；
- 典型观测值：

```js
{
  innerWidth: 683,
  sessionListWidth: 0,
  conversationMinWidth: '520px',
  activeSurface: 'agent',
  workbar: { width: 0 }
}
```

- toolbar 可以显示，但实际 Workbar 内容 panel 变成空白或黑屏；
- 点击 Workbar 入口与 persisted controller 状态互相覆盖；
- resize 时多个层级同时做 width/grid/transition 插值，导致拖拽卡顿；
- `WorkbarSurface` 并不是单一 DOM：toolbar/frame 是 `.maka-session-workbar`，实际内容是 `.maka-session-workbar-panel[data-overlay]`，用一个 surface 状态同时接管两者容易产生不同步。

结论：

- 不再继续 `activeSurface` takeover；
- 不要让一个新的响应式状态同时成为 AppShell、sidebar、Workbar 和 controller 的第二个状态源；
- 应优先保留现有 Workbar controller 和现有 DOM 拓扑。

### 3. 窄屏 fixed overlay

随后尝试只在窄屏 CSS 中把 Workbar frame、overlay panel 和 resize handle 改成 `position: fixed`，绕过主 grid 将 Workbar 计算为零宽度。

这个方案后来回退，原因是它破坏了原生窗口和现有 shell 行为：

- 原生窗口最小宽度表现异常；
- sidebar 自动隐藏/恢复行为受到影响；
- 主 grid 与 viewport overlay 的尺寸 authority 不一致；
- resize handle、titlebar reserve、frame、overlay panel 需要分别维护位置；
- 这仍然是在现有 Workbar 结构上叠加第二套布局系统，架构复杂度和回归风险都偏高。

当前代码中已经没有这组 fixed overlay CSS。

## 关键结构与诊断信息

### Workbar DOM 不是单一 surface

现有 Workbar 大致包含两类节点：

- frame / toolbar：`.maka-session-workbar[data-placement="right"]`
- 活跃内容：`.maka-session-workbar-panel[data-overlay][data-placement="right"]`

二者共享 controller 状态，但不是同一个盒子。后续修复应先保证这两个节点在点击、展开、收起和 resize 时由同一套已有 layout state 驱动，而不是增加新的 surface takeover state。

### 现有状态 authority

Workbar controller 已经负责：

- `rightCollapsed`；
- `rightWidth`；
- active tab；
- session 绑定；
- resize reducer；
- localStorage 持久化。

sidebar 也已有自己的 layout store 和 collapse handle。后续响应式逻辑应尽量只调用已有的 collapse/expand command，不复制这些 boolean 或尺寸到 `AppShellContent`。

## 最终方案（2026-09-24 评审确定）

经架构与交互评审，确定采用「响应式自动收起，用户手动打开后挤压 chat 区」的方案，废弃 overlay drawer 方向。

交互规则（对齐 ChatGPT Desktop 的窄屏行为）：

1. 窗口变窄时，响应式策略按段自动收起：先自动收起 Workbar，再自动收起 sidebar；
2. 响应式策略只调用已有的 collapse/expand API（`workbar.commands.setWorkbarCollapsed` / `sessionRailLayoutStore.setCollapsed`），不复制任何 boolean 或尺寸状态到 `AppShellContent`；
3. 策略只在跨入更窄档位时触发一次收起，不能每次 resize 事件都强制设置状态，否则用户在窄屏手动打开后会被立刻收回，点击表现为「不响应」；
4. 窄屏下用户通过 titlebar toggle 可以手动重新打开 sidebar / Workbar；打开后二者继续使用现有 grid 占位列，chat 区收缩；
5. 不做 overlay drawer、不做互斥：用户主动打开就接受 chat 被挤压，允许左右同时打开；
6. 恢复宽屏时自动恢复：只恢复「由响应式策略收起」的面板，不能用无条件 expand 覆盖用户在窄屏期间的主动操作。

实现要点：

- 需要记录两个临时标记（`workbarSuppressedByPolicy`、`sidebarSuppressedByPolicy`），区分「策略收起」和「用户收起」；
- 阈值建议两档：Workbar 收起阈值、sidebar 收起阈值；`SAFE_MIN_WIDTH = 560` 只是原生窗口下限，不能直接当作布局阈值；
- `.maka-detail-with-artifacts` 的 main track 目前是 `minmax(var(--maka-conversation-min-width), 1fr)`；窄屏手动打开两侧面板时必须能被压缩，需要改为 `minmax(0, 1fr)` 或为窄屏档位降级该下限，chat 内容内部自行滚动；
- `.maka-shell-astryx` 的 `min-width: calc(var(--maka-sidenav-width) + var(--maka-conversation-min-width) + gap)` 同样需要随档位放开，否则 sidebar 打开时整个 shell 横向溢出；
- Workbar 的 `rightWidth`（340–600）和 sidebar 宽度不做窄屏特判，继续保持用户持久化的值；
- 不引入 `activeSurface`，不改变 Workbar DOM placement，不使用 `position: fixed/absolute`。

验证路径：

1. 宽 → 窄：Workbar 先自动收起，继续变窄 sidebar 自动收起；
2. 窄屏点击 titlebar 的 Workbar / sidebar toggle 能打开，chat 区被挤压且 composer 不横向溢出；
3. 窄屏手动打开后，窗口在同档内小幅 resize 不会把面板再收回去；
4. 窄屏手动关闭过的面板，恢复宽屏后保持关闭；被策略收起的面板恢复宽屏后自动展开；
5. 增加策略的窄屏状态转换测试（进入窄屏 / 手动打开 / 同档 resize / 恢复宽屏），再做 Electron 手动验证。

## 诊断入口（实现前保留）

1. 先保持当前简单基线，不恢复 `activeSurface`。
2. 用窄窗口复现 Workbar 点击不响应问题，记录点击前后：
   - `workbar.host.rightCollapsed`
   - `workbar.host.rightWidth`
   - `panelsState.right.activeTabId`
   - `.maka-session-workbar[data-placement="right"]` 的 `hidden`、`data-collapsed` 和 bounding rect
   - `.maka-session-workbar-panel[data-overlay][data-placement="right"]` 的 `hidden`、`data-collapsed` 和 bounding rect
3. 优先检查点击是否真的到达 `commands.toggleRight` / `onToggleRightPanel`，再检查 CSS；不要先修改 grid。
4. 如果确实需要窄屏自动隐藏 sidebar/workbar，增加一个单向、可验证的 viewport policy：
   - policy 只调用现有 collapse/expand API；
   - controller 仍是唯一尺寸和可见性 authority；
   - 不改变 Workbar DOM placement；
   - resize 期间暂停 policy，避免拖拽和自动隐藏互相竞争。
5. 为 policy 增加一个窄屏状态转换测试，再做 Electron 手动验证。

## 验证记录

已通过：

```bash
npm --workspace @maka/desktop run typecheck -- --pretty false
git diff --check
```

注意：上方「最终方案」已确定实现方向；Workbar 窄屏“点击后闪烁/不响应”的旧问题将随「响应式自动收起 + 手动打开挤压 chat」方案一并解决，不再单独修复 fixed overlay 路径。

## 当前进展（2026-09-24，stacked Workbar 内容裁切）

已按上述方案完成第一版实现，并新增以下结构：

- `responsive-shell-policy.ts` 负责窄屏自动收起/恢复、stacked 布局判定，以及在窗口不足时计算 sidebar 的有效展示宽度；
- frame 通过 `--maka-sidenav-width` 发布有效 sidebar 宽度，使最小窗口重新打开 sidebar 时仍保留聊天区最小宽度；
- Workbar 增加 `data-layout-placement="stacked"` 语义，stacked 时取消 titlebar clearance、右侧拖拽区域和固定内容宽度；
- stacked Workbar 的实际 `WorkbarPanel` overlay 也会继承 stacked 语义，并设置为可滚动。

已验证：

```bash
npm --workspace @maka/desktop run typecheck -- --pretty false
git diff --check
```

仍存在的问题：窄屏打开 Workbar 后，截图显示 Token usage 等内容只显示到 stacked 行底部，超出部分被裁切。当前判断外层 stacked 行高度（`minmax(180px, min(40dvh, 360px))`）与具体 Workbar 内容的滚动容器仍未形成稳定的高度约束；仅给 overlay 增加 `overflow: auto` 尚未解决实际显示问题。

后续排查应在真实 Electron 窗口中记录以下节点的 computed height、`min-height`、`overflow` 和 bounding rect：

- `.maka-detail-with-artifacts`；
- `.maka-session-workbar[data-layout-placement="stacked"]`；
- `.maka-session-workbar-panel[data-layout-placement="stacked"]`；
- 当前激活的 Token usage 内容根节点。

重点确认到底是 grid 行高度不足、Card/Section 的 `height: 100%` 形成了错误的高度传递，还是 Token usage 内容自身没有把滚动交给 Workbar panel。不要继续叠加 selector override，先确定唯一滚动容器和高度 authority。
