import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function summarizeMotion(samples, expected) {
  assert(samples.length > 1, 'Motion samples are missing');
  const checkpoints = samples.filter((sample, index) => !index || sample.checkpoint !== samples[index - 1].checkpoint)
    .map(sample => sample.checkpoint);
  const first = checkpoints.indexOf(expected[0]);
  assert(first >= 0, 'First semantic checkpoint was not observed');
  assert.deepEqual(checkpoints.slice(first, first + expected.length), expected, 'Continuous playback skipped or reordered a checkpoint');
  assert(checkpoints.length > first + expected.length, 'Loop restart was not observed');
  assert(samples.every(sample => sample.players === 1 && !sample.overflow && sample.textLength > 50), 'Blank, duplicate or overflowing replay');
  assert(samples.some(sample => sample.camera === 'moving'), 'No continuous camera transition was observed');
  assert(samples.some(sample => sample.camera === 'stable'), 'No stable reading interval was observed');
  const transforms = new Set(samples.map(sample => sample.transform));
  assert(transforms.size > 5, 'Camera did not move through intermediate frames');
  return { checkpoints, transforms: transforms.size, samples: samples.length, durationMs: samples.at(-1).at - samples[0].at };
}

async function main() {
  const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
  const session = `gold-band-motion-${process.pid}`;
  const origin = process.env.SITE_URL || 'http://127.0.0.1:1461';
  const language = process.env.SITE_LANGUAGE || 'zh';
  const theme = process.env.SITE_THEME || 'dark';
  const width = Number(process.env.SITE_WIDTH || 1280);
  assert(['zh', 'en'].includes(language) && ['light', 'dark'].includes(theme));
  const output = resolve(process.env.SITE_MOTION_OUTPUT || `.codex-temp/site-motion-${language}-${theme}-${width}.json`);
  mkdirSync(resolve('.codex-temp'), { recursive: true });
  const browser = (...args) => {
    const responsePath = `${output}.browser.json`;
    const descriptor = openSync(responsePath, 'w');
    try {
      execFileSync(executable, ['--session', session, '--json', ...args], {
        stdio: ['ignore', descriptor, 'inherit'], timeout: 45_000, windowsHide: true,
      });
    } finally { closeSync(descriptor); }
    const result = JSON.parse(readFileSync(responsePath, 'utf8'));
    assert(result.success, JSON.stringify(result));
    return result.data;
  };
  const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
  const report = { version: 1, language, theme, width, origin, cases: [], visualReview: 'pending', throttling: 'none', instrumentation: 'RAF timestamps; DOM sample every 100ms; no recording or screenshots during measurement' };
  try {
    browser('open', origin);
    browser('set', 'viewport', String(width), '900');
    evaluate(`localStorage.setItem('gold-band.site.appearance.v1',JSON.stringify({version:1,theme:${JSON.stringify(theme)}}))`);
    for (const chapter of ['before', 'during', 'after', 'personalize']) {
      const bytes = readFileSync(`marketing/site/media/${language}-${chapter}${theme === 'light' ? '-light' : ''}.json`);
      const recording = JSON.parse(bytes);
      const expected = recording.events.filter(e => e.type === 5 && e.data.tag === 'site-shot').map(e => e.data.payload.id);
      const sourceSha256 = createHash('sha256').update(bytes).digest('hex');
      browser('open', `${origin}/${language}/?motion=${process.pid}#${chapter}`);
      browser('wait', '--fn', `document.querySelector('[data-chapter-media="${chapter}"] [data-player-state="ready"]') !== null`);
      const asset = evaluate(`performance.getEntriesByType('resource').map(e=>e.name).find(n=>n.includes('/${language}-${chapter}-')&&n.endsWith('.json'))`);
      assert(asset);
      const response = await fetch(asset);
      assert(response.ok);
      assert.equal(createHash('sha256').update(Buffer.from(await response.arrayBuffer())).digest('hex'), sourceSha256);
      evaluate(`(() => {
        const state = window.__motion = {samples:[],frames:[],longTasks:[],done:false};
        let last=performance.now(), sampled=0, previous='', textLength=0;
        const observer=new PerformanceObserver(list=>state.longTasks.push(...list.getEntries().map(e=>({at:e.startTime,duration:e.duration}))));
        observer.observe({type:'longtask'});
        const tick=now=>{
          if(state.done){observer.disconnect();return;}
          state.frames.push(now-last);last=now;
          if(now-sampled>=100){
            sampled=now; const plane=document.querySelector('.replayer-wrapper');
            const checkpoint=plane?.dataset.checkpoint;
            if(checkpoint!==previous){textLength=plane?.querySelector('iframe')?.contentDocument?.body.innerText.length??0;previous=checkpoint;}
            state.samples.push({at:now,checkpoint,camera:plane?.dataset.cameraState,transform:plane?.style.transform,
              textLength,players:document.querySelectorAll('.replayer-wrapper iframe').length,overflow:document.documentElement.scrollWidth>innerWidth});
          }
          requestAnimationFrame(tick);
        };requestAnimationFrame(tick);return true;
      })()`);
      const deadline = Date.now() + 90_000;
      let complete = false;
      while (Date.now() < deadline) {
        browser('wait', '1000');
        complete = evaluate(`(()=>{const s=window.__motion.samples;const ids=s.filter((v,i)=>!i||v.checkpoint!==s[i-1].checkpoint).map(v=>v.checkpoint);const start=ids.indexOf(${JSON.stringify(expected[0])});return start>=0&&ids.length>start+${expected.length};})()`);
        if (complete) break;
      }
      const measured = evaluate('(()=>{window.__motion.done=true;return window.__motion;})()');
      writeFileSync(output, JSON.stringify({ ...report, pending: { chapter, sourceSha256, expected, measured } }, null, 2));
      assert(complete, 'Continuous playback did not complete within 90 seconds');
      const summary = summarizeMotion(measured.samples, expected);
      const frames = measured.frames.slice(1).sort((a,b)=>a-b);
      report.cases.push({ chapter, sourceSha256, expected, ...summary,
        frameP95: frames[Math.floor(frames.length * .95)], frameMax: frames.at(-1), framesOver50ms: frames.filter(v=>v>50).length,
        longTasks: measured.longTasks, measured });
      writeFileSync(output, JSON.stringify(report, null, 2));
      console.log(JSON.stringify({ chapter, samples: summary.samples, frameP95: report.cases.at(-1).frameP95, longTasks: measured.longTasks.length }));
    }
  } finally { browser('close'); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
