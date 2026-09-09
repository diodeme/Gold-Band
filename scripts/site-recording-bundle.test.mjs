import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { bundleRecording } from './site-recording-bundle.mjs';

const recording = () => ({ version: 1, bytes: 0, reason: 'manual', events: [
  { type: 2, timestamp: 1000, data: { node: { id: 1, type: 2, tagName: 'link', attributes: { rel: 'stylesheet', href: '/main.css' }, childNodes: [] } } },
  { type: 5, timestamp: 2000, data: { tag: 'recording-end', payload: {} } },
] });
test('writes and hashes the transitive CSS/SVG/font closure and rejects missing nested files', async () => {
  const root = await mkdtemp(join(process.env.RECORDING_ATTACHMENTS || tmpdir(), 'bundle-test-'));
  const sourceRoot = join(root, 'source');
  const outputRoot = join(root, 'output');
  await mkdir(sourceRoot);
  try {
    await writeFile(join(sourceRoot, 'main.css'), '@import "nested.css";.a{background:url(icon.svg)}');
    await writeFile(join(sourceRoot, 'nested.css'), '@import "main.css";@font-face{src:url(font.woff2)}');
    await writeFile(join(sourceRoot, 'icon.svg'), '<svg xmlns="http://www.w3.org/2000/svg"><image href="pixel.png"/></svg>');
    await writeFile(join(sourceRoot, 'font.woff2'), 'font');
    const options = { recording: recording(), sourceRoot, outputRoot, publicPath: '/media/pilot/', captureOrigin: 'http://localhost:1443/' };
    await assert.rejects(bundleRecording(options), error => error.code === 'site.resource-missing' && error.params.path === 'pixel.png');
    await writeFile(join(sourceRoot, 'pixel.png'), 'pixel');
    const result = await bundleRecording(options);
    assert.equal(result.resources.length, 5);
    for (const resource of result.resources) {
      const bytes = await readFile(join(outputRoot, resource.url.slice('/media/pilot/'.length)));
      assert.equal(createHash('sha256').update(bytes).digest('hex'), resource.sha256);
    }
    const bytes = await readFile(join(outputRoot, 'events.json'));
    assert.equal(bytes.length, result.metadata.byteLength);
    assert.equal(JSON.parse(bytes).bytes, bytes.length);
    assert.match(await readFile(join(outputRoot, 'resources/nested.css'), 'utf8'), /\/media\/pilot\/resources\/font.woff2/);
    assert.match(await readFile(join(outputRoot, 'resources/icon.svg'), 'utf8'), /\/media\/pilot\/resources\/pixel.png/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('rejects truncated recordings before publishing', async () => {
  await assert.rejects(bundleRecording({ recording: { ...recording(), reason: 'duration' }, sourceRoot: '.', outputRoot: '.', publicPath: '/media/', captureOrigin: 'http://localhost/' }), error => error.code === 'site.recording-budget');
});
