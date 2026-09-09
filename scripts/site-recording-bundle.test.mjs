import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { bundleRecording } from './site-recording-bundle.mjs';
import { publishSceneAssets } from './site-publish-assets.mjs';

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
test('rebases an existing structured bundle without rewriting prose or losing transitive resources', async () => {
  const root = await mkdtemp(join(process.env.RECORDING_ATTACHMENTS || tmpdir(), 'rebase-test-'));
  try {
    const sourceRoot = join(root, 'source');
    await mkdir(join(sourceRoot, 'resources'), { recursive: true });
    await writeFile(join(sourceRoot, 'resources/main.css'), '@font-face{src:url(font.woff2)}');
    await writeFile(join(sourceRoot, 'resources/font.woff2'), 'font');
    const input = recording();
    input.events[0].data.node.attributes.href = '/media/workflow/en-dark/resources/main.css';
    input.events[1].data.payload = { text: '/media/workflow/en-dark/resources/main.css' };
    const result = await bundleRecording({ recording: input, sourceRoot, sourcePublicPath: '/media/workflow/en-dark/', outputRoot: join(root, 'output'), publicPath: '/product/media/workflow/en-dark/', captureOrigin: 'https://static.invalid/' });
    assert.equal(result.recording.events[1].data.payload.text, input.events[1].data.payload.text);
    assert.equal(result.resources.length, 2);
    assert.ok(result.resources.every(item => item.url.startsWith('/product/')));
    assert.equal(result.metadata.byteLength, Buffer.byteLength(JSON.stringify(result.recording)));
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('publishes deployment metadata and posters together while preserving canonical input bytes', async () => {
  const root = await mkdtemp(join(process.env.RECORDING_ATTACHMENTS || tmpdir(), 'publish-test-'));
  try {
    const sourceRoot = join(root, 'source'), variant = join(sourceRoot, 'en-dark');
    await mkdir(variant, { recursive: true });
    const input = recording();
    input.events[0].data.node.attributes.href = '/media/workflow/en-dark/main.css';
    await writeFile(join(variant, 'main.css'), '.readable{color:red}');
    const original = JSON.stringify(input);
    await writeFile(join(variant, 'events.json'), original);
    await writeFile(join(variant, 'poster.png'), 'poster');
    const asset = { eventsUrl: '/media/workflow/en-dark/events.json', poster: '/media/workflow/en-dark/poster.png', checkpoints: [{ stepId: 'one', poster: '/media/workflow/en-dark/poster.png' }] };
    await writeFile(join(sourceRoot, 'manifest.json'), JSON.stringify({ assets: [asset] }));
    const outputRoot = join(root, 'output');
    await publishSceneAssets({ sourceRoot, outputRoot, directory: 'workflow', base: '/product/' });
    const published = JSON.parse(await readFile(join(outputRoot, 'manifest.json'), 'utf8')).assets[0];
    const bytes = await readFile(join(outputRoot, 'en-dark/events.json'));
    assert.equal(published.sha256, createHash('sha256').update(bytes).digest('hex'));
    assert.equal(published.byteLength, bytes.length);
    assert.equal(published.poster, '/product/media/workflow/en-dark/poster.png');
    assert.equal(published.checkpoints[0].poster, published.poster);
    assert.deepEqual(JSON.parse(await readFile(join(outputRoot, 'en-dark/asset.json'), 'utf8')), published);
    assert.equal(await readFile(join(variant, 'events.json'), 'utf8'), original);
  } finally { await rm(root, { recursive: true, force: true }); }
});
