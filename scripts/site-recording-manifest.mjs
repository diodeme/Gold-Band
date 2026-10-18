import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';
import { SceneAssetSchema, SceneManifestSchema } from '../marketing/site/replay-model.ts';

export async function publishSceneManifest(directory, scene) {
  const assets = [], mobileAssets = [];
  for (const mobile of [false, true]) for (const language of ['zh', 'en']) for (const theme of ['dark', 'light']) {
    let text;
    try { text = await readFile(resolve(directory, `${language}-${theme}${mobile ? '-mobile' : ''}/asset.json`), 'utf8'); }
    catch (error) { if (error.code === 'ENOENT') continue; throw error; }
    const asset = SceneAssetSchema.parse(JSON.parse(text));
    assert.equal(asset.scene, scene);
    assert.equal(asset.language, language);
    assert.equal(asset.theme, theme);
    (mobile ? mobileAssets : assets).push(asset);
  }
  const manifest = SceneManifestSchema.parse({ version: 1, assets, ...(mobileAssets.length ? { mobileAssets } : {}) });
  await writeFile(resolve(directory, 'manifest.json'), JSON.stringify(manifest, null, 2));
  return manifest;
}
