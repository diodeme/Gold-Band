import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { referenceAnimationsSettled } from './site-reference-readiness.mjs';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-reference-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
const output = resolve('.codex-temp/site-reference-review');
mkdirSync(output, { recursive: true });
const browser = (...args) => {
  const path = resolve(output, 'response.json'); const file = openSync(path, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], windowsHide: true, timeout: 45000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(path, 'utf8'));
  assert(result.success, JSON.stringify(result)); return result.data;
};
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const report = [];
try {
  for (const width of [1440, 1280, 768, 390, 320]) {
    const height = width < 500 ? 844 : 880;
    browser('set', 'viewport', String(width), String(height));
    for (const theme of ['light', 'dark']) {
      browser('open', 'https://pi.dev/');
      browser('wait', '--fn', `Boolean(document.querySelector('button[aria-label^="Theme preference:"]'))`);
      for (let attempt = 0; attempt < 3; attempt++) {
        const label = evaluate(`document.querySelector('button[aria-label^="Theme preference:"]').getAttribute('aria-label')`);
        if (label.startsWith(`Theme preference: ${theme}.`) || label.startsWith(`Theme preference: ${theme} (`)) break;
        evaluate(`document.querySelector('button[aria-label^="Theme preference:"]').click()`);
      }
      const themeLabel = evaluate(`document.querySelector('button[aria-label^="Theme preference:"]').getAttribute('aria-label')`);
      assert(themeLabel.includes(`Theme preference: ${theme}`), themeLabel);
      browser('wait', '--fn', `document.querySelector('#stickyNav')?.checkVisibility({checkOpacity:true,checkVisibilityCSS:true})`);
      evaluate(`document.fonts.ready.then(() => true)`);
      // Navigation appears before the hero's finite entrance animations finish.
      browser('wait', '--fn', `(${referenceAnimationsSettled.toString()})(document, innerHeight)`);
      const reference = resolve(output, `pi-${theme}-${width}.png`);
      browser('screenshot', reference);
      const screenshots = [];
      for (const language of ['zh', 'en']) {
        browser('open', origin);
        evaluate(`localStorage.setItem('gold-band.site.appearance.v1',JSON.stringify({version:1,theme:'${theme}'}))`);
        browser('open', `${origin}/${language}/?reference=${theme}-${width}`);
        browser('wait', '--fn', `document.documentElement.dataset.siteTheme === '${theme}' && document.querySelector('.poster')?.naturalWidth > 0 && !document.querySelector('.media-status')`);
        evaluate(`document.fonts.ready.then(() => true)`);
        const image = resolve(output, `gold-band-${language}-${theme}-${width}.png`);
        browser('screenshot', image);
        screenshots.push({ language, image, ...evaluate(`({ width: innerWidth, height: innerHeight, scrollY, overflow: document.documentElement.scrollWidth > innerWidth })`) });
      }
      report.push({ reference, theme, width, height, date: new Date().toISOString(), screenshots, visualReview: 'pending' });
      writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
      console.log(JSON.stringify({ theme, width }));
    }
  }
} finally { browser('close'); }
