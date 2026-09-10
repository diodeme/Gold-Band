import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-control-layout-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve(process.env.CONTROL_REPORT_DIR || '.codex-temp/site-control-layout');
mkdirSync(output, { recursive: true });
function browser(...args) {
  const path = resolve(output, 'response.json'), file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45000, windowsHide: true }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const report = [];
try {
  browser('open', origin);
  for (const motion of ['no-preference', 'reduced-motion']) for (const theme of ['dark', 'light']) for (const language of ['zh', 'en']) for (const width of [320, 390, 768, 1280, 1440]) {
    browser('set', 'media', theme, motion);
    evaluate(`localStorage.setItem('gold-band.site.appearance.v1',JSON.stringify({version:1,theme:'${theme}'}))`);
    browser('set', 'viewport', String(width), '900');
    browser('open', `${origin}/${language}/#during`);
    const selector = motion === 'reduced-motion' ? '[data-chapter-media="during"] .poster-play' : '.replay-toggle';
    browser('wait', '--fn', `Boolean(document.querySelector('${selector}'))`);
    const state = evaluate(`(() => {
      const button = document.querySelector('${selector}'), media = button.closest('.chapter-media');
      const stage = media.querySelector('.media-surface').getBoundingClientRect(), control = button.getBoundingClientRect(), host = media.getBoundingClientRect();
      return { stage:stage.toJSON(), control:control.toJSON(), host:host.toJSON(), overflow:document.documentElement.scrollWidth>innerWidth,
        overlap:control.left<stage.right&&control.right>stage.left&&control.top<stage.bottom&&control.bottom>stage.top,
        contained:control.left>=host.left-1&&control.right<=host.right+1&&control.top>=host.top-1&&control.bottom<=host.bottom+1 };
    })()`);
    report.push({ motion, theme, language, width, ...state });
    writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    browser('screenshot', '', resolve(output, `${language}-${theme}-${width}-${motion}.png`));
    assert.equal(state.overlap, false, 'Playback control overlaps the recording surface');
    assert(state.contained, 'Playback control escapes its reserved media region');
    assert(!state.overflow, 'Document overflows');
    assert(state.control.width >= 44 && state.control.height >= 44, 'Playback target is too small');
    assert(Math.abs(state.stage.width / state.stage.height - 1440 / 880) < 0.01, 'Recording aspect ratio changed');
  }
  console.log(JSON.stringify({ checks: report.length }));
} finally { browser('close'); }
