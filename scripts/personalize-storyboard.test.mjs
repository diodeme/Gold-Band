import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { measurePersonalizeCamera, personalizeSourceStoryboard, PERSONALIZE_STEPS } from './personalize-source-storyboard.mjs';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { publishSceneManifest } from './site-recording-manifest.mjs';

test('UI driver scopes theme selection, preserves reading intent, and exercises actual responsive widths', () => {
  const commands = [], shots = [];
  let width = 320;
  personalizeSourceStoryboard({
    language: 'en', capture: false, screenshot: id => shots.push(id),
    browser(...args) { commands.push(args); if (args[0] === 'set') width = Number(args[2]); },
    evaluate(code) {
      if (code === '({width:innerWidth,height:innerHeight})') return { width: 320, height: 780 };
      if (code.includes('workspace-navigation')) return width === 1440 ? [250, 800, 390] : width === 900 ? [0, 550, 350] : [0, 600, 0];
      return null;
    },
  });
  assert.deepEqual(shots.filter(id => PERSONALIZE_STEPS.includes(id)), PERSONALIZE_STEPS);
  const focus = commands.findIndex(args => args[0] === 'focus');
  assert.deepEqual(commands[focus + 1], ['press', 'Control+Home']);
  assert(commands.some(args => args[0] === 'find' && args.includes('$ gold-band run workflow ready Gold Band') && args.includes('--exact')));
  assert(commands.some(args => args[0] === 'wait' && args[2]?.includes('dataset.theme === "builtin.gold-band"')));
});

test('targeted mobile publication retains existing primary assets and rejects misidentified variants', async () => {
  const directory = await mkdtemp(resolve(process.env.RECORDING_ATTACHMENTS || tmpdir(), 'pm-'));
  const primary = JSON.parse(await readFile('marketing/site/media/before/zh-dark/asset.json', 'utf8'));
  const mobile = JSON.parse(await readFile('marketing/site/media/before/zh-dark-mobile/asset.json', 'utf8'));
  try {
    for (const [variant, asset] of [['zh-dark', primary], ['zh-dark-mobile', mobile]]) {
      await mkdir(resolve(directory, variant));
      await writeFile(resolve(directory, variant, 'asset.json'), JSON.stringify(asset));
    }
    const manifest = await publishSceneManifest(directory, 'before');
    assert.deepEqual(manifest.assets, [primary]); assert.deepEqual(manifest.mobileAssets, [mobile]);
    await writeFile(resolve(directory, 'zh-dark-mobile/asset.json'), JSON.stringify({ ...mobile, language: 'en' }));
    await assert.rejects(publishSceneManifest(directory, 'before'));
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('clipped theme geometry stays inside the normalized recording viewport', () => {
  const dom = new JSDOM('<section><button data-slot="sheet-trigger"></button></section>');
  Object.assign(globalThis, { document: dom.window.document, innerWidth: 1440, innerHeight: 900 });
  document.querySelector('section').getBoundingClientRect = () => ({ x: 0.9888474358452692 * 1440 + 8, y: 0, right: 1450, height: 40 });
  try {
    const rect = measurePersonalizeCamera('theme').desktop;
    assert(rect.x + rect.width <= 1);
  } finally { dom.window.close(); delete globalThis.document; delete globalThis.innerWidth; delete globalThis.innerHeight; }
});
