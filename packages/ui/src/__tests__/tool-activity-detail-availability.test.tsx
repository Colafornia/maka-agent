/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { act, createElement } from 'react';
import { createRoot } from 'react-dom/client';
import { renderToStaticMarkup } from 'react-dom/server';
import { parseHTML } from 'linkedom';
import { UI_LOCALES, type UiLocale } from '@maka/core/ui-locale';
import type { ToolResultContent } from '@maka/core/events';
import { ToolCallDetail, ToolTrow } from '../tool-activity.js';
import type { ToolActivityItem } from '../materialize.js';
import { LocaleProvider } from '../locale-context.js';
import {
  MakaClientSessionScope,
  MakaClientSlotCore,
  MakaClientSlotProvider,
} from '../client-plugin-slots.js';
import { getToolActivityCopy } from '../tool-activity/copy.js';
import { ToolResultPreview } from '../tool-activity/tool-result-preview.js';

const baseItem: ToolActivityItem = {
  toolUseId: 'detail-test',
  toolName: 'CustomTool',
  status: 'completed',
  args: undefined,
};

function renderRow(changes: Partial<ToolActivityItem>, locale: UiLocale = 'en'): string {
  return renderToStaticMarkup(createElement(LocaleProvider, {
    locale,
    children: createElement(ToolTrow, { items: [{ ...baseItem, ...changes }] }),
  }));
}

// Astryx renders this path only for the row's detail chevron.
const CHEVRON = 'path[d="M6 9l6 6 6-6"]';
function assertExpandable(markup: string, expected: boolean): void {
  const { document } = parseHTML(markup);
  const row = document.querySelector('[data-slot="chat-tool-call-row"]');
  assert.ok(row, 'the actual Astryx call row is rendered');
  assert.equal(row.getAttribute('role'), expected ? 'button' : null);
  assert.equal(row.getAttribute('tabindex'), expected ? '0' : null);
  assert.equal(row.getAttribute('aria-expanded'), expected ? 'false' : null);
  assert.equal(row.querySelector(CHEVRON) !== null, expected, 'row detail chevron');
}

const archivedResult: Extract<ToolResultContent, { kind: 'archived_tool_result' }> = {
  kind: 'archived_tool_result',
  status: 'not_loaded',
  runtimeEventId: 'event-1',
  toolCallId: 'detail-test',
  toolName: 'CustomTool',
  originalEstimatedTokens: 800,
  originalBytes: 3200,
  rewriteVersion: 1,
  reason: 'tool_result_pruned',
};

