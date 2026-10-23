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
  input([任务输入])
  output([成果输出])
b1_0[规划 1]
input --> b1_0
b1_1[规划 2]
input --> b1_1
b2_0[拆解 1]
b1_0 --> b2_0
b2_1[拆解 2]
b1_0 --> b2_1
b2_2[拆解 3]
b1_1 --> b2_2
b2_3[拆解 4]
b1_1 --> b2_3
b3_0[执行 1]
b2_0 --> b3_0
b3_1[执行 2]
b2_0 --> b3_1
b3_2[执行 3]
b2_1 --> b3_2
b3_3[执行 4]
b2_1 --> b3_3
b3_4[执行 5]
b2_2 --> b3_4
b3_5[执行 6]
b2_2 --> b3_5
b3_6[执行 7]
b2_3 --> b3_6
b3_7[执行 8]
b2_3 --> b3_7
b4_0[01]
b3_0 --> b4_0
b4_1[02]
b3_0 --> b4_1
b4_2[03]
b3_1 --> b4_2
b4_3[04]
b3_1 --> b4_3
b4_4[05]
b3_2 --> b4_4
b4_5[06]
b3_2 --> b4_5
b4_6[07]
b3_3 --> b4_6
b4_7[08]
b3_3 --> b4_7
b4_8[09]
b3_4 --> b4_8
b4_9[10]
b3_4 --> b4_9
b4_10[11]
b3_5 --> b4_10
b4_11[12]
b3_5 --> b4_11
b4_12[13]
b3_6 --> b4_12
b4_13[14]
b3_6 --> b4_13
b4_14[15]
b3_7 --> b4_14
b4_15[16]
b3_7 --> b4_15
m3_0[汇总 1]
b4_0 --> m3_0
b4_1 --> m3_0
m3_1[汇总 2]
b4_2 --> m3_1
b4_3 --> m3_1
m3_2[汇总 3]
b4_4 --> m3_2
b4_5 --> m3_2
m3_3[汇总 4]
b4_6 --> m3_3
b4_7 --> m3_3
m3_4[汇总 5]
b4_8 --> m3_4
b4_9 --> m3_4
m3_5[汇总 6]
b4_10 --> m3_5
b4_11 --> m3_5
m3_6[汇总 7]
b4_12 --> m3_6
b4_13 --> m3_6
m3_7[汇总 8]
b4_14 --> m3_7
b4_15 --> m3_7
m2_0[复核 1]
m3_0 --> m2_0
m3_1 --> m2_0
m2_1[复核 2]
m3_2 --> m2_1
m3_3 --> m2_1
m2_2[复核 3]
m3_4 --> m2_2
m3_5 --> m2_2
m2_3[复核 4]
m3_6 --> m2_3
m3_7 --> m2_3
m1_0[交付 1]
m2_0 --> m1_0
m2_1 --> m1_0
m1_1[交付 2]
m2_2 --> m1_1
m2_3 --> m1_1
m1_0 --> output
m1_1 --> output
```
