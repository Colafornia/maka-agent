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

// Capture on the base revision before installing Pele:
// node packages/ui/scripts/benchmark-diagrams.mjs --capture /tmp/maka-diagrams-before
// Then compare on the changed revision (five interleaved fresh-page runs each):
// node packages/ui/scripts/benchmark-diagrams.mjs --compare /tmp/maka-diagrams-before
import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { gzipSync } from 'node:zlib';
import { build } from 'esbuild';
import { chromium } from '@playwright/test';

const root = resolve(import.meta.dirname, '../../..');
const baseRef = process.argv.includes('--base-ref') ? process.argv[process.argv.indexOf('--base-ref') + 1] : undefined;
const mermaidRoot = process.argv.includes('--mermaid-root') ? process.argv[process.argv.indexOf('--mermaid-root') + 1] : resolve(root, 'node_modules/mermaid');
const hasPele = baseRef ? undefined : JSON.parse(await readFile(resolve(root, 'packages/ui/package.json'))).dependencies.pele;
const renderer = hasPele ? 'pele' : 'mermaid';
const source = `flowchart LR\n${Array.from({ length: 100 }, (_, i) => `n${i}[步骤 ${i}] --> n${i + 1}[步骤 ${i + 1}]`).join('\n')}`;
const options = {
  bundle: true, minify: true, write: false, format: 'iife', outdir: '/virtual',
  loader: { '.woff2': 'dataurl', '.woff': 'dataurl', '.ttf': 'dataurl', '.svg': 'dataurl' },
  define: { 'process.env.NODE_ENV': '"production"' },
  ...(baseRef ? {
    alias: { mermaid: mermaidRoot },
    plugins: [{ name: 'baseline-diagrams', setup(build) {
      build.onLoad({ filter: /packages\/ui\/src\/(mermaid-diagram\.tsx|styles\.css)$/ }, args => ({
        contents: execFileSync('git', ['show', `${baseRef}:${args.path.slice(root.length + 1)}`], { cwd: root, encoding: 'utf8' }),
        loader: args.path.endsWith('.css') ? 'css' : 'tsx', resolveDir: resolve(args.path, '..'),
      }));
    } }],
  } : {}),
};
const micro = await build({ ...options, stdin: {
  contents: `import * as engine from '${renderer}'; window.engine = engine;`, resolveDir: root,
} });
const app = await build({ ...options, stdin: {
  resolveDir: root, loader: 'js', contents: `
    import React from 'react';
    import {createRoot} from 'react-dom/client';
    import {Theme} from '@astryxdesign/core';
    import {makaTheme} from './apps/desktop/src/renderer/astryx-theme/maka.js';
    import {MermaidDiagram} from './packages/ui/src/mermaid-diagram.tsx';
    import {LocaleProvider} from './packages/ui/src/locale-context.tsx';
    import './apps/desktop/src/renderer/styles.css';
    const root=createRoot(document.getElementById('root'));
    window.start = (source,dark=false) => {
      document.documentElement.classList.toggle('dark',dark);
      root.render(React.createElement(Theme,{theme:makaTheme,mode:dark?'dark':'light'},
        React.createElement(LocaleProvider,{locale:'en'},React.createElement(MermaidDiagram,{code:source,density:'default'}))));
    };
  `,
} });
const current = {
  renderer, version: hasPele ?? JSON.parse(await readFile(resolve(mermaidRoot, 'package.json'))).version,
  micro: micro.outputFiles.find(f => f.path.endsWith('.js')).text,
  app: app.outputFiles.find(f => f.path.endsWith('.js')).text,
  css: app.outputFiles.find(f => f.path.endsWith('.css')).text,
};
const [mode, directory] = process.argv.slice(2);
assert.ok(directory && ['--capture', '--compare'].includes(mode), 'Pass --capture or --compare and a baseline directory');
if (mode === '--capture') {
  await mkdir(directory, { recursive: true });
  await writeFile(resolve(directory, 'baseline.json'), JSON.stringify(current));
  console.log(`Captured ${renderer} production bundles in ${directory}`);
} else {
  const baseline = JSON.parse(await readFile(resolve(directory, 'baseline.json')));
  const bundles = [baseline, current];
  const server = createServer((req, res) => {
    const [, side, asset] = req.url.split('/');
    const bundle = bundles[Number(side)];
    if (asset?.endsWith('.js')) res.setHeader('Content-Type', 'text/javascript; charset=utf-8').end(bundle[asset.slice(0, -3)]);
    else if (asset === 'app.css') res.setHeader('Content-Type', 'text/css; charset=utf-8').end(bundle.css);
    else res.setHeader('Content-Type', 'text/html; charset=utf-8').end(`<html><head><meta charset="utf-8"><link rel="stylesheet" href="/${side}/app.css"></head><body><div id="root" style="width:640px"></div></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ headless: true });
  const samples = [[], []];
  try {
    for (let run = 0; run < 5; run++) for (const side of [0, 1]) {
      const page = await browser.newPage();
      await page.goto(`http://127.0.0.1:${server.address().port}/${side}/`);
      const sample = await page.evaluate(async ({ bundle, source }) => {
        const load = code => { const s = document.createElement('script'); s.textContent = code; document.body.append(s); };
        let start = performance.now();
        load(bundle.micro);
        const engine = bundle.renderer === 'mermaid' ? window.engine.default : window.engine;
        if (bundle.renderer === 'mermaid') engine.initialize({ startOnLoad: false, securityLevel: 'strict', htmlLabels: false, maxTextSize: 20000, maxEdges: 500, theme: 'default' });
        const rendered = bundle.renderer === 'mermaid' ? await engine.render('benchmark', source) : engine.render(source, { limit: 20000, maxEdges: 500, links: false, images: false, maxWidth: 640 });
        const coldMs = performance.now() - start;
        const svg = new DOMParser().parseFromString(rendered.svg, 'image/svg+xml');
        if (svg.querySelectorAll('.node, .pele-node').length !== 101
          || !svg.documentElement.textContent.replace(/\s+/g, '').includes('步骤100')) throw new Error('Incomplete large-diagram output');
        // A separate page is used for the end-to-end component path below.
        return { coldMs, svgChars: rendered.svg.length };
      }, { bundle: bundles[side], source });
      await page.close();
      const appPage = await browser.newPage();
      await appPage.goto(`http://127.0.0.1:${server.address().port}/${side}/`);
      const appMs = await appPage.evaluate(async ({ code, source }) => {
        const start = performance.now();
        const script = document.createElement('script'); script.textContent = code; document.body.append(script);
        window.start(source);
        while (!document.querySelector('[data-maka-mermaid-state="rendered"] svg')) {
          if (document.querySelector('[data-maka-mermaid-state="error"]')) throw new Error('Component rendering failed');
          if (performance.now() - start > 30000) throw new Error('Component timed out');
          await new Promise(requestAnimationFrame);
        }
        await new Promise(requestAnimationFrame);
        return performance.now() - start;
      }, { code: bundles[side].app, source });
      samples[side].push({ ...sample, appMs });
      await appPage.close();
    }
    const summary = values => ({ median: [...values].sort((a,b)=>a-b)[2], min: Math.min(...values), max: Math.max(...values) });
    console.log(JSON.stringify(bundles.map((b,i)=>({ renderer:b.renderer, version:b.version,
      rendererGzipBytes:gzipSync(b.micro).length, componentGzipBytes:gzipSync(b.app).length,
      coldMs:summary(samples[i].map(s=>s.coldMs)), appMs:summary(samples[i].map(s=>s.appMs)),
      successes:samples[i].length, failures:0, samples:samples[i],
    })), null, 2));
  } finally { await browser.close(); await new Promise(resolve => server.close(resolve)); }
}
