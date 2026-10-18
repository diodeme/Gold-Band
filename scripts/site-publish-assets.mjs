import { cp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { bundleRecording } from './site-recording-bundle.mjs';

// Canonical recordings remain byte-identical; only non-root deployment output is derived.
export async function publishSceneAssets({ sourceRoot, outputRoot, directory, base }) {
  if (base === '/') { await cp(sourceRoot, outputRoot, { recursive: true }); return; }
  const manifest = JSON.parse(await readFile(resolve(sourceRoot, 'manifest.json'), 'utf8'));
  for (const asset of [...manifest.assets, ...(manifest.mobileAssets ?? [])]) {
    const originalPath = new URL('.', `https://static.invalid${asset.eventsUrl}`).pathname;
    const variant = basename(originalPath);
    const source = resolve(sourceRoot, variant);
    const destination = resolve(outputRoot, variant);
    const publicPath = `${base}media/${directory}/${variant}/`;
    const recording = JSON.parse(await readFile(resolve(source, 'events.json'), 'utf8'));
    const bundled = await bundleRecording({ recording, captureOrigin: 'https://static.invalid/', sourceRoot: source, sourcePublicPath: originalPath, outputRoot: destination, publicPath });
    const posters = new Set([asset.poster, ...asset.checkpoints.map(point => point.poster)]);
    for (const poster of posters) {
      if (!poster.startsWith(originalPath) || basename(poster) !== poster.slice(originalPath.length)) throw { code: 'site.resource-path' };
      await cp(resolve(source, basename(poster)), resolve(destination, basename(poster)));
    }
    Object.assign(asset, bundled.metadata, { poster: `${publicPath}${basename(asset.poster)}` });
    asset.checkpoints = asset.checkpoints.map(point => ({ ...point, poster: `${publicPath}${basename(point.poster)}` }));
    await writeFile(resolve(destination, 'asset.json'), JSON.stringify(asset, null, 2));
  }
  await mkdir(outputRoot, { recursive: true });
  await writeFile(resolve(outputRoot, 'manifest.json'), JSON.stringify(manifest, null, 2));
}
