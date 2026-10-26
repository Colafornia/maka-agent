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
  actor User
  participant App as Desktop app
  participant Host as Runtime Host
  participant Bash as Bash tool
  User->>App: Run the tests
  App->>Host: Start turn
  activate Host
  Host-->>App: Allow npm test?
  User->>App: Allow once
  App->>Host: Permission granted
  Host->>Bash: npm test
  Bash-->>Host: 812 passed
  Host-->>App: Turn ended
  deactivate Host
  Note over User,Bash: Each step is a RuntimeEvent in the log
```
