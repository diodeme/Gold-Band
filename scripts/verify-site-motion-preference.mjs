import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-motion-preference-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve('.codex-temp/site-motion-preference');
mkdirSync(output, { recursive: true });
const browser = (...args) => {
  const path = resolve(output, 'response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], windowsHide: true, timeout: 45_000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result));
  return result.data;
};
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const wait = code => browser('wait', '--fn', code);
const state = `(()=>{const p=document.querySelector('.replayer-wrapper');return {checkpoint:p?.dataset.checkpoint,transform:p?.style.transform,text:p?.querySelector('iframe')?.contentDocument.body.innerText,label:document.querySelector('.replay-toggle')?.getAttribute('aria-label')};})()`;
const report = { version: 1, cases: [] };
try {
  browser('open', origin);
  for (const language of ['zh', 'en']) for (const width of [1280, 390]) {
    const theme = language === 'zh' ? 'dark' : 'light';
    const pause = language === 'zh' ? '暂停演示' : 'Pause demo';
    const resume = language === 'zh' ? '继续演示' : 'Resume demo';
    browser('set', 'viewport', String(width), '900');
    browser('set', 'media', theme);
    browser('open', `${origin}/${language}/?motion-preference=${process.pid}-${width}#during`);
    wait(`document.querySelector('.replay-toggle')?.getAttribute('aria-label')===${JSON.stringify(pause)}`);
    wait(`document.querySelector('.replayer-wrapper')?.dataset.checkpoint==='result-menu'`);
    browser('set', 'media', theme, 'reduced-motion');
    wait(`document.querySelector('.replay-toggle')?.getAttribute('aria-label')===${JSON.stringify(resume)}`);
    const paused = evaluate(state);
    browser('wait', '1200');
    assert.deepEqual(evaluate(state), paused, 'Reduced-motion change did not freeze replay');
    assert(evaluate('document.documentElement.scrollWidth <= innerWidth'));
    const shot = browser('screenshot');
    const image = resolve(output, `${language}-${width}-paused.png`);
    copyFileSync(shot.path || shot.screenshotPath, image);
    browser('set', 'media', theme);
    assert.equal(evaluate(`document.querySelector('.replay-toggle')?.getAttribute('aria-label')`), resume);
    evaluate(`document.querySelector('.replay-toggle').click()`);
    wait(`document.querySelector('.replayer-wrapper')?.dataset.checkpoint!==${JSON.stringify(paused.checkpoint)}`);
    assert.equal(evaluate(`document.querySelector('.replay-toggle')?.getAttribute('aria-label')`), pause);
    browser('set', 'media', theme, 'reduced-motion');
    browser('open', `${origin}/${language}/?motion-poster=${process.pid}-${width}#during`);
    wait(`document.querySelector('[data-chapter-media="during"] .poster-play')!==null`);
    assert.equal(evaluate(`document.querySelectorAll('.replayer-wrapper').length`), 0);
    assert.equal(evaluate(`performance.getEntriesByType('resource').filter(e=>new URL(e.name).pathname.endsWith('.json')).length`), 0);
    report.cases.push({language, theme, width, runtimePause: true, frozenCheckpoint: paused.checkpoint, explicitResume: true, reducedEntryPosterOnly: true, image});
    writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report.cases.at(-1)));
  }
} finally { browser('close'); }
