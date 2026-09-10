import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareArchiveChangeSet } from './prepare-demo-files.mjs';
import { capturedBlobHash, hash } from './export-demo-session.mjs';

test('maps immutable file versions and attachments without copying bodies into the file list', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gold-band-file-index-'));
  try {
    const resources = new Map();
    async function add(source, body) {
      const bytes = Buffer.from(body);
      const digest = hash(bytes);
      const absolutePath = join(root, digest);
      await writeFile(absolutePath, bytes);
      resources.set(source, { absolutePath, path: `resources/${digest}`, bytes: bytes.length, sourceBytes: bytes.length, sha256: digest });
    }
    const prefix = 'rounds/r/nodes/n/a';
    const content = '原始文件内容\n';
    const digest = capturedBlobHash(Buffer.from(content));
    const set = { id: 'set', changes: [{ beforeVersion: { contentHash: digest, byteLength: Buffer.byteLength(content) }, afterVersion: null }],
      attachments: [{ id: 'report', relativePath: 'report.md' }, { id: 'missing', relativePath: 'missing.txt' }, { id: 'image', relativePath: 'image.png' }] };
    const source = `${prefix}/turn-file-change-sets/set.json`;
    await add(source, JSON.stringify(set));
    await add(`${prefix}/acp.file-blobs/${digest.slice(0, 2)}/${digest}`, content);
    await add(`${prefix}/attachments/report.md`, '# 原始报告\r\n');
    await add(`${prefix}/attachments/image.png`, Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jA1kAAAAASUVORK5CYII=', 'base64'));
    const lookup = async source => { const entry = resources.get(source); if (!entry) throw new Error('archive.required-resource-missing'); return entry; };
    const result = await prepareArchiveChangeSet(source, lookup);
    assert.deepEqual(result.changeSet, set);
    assert.equal(result.versions[digest].path, `resources/${hash(Buffer.from(content))}`);
    assert.equal('content' in result.versions[digest], false);
    assert.equal(result.attachments.report.kind, 'text');
    assert.equal(result.attachments.report.lineEnding, 'crlf');
    assert.equal(result.attachments.image.kind, 'image');
    assert.equal(result.attachments.image.width, 1);
    assert.deepEqual(result.missing, [{ path: `${prefix}/attachments/missing.txt`, reason: 'attachment-missing' }]);
    set.attachments[0].relativePath = '../outside.txt';
    await add(source, JSON.stringify(set));
    await assert.rejects(prepareArchiveChangeSet(source, lookup), /archive.attachment-path-invalid/);
  } finally { await rm(root, { recursive: true, force: true }); }
});
