import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prepareArchiveDirectories } from './prepare-demo-directories.mjs';

test('propagates image resource boundary and I/O failures', async () => {
  const resource = { source: 'attempt/image.png', path: `resources/${'a'.repeat(64)}`, sha256: 'a'.repeat(64), bytes: 12 };
  for (const message of ['archive.resource-outside-root', 'EACCES']) {
    await assert.rejects(prepareArchiveDirectories([resource], async () => 'asset', async () => { throw new Error(message); }), { message });
  }
});

test('emits one immediate listing per source directory with independent file resources', async () => {
  const resource = (source, digit) => ({ source, path: `resources/${digit.repeat(64)}`, sha256: digit.repeat(64), bytes: 12 });
  const assets = new Map();
  const references = await prepareArchiveDirectories([
    resource('attempt/attachments/nested/report.md', 'a'), resource('attempt/acp.raw.jsonl', 'b'), resource('other/acp.raw.jsonl', 'c'),
  ], async value => { const id = `asset-${assets.size}`; assets.set(id, value); return id; }, () => { throw new Error('Text metadata must not load bodies'); });
  const listing = assets.get(references.get('attempt'));
  assert.deepEqual(listing.entries.map(entry => entry.name), ['attachments', 'acp.raw.jsonl']);
  assert.equal(listing.entries[0].directory, references.get('attempt/attachments'));
  assert.equal(listing.entries[1].resource.sha256, 'b'.repeat(64));
  assert.equal(JSON.stringify(listing).includes('report.md'), false);
  assert.equal(assets.get(references.get('other')).entries[0].resource.sha256, 'c'.repeat(64));
  await assert.rejects(prepareArchiveDirectories([resource('../outside', 'a')], async () => '', async () => {}), /source-invalid/);
  await assert.rejects(prepareArchiveDirectories([resource('a', 'a'), resource('a/b', 'b')], async () => '', async () => {}), /source-conflict/);
});
