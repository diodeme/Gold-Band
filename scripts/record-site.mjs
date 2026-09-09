import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { portableRecording } from './site-recording-assets.mjs';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = 'gold-band-site-record';
const url = process.env.RECORDING_URL || 'http://127.0.0.1:1442';
const output = resolve(process.env.RECORDING_OUTPUT || 'marketing/site/media');
assert(process.env.RECORDING_ATTACHMENTS, 'RECORDING_ATTACHMENTS is required for process output');
const temporary = resolve(process.env.RECORDING_ATTACHMENTS);
const selectedScenes = process.env.SITE_SCENES?.split(',') || ['before', 'during', 'after', 'personalize'];
if (selectedScenes.includes('during')) {
  execFileSync(process.execPath, [resolve('scripts/record-workflow.mjs')], {
    windowsHide: true, stdio: 'inherit',
    env: { ...process.env, RECORDING_OUTPUT: resolve(output, 'workflow') },
  });
}
if (selectedScenes.includes('before')) {
  execFileSync(process.execPath, [resolve('scripts/record-workflow.mjs')], {
    windowsHide: true, stdio: 'inherit',
    env: { ...process.env, RECORDING_SCENE: 'before', RECORDING_OUTPUT: resolve(output, 'before') },
  });
}
mkdirSync(output, { recursive: true });
mkdirSync(temporary, { recursive: true });
function browser(...args) {
  const responsePath = resolve(temporary, 'browser-response.json');
  const file = openSync(responsePath, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45_000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(responsePath, 'utf8'));
  assert.equal(result.success, true, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const waitFor = code => browser('wait', '--fn', code);
const hold = ms => browser('wait', String(ms));
const clickFile = name => evaluate(`Array.from(document.querySelectorAll('button[aria-label]')).find(e=>e.getAttribute('aria-label').includes(${JSON.stringify(name)})).click()`);
const panels = () => evaluate(`['workspace-navigation','workspace-center','workspace-right'].map(id=>Math.round(document.getElementById(id)?.getBoundingClientRect().width||0))`);
function screenshot(name) {
  const shot = browser('screenshot');
  const path = shot.path || shot.screenshotPath;
  assert(path, JSON.stringify(shot));
  copyFileSync(path, resolve(output, `${name}.png`));
}
const report = [];
let opened = false;
try {
  for (const language of (process.env.SITE_LANGUAGES?.split(',') || ['zh', 'en'])) for (const scene of selectedScenes.filter(scene => scene !== 'during' && scene !== 'before')) {
    browser('open', `${url}/?language=${language}&scene=${scene}`);
    opened = true;
    browser('set', 'viewport', '1440', '880');
    waitFor(`Boolean(document.getElementById('workspace-center')) && document.querySelector('img').getBoundingClientRect().width < 100`);
    evaluate('localStorage.clear()');
    if (scene === 'after' || scene === 'personalize') {
      waitFor(`Array.from(document.querySelectorAll('button[aria-label]')).some(e=>e.getAttribute('aria-label').includes('docs/workspace-notes.md'))`);
      clickFile('docs/workspace-notes.md');
      waitFor(`Boolean(document.querySelector('[data-right-workspace-dock]'))`);
    }
    evaluate('document.fonts.ready.then(()=>true)');
    hold(800);
    evaluate('window.goldBandPreview.start()');
    const layouts = [];
    if (scene === 'after') {
      hold(2600); screenshot(`${language}-${scene}`);
      clickFile('src/config.json'); hold(4000);
      clickFile('docs/workspace-notes.md'); hold(2500);
    } else {
      screenshot(`${language}-${scene}`);
      for (const width of [1440, 900, 600, 900, 1440]) {
        browser('set', 'viewport', String(width), '880'); hold(1800);
        layouts.push({ width, panels: panels() });
      }
      evaluate("window.goldBandPreview.appearance('dark','mono')"); hold(2200);
      evaluate("window.goldBandPreview.appearance('dark','default')"); hold(1600);
      assert.deepEqual(layouts.map(item => item.panels.filter(width => width > 10).length), [3, 2, 1, 2, 3]);
    }
    const recording = portableRecording(evaluate('window.goldBandPreview.stop()'), url);
    assert.equal(recording.reason, 'manual');
    assert(recording.events.some(event => event.type === 2));
    assert(recording.events.some(event => event.type === 3), `${scene} must contain actual DOM changes`);
    assert.equal(evaluate('document.documentElement.dataset.colorScheme'), 'dark');
    assert(!evaluate("document.body.innerText.includes('ACP 会话失败') || document.body.innerText.includes('错误阻塞预览')"));
    const serialized = JSON.stringify(recording);
    const durationMs = recording.events.at(-1).timestamp - recording.events[0].timestamp;
    if (Buffer.byteLength(serialized, 'utf8') > 12 * 1024 * 1024 || recording.events.length > 12000 || durationMs > 60000) {
      throw { code: 'site.asset-budget-exceeded', params: { language, scene } };
    }
    assert.equal(Buffer.byteLength(serialized, 'utf8'), recording.bytes);
    writeFileSync(resolve(output, `${language}-${scene}.json`), serialized);
    const item = { language, scene, bytes: recording.bytes, events: recording.events.length, durationMs: recording.events.at(-1).timestamp - recording.events[0].timestamp, layouts };
    report.push(item); console.log(JSON.stringify(item));
  }
  writeFileSync(resolve(temporary, 'report.json'), JSON.stringify(report, null, 2));
} finally { if (opened) browser('close'); }
