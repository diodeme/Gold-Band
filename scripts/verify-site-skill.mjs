import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-skill-review-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1463';
const output = resolve('.codex-temp/site-skill-verification');
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
    const name = `${language}-before${theme === 'light' ? '-light' : ''}`;
    const steps = evaluate(`window.checkpointCapture.load('${name}')`).filter(id => id.startsWith('skill'));
    const candidates = [];
    for (const step of steps) {
      const shot = evaluate(`window.checkpointCapture.seek('${step}')`);
      const icons = evaluate(`(() => {
        const doc = document.querySelector('iframe').contentDocument;
        const strip = doc.querySelector('[data-testid="skill-agent-overflow"]');
        return [...(strip?.querySelectorAll('img') || [])].map(image => image.getBoundingClientRect().toJSON());
      })()`);
      const frame = shot.mobile;
      const bounds = { left: frame.x * shot.width, top: frame.y * shot.height,
        right: (frame.x + frame.width) * shot.width, bottom: (frame.y + frame.height) * shot.height };
      const contained = icons.length > 2 && icons.every(rect => rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1
        && rect.top >= bounds.top - 1 && rect.bottom <= bounds.bottom + 1);
      const scale = Math.min(280 / (shot.width * frame.width), 171 / (shot.height * frame.height));
      candidates.push({ step, icons: icons.length, contained, scale });
    }
    report.push({ name, candidates });
    writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    assert(candidates.some(candidate => candidate.contained && candidate.scale >= 0.8),
      `${name}: no readable mobile Skill shot contains the source and synchronization icons: ${JSON.stringify(candidates)}`);
  }
  console.log(JSON.stringify(report));
} finally { browser('close'); }
