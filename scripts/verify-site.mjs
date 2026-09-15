import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = 'gold-band-site-verification';
const origin = process.env.SITE_URL || 'http://127.0.0.1:1441';
const output = resolve('.codex-temp/site-verification');
mkdirSync(output, { recursive: true });
function browser(...args) {
  const path = resolve(output, 'browser-response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], timeout: 45_000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert.equal(result.success, true, JSON.stringify(result));
  return result.data;
}
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const waitFor = code => browser('wait', '--fn', code);
const scrollTo = chapter => evaluate(`document.getElementById('${chapter}').scrollIntoView({behavior:'instant',block:'center'})`);
const sourcePanels = () => evaluate(`['workspace-navigation','workspace-center','workspace-right'].map(id=>Math.round(document.querySelector('.interactive-plane iframe').contentDocument.getElementById(id)?.getBoundingClientRect().width||0))`);
const report = [];
try {
  for (const language of ['zh', 'en']) for (const [width, height] of [[1600, 1000], [390, 844], [320, 740], [768, 1024]]) {
    browser('open', `${origin}/${language}/`);
    browser('set', 'viewport', String(width), String(height));
    waitFor("document.querySelector('.story') !== null");
    const initial = evaluate(`({ overflow:document.documentElement.scrollWidth>innerWidth, mobile:!!document.querySelector('.mobile-story'), preview:!!document.querySelector('.interactive-plane'), images:Array.from(document.querySelectorAll('.poster')).filter(e=>e.complete&&e.naturalWidth>0).length, requests:performance.getEntriesByType('resource').map(e=>e.name) })`);
    assert.equal(initial.overflow, false);
    assert.equal(initial.mobile, width < 1024);
    assert.equal(initial.preview, false);
    assert(!initial.requests.some(url => /\/assets\/(preview|webview-bootstrap|mermaid)-.*\.js/.test(url)));
    if (width < 1024) assert(evaluate(`Array.from(document.querySelectorAll('.chapter')).every(e=>e.children[0].hasAttribute('data-chapter-media')&&e.children[1].classList.contains('chapter-copy'))`));
    for (const chapter of ['before', 'during', 'after', 'personalize']) {
      scrollTo(chapter);
      waitFor(`document.querySelector('[data-chapter-media=${chapter}] [data-player-state]')?.dataset.playerState==='ready'`);
      assert.equal(evaluate("document.querySelectorAll('.replayer-wrapper iframe').length"), 1);
      assert.equal(evaluate("document.querySelectorAll('.interactive-plane iframe').length"), 0);
      assert.equal(evaluate('document.documentElement.scrollWidth>innerWidth'), false);
    }
    const screenshot = browser('screenshot');
    report.push({ language, width, height, initial, screenshot });
    console.log(`Verified ${language} ${width}x${height}`);
  }
  browser('open', `${origin}/zh/`);
  browser('set', 'viewport', '1600', '1000');
  scrollTo('personalize');
  waitFor("Boolean(document.querySelector('.try-button'))");
  evaluate("document.querySelector('.try-button').click()");
  waitFor("Boolean(document.querySelector('.interactive-plane iframe')?.contentDocument.querySelector('[data-right-workspace-dock]'))");
  assert.equal(evaluate("document.querySelectorAll('.replayer-wrapper iframe').length"), 0);
  const appearances = [];
  for (const [label, scheme] of [['浅色', 'light'], ['深色', 'dark']]) {
    evaluate(`Array.from(document.querySelectorAll('[role=tab]')).find(e=>e.textContent.includes('${label}')).dispatchEvent(new MouseEvent('mousedown',{bubbles:true,button:0}))`);
    waitFor(`document.querySelector('.interactive-plane iframe').contentDocument.documentElement.dataset.colorScheme==='${scheme}'`);
    assert.equal(evaluate('getComputedStyle(document.documentElement).backgroundColor'), 'rgb(16, 17, 18)');
    appearances.push({ scheme, screenshot: browser('screenshot') });
  }
  evaluate("Array.from(document.querySelectorAll('[role=tab]')).find(e=>e.textContent==='等宽').dispatchEvent(new MouseEvent('mousedown',{bubbles:true,button:0}))");
  assert.equal(evaluate("document.querySelector('.interactive-plane iframe').contentDocument.documentElement.dataset.font"), 'custom');
  const layouts = [sourcePanels()];
  const handle = evaluate("document.querySelector('[aria-label=调整预览宽度]').getBoundingClientRect().toJSON()");
  const startX = Math.round(handle.x + handle.width / 2);
  const startY = Math.round(handle.y + handle.height / 2);
  browser('mouse', 'move', String(startX), String(startY));
  browser('mouse', 'down', 'left');
  // The CLI's separate mouse-move command does not retain the pressed buttons.
  evaluate(`document.dispatchEvent(new PointerEvent('pointermove',{bubbles:true,buttons:1,button:-1,pointerId:1,pointerType:'mouse',clientX:${startX - 350},clientY:${startY},movementX:-350}))`);
  const draggedX = evaluate("document.querySelector('[aria-label=调整预览宽度]').getBoundingClientRect().x");
  assert(Math.abs(handle.x - draggedX - 350) < 3, 'Resize handle must track pointer in page coordinates');
  browser('mouse', 'up', 'left');
  waitFor("document.querySelector('.interactive-plane iframe').contentDocument.getElementById('workspace-navigation').getBoundingClientRect().width<10 && document.querySelector('.interactive-plane iframe').contentDocument.getElementById('workspace-right').getBoundingClientRect().width>10");
  layouts.push(sourcePanels());
  evaluate("document.querySelector('[aria-label=调整预览宽度]').focus()");
  browser('press', 'Home');
  waitFor("document.querySelector('.interactive-plane iframe').contentWindow.innerWidth<610");
  layouts.push(sourcePanels());
  evaluate("document.querySelector('[aria-label=调整预览宽度]').focus({preventScroll:true})");
  browser('press', 'End');
  waitFor("document.querySelector('.interactive-plane iframe').contentWindow.innerWidth>1350");
  waitFor("document.querySelector('.interactive-plane iframe').contentDocument.getElementById('workspace-navigation').getBoundingClientRect().width>10");
  layouts.push(sourcePanels());
  assert.deepEqual(layouts.map(panels => panels.filter(width => width > 10).length), [3, 2, 1, 3]);
  scrollTo('after');
  waitFor("document.querySelectorAll('.interactive-plane iframe').length===0");
  report.push({ appearances, layouts });
  browser('open', `${origin}/zh/`);
  browser('set', 'viewport', '390', '844');
  scrollTo('before');
  waitFor("document.querySelector('[data-player-state]')?.dataset.playerState==='ready'");
  browser('click', 'button[aria-label="播放或暂停"]');
  const pausedProgress = evaluate("document.querySelector('.rr-progress__step').style.width");
  browser('wait', '400');
  assert.equal(evaluate("document.querySelector('.rr-progress__step').style.width"), pausedProgress);
  evaluate("Array.from(document.querySelectorAll('.rr-controller button')).find(e=>e.textContent==='2x').click()");
  assert(evaluate("Array.from(document.querySelectorAll('.rr-controller button')).some(e=>e.textContent==='2x'&&e.classList.contains('active'))"));
  evaluate("(()=>{const e=document.querySelector('.rr-progress');const r=e.getBoundingClientRect();e.dispatchEvent(new MouseEvent('click',{bubbles:true,clientX:r.left+r.width/2,clientY:r.top+2}));})()");
  assert(Math.abs(evaluate("parseFloat(document.querySelector('.rr-progress__step').style.width)") - 50) < 1);
  browser('click', 'button[aria-label="全屏"]');
  waitFor('Boolean(document.fullscreenElement)');
  waitFor("Math.abs(parseFloat(document.querySelector('.replayer-wrapper').style.transform.slice(6))-innerWidth/1440)<0.002");
  report.push({ fullscreen: browser('screenshot'), playback: 'pause / seek / 2x / fullscreen passed' });
  evaluate('document.exitFullscreen()');
  waitFor('!document.fullscreenElement');
  scrollTo('before');
  waitFor("document.querySelector('[data-chapter-media=before] [data-player-state]')?.dataset.playerState==='ready'");
  evaluate("window.__siteFetch=window.fetch; window.fetch=(input,init)=>String(input).includes('during')?Promise.resolve(new Response('',{status:503})):window.__siteFetch(input,init)");
  scrollTo('during');
  waitFor("document.querySelector('[data-chapter-media=during] [data-player-state]')?.dataset.playerState==='error'");
  evaluate('window.fetch=window.__siteFetch');
  evaluate("Array.from(document.querySelectorAll('button')).find(e=>e.textContent.includes('重新加载')).click()");
  waitFor("document.querySelector('[data-player-state]')?.dataset.playerState==='ready'");
  report.push({ recovery: 'recording request failure and retry passed' });
  for (const language of ['zh', 'en']) for (const page of ['documentation', 'demo']) {
    browser('open', `${origin}/${language}/${page}`);
    waitFor("Boolean(document.querySelector('.placeholder-page'))");
    assert.equal(evaluate('document.querySelectorAll("iframe").length'), 0);
    assert.equal(evaluate('document.documentElement.lang'), language === 'zh' ? 'zh-CN' : 'en');
    const target = language === 'zh' ? 'Switch to English' : '切换为中文';
    assert.equal(evaluate(`document.querySelector('a[aria-label="${target}"]').getAttribute('href')`), `/${language === 'zh' ? 'en' : 'zh'}/${page}`);
  }
  writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
} finally { browser('close'); }
