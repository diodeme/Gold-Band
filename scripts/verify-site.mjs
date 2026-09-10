import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, copyFileSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-site-verification-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1441';
const output = resolve('.codex-temp/site-verification');
mkdirSync(output, { recursive: true });
function browser(...args) {
  const path = resolve(output, 'browser-response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45_000, windowsHide: true }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert.equal(result.success, true, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const waitFor = code => browser('wait', '--fn', code);
function screenshot(name) {
  const shot = browser('screenshot');
  const path = shot.path || shot.screenshotPath;
  assert(path, 'Browser did not return a screenshot file');
  const destination = resolve(output, `${name}.png`);
  copyFileSync(path, destination);
  return destination;
}
const viewports = [[1440, 880], [1280, 880], [768, 1024], [390, 844], [320, 740]];
const report = { version: 1, technical: [], demo: [], visualAcceptance: 'pending-scene-and-reference-review' };
try {
  browser('open', origin);
  for (const language of ['zh', 'en']) for (const [width, height] of viewports) {
    browser('set', 'viewport', String(width), String(height));
    browser('open', `${origin}/${language}/?verify=${process.pid}-${width}#before`);
    waitFor("document.querySelector('[data-player-state]')?.dataset.playerState === 'ready'");
    const initial = evaluate(`({ overflow:document.documentElement.scrollWidth>innerWidth,
      mobile:!!document.querySelector('.mobile-story'), requests:performance.getEntriesByType('resource').map(e=>e.name),
      downloads:[...document.querySelectorAll('a')].filter(e=>e.textContent.includes(${JSON.stringify(language === 'zh' ? '下载 Gold Band' : 'Download Gold Band')})).map(e=>e.href) })`);
    assert.equal(initial.overflow, false);
    assert.equal(initial.mobile, width < 1024);
    assert.equal(initial.downloads.length, 2);
    assert.equal(initial.downloads[0], initial.downloads[1]);
    assert(initial.requests.filter(url => /\.json(?:\?|$)/.test(url)).length <= 1, 'First chapter loaded unrelated recordings');
    assert(!initial.requests.some(url => /\/(?:preview|webview-bootstrap|archive)[^/]*\.(?:js|json)/.test(url)));
    if (width < 1024) assert(evaluate("[...document.querySelectorAll('.chapter')].every(e=>e.children[0].hasAttribute('data-chapter-media')&&e.children[1].classList.contains('chapter-copy'))"));
    const chapters = [];
    for (const chapter of ['before', 'during', 'after', 'personalize']) {
      evaluate(`document.getElementById(${JSON.stringify(chapter)}).scrollIntoView({behavior:'instant',block:'start'})`);
      waitFor(`document.querySelector('[data-chapter-media="${chapter}"] [data-player-state]')?.dataset.playerState === 'ready'`);
      waitFor(`document.querySelector('[data-chapter-media="${chapter}"] [data-camera-state="stable"]') !== null`);
      assert.equal(evaluate("document.querySelectorAll('.replayer-wrapper iframe').length"), 1);
      assert.equal(evaluate('document.documentElement.scrollWidth>innerWidth'), false);
      const pauseLabel = language === 'zh' ? '暂停演示' : 'Pause demo';
      const resumeLabel = language === 'zh' ? '继续演示' : 'Resume demo';
      evaluate("document.querySelector('.replay-toggle').click()");
      waitFor(`document.querySelector('.replay-toggle')?.getAttribute('aria-label') === ${JSON.stringify(resumeLabel)}`);
      const state = evaluate(`({checkpoint:document.querySelector('.replayer-wrapper').dataset.checkpoint,
        bodyLength:document.querySelector('.replayer-wrapper iframe').contentDocument.body.innerText.length,
        brokenImages:[...document.querySelectorAll('.poster')].filter(e=>e.complete&&e.naturalWidth===0).length})`);
      assert(state.bodyLength > 50, 'Replay is blank');
      assert.equal(state.brokenImages, 0);
      const image = screenshot(`${language}-${width}-${chapter}`);
      evaluate("document.querySelector('.replay-toggle').click()");
      waitFor(`document.querySelector('.replay-toggle')?.getAttribute('aria-label') === ${JSON.stringify(pauseLabel)}`);
      chapters.push({ chapter, ...state, image });
    }
    report.technical.push({ language, width, height, initial, chapters });
    console.log(`Verified website ${language} ${width}x${height}`);
  }
  for (const language of ['zh', 'en']) {
    browser('set', 'viewport', '1280', '880');
    browser('open', `${origin}/${language}/documentation?verify=${process.pid}`);
    waitFor(`document.querySelector('h1')?.textContent === ${JSON.stringify(language === 'zh' ? '文档' : 'Documentation')}`);
    const href = evaluate("[...document.querySelectorAll('nav a')].find(e=>e.textContent==='Demo').href");
    browser('open', `${href}#demo-review?project=default&run=run-051&round=round-001&node=accept&attempt=attempt-001&branch=root`);
    waitFor("document.body.innerText.includes('round-001/accept/attempt-001')");
    assert(evaluate(`document.body.innerText.includes(${JSON.stringify(language === 'zh' ? '审阅项目结构' : 'Review project structure')})`));
    const images = [screenshot(`${language}-demo-desktop`)];
    for (const width of [390, 1280]) {
      browser('set', 'viewport', String(width), '880');
      waitFor('document.documentElement.scrollWidth <= innerWidth');
      assert(evaluate("document.body.innerText.includes('round-001/accept/attempt-001')"));
      images.push(screenshot(`${language}-demo-${width}`));
    }
    browser('open', `${href}#demo-review?run=missing`);
    waitFor("document.querySelector('[role=alert]') !== null");
    assert(!evaluate("document.body.innerText.includes('round-001/accept/attempt-001')"));
    report.demo.push({ language, href, images, invalidLocatorRejected: true });
    console.log(`Verified Demo ${language}`);
  }
  writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
} finally { browser('close'); }
