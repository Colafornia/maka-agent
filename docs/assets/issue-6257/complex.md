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
flowchart LR
subgraph b0[Eval batch 1]
direction TB
t0_0[Check out repo]
t0_1[Install deps]
t0_2[Read task]
t0_3[Plan change]
t0_4[Search code]
t0_5[Read file]
t0_6[Edit file]
t0_7[Run tests]
t0_8[Fix failure]
t0_9[Rerun tests]
t0_10[Run lint]
t0_11[Typecheck]
t0_12[Collect diff]
t0_13[Run verifier]
t0_14[Score task]
t0_15[Save record]
t0_0 --> t0_1
t0_0 --> t0_2
t0_1 --> t0_2
t0_1 --> t0_3
t0_2 --> t0_3
t0_2 --> t0_4
t0_3 --> t0_4
t0_3 --> t0_5
t0_4 --> t0_5
t0_4 --> t0_6
t0_5 --> t0_6
t0_5 --> t0_7
t0_6 --> t0_7
t0_6 --> t0_8
t0_7 --> t0_8
t0_7 --> t0_9
t0_8 --> t0_9
t0_8 --> t0_10
t0_9 --> t0_10
t0_9 --> t0_11
t0_10 --> t0_11
t0_10 --> t0_12
t0_11 --> t0_12
t0_11 --> t0_13
t0_12 --> t0_13
t0_12 --> t0_14
t0_13 --> t0_14
t0_13 --> t0_15
t0_14 --> t0_15
end
subgraph b1[Eval batch 2]
direction TB
t1_0[Check out repo]
t1_1[Install deps]
t1_2[Read task]
t1_3[Plan change]
t1_4[Search code]
t1_5[Read file]
t1_6[Edit file]
t1_7[Run tests]
t1_8[Fix failure]
t1_9[Rerun tests]
t1_10[Run lint]
t1_11[Typecheck]
t1_12[Collect diff]
t1_13[Run verifier]
t1_14[Score task]
t1_15[Save record]
t1_0 --> t1_1
t1_0 --> t1_2
t1_1 --> t1_2
t1_1 --> t1_3
t1_2 --> t1_3
t1_2 --> t1_4
t1_3 --> t1_4
t1_3 --> t1_5
t1_4 --> t1_5
t1_4 --> t1_6
t1_5 --> t1_6
t1_5 --> t1_7
t1_6 --> t1_7
t1_6 --> t1_8
t1_7 --> t1_8
t1_7 --> t1_9
t1_8 --> t1_9
t1_8 --> t1_10
t1_9 --> t1_10
t1_9 --> t1_11
t1_10 --> t1_11
t1_10 --> t1_12
t1_11 --> t1_12
t1_11 --> t1_13
t1_12 --> t1_13
t1_12 --> t1_14
t1_13 --> t1_14
t1_13 --> t1_15
t1_14 --> t1_15
end
t0_12 --> t1_0
t0_13 --> t1_1
t0_14 --> t1_2
t0_15 --> t1_3
subgraph b2[Eval batch 3]
direction TB
t2_0[Check out repo]
t2_1[Install deps]
t2_2[Read task]
t2_3[Plan change]
t2_4[Search code]
t2_5[Read file]
t2_6[Edit file]
t2_7[Run tests]
t2_8[Fix failure]
t2_9[Rerun tests]
t2_10[Run lint]
t2_11[Typecheck]
t2_12[Collect diff]
t2_13[Run verifier]
t2_14[Score task]
t2_15[Save record]
t2_0 --> t2_1
t2_0 --> t2_2
t2_1 --> t2_2
t2_1 --> t2_3
t2_2 --> t2_3
t2_2 --> t2_4
t2_3 --> t2_4
t2_3 --> t2_5
t2_4 --> t2_5
t2_4 --> t2_6
t2_5 --> t2_6
t2_5 --> t2_7
t2_6 --> t2_7
t2_6 --> t2_8
t2_7 --> t2_8
t2_7 --> t2_9
t2_8 --> t2_9
t2_8 --> t2_10
t2_9 --> t2_10
t2_9 --> t2_11
t2_10 --> t2_11
t2_10 --> t2_12
t2_11 --> t2_12
t2_11 --> t2_13
t2_12 --> t2_13
t2_12 --> t2_14
t2_13 --> t2_14
t2_13 --> t2_15
t2_14 --> t2_15
end
t1_12 --> t2_0
t1_13 --> t2_1
t1_14 --> t2_2
t1_15 --> t2_3
subgraph b3[Eval batch 4]
direction TB
t3_0[Check out repo]
t3_1[Install deps]
t3_2[Read task]
t3_3[Plan change]
t3_4[Search code]
t3_5[Read file]
t3_6[Edit file]
t3_7[Run tests]
t3_8[Fix failure]
t3_9[Rerun tests]
t3_10[Run lint]
t3_11[Typecheck]
t3_12[Collect diff]
t3_13[Run verifier]
t3_14[Score task]
t3_15[Save record]
t3_0 --> t3_1
t3_0 --> t3_2
t3_1 --> t3_2
t3_1 --> t3_3
t3_2 --> t3_3
t3_2 --> t3_4
t3_3 --> t3_4
t3_3 --> t3_5
t3_4 --> t3_5
t3_4 --> t3_6
t3_5 --> t3_6
t3_5 --> t3_7
t3_6 --> t3_7
t3_6 --> t3_8
t3_7 --> t3_8
t3_7 --> t3_9
t3_8 --> t3_9
t3_8 --> t3_10
t3_9 --> t3_10
t3_9 --> t3_11
t3_10 --> t3_11
t3_10 --> t3_12
t3_11 --> t3_12
t3_11 --> t3_13
t3_12 --> t3_13
t3_12 --> t3_14
t3_13 --> t3_14
t3_13 --> t3_15
t3_14 --> t3_15
end
t2_12 --> t3_0
t2_13 --> t3_1
t2_14 --> t3_2
t2_15 --> t3_3
subgraph b4[Eval batch 5]
direction TB
t4_0[Check out repo]
t4_1[Install deps]
t4_2[Read task]
t4_3[Plan change]
t4_4[Search code]
t4_5[Read file]
t4_6[Edit file]
t4_7[Run tests]
t4_8[Fix failure]
t4_9[Rerun tests]
t4_10[Run lint]
t4_11[Typecheck]
t4_12[Collect diff]
t4_13[Run verifier]
t4_14[Score task]
t4_15[Save record]
t4_0 --> t4_1
t4_0 --> t4_2
t4_1 --> t4_2
t4_1 --> t4_3
t4_2 --> t4_3
t4_2 --> t4_4
t4_3 --> t4_4
t4_3 --> t4_5
t4_4 --> t4_5
t4_4 --> t4_6
t4_5 --> t4_6
t4_5 --> t4_7
t4_6 --> t4_7
t4_6 --> t4_8
t4_7 --> t4_8
t4_7 --> t4_9
t4_8 --> t4_9
t4_8 --> t4_10
t4_9 --> t4_10
t4_9 --> t4_11
t4_10 --> t4_11
t4_10 --> t4_12
t4_11 --> t4_12
t4_11 --> t4_13
t4_12 --> t4_13
t4_12 --> t4_14
t4_13 --> t4_14
t4_13 --> t4_15
t4_14 --> t4_15
end
t3_12 --> t4_0
t3_13 --> t4_1
t3_14 --> t4_2
t3_15 --> t4_3
subgraph b5[Eval batch 6]
direction TB
t5_0[Check out repo]
t5_1[Install deps]
t5_2[Read task]
t5_3[Plan change]
t5_4[Search code]
t5_5[Read file]
t5_6[Edit file]
t5_7[Run tests]
t5_8[Fix failure]
t5_9[Rerun tests]
t5_10[Run lint]
t5_11[Typecheck]
t5_12[Collect diff]
t5_13[Run verifier]
t5_14[Score task]
t5_15[Save record]
t5_0 --> t5_1
t5_0 --> t5_2
t5_1 --> t5_2
t5_1 --> t5_3
t5_2 --> t5_3
t5_2 --> t5_4
t5_3 --> t5_4
t5_3 --> t5_5
t5_4 --> t5_5
t5_4 --> t5_6
t5_5 --> t5_6
t5_5 --> t5_7
t5_6 --> t5_7
t5_6 --> t5_8
t5_7 --> t5_8
t5_7 --> t5_9
t5_8 --> t5_9
t5_8 --> t5_10
t5_9 --> t5_10
t5_9 --> t5_11
t5_10 --> t5_11
t5_10 --> t5_12
t5_11 --> t5_12
t5_11 --> t5_13
t5_12 --> t5_13
t5_12 --> t5_14
t5_13 --> t5_14
t5_13 --> t5_15
t5_14 --> t5_15
end
t4_12 --> t5_0
t4_13 --> t5_1
t4_14 --> t5_2
t4_15 --> t5_3
subgraph b6[Eval batch 7]
direction TB
t6_0[Check out repo]
t6_1[Install deps]
t6_2[Read task]
t6_3[Plan change]
t6_4[Search code]
t6_5[Read file]
t6_6[Edit file]
t6_7[Run tests]
t6_8[Fix failure]
t6_9[Rerun tests]
t6_10[Run lint]
t6_11[Typecheck]
t6_12[Collect diff]
t6_13[Run verifier]
t6_14[Score task]
t6_15[Save record]
t6_0 --> t6_1
t6_0 --> t6_2
t6_1 --> t6_2
t6_1 --> t6_3
t6_2 --> t6_3
t6_2 --> t6_4
t6_3 --> t6_4
t6_3 --> t6_5
t6_4 --> t6_5
t6_4 --> t6_6
t6_5 --> t6_6
t6_5 --> t6_7
t6_6 --> t6_7
t6_6 --> t6_8
t6_7 --> t6_8
t6_7 --> t6_9
t6_8 --> t6_9
t6_8 --> t6_10
t6_9 --> t6_10
t6_9 --> t6_11
t6_10 --> t6_11
t6_10 --> t6_12
t6_11 --> t6_12
t6_11 --> t6_13
t6_12 --> t6_13
t6_12 --> t6_14
t6_13 --> t6_14
t6_13 --> t6_15
t6_14 --> t6_15
end
t5_12 --> t6_0
t5_13 --> t6_1
t5_14 --> t6_2
t5_15 --> t6_3
subgraph b7[Eval batch 8]
direction TB
t7_0[Check out repo]
t7_1[Install deps]
t7_2[Read task]
t7_3[Plan change]
t7_4[Search code]
t7_5[Read file]
t7_6[Edit file]
t7_7[Run tests]
t7_8[Fix failure]
t7_9[Rerun tests]
t7_10[Run lint]
t7_11[Typecheck]
t7_12[Collect diff]
t7_13[Run verifier]
t7_14[Score task]
t7_15[Save record]
t7_0 --> t7_1
t7_0 --> t7_2
t7_1 --> t7_2
t7_1 --> t7_3
t7_2 --> t7_3
t7_2 --> t7_4
t7_3 --> t7_4
t7_3 --> t7_5
t7_4 --> t7_5
t7_4 --> t7_6
t7_5 --> t7_6
t7_5 --> t7_7
t7_6 --> t7_7
t7_6 --> t7_8
t7_7 --> t7_8
t7_7 --> t7_9
t7_8 --> t7_9
t7_8 --> t7_10
t7_9 --> t7_10
t7_9 --> t7_11
t7_10 --> t7_11
t7_10 --> t7_12
t7_11 --> t7_12
t7_11 --> t7_13
t7_12 --> t7_13
t7_12 --> t7_14
t7_13 --> t7_14
t7_13 --> t7_15
t7_14 --> t7_15
end
t6_12 --> t7_0
t6_13 --> t7_1
t6_14 --> t7_2
t6_15 --> t7_3
```
