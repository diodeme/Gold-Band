import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const executable = process.env.AGENT_BROWSER_BIN || 'agent-browser';
const session = `gold-band-checkpoints-${process.pid}`;
const origin = process.env.SITE_URL || 'http://127.0.0.1:1463';
const output = resolve('marketing/site/media/checkpoints');
const temporary = resolve('.codex-temp/site-checkpoints');
mkdirSync(output, { recursive: true }); mkdirSync(temporary, { recursive: true });
const browser = (...args) => {
  const response = resolve(temporary, 'response.json');
  const file = openSync(response, 'w');
  try { execFileSync(executable, ['--session', session, '--json', ...args], { stdio: ['ignore', file, 'inherit'], windowsHide: true, timeout: 45000 }); }
  finally { closeSync(file); }
  const result = JSON.parse(readFileSync(response, 'utf8'));
  assert(result.success, JSON.stringify(result)); return result.data;
};
const evaluate = code => browser('eval', '-b', Buffer.from(code).toString('base64')).result;
const manifestPath = resolve(output, 'index.json');
let manifest = {};
const timings = [];
try { manifest = JSON.parse(readFileSync(manifestPath, 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
try {
  browser('open', `${origin}/checkpoints.html`);
  browser('wait', '--fn', 'Boolean(window.checkpointCapture)');
  for (const theme of (process.env.SITE_THEMES?.split(',') || ['dark', 'light'])) for (const language of ['zh', 'en']) for (const chapter of (process.env.SITE_SCENES?.split(',') || ['before', 'during', 'after', 'personalize'])) {
    const name = `${language}-${chapter}${theme === 'light' ? '-light' : ''}`;
    if (chapter === 'personalize') {
      evaluate(`window.checkpointCapture.load(${JSON.stringify(name)}).then(() => window.checkpointCapture.seek('closing'))`);
      const text = evaluate(`document.querySelector('iframe').contentDocument.body.innerText`);
      const requirement = language === 'en' ? 'Keep per-turn file changes available for review.' : '保留每轮文件变更，方便审阅。';
      if (!text.includes(requirement)) {
        browser('screenshot', resolve(temporary, `${name}-seek-failure.png`));
        assert.fail(`${name}: seeking to closing removed the conversation`);
      }
    }
    const ids = evaluate(`window.checkpointCapture.load(${JSON.stringify(name)})`);
    const entries = {};
    for (const id of ids) {
      const { elapsedMs, ...entry } = evaluate(`window.checkpointCapture.seek(${JSON.stringify(id)})`);
      timings.push({ name, id, elapsedMs });
      browser('set', 'viewport', String(entry.width), String(entry.height));
      browser('screenshot', '', resolve(output, `${name}-${id}.png`));
      entries[id] = entry;
    }
    manifest[name] = entries;
    writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
    console.log(JSON.stringify({ name, images: ids.length }));
  }
  writeFileSync(resolve(temporary, 'timings.json'), JSON.stringify(timings, null, 2));
} finally { browser('close'); }
