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

<details open>
<summary>English</summary>

## Issue #6257 evidence

Captured on 2026-10-23 using baseline `a509747d6` (Mermaid.js 11.17.2) and implementation `ae3240952` (Pele 1.9.0). This branch hosts review assets separately from the code branch `feat/6257-pele`.

- Screenshots: `flow-{light,dark}.png`, `sequence-{light,dark}.png`, `mindmap-{light,dark}.png`; Mermaid on the left, Pele on the right. Both sides use the same source, container width, real Maka shared component, and theme. Native layout and color differences are retained. The outer screenshot crop removes empty space without resizing or editing the generated diagrams.
- Recording: `complex-rendering.gif` and `complex-final.png`, using `complex.md` (128 nodes, 260 edges). Independent site-isolated iframes start together with modules loaded and diagram caches empty. Playback is 1× with timestamp-based frame intervals and no CPU throttling. The final frame holds for 1.2 seconds. Single-run component-to-next-paint times: Mermaid 614.1 ms, Pele 120.7 ms. These are illustrative, separate from the formal benchmark. The previous eight-stage example was re-recorded from its recovered capture script because the original GIF had been overwritten; the development app was running during this capture.
- Benchmark: `benchmark.json`, five alternating A/B runs per renderer in fresh pages, production esbuild bundles, 640 px container, 101 nodes and 100 edges with Chinese labels. The engine path includes script execution, initialization, and SVG rendering. The component path separately includes script execution, real Theme + MermaidDiagram mounting, SVG completion, and the next animation frame. Neither includes network transfer or full Electron startup. Every engine result must contain all 101 nodes and the final Chinese label; errors and timeouts fail the script.
- Environment: macOS 26.3.1 arm64, 12 logical CPUs, Node 24.22.1, Playwright Chromium, no CPU throttling. Pre-run load averages: 4.18 / 6.29 / 6.13; other user applications were running. Results are local observations rather than universal speed guarantees.
- Production output: `bundles.json` sums individually gzipped JS files from actual desktop Vite builds: 2,435,078 → 1,732,078 bytes, 143 → 48 chunks. This is total emitted JS, not initial download size. Temporary directory names identify the saved baseline build.
- Independent engine profile: `profile.json` records 306 browser layout events for Mermaid and zero for Pele, using the same unchanged engine versions. This separate profiling run is excluded from the five-run timing results. Function sample counts are included; bundled third-party source excerpts are omitted.
- Ablation: `ablation.md` records ten isolated variants, four removed mechanisms, five retained mechanisms with observed regressions, and the additional PNG font-pixel control. Full replay logs remain local; no performance gain is attributed to ablation itself.
- Sources: the four `.md` files and `example-sources.md`. Visual examples adapt [Pele flowcharts](https://pele.run/examples/flowchart), [sequence diagrams](https://pele.run/examples/sequence), and [mindmaps](https://pele.run/examples/mindmap) with balanced Chinese labels. `visual-timings.json` and `frames.json` preserve the recording measurements.

Run `node --test packages/ui/scripts/mermaid.browser.test.mjs` for browser verification. `packages/ui/scripts/benchmark-diagrams.mjs` documents baseline capture and five-run comparison. Full installer verification, OS-native clipboard end-to-end testing, and exhaustive Mermaid extension compatibility were not run.

</details>

<details>
<summary>中文</summary>

## Issue #6257 审阅素材

2026-10-23 采集，对比基线 `a509747d6`（Mermaid.js 11.17.2）与实现 `ae3240952`（Pele 1.9.0）。本分支单独保存审阅素材，代码位于 `feat/6257-pele`。

- 截图：`flow-{light,dark}.png`、`sequence-{light,dark}.png`、`mindmap-{light,dark}.png`；左侧为 Mermaid，右侧为 Pele。两侧使用相同源码、容器宽度、真实 Maka 共享组件和主题，保留引擎原生布局与配色差异。仅裁去外层空白，没有缩放或修改生成的图表。
- 录屏：`complex-rendering.gif` 与 `complex-final.png`，使用 `complex.md`（128 节点、260 连线）。独立站点隔离的 iframe 同步开始，模块已载入、图表缓存为空。按采集时间戳以 1× 原速播放，无 CPU 降速，末帧停留 1.2 秒。此次单次组件调用至下一绘制帧耗时：Mermaid 614.1 ms，Pele 120.7 ms。这是过程演示，与正式基准分开统计。原 GIF 已被覆盖，因此从找回的原脚本恢复此前八阶段示例并重新录制；采集时开发应用正在运行。
- 基准：`benchmark.json`，每个引擎五次，新页面交替 A/B，使用生产模式 esbuild 包、640 px 容器、101 节点及 100 连线中文输入。引擎路径包含脚本执行、初始化与 SVG 渲染；组件路径独立测量，包含脚本执行、真实 Theme + MermaidDiagram 挂载、SVG 完成与下一动画帧。均不包含网络传输或完整 Electron 启动。每次引擎输出必须包含全部 101 节点及最后的中文标签；错误或超时使脚本失败。
- 环境：macOS 26.3.1 arm64、12 个逻辑核心、Node 24.22.1、Playwright Chromium，无 CPU 降速。测量前负载为 4.18 / 6.29 / 6.13，其他用户应用仍在运行。数据是本机观察结果，不代表所有环境下的性能保证。
- 生产产物：`bundles.json` 对实际桌面 Vite 构建逐个压缩 JS 后求和：2,435,078 → 1,732,078 字节，143 → 48 个文件。这是全部生成的 JS，不是首屏下载量。临时目录名用于标识已保存的基线构建。
- 独立引擎采样：`profile.json` 记录 Mermaid 触发 306 次浏览器布局、Pele 为零次，使用相同且未变更的引擎版本。该独立采样不计入五轮耗时结果。保留函数采样次数，省略打包后的第三方源码片段。
- 消融：`ablation.md` 记录十组独立变体、四项已删除机制、五项因回归而保留的机制，以及额外的 PNG 字体像素对照。完整重放日志仍保留在本地；没有将任何性能提升归因于消融本身。
- 来源：四个 `.md` 文件与 `example-sources.md`。视觉样例参考 [Pele 流程图](https://pele.run/examples/flowchart)、[时序图](https://pele.run/examples/sequence)及[思维导图](https://pele.run/examples/mindmap)，改为均衡的中文标签。`visual-timings.json` 与 `frames.json` 保留录制测量记录。

运行 `node --test packages/ui/scripts/mermaid.browser.test.mjs` 可执行浏览器验证。`packages/ui/scripts/benchmark-diagrams.mjs` 注明基线采集与五轮对比方法。未执行完整安装包验证、操作系统原生剪贴板端到端测试及全部 Mermaid 扩展兼容性验证。

</details>
