import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkDataset, copyDataset } from '../marketing/demo/build-dataset.mjs';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';

test('production rejects an absent real history dataset', () => {
  assert.throws(() => checkDataset('missing-demo-dataset'), /dataset is required/);
});
test('an incomplete exported candidate cannot pass the production gate', { skip: !process.env.DEMO_DATASET_PATH }, () => {
  assert.throws(() => checkDataset(process.env.DEMO_DATASET_PATH), /missing references/);
  assert.doesNotThrow(() => checkDataset(process.env.DEMO_DATASET_PATH, true));
});

test('publication copies only verified inventory entries and rejects traversal', () => {
  const root = mkdtempSync(join(tmpdir(), 'demo-publish-'));
  try {
    const source = join(root, 'source'); const destination = join(root, 'dist');
    mkdirSync(source); writeFileSync(join(source, 'current.json'), '{}'); writeFileSync(join(source, 'stale.json'), 'private-old-candidate');
    writeFileSync(join(source, 'publish-index.json'), JSON.stringify({ files: [{ path: 'current.json', byteLength: 2, sha256: createHash('sha256').update('{}').digest('hex') }] }));
    copyDataset(source, destination);
    assert.equal(existsSync(join(destination, 'current.json')), true);
    assert.equal(existsSync(join(destination, 'stale.json')), false);
    writeFileSync(join(source, 'current.json'), '{"changed":true}');
    assert.throws(() => copyDataset(source, destination), /integrity/);
    writeFileSync(join(source, 'publish-index.json'), JSON.stringify({ files: [{ path: '../outside.json' }] }));
    assert.throws(() => copyDataset(source, destination), /Unsafe/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
