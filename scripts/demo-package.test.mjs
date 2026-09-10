import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { copyDataset } from '../marketing/demo/build-dataset.mjs';

test('publication copies original task and workflow along with the dataset', () => {
  const source = mkdtempSync(join(tmpdir(), 'demo-publish-'));
  const destination = join(source, 'output');
  try {
    const files = ['task.json', 'workflow.json', 'dataset.json'].map(path => {
      const bytes = Buffer.from(JSON.stringify({ id: path, title: 'original task' }));
      writeFileSync(join(source, path), bytes);
      return { path, byteLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
    });
    writeFileSync(join(source, 'publish-index.json'), JSON.stringify({ files }));
    copyDataset(source, destination);
    for (const { path } of files) assert.deepEqual(readFileSync(join(destination, path)), readFileSync(join(source, path)));
  } finally { rmSync(source, { recursive: true, force: true }); }
});
