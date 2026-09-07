import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = 'gold-band-rrweb-verification';
const url = process.env.RRWEB_DEMO_URL || 'http://127.0.0.1:1438/rrweb-demo/';
const output = resolve('.codex-temp/rrweb-verification');
mkdirSync(output, { recursive: true });
function browser(...args) {
  // A newly started browser daemon can inherit pipes on Windows; use a file sink.
  const responsePath = resolve(output, 'browser-response.json');
  const response = openSync(responsePath, 'w');
  try {
    execFileSync(executable, ['--session', session, '--json', ...args], {
      stdio: ['ignore', response, 'inherit'], timeout: 45_000,
    });
  } finally { closeSync(response); }
  const result = JSON.parse(readFileSync(responsePath, 'utf8'));
  assert.equal(result.success, true, JSON.stringify(result));
  return result.data;
}
function evaluate(code) { return browser('eval', '-b', Buffer.from(code).toString('base64')).result; }
const waitFor = (expression) => browser('wait', '--fn', expression);
const clickText = (text) => evaluate(`Array.from(document.querySelectorAll('button')).find(e=>e.textContent.trim()===${JSON.stringify(text)}).click()`);
const facts = `(frame) => {
  const d = frame.contentDocument;
  return {
    width: frame.contentWindow.innerWidth,
    panels: ['workspace-navigation','workspace-center','workspace-right'].map(id => Math.round(d.getElementById(id)?.getBoundingClientRect().width || 0)),
    right: Boolean(d.querySelector('[data-right-workspace-dock]')),
    note: d.querySelector('[data-right-workspace-dock]')?.textContent.includes('Workspace notes') || false,
    background: frame.contentWindow.getComputedStyle(d.body).backgroundColor,
    scheme: d.documentElement.dataset.colorScheme,
  };
}`;
const report = [];
try {
  browser('open', url);
  browser('set', 'viewport', '1600', '1100');
  waitFor(`document.querySelector('[data-demo-phase]')?.dataset.demoPhase === 'ready'`);
  for (const theme of ['light', 'dark']) {
    if (theme === 'dark') {
      evaluate(`document.querySelector('button[aria-label="深色"]').click()`);
      waitFor(`document.querySelector('[data-demo-phase]')?.dataset.demoPhase === 'ready'`);
    }
    evaluate(`(() => {
      window.__facts = ${facts};
      window.__sourceSamples = [];
      window.__sampleTimer = setInterval(() => {
        const frame = document.querySelector('.capture-plane iframe');
        if (frame?.contentDocument.getElementById('workspace-center')) window.__sourceSamples.push({ at:Date.now(), ...window.__facts(frame) });
      }, 100);
    })()`);
    clickText('录制布局演示');
    waitFor(`document.querySelector('[data-demo-phase]').dataset.demoPhase === 'replay' || document.querySelector('[data-demo-phase]').dataset.demoPhase === 'error'`);
    assert.equal(evaluate(`document.querySelector('[data-demo-phase]').dataset.demoPhase`), 'replay');
    evaluate(`clearInterval(window.__sampleTimer)`);
    const sourceSamples = evaluate('window.__sourceSamples');
    evaluate(`(() => {
      const original = URL.createObjectURL;
      window.__recording = null;
      URL.createObjectURL = function(blob) { blob.text().then(text => window.__recording = JSON.parse(text)); return original.call(this,blob); };
      const click = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = function() {};
      Array.from(document.querySelectorAll('button')).find(e=>e.textContent.trim()==='导出 JSON').click();
      URL.createObjectURL = original;
      HTMLAnchorElement.prototype.click = click;
    })()`);
    waitFor('Boolean(window.__recording)');
    const recording = evaluate('window.__recording');
    writeFileSync(resolve(output, `${theme}-recording.json`), JSON.stringify(recording));
    const events = recording.events;
    const start = events[0].timestamp;
    const end = events.at(-1).timestamp;
    const chapters = events.filter(e => e.type === 5 && e.data.tag === 'chapter');
    assert.equal(chapters.length, 5);
    assert(events.some(e => e.type === 3 && e.data.source === 4 && e.data.width === 600));
    // End playback before seeking so every comparison uses a stationary frame.
    waitFor(`document.querySelector('.rr-progress__step')?.style.width === '100%'`);
    const comparisons = [];
    for (let index = 0; index < chapters.length; index++) {
      const at = chapters[index].timestamp + 1700;
      const source = sourceSamples.reduce((best, item) => Math.abs(item.at - at) < Math.abs(best.at - at) ? item : best);
      const fraction = (at - start) / (end - start);
      evaluate(`(() => { const e=document.querySelector('.rr-progress'); const r=e.getBoundingClientRect(); e.dispatchEvent(new MouseEvent('click',{bubbles:true,clientX:r.left+r.width*${fraction},clientY:r.top+2})); })()`);
      waitFor(`document.querySelector('.replayer-wrapper iframe').contentWindow.innerWidth === ${source.width}`);
      const replay = evaluate(`window.__facts(document.querySelector('.replayer-wrapper iframe'))`);
      assert.equal(replay.width, source.width);
      assert.equal(replay.right, source.right);
      assert.equal(replay.note, source.note);
      assert.equal(replay.scheme, theme);
      assert.equal(replay.background, source.background);
      for (let i = 0; i < 3; i++) assert(Math.abs(replay.panels[i] - source.panels[i]) <= 2, JSON.stringify({ source, replay }));
      const screenshot = browser('screenshot');
      comparisons.push({ chapter: chapters[index].data.payload.name, source, replay, screenshot });
    }
    const counts = comparisons.map(({ replay }) => replay.panels.filter(width => width > 10).length);
    assert.deepEqual(counts, [3, 2, 1, 2, 3]);
    assert.equal(evaluate(`document.querySelectorAll('.capture-plane iframe').length`), 0);
    report.push({ theme, bytes: recording.bytes, events: events.length, durationMs: end-start, counts, comparisons });
    console.log(`${theme}: ${counts.join(' -> ')}, ${recording.bytes} bytes, ${events.length} events`);
  }
  browser('set', 'viewport', '390', '844');
  waitFor(`document.querySelector('.rr-player').getBoundingClientRect().width < 400`);
  assert(evaluate('document.documentElement.scrollWidth <= innerWidth'));
  report.push({ mobile: { width:390, height:844, screenshot: browser('screenshot') } });
  browser('set', 'viewport', '1600', '1100');
  waitFor(`document.querySelector('.rr-player').getBoundingClientRect().width > 1000`);
  writeFileSync(resolve(output, 'report.json'), JSON.stringify(report, null, 2));
  console.log(`Verified production replay; report: ${output}`);
} finally {
  browser('close');
}
