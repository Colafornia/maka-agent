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
sequenceDiagram
  autonumber
  actor C as 客户
  participant W as 应用
  participant A as 订单服务
  participant B as 支付服务
  C->>W: 提交订单
  W->>A: 创建订单
  activate A
  A->>B: 支付授权
  B-->>A: 授权成功
  A-->>W: 订单确认
  deactivate A
  W-->>C: 完成下单
  Note over C,B: 一个订单 · 一次交易
```
