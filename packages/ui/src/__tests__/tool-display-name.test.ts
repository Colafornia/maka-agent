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
import { MAKA_CATALOG_TOOLS } from '@maka/core/tool-catalog';
import { UI_LOCALES } from '@maka/core/ui-locale';
import type { ToolActivityItem } from '../materialize.js';
import { resolveToolDisplayName } from '../tool-activity/display-name.js';
import { BUILTIN_TOOL_LABELS } from '../tool-activity/copy.js';

function item(toolName: string, displayName?: string): ToolActivityItem {
  return {
    toolUseId: toolName,
    toolName,
    ...(displayName !== undefined ? { displayName } : {}),
    status: 'completed',
    args: {},
  };
}

describe('tool display names', () => {
  it('renders persisted built-in tools in the active locale', () => {
    const browser = item('browser_navigate', '浏览器导航');
    const search = item('WebSearch', 'Web search');

    assert.equal(resolveToolDisplayName(browser, 'en'), 'Browser navigation');
    assert.equal(resolveToolDisplayName(browser, 'zh'), '浏览器导航');
    assert.equal(resolveToolDisplayName(search, 'en'), 'Web search');
    assert.equal(resolveToolDisplayName(search, 'zh'), '联网搜索');
  });

  it('resolves every built-in tool through every locale cell', () => {
    for (const locale of UI_LOCALES) {
      for (const [toolName, labels] of Object.entries(BUILTIN_TOOL_LABELS)) {
        assert.equal(
          resolveToolDisplayName(item(toolName, 'wrong persisted label'), locale),
          labels[locale],
          `${toolName} in ${locale}`,
        );
      }
    }
  });

  it('covers every product catalog tool with a built-in label', () => {
    for (const { name } of MAKA_CATALOG_TOOLS) {
      assert.ok(Object.hasOwn(BUILTIN_TOOL_LABELS, name), name);
    }
  });

  it('localizes Maka-owned tools outside the product binding catalog', () => {
    assert.equal(resolveToolDisplayName(item('MakaSettingsGet', 'Read Maka settings'), 'zh'), '读取 Maka 设置');
    assert.equal(resolveToolDisplayName(item('SkillSearch', 'SkillSearch'), 'zh'), '搜索技能');
    assert.equal(resolveToolDisplayName(item('SubmitPlan'), 'zh'), '提交计划');
    assert.equal(resolveToolDisplayName(item('tool_search'), 'zh'), '启用能力');
  });

  it('keeps provider labels for external tools and falls back to their identifiers', () => {
    assert.equal(resolveToolDisplayName(item('mcp__acme__lookup', 'Acme Lookup'), 'zh'), 'Acme Lookup');
    assert.equal(resolveToolDisplayName(item('custom_lookup'), 'en'), 'custom_lookup');
    assert.equal(resolveToolDisplayName(item('empty_label', ''), 'en'), 'empty_label');
  });
});