describe('tool row detail availability', () => {
  it('omits activation and chevrons for empty bodies and permission-denied output', () => {
    for (const status of ['completed', 'running', 'interrupted'] as const) {
      assertExpandable(renderRow({ status }), false);
    }
    assertExpandable(renderRow({
      status: 'errored',
      args: { path: '/private/data' },
      result: { kind: 'text', text: 'User denied permission request' },
    }), false);
  });

  it('omits repeated quiet single lines using the target trim, redaction and cap', () => {
    for (const command of [
      'git status',
      '  git status  \n',
      `echo ${'x'.repeat(300)}`,
      'curl -H "Authorization: Bearer secret-token-value" https://example.com',
    ]) {
      const markup = renderRow({ toolName: 'Bash', args: { command } });
      assertExpandable(markup, false);
      assert.doesNotMatch(markup, /secret-token-value/);
    }
    assertExpandable(renderRow({
      intent: 'git status', args: { command: 'git status' },
    }), false);
    assertExpandable(renderRow({
      args: { command: 'git status' },
      intent: 'Inspect working tree',
    }), true);
    assertExpandable(renderRow({ args: { command: 'echo first\necho second' } }), true);
  });

  it('keeps quiet JSON titles and non-repeated content expandable', () => {
    assertExpandable(renderRow({
      args: { command: 'git status' },
      result: { kind: 'json', value: { content: 'git status' } },
    }), true);
    assertExpandable(renderRow({
      result: { kind: 'json', value: { content: 'first\nsecond' } },
    }), true);
    assertExpandable(renderRow({
      intent: 'same', result: { kind: 'json', value: { content: '  same  ' } },
    }), false);
    assertExpandable(renderRow({
      intent: 'same', result: { kind: 'json', value: { content: 'other' } },
    }), true);
  });

  it('keeps full args when they reveal information beyond the target', () => {
    assertExpandable(renderRow({ args: 42, intent: '42' }), false);
    assertExpandable(renderRow({ args: ['first', 'second'], intent: 'first' }), true);
    assertExpandable(renderRow({ args: ['x'.repeat(300)], intent: 'x'.repeat(300) }), true);
    assertExpandable(renderRow({ args: {} }), true);
  });

  it('omits image and archived placeholder details and localizes archive stats', () => {
    assertExpandable(renderRow({ result: {
      kind: 'image', mimeType: 'image/png', ref: { kind: 'workspace_file', relativePath: 'image.png' },
    } }), false);
    for (const locale of UI_LOCALES) {
      for (const status of ['not_loaded', 'missing', 'corrupt'] as const) {
        const markup = renderRow({ result: { ...archivedResult, status } }, locale);
        assertExpandable(markup, false);
        assert.ok(markup.includes(getToolActivityCopy(locale).result.archivedStatus[status]));
        assert.doesNotMatch(markup, /\[archived_tool_result\]/);
      }
    }
  });

  it('preserves rich output families and summary activation', () => {
    const results: ToolResultContent[] = [
      { kind: 'text', text: 'actual output' },
      { kind: 'summary', original: 'original', summarized: 'summary body', reason: 'too_large' },
      { kind: 'json', value: { ok: true, content: 'actual output' } },
      { kind: 'file_diff', paths: ['a.ts'], diff: '@@ -1 +1 @@\n-old\n+new' },
      {
        kind: 'terminal', cmd: 'npm test', cwd: '/repo', status: 'completed', exitCode: 0,
        output: { mode: 'pipes', stdout: 'output', stderr: '', stdoutTruncated: false, stderrTruncated: false, redacted: false },
      },
      {
        kind: 'shell_run', cmd: 'npm test', cwd: '/repo', status: 'completed', mode: 'pipes',
        ref: 'maka://runtime/background-tasks/test', startedAt: 1, updatedAt: 2, revision: 1,
      },
      { kind: 'web_search', provider: 'tavily', query: 'Maka', rows: [] },
    ];
    for (const result of results) assertExpandable(renderRow({ result }), true);
  });

  it('keeps sandbox and bypass decorations expandable even without a body', () => {
    const results: ToolResultContent[] = [
      { kind: 'text', text: 'User denied permission request', sandboxDenial: { likely: true, backend: 'macos-seatbelt' } },
      { kind: 'text', text: 'requires bypass', sandboxFailure: { reason: 'requires_bypass', source: 'client_capability' } },
    ];
    for (const result of results) {
      assertExpandable(renderRow({ result, status: 'errored' }), true);
      const detail = renderToStaticMarkup(createElement(LocaleProvider, {
        locale: 'en',
        children: createElement(ToolCallDetail, { item: { ...baseItem, result, status: 'errored' } }),
      }));
      assert.match(detail, /maka-(sandbox-blocked|requires-bypass)-banner/);
    }
  });

  it('renders summary text through the same redaction, localization and line cap as text', () => {
    for (const locale of UI_LOCALES) {
      const text = `summary body\nAuthorization: Bearer secret-token-value\nUser denied permission request\n${'line\n'.repeat(250)}`;
      const render = (content: ToolResultContent) => renderToStaticMarkup(
        createElement(LocaleProvider, { locale, children: createElement(ToolResultPreview, { content }) }),
      );
      const summary = render({ kind: 'summary', summarized: text, original: 'original-hidden', reason: 'too_large' });
      assert.equal(summary.replace('data-kind="summary"', 'data-kind="text"'), render({ kind: 'text', text }));
      assert.match(summary, /summary body/);
      assert.ok(summary.includes(getToolActivityCopy(locale).permissionDenied));
      assert.doesNotMatch(summary, /secret-token-value|original-hidden|\[summary\]/);
    }
  });
});

