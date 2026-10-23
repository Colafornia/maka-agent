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

同屏素材选型与取景 · 2026-10-23

本次只替换审阅素材，生产代码和自动化测试未改动；snapshot.json 所记录的代码摘要仍有效。

1. 均衡流程图（flow.mmd，8 节点 / 10 连线）
   参考 https://pele.run/examples/flowchart 的 Tree 示例。
   保留“分支—汇合—再分支”的层次，补充末端汇合，改为等长中文任务标签。
   左右两路和下层三路长度一致，避免跨子图的回边将图形拉长。

2. 订单时序图（sequence.mmd，4 个参与者 / 6 条消息）
   参考 https://pele.run/examples/sequence 的 Four participants 示例。
   沿用订单发起、授权与确认的交换顺序，改为中文名称；保留编号、激活及跨列注释。

3. 思维导图（mindmap.mmd，13 节点 / 12 分支）
   参考 https://pele.run/examples/mindmap 的中心主题展开形式。
   内容改为 Maka 工作流程，四组各两个子主题，分支数量与文本长度均衡。

4. GIF 对称分支图（complex.mmd，46 节点 / 60 连线）
   参考官方 Tree、Fan out 和 Fan in 的结构，自行扩展为四层二叉展开、三层镜像合并。
   中间 16 个叶子使用短编号，降低标签拥挤；没有交叉跳层边或回边。
   该图用于易读地展示同步渲染过程；正式五轮性能基准仍使用原来的 101 节点 / 100 连线输入，两个数据集不混用。

比较与取景
- 两侧各使用相同源码、相同 iframe 尺寸及真实 Maka 共享组件。
- iframe 高度均为 1000px；保留产品原生的 viewport 高度规则。PNG 只收紧外层面板的空白，图形未后期放大或重新布局。
- 显示实际引擎生成的图形、字号、配色和工具栏，包含两侧排版差异。
- GIF 独立站点 iframe 同步开始，模块已载入、图表缓存为空；1× 原速，无 CPU 降速；各帧按采集时间戳编码。
- 单次可视化数字仅说明此次录制。原始计时在 visual-timings.json，逐帧时间在 frames.json。
- 六张 PNG、静态分支图和 GIF 均已人工查看；浏览器和本地 HTTP 服务已随脚本退出关闭。

capture.mjs / encode-gif.py 保存本次采集脚本，依赖本机已保存的两版生产组件 bundle 与临时 Pillow 环境。它们是本地素材工具，不是版本化的产品代码或新增依赖。
