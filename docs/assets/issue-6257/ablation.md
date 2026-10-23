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

Issue #6257 · 消融实验 · 2026-10-23

结论
删除了 4 项没有显示出必要性的机制，并合并了渲染编排。组件从 567 行减至 525 行，净减少 42 行。没有增加新的生产模块、状态或依赖。
另外 5 项消融产生了预期回归，因此保留。完整结果、逐组日志和可重放变体位于 ablation/。

方法
基线：pre-ablation.patch / pre-ablation.snapshot.json，SHA-256 63019955e3d9d5021c50beeeb8742f516b69d08b18354aafa5d9710e365a0ffe。
每次只移除一项，在 esbuild onLoad 中替换真实组件源码，不改测试预期、不另建模拟渲染器。所有变体运行同一组 2 项真实引擎缓存测试及 3 项 Chromium 测试。
共 10 组主实验：1 组基线、4 组可删除候选、5 组负对照。实验结果只涉及已执行的输入与行为，不构成一般正确性或安全证明。

逐项结果
机制                              缓存测试   浏览器测试   决定与证据
原始基线                           2/2       3/3         对照通过
去掉模块 Promise 缓存              2/2       3/3         删除。直接动态 import，模块加载交给原生模块系统。
去掉 effect 中提前查缓存的路径      2/2       3/3         删除。等待模块/字体后统一查缓存，取消双入口。
去掉独立缓存字符数计数器            2/2       3/3         删除。直接从至多 24 个 Map 条目计算，避免镜像状态。
去掉 32px 宽度分桶                 2/2       3/3         删除。保留 clientWidth 整数宽度及相同值不更新；30 帧无布局振荡。
去掉重挂载初始缓存                 0/2       3/3         保留。重挂载首帧闪回 loading/源码。
去掉实例 SVG 前缀隔离              1/2       3/3         保留。真实 Sankey 重复实例出现重复渐变 ID。
去掉 ResizeObserver                2/2       2/3         保留。窗口变窄后未重排，方向变化等待超时。
去掉 PNG 字体嵌入                  2/2       2/3         保留。原测试发现导出缺少打包字体；另加真实像素负对照确认。
去掉 PNG computed styles           2/2       2/3         保留。导出文字 paint 未解析，无法匹配实际主题颜色。

字体验证补强
原输入主要为中文，字体规则存在性不足以证明拉丁字形按产品字体导出。
在现有浏览器用例中加入 Start123 标签，并比较相同尺寸、背景、paint 的 PNG：正常导出与仅移除 @font-face 的控制图，其 RGBA SHA-256 必须不同。
正常实现 3/3 通过。去掉字体嵌入且删除“字体规则存在”断言后，仍在实际字形像素断言处失败，详见 ablation/font-check/。这两组补充实验未替换主实验记录。

最终实现
只保留 Map 作为缓存内容的权威，淘汰时派生总字符数。缓存仍同时受 24 条目 / 4 MiB 字符预算约束。
将加载、取消、命中、同步 Pele 渲染、写缓存与错误处理集中到一个 effect；删除 renderMermaid 的回调/null 协议、loadPele 包装、trim 缓存单次调用包装，以及一次性 options 常量。
保留原有安全过滤、来源/边数/输出限制及自动渲染预算：这些属于原任务要求，不将有限恶意输入测试的通过解释为可以撤去安全边界。

合并验证
UI build + 全部 783 项通过；3 项 Chromium 测试通过（包括新的真实字体像素对照）；缓存/PNG/导出尺寸定向 14 项通过。
桌面类型检查、实际 Vite 构建、renderer entry/许可证产物一致性检查、涉及文件 lint、git diff --check 通过。
五轮性能基准和同屏截图/GIF已按最终实现重新采集，原始数据见 benchmark.json、visual-timings.json。这里不主张结构简化本身带来可测速度提升。
对最终补丁做了本地自审：没有剩余阻塞项；无远端 CI 或独立评审结果。最终固定补丁及文件摘要见 review.patch / snapshot.json。

重放主实验（仓库根目录）
MAKA_ABLATION_SOURCE=docs/screenshots/issue-6257/ablation/baseline.tsx \
MAKA_ABLATION_OUTPUT=docs/screenshots/issue-6257/ablation/replay \
node docs/screenshots/issue-6257/ablation/run.mjs

该命令使用保存的消融前生产组件和当前强化后的浏览器测试，生成本地测试变体与日志；不改生产源码。需要已安装 Playwright Chromium 和本地 HTTP 监听权限。

状态
分支 feat/6257-pele，已提交 ae3240952a6f7c2d37df6e052c0466c224c731b1，包含 Generated-by: Codex；已推送至用户确认的 origin（https://github.com/Colafornia/maka-agent.git），并设置上游跟踪。贡献指南要求的六项检查与提交钩子全部通过。用户原有改动保持不动。审阅素材位于 Git 忽略目录，未增加版本化的实验框架。没有保留运行中的浏览器或 HTTP 服务。
