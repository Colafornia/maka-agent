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
```mermaid
<details open>
<summary>English</summary>

## Example sources and framing

Re-captured on 2026-10-26. Only review assets changed; production code and tests did not. Each example adapts a Pele example to a Maka scenario and uses English labels.

1. Agent turn flowchart (`flow.md`, 6 nodes, 8 edges). Adapts the "Decision with loops" example in [Pele flowcharts](https://pele.run/examples/flowchart). The model output either ends the turn, runs a read-only tool, or asks the user before an edit or shell command. Tool results and denials return to the model.
2. Runtime Host architecture (`architecture.md`, 9 nodes, 8 edges, 2 groups). Adapts the "Two-way between groups" example in [Pele flowcharts](https://pele.run/examples/flowchart) and the system map in `ARCHITECTURE.md`. Desktop, TUI and CLI, and Eval call SessionManager. AgentRun calls the model, runs tools, and appends to the RuntimeEvent log.
3. Tool approval sequence (`sequence.md`, 4 participants, 8 messages). Adapts the "Four participants" example in [Pele sequence diagrams](https://pele.run/examples/sequence). It keeps numbering, activation, and a note across all lifelines.
4. GIF stress diagram (`complex.md`, 128 nodes, 260 edges). Keeps the previous topology: eight groups of 16 nodes, 29 edges inside each group, and 28 edges between adjacent groups. Labels now describe eight eval batches. The five-run benchmark still uses its separate 101-node, 100-edge input.

Comparison and framing:

- Both sides use the same source, iframe size, real Maka shared component, and theme.
- The screenshot iframe is 1000 px tall and the GIF iframe is 590 px tall. The product's viewport height rule still applies. The PNG crop only tightens the outer panels; diagrams are not scaled or edited.
- The GIF iframes are site-isolated and start together with modules loaded and diagram caches empty. Playback is 1×, without CPU throttling, and frames use their capture timestamps.
- Single-run numbers describe this recording only. `visual-timings.json` holds the timings and `frames.json` holds the frame times. The development app was running during this capture.
- All six PNGs, the final frame, and the GIF were inspected manually.

The capture scripts are local review tools that reuse the saved production component bundles from 2026-10-23. They are not product code or new dependencies.

</details>

<details>
<summary>中文</summary>

## 示例来源与取景

2026-10-26 重新采集。仅更新审阅素材，生产代码和测试未改动。每个示例都从 Pele 官方示例改编为 Maka 场景，并使用英文标签。

1. 智能体单轮流程图（`flow.md`，6 个节点、8 条连线）。改编自 [Pele 流程图](https://pele.run/examples/flowchart) 中的 "Decision with loops" 示例。模型输出可以结束本轮、运行只读工具，或在编辑文件和执行 Shell 命令前询问用户。工具结果和拒绝都会返回模型。
2. Runtime Host 架构图（`architecture.md`，9 个节点、8 条连线、2 个分组）。改编自 [Pele 流程图](https://pele.run/examples/flowchart) 中的 "Two-way between groups" 示例及 `ARCHITECTURE.md` 的系统图。桌面端、TUI 与 CLI、Eval 调用 SessionManager；AgentRun 调用模型、运行工具，并写入 RuntimeEvent 日志。
3. 工具审批时序图（`sequence.md`，4 个参与者、8 条消息）。改编自 [Pele 时序图](https://pele.run/examples/sequence) 中的 "Four participants" 示例，保留编号、激活和横跨全部生命线的注释。
4. GIF 压力测试图（`complex.md`，128 个节点、260 条连线）。沿用此前的拓扑：8 组、每组 16 个节点，组内各 29 条连线，相邻组之间共 28 条连线。标签改为 8 个评测批次。五轮性能基准仍使用另一份 101 个节点、100 条连线的输入。

对比与取景：

- 两侧使用相同的源码、iframe 尺寸、真实 Maka 共享组件和主题。
- 截图 iframe 高 1000 px，GIF iframe 高 590 px，仍受产品原有的视口高度规则约束。PNG 只收紧外层面板，未缩放或修改图表。
- GIF 的两个 iframe 站点隔离，模块已载入、图表缓存为空时同步开始。按 1× 原速播放，无 CPU 降速，各帧按采集时间戳编码。
- 单次运行数字只描述此次录制。`visual-timings.json` 保存耗时，`frames.json` 保存逐帧时间。采集时开发版应用正在运行。
- 六张 PNG、末帧静态图和 GIF 均已人工检查。

采集脚本是本地审阅工具，复用 2026-10-23 保存的两版生产组件构建包，不属于产品代码，也不是新增依赖。

</details>
