import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-theme-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve('.codex-temp/site-theme-verification');
mkdirSync(output, { recursive: true });
const browser = (...args) => {
  const path = resolve(output, 'response.json');
  const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], windowsHide: true, timeout: 45000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result));
  return result.data;
};
const evaluate = source => browser('eval', '-b', Buffer.from(source).toString('base64')).result;
const wait = source => browser('wait', '--fn', source);
const state = () => evaluate(`({ theme: document.documentElement.dataset.siteTheme,
  scrollY,
  chapter: document.querySelector('[data-chapter-media] [data-player-state]')?.closest('[data-chapter-media]').dataset.chapterMedia,
  checkpoint: document.querySelector('.replayer-wrapper')?.dataset.checkpoint,
  pause: document.querySelector('.replay-toggle')?.getAttribute('aria-label'),
  players: document.querySelectorAll('.replayer-wrapper').length,
  overflow: document.documentElement.scrollWidth > innerWidth,
  frameTheme: document.querySelector('.replayer-wrapper iframe')?.contentDocument.documentElement.dataset.colorScheme })`);
const report = [];
try {
  browser('open', origin);
  for (const language of ['zh', 'en']) for (const chapter of (process.env.SITE_SCENES?.split(',') || ['before', 'during', 'after', 'personalize'])) {
    const checkpoint = { before: 'workflow-options', during: 'result-menu', after: 'report-result', personalize: 'layout-5' }[chapter];
    assert(checkpoint, `Unknown scene: ${chapter}`);
    evaluate(`localStorage.setItem('gold-band.site.appearance.v1', JSON.stringify({version:1,theme:'dark'}))`);
    browser('set', 'viewport', '1440', '880');
    browser('open', `${origin}/${language}/?theme-review=${chapter}#${chapter}`);
    if (chapter === 'personalize') wait(`document.querySelector('.replayer-wrapper')?.dataset.checkpoint === 'layout-3'`);
    wait(`document.querySelector('.replayer-wrapper')?.dataset.checkpoint === '${checkpoint}' && document.querySelector('.replayer-wrapper')?.dataset.cameraState === 'stable'`);
    evaluate(`document.querySelector('.replay-toggle').click()`);
    const before = state();
    report.push({ language, phase: 'before', ...before });
    assert.equal(before.chapter, chapter);
    assert.equal(before.theme, 'dark');
    assert.equal(before.checkpoint, checkpoint);
    const label = language === 'zh' ? '主题' : 'Theme';
    const choose = (target, measure = false) => {
      const button = evaluate(`(() => { const element = document.querySelector('button[aria-label="${label}"]'); const rect=element.getBoundingClientRect(); element.focus({preventScroll:true}); return { top: rect.top, bottom: rect.bottom }; })()`);
      assert(button.top >= 0 && button.bottom < 150, 'Theme control must remain visible');
      browser('press', 'ArrowDown');
      wait(`Boolean(document.querySelector('[role="menuitemradio"]'))`);
      report.push({ language, phase: 'menu-open', ...state() });
      const choice = language === 'zh' ? target === 'light' ? '浅色' : '深色' : target === 'light' ? 'Light' : 'Dark';
      if (measure) evaluate(`(() => { const started=performance.now(); window.themeRestoreMs=null; const observer=new MutationObserver(()=>{ if(document.querySelector('[data-player-theme="${target}"]')?.dataset.playerState==='ready'){ window.themeRestoreMs=performance.now()-started; observer.disconnect(); } }); observer.observe(document.querySelector('.recording'),{attributes:true,attributeFilter:['data-player-state']}); })()`);
      evaluate(`[...document.querySelectorAll('[role="menuitemradio"]')].find(e=>e.textContent.trim()===${JSON.stringify(choice)}).click()`);
    };
    for (const [iteration, target] of ['light', 'dark', 'light'].entries()) {
      if (iteration === 0) evaluate(`(() => { window.themeFetch = window.fetch; window.fetch = (...args) => String(args[0]).endsWith('.json') ? new Promise(resolve => { window.releaseTheme = () => resolve(new Response('', {status:503})); }) : window.themeFetch(...args); })()`);
      choose(target, iteration > 0);
      if (iteration === 0) {
        wait(`document.querySelector('[data-player-state="loading"]') && document.querySelector('[data-checkpoint-poster="${checkpoint}"][data-poster-theme="light"] img')?.naturalWidth > 0`);
        const loading = state();
        assert.equal(loading.scrollY, before.scrollY); assert.equal(loading.players, 0);
        browser('screenshot', resolve(output, `${language}-${chapter}-loading-light.png`));
        evaluate('window.releaseTheme()');
        wait(`Boolean(document.querySelector('[data-player-state="error"]'))`);
        assert.equal(evaluate(`document.querySelector('[data-checkpoint-poster]')?.dataset.checkpointPoster`), before.checkpoint);
        evaluate(`window.fetch = window.themeFetch; document.querySelector('.media-status button').click()`);
        report.push({ language, phase: 'loading-and-retry', ...loading });
      }
      wait(`document.querySelector('[data-player-theme="${target}"]')?.dataset.playerState === 'ready' && document.querySelector('.replayer-wrapper iframe')?.contentDocument.documentElement.dataset.colorScheme === '${target}'`);
      const after = state();
      assert.equal(after.theme, target);
      assert.equal(after.frameTheme, target);
      assert.equal(after.chapter, before.chapter);
      assert.equal(after.checkpoint, before.checkpoint);
      assert.equal(after.pause, before.pause);
      assert.equal(after.scrollY, before.scrollY);
      if (iteration > 0) assert.equal(evaluate(`document.activeElement?.getAttribute('aria-label')`), label);
      if (chapter === 'personalize') {
        const requirement = language === 'en' ? 'Keep per-turn file changes available for review.' : '保留每轮文件变更，方便审阅。';
        assert(evaluate(`document.querySelector('.replayer-wrapper iframe').contentDocument.body.innerText`).includes(requirement), 'Restored replay lost conversation content');
      }
      if (chapter === 'before' || chapter === 'during') {
        const overlay = evaluate(`(() => {
          const doc = document.querySelector('.replayer-wrapper iframe').contentDocument;
          const menu = doc.querySelector('[role="listbox"][data-state="open"]');
          return menu && { opacity: Number(doc.defaultView.getComputedStyle(menu).opacity),
            width: menu.getBoundingClientRect().width, height: menu.getBoundingClientRect().height,
            options: menu.querySelectorAll('[role="option"]').length };
        })()`);
        assert(overlay && overlay.opacity >= 0.99 && overlay.width > 0 && overlay.height > 0 && overlay.options >= 2,
          `Restored menu is invisible: ${JSON.stringify(overlay)}`);
        after.overlay = overlay;
      }
      assert.equal(after.players, 1);
      assert.equal(after.overflow, false);
      report.push({ language, target, ...after });
      if (iteration > 0) {
        const restoreMs = evaluate('window.themeRestoreMs');
        assert.equal(typeof restoreMs, 'number');
        assert(restoreMs < 1000, `Local production restore exceeded 1s: ${restoreMs}`);
        report.at(-1).restoreMs = restoreMs;
      }
    }
    evaluate(`(() => { window.themeFetch = window.fetch; window.fetch = (...args) => String(args[0]).endsWith('.json') && !String(args[0]).includes('-light-') ? new Promise(resolve => { window.releaseTheme = async () => { resolve(await window.themeFetch(args[0])); }; }) : window.themeFetch(...args); })()`);
    choose('dark');
    wait(`document.querySelector('[data-player-theme="dark"]')?.dataset.playerState === 'loading'`);
    choose('light');
    wait(`document.querySelector('[data-player-theme="light"]')?.dataset.playerState === 'ready'`);
    evaluate('window.fetch = window.themeFetch; window.releaseTheme()');
    evaluate('new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))');
    const rapid = state();
    assert.equal(rapid.theme, 'light'); assert.equal(rapid.chapter, before.chapter);
    assert.equal(rapid.checkpoint, before.checkpoint); assert.equal(rapid.pause, before.pause);
    assert.equal(rapid.players, 1); assert.equal(rapid.scrollY, before.scrollY);
    report.push({ language, phase: 'rapid-replacement', ...rapid });
    for (const width of [320, 1440]) {
      browser('set', 'viewport', String(width), width === 320 ? '844' : '880');
      wait(`document.querySelector('.replayer-wrapper')?.dataset.checkpoint === '${checkpoint}'`);
      const restored = state();
      assert.equal(restored.checkpoint, before.checkpoint);
      assert.equal(restored.pause, before.pause);
      assert.equal(restored.players, 1);
      assert.equal(restored.overflow, false);
      browser('screenshot', resolve(output, `${language}-${chapter}-${width}-light.png`));
      report.push({ language, width, ...restored });
    }
    evaluate(`document.querySelector('a[hreflang]').click()`);
    const nextLanguage = language === 'zh' ? 'en' : 'zh-CN';
    wait(`document.documentElement.lang === '${nextLanguage}' && document.querySelector('[data-chapter-media="${chapter}"] [data-player-state]')?.dataset.playerState === 'ready'`);
    assert.equal(state().chapter, chapter); assert.equal(state().theme, 'light');
    report.push({ language, phase: 'language-switch', ...state() });
  }
  writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ checks: report.length, output }));
} catch (error) {
  writeFileSync(resolve(output, 'failure.json'), JSON.stringify({ report, state: state(), error: String(error) }, null, 2));
  browser('screenshot', resolve(output, 'failure.png'));
  throw error;
} finally { browser('close'); }
