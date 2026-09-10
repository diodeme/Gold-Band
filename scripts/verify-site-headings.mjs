import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-headings-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve('.codex-temp/site-heading-verification');
mkdirSync(output, { recursive: true });
const report = [];
function browser(...args) {
  const path = resolve(output, 'response.json');
  const file = openSync(path, 'w');
  try {
    execFileSync(executable, ['--session', session, '--json', ...args], {
      stdio: ['ignore', file, 'inherit'], windowsHide: true, timeout: 45000,
    });
  } finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
try {
  for (const width of [320, 390, 768, 1280, 1440]) {
    browser('set', 'viewport', String(width), '844');
    for (const language of ['zh', 'en']) {
      browser('open', `${origin}/${language}/#during`);
      browser('wait', '--fn', `Boolean(document.querySelector('#during h2'))`);
      evaluate('document.fonts.ready.then(() => true)');
      const headings = evaluate(`Array.from(document.querySelectorAll('.chapter-copy h2')).map(element => {
        const lines = new Map();
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        const range = document.createRange();
        while (walker.nextNode()) {
          const node = walker.currentNode;
          for (let i = 0; i < node.length; i++) {
            range.setStart(node, i); range.setEnd(node, i + 1);
            const rect = range.getBoundingClientRect();
            if (!rect.width || !rect.height) continue;
            const y = Math.round(rect.top);
            lines.set(y, (lines.get(y) || '') + node.textContent[i]);
          }
        }
        return { text: element.textContent, lines: [...lines.values()] };
      })`);
      report.push({ width, language, headings });
      writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
      for (const heading of headings) {
        assert(heading.lines.length, `Invisible heading: ${heading.text}`);
        if (language === 'zh' && heading.lines.length > 1) {
          assert(heading.lines.at(-1).replace(/[\p{P}\s]/gu, '').length > 1,
            `Orphan heading at ${width}px: ${JSON.stringify(heading.lines)}`);
        }
      }
      assert.equal(evaluate('document.documentElement.scrollWidth > innerWidth'), false);
      console.log(JSON.stringify({ width, language, headings }));
    }
  }
} finally { browser('close'); }
