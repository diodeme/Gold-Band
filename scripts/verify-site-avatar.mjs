import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-avatar-review-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1463';
const output = resolve('.codex-temp/site-avatar-verification');
mkdirSync(output, { recursive: true });
function browser(...args) {
  const path = resolve(output, 'response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45000, windowsHide: true }); }
  finally { closeSync(file); }
  const response = JSON.parse(readFileSync(path, 'utf8'));
  assert(response.success, JSON.stringify(response));
  return response.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const report = [];
try {
  browser('open', `${origin}/checkpoints.html`);
  browser('set', 'viewport', '1440', '880');
  browser('wait', '--fn', 'Boolean(window.checkpointCapture)');
  for (const language of ['zh', 'en']) for (const theme of ['dark', 'light']) {
    const name = `${language}-personalize${theme === 'light' ? '-light' : ''}`;
    evaluate(`window.checkpointCapture.load('${name}')`);
    const shot = evaluate("window.checkpointCapture.seek('avatar-option')");
    const menu = evaluate(`(() => {
      const doc = document.querySelector('iframe').contentDocument;
      const menu = doc.querySelector('[role="menu"][data-state="open"]');
      return menu && { bounds: menu.getBoundingClientRect().toJSON(), items: menu.querySelectorAll('[role="menuitem"]').length };
    })()`);
    assert(menu?.items >= 2, `${name}: missing avatar choices`);
    for (const mode of ['desktop', 'mobile']) {
      const frame = shot[mode];
      const bounds = { left: frame.x * shot.width, top: frame.y * shot.height,
        right: (frame.x + frame.width) * shot.width, bottom: (frame.y + frame.height) * shot.height };
      const rect = menu.bounds;
      const contained = rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1
        && rect.top >= bounds.top - 1 && rect.bottom <= bounds.bottom + 1;
      const scale = Math.min((mode === 'desktop' ? 836 : 280) / (shot.width * frame.width),
        (mode === 'desktop' ? 512 : 171) / (shot.height * frame.height));
      report.push({ name, mode, bounds, menu, contained, scale });
      writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
      assert(contained, `${name}/${mode}: avatar menu is clipped`);
      assert(scale >= 0.8, `${name}/${mode}: avatar choices are too small (${scale})`);
    }
  }
  console.log(JSON.stringify({ checks: report.length }));
} finally { browser('close'); }