it('updates the same row for live output and keyed plugin registration, disposal and abdication', async () => {
  const original = {
    document: globalThis.document, window: globalThis.window,
    Element: globalThis.Element, HTMLElement: globalThis.HTMLElement, Node: globalThis.Node,
    matchMedia: globalThis.matchMedia,
    requestAnimationFrame: globalThis.requestAnimationFrame,
    cancelAnimationFrame: globalThis.cancelAnimationFrame,
    IS_REACT_ACT_ENVIRONMENT: (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT,
  };
  const { document, window } = parseHTML('<div id="root"></div>');
  window.getComputedStyle = () => ({ getPropertyValue: () => '' }) as unknown as CSSStyleDeclaration;
  window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList;
  Object.assign(globalThis, {
    document, window, Element: window.Element, HTMLElement: window.HTMLElement, Node: window.Node,
    matchMedia: window.matchMedia, requestAnimationFrame: () => 1, cancelAnimationFrame() {},
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const container = document.querySelector('#root');
  assert.ok(container);
  const root = createRoot(container);
  const core = new MakaClientSlotCore();
  const render = (changes: Partial<ToolActivityItem>, activityObserved = true) => act(() => root.render(
    <LocaleProvider locale="en">
      <MakaClientSlotProvider core={core}>
        <MakaClientSessionScope sessionId="session-1">
          <ToolTrow items={[{ ...baseItem, ...changes }]} activityObserved={activityObserved} />
        </MakaClientSessionScope>
      </MakaClientSlotProvider>
    </LocaleProvider>,
  ));
  const row = () => {
    const node = container.querySelector('[data-slot="chat-tool-call-row"]');
    assert.ok(node);
    return node;
  };
  const expandable = (expected: boolean) => {
    assert.equal(row().getAttribute('role'), expected ? 'button' : null);
    assert.equal(row().querySelector(CHEVRON) !== null, expected);
  };
  try {
    await render({ status: 'running' });
    expandable(false);
    await render({ status: 'running', outputChunks: [
      { seq: 1, stream: 'stdout', text: 'live output', redacted: false, createdAt: 1 },
    ] });
    expandable(true);
    await act(() => row().dispatchEvent(new window.Event('click', { bubbles: true })));
    assert.match(container.textContent, /live output/);
    await render({ status: 'completed', result: {
      kind: 'summary', summarized: 'visible summary', original: 'hidden original', reason: 'too_large',
    } });
    assert.match(container.textContent, /visible summary/);
    assert.doesNotMatch(container.textContent, /hidden original|\[summary\]/);
    await render({});
    expandable(false);
    await act(() => { core.register({ name: 'conversation.tool.detail', key: 'OtherTool' }, () => <p>other plugin</p>); });
    expandable(false);
    let dispose: () => void = () => {};
    await act(() => { dispose = core.register({ name: 'conversation.tool.detail', key: 'CustomTool' }, () => <p>matching plugin</p>); });
    expandable(true);
    // Detail state is retained by Astryx; matching output appears as soon as
    // this already-open row gets a contribution.
    assert.match(container.textContent, /matching plugin/);
    assert.doesNotMatch(container.textContent, /other plugin/);
    await act(() => dispose());
    expandable(false);
    await act(() => { core.register({ name: 'conversation.tool.detail', key: 'CustomTool' }, () => <p>active plugin</p>); });
    expandable(true);
    const entry = core.activeEntries('conversation.tool.detail').find((candidate) => candidate.options.key === 'CustomTool');
    assert.ok(entry);
    await act(() => core.abdicate('conversation.tool.detail', entry));
    expandable(false);
  } finally {
    await act(() => root.unmount());
    Object.assign(globalThis, original);
  }
});
