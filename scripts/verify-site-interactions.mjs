import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-interaction-review-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1460';
const output = resolve('.codex-temp/site-interaction-verification');
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
  browser('wait', '--fn', 'Boolean(window.checkpointCapture)');
  for (const language of ['zh', 'en']) for (const theme of ['dark', 'light']) {
    const name = `${language}-during${theme === 'light' ? '-light' : ''}`;
    evaluate(`window.checkpointCapture.load('${name}')`);
    for (const step of ['question', 'permission']) {
      const shot = evaluate(`window.checkpointCapture.seek('${step}')`);
      const rect = evaluate(`(() => {
        const doc = document.querySelector('iframe').contentDocument;
        const question = '${step}' === 'question';
        const button = [...doc.querySelectorAll('button')].find(element =>
          (question ? ['Enable file snapshots', '启用文件变更快照'] : ['Allow once', '允许一次']).includes(element.textContent.trim()));
        const control = question ? button?.closest('[data-slot="card"]') : button?.parentElement?.parentElement;
        if (!control) return null;
        const rect = control.getBoundingClientRect();
        const covering = [...doc.querySelectorAll('[role="dialog"][data-state="open"]')].filter(dialog => {
          const bounds = dialog.getBoundingClientRect();
          return !dialog.contains(control) && bounds.left < rect.right && bounds.right > rect.left
            && bounds.top < rect.bottom && bounds.bottom > rect.top;
        });
        return { ...rect.toJSON(), coveringDialogs: covering.length };
      })()`);
      assert(rect, `${name}/${step}: missing interaction`);
      assert.equal(rect.coveringDialogs, 0, `${name}/${step}: interaction is covered by a dialog`);
      for (const mode of ['desktop', 'mobile']) {
        const frame = shot[mode];
        const bounds = { left: frame.x * shot.width, top: frame.y * shot.height,
          right: (frame.x + frame.width) * shot.width, bottom: (frame.y + frame.height) * shot.height };
        const contained = rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1
          && rect.top >= bounds.top - 1 && rect.bottom <= bounds.bottom + 1;
        const scale = Math.min((mode === 'desktop' ? 836 : 280) / (shot.width * frame.width),
          (mode === 'desktop' ? 512 : 171) / (shot.height * frame.height));
        report.push({ name, step, mode, bounds, rect, contained, scale });
        writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
        assert(contained, `${name}/${step}/${mode}: interaction is clipped`);
        assert(scale >= 0.8, `${name}/${step}/${mode}: interaction is too small (${scale})`);
      }
    }
  }
  console.log(JSON.stringify({ checks: report.length }));
} finally { browser('close'); }
