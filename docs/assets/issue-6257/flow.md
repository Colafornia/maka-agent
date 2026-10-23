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
flowchart TD
  A([接收任务]) --> B[解析需求]
  A --> C[检索资料]
  B --> D{汇总计划}
  C --> D
  D --> E[方案设计]
  D --> F[功能实现]
  D --> G[结果验证]
  E --> H([完成交付])
  F --> H
  G --> H
```
