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

# fix(ui): hide empty tool details and render summary bodies

## Summary

Tool rows previously always received a `resultDetail` element, so calls with no new information displayed a chevron and opened an empty or placeholder panel. Rows now omit `resultDetail` for empty bodies, repeated untitled single-line invocation text, and image/archive placeholders. Summary results display their `summarized` body with the same redaction, localization and line cap as text results. Archived results show a localized status in row stats.

The synchronous slot registry supports exact tool-name matching. `ToolTrow` subscribes once to the keyed `conversation.tool.detail` slot using its numeric version, then checks active keys for each row. Matching plugin contributions keep details expandable; unrelated, disposed and abdicated contributions do not.

For `argsOnly`, the full trimmed body must equal the displayed target to omit details. Multiline or longer arguments remain expandable when the row omits part of their content. Quiet text is compared directly with the displayed target after applying its source's formatter: `formatToolIntent` for intent (240-character cap), or `boundedToolTarget` for invocation (first line, trim and 120-character cap). The target is not truncated a second time. Live output and sandbox/bypass decorations keep details available.

The screenshots render actual Maka/Astryx components with fixed fixtures and the built desktop stylesheet in Chromium. They are component evidence, not screenshots of a live provider session. Before uses the fetched `main` implementation (`5735554b6`); after uses this change. The summary row is expanded in both. The four placeholder/repeated rows lose their chevrons, and the terminal row retains its chevron.

![Before: all six rows have detail chevrons and summary shows a placeholder](before.png)

![After: only summary and terminal have detail chevrons; summary shows its body](after.png)

## Verification

- Relevant rendering and plugin suites: 42 passed, including 9 regression tests after pruning.
- Full UI suite: 698 passed.
- Pruned 9-test regression suite against the original implementation: 7 fail, 2 pass; the same suite passes against both the pre-ablation implementation and the simplified implementation.
- Intent comparison regression: the shared 130-character prefix with different suffixes failed before the review fix and passes after it. Repeated intent text, including whitespace normalization and the 240-character cap, remains non-expandable.
- `npm run lint`, `npm run format:check`, `npm run build`, and `npm run typecheck`: passed.
- `npx knip --workspace apps/desktop --workspace packages/ui`: passed.
- `npm run check:locale-hygiene`: passed. The requested pnpm invocation was rejected by the repository's npm package-manager pin, so the same script was run with npm.
- Chromium fixture capture: 6 expandable rows before, 2 after.
- Full cross-workspace test suite and live application/provider testing were not run.

## Ablation review

The formatter callback parameter was removed: detail comparison now directly selects intent or invocation formatting using a boolean source flag. The existing detail-policy helper keeps decoration precedence and body decisions together; the shared target helper prevents invocation truncation from drifting between row rendering and comparison. The keyed hook and its typed slot-name restriction remain because rows need one subscription with stable numeric snapshots and exact active-key lookups. Key membership is passed to the row builder as a boolean. Archive copy remains a catalog lookup, and summary remains in the existing text branch rather than acquiring a separate preview pipeline.

Temporary compiled-module variants tested whether the remaining behavior could be removed. Each variant below failed its targeted regression; the modules were restored after each experiment.

| Removed or simplified behavior | Regression detected |
| --- | --- |
| Exact tool-name lookup, replaced by slot-wide occupancy | An unrelated plugin made the row expandable |
| Slot subscription | Registering a matching plugin failed to update the row |
| Invocation target cap | A repeated long command became expandable |
| Intent formatting, replaced by a uniform 120-character cap | Distinct suffixes after a shared long prefix became inaccessible |
| Decoration exception | Sandbox/bypass recovery rows became non-expandable |
| Shared text/summary line cap | The summary omitted its hidden-line notice and exposed the final lines |

The test review merged overlapping quiet-text cases, removed duplicate summary/JSON activation and banner-body assertions already exercised elsewhere, replaced the numeric args fixture with an empty object, and shared locale rendering, row assertions, JSON/summary constructors and terminal output fixtures. The summary fixture now exceeds 500 lines and asserts the hidden-line notice; previously its 250 lines never reached the cap. Code text is compared rather than complete generated markup. Plugin mutations use asynchronous `act` to include queued registry notifications. The file is 30 lines shorter, with 9 tests instead of 10.

## AI use

- [ ] No generative tool made a substantive contribution
- [x] Generative tooling made a substantive contribution

Tool(s) and scope: OpenAI Codex implemented the UI changes, regression tests and review evidence. Human review and submission remain with the contributor of record.

## Checklist

- [x] Tests cover the change and fail without it
- [x] Lint, format, typecheck and the affected suites pass locally

Does this PR entail a change in behavior?

- [x] Yes — described under Summary above
- [ ] No
