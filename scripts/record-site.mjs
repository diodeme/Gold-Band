import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-site-record-${process.pid}`;
const url = process.env.SITE_URL || 'http://127.0.0.1:1460';
const output = resolve('marketing/site/media');
const temporary = resolve('.codex-temp/site-recording');
mkdirSync(output, { recursive: true });
mkdirSync(temporary, { recursive: true });
function browser(...args) {
  const responsePath = resolve(temporary, 'browser-response.json');
  const file = openSync(responsePath, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45_000, windowsHide: true }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(responsePath, 'utf8'));
  assert.equal(result.success, true, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const waitFor = code => browser('wait', '--fn', code);
const hold = ms => browser('wait', String(ms));
function screenshot(name) {
  const shot = browser('screenshot');
  const path = shot.path || shot.screenshotPath;
  assert(path, JSON.stringify(shot));
  copyFileSync(path, resolve(output, `${name}.png`));
}
const report = [];
try {
  for (const theme of (process.env.SITE_THEMES?.split(',') || ['dark'])) for (const language of ['zh', 'en']) for (const scene of (process.env.SITE_SCENES?.split(',') || ['before', 'during', 'after', 'personalize'])) {
    assert(['dark', 'light'].includes(theme), 'Invalid recording theme');
    const name = `${language}-${scene}${theme === 'light' ? '-light' : ''}`;
    browser('open', `${url}/preview.html?language=${language}&scene=${scene}&theme=${theme}`);
    browser('set', 'viewport', '1440', '880');
    waitFor(`Boolean(document.getElementById('workspace-center')) && document.querySelector('img').getBoundingClientRect().width < 100`);
    evaluate('localStorage.clear()');
    if (scene === 'before') {
      evaluate(`Array.from(document.querySelectorAll('[role=tab]')).find(e=>e.textContent==='Direct').dispatchEvent(new MouseEvent('mousedown',{bubbles:true,button:0}))`);
      waitFor(`Array.from(document.querySelectorAll('[role=tab]')).some(e=>e.textContent==='Direct'&&e.getAttribute('data-state')==='active')`);
    }
    evaluate('document.fonts.ready.then(()=>true)');
    hold(800);
    {
      evaluate('window.goldBandPreview.storyboard().then(value => { window.recordingResult = value }).catch(error => { window.recordingResult = error }); null');
      const deadline = Date.now() + 65_000;
      let result;
      while (Date.now() < deadline) {
        const state = evaluate('({ result: window.recordingResult, viewport: window.goldBandPreview.viewportTarget, pointer: window.goldBandPreview.pointerTarget })');
        result = state.result;
        if (result) break;
        if (state.viewport) browser('set', 'viewport', String(state.viewport.width), String(state.viewport.height));
        if (state.pointer) {
          browser('mouse', 'move', String(state.pointer.x), String(state.pointer.y));
          evaluate('window.goldBandPreview.pointerTarget = null');
        }
        hold(1000);
      }
      if (result?.name !== name) {
        const state = evaluate('({ checkpoint: window.goldBandPreview.lastCheckpoint, text: document.body.innerText })');
        writeFileSync(resolve(temporary, 'failure.json'), JSON.stringify({ result, ...state }, null, 2));
        browser('screenshot', resolve(temporary, 'failure.png'));
        assert.fail(JSON.stringify({ result, checkpoint: state.checkpoint }));
      }
      if (scene === 'during') {
        evaluate('window.goldBandPreview.advance("false")');
        waitFor('document.body.innerText.includes("\\\"result\\\"")');
      }
      assert.equal(evaluate('document.documentElement.dataset.colorScheme'), theme);
      screenshot(name);
      report.push({ language, scene, theme, ...result });
      console.log(JSON.stringify(report.at(-1)));
      continue;
    }
  }
  writeFileSync(resolve(temporary, `report-${process.env.SITE_THEMES || 'dark'}.json`), JSON.stringify(report, null, 2));
} finally {
  try { evaluate('window.goldBandPreview?.disposeReview()'); }
  finally { browser('close'); }
}
