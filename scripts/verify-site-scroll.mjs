import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function verifyScroll(before, after, desktop) {
  assert.equal(before.paused, true, 'Replay was not paused before scrolling');
  assert(after.scrollY - before.scrollY >= 30, 'Page did not actually scroll');
  assert.equal(after.active, 'during');
  assert.equal(after.players, 1);
  assert.equal(after.samePlayer, true, 'Scroll replaced the active renderer');
  assert.equal(after.overflow, false);
  assert.equal(after.paused, true);
  assert.deepEqual(after.frame, before.frame, 'Scrolling advanced a paused replay');
  const displacement = after.mediaTop - before.mediaTop;
  if (desktop) {
    assert.equal(after.sticky, 'sticky');
    assert(Math.abs(displacement) <= 1, 'Desktop media did not remain sticky');
    assert(after.mediaTop >= after.headerBottom - 1, 'Sticky media is hidden by the header');
    assert(after.mediaRight <= after.copyLeft, 'Desktop media overlaps the chapter copy');
  } else {
    assert.equal(after.sticky, null);
    assert(Math.abs(displacement + after.scrollY - before.scrollY) <= 1, 'Mobile media did not follow natural page scrolling');
    assert(after.mediaBottom <= after.copyTop, 'Mobile media is not above the chapter copy');
  }
}

function main() {
  const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
  const origin = process.env.SITE_URL || 'http://127.0.0.1:1441';
  const session = `gold-band-scroll-${process.pid}`;
  const output = resolve('.codex-temp/site-scroll-verification');
  mkdirSync(output, { recursive: true });
  const report = { version: 1, origin, cases: [] };
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
  const wait = code => browser('wait', '--fn', code);
  const measure = `(() => {
    const media = document.querySelector('[data-chapter-media="during"]');
    const copy = document.querySelector('#during .chapter-copy').getBoundingClientRect();
    const bounds = media.getBoundingClientRect();
    const plane = media.querySelector('.replayer-wrapper');
    return { scrollY, active: document.querySelector('section[data-active="true"]')?.id,
      players: document.querySelectorAll('.replayer-wrapper').length,
      samePlayer: plane === window.scrollVerificationPlayer,
      paused: /Resume|继续/.test(document.querySelector('.replay-toggle')?.getAttribute('aria-label') || ''),
      frame: { checkpoint: plane.dataset.checkpoint, transform: plane.style.transform,
        body: plane.querySelector('iframe').contentDocument.body.innerText },
      overflow: document.documentElement.scrollWidth > innerWidth,
      mediaTop: bounds.top, mediaBottom: bounds.bottom, mediaRight: bounds.right,
      copyLeft: copy.left, copyTop: copy.top,
      headerBottom: document.querySelector('header').getBoundingClientRect().bottom,
      sticky: document.querySelector('.sticky-demonstration') ? getComputedStyle(document.querySelector('.sticky-demonstration')).position : null };
  })()`;
  try {
    browser('open', origin);
    for (const language of ['zh', 'en']) for (const theme of ['dark', 'light']) for (const [width, height] of [[1440, 880], [1280, 880], [768, 1024], [390, 844], [320, 740]]) {
      browser('set', 'viewport', String(width), String(height));
      evaluate(`localStorage.setItem('gold-band.site.appearance.v1', JSON.stringify({version:1,theme:${JSON.stringify(theme)}}))`);
      browser('open', `${origin}/${language}/?scroll-verification=${process.pid}-${theme}-${width}#during`);
      wait(`document.querySelector('[data-chapter-media="during"] [data-player-state="ready"]') !== null`);
      wait(`document.querySelector('[data-chapter-media="during"] [data-camera-state="stable"]') !== null`);
      evaluate(`new Promise(resolve => { let last = scrollY, stable = 0; const started = performance.now(); function sample() {
        stable = scrollY === last ? stable + 1 : 0; last = scrollY;
        if (stable >= 10 || performance.now() - started > 3000) resolve(); else requestAnimationFrame(sample);
      } requestAnimationFrame(sample); })`);
      evaluate(`const button = document.querySelector('.replay-toggle'); if (/Pause|暂停/.test(button.getAttribute('aria-label'))) button.click(); window.scrollVerificationPlayer = document.querySelector('.replayer-wrapper')`);
      const before = evaluate(measure);
      browser('scroll', 'down', '40');
      evaluate('new Promise(resolve => setTimeout(resolve, 500))');
      const after = evaluate(measure);
      const name = `${language}-${theme}-${width}`;
      browser('screenshot', '', resolve(output, `${name}.png`));
      report.cases.push({ language, theme, width, height, before, after });
      verifyScroll(before, after, width >= 1024);
      console.log(`Verified scroll ${name}`);
    }
  } finally {
    writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
    browser('close');
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
