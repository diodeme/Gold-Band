import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createDirectoryVerifier, indexArchiveSourceVersions, verifyArchiveFileReferences } from './verify-demo-session.mjs';

test('verifies directory graph references once and rejects cycles, conflicts and missing resources', async () => {
  const root = `assets/${'b'.repeat(64)}.json`;
  const child = `assets/${'c'.repeat(64)}.json`;
  const { resources } = fixture();
  const file = { name: 'report.md', kind: 'file', resource: [...resources.values()][0], image: null };
  const directory = { name: 'attachments', kind: 'directory', directory: child, hasChildren: true };
  const listings = new Map([[root, { entries: [directory] }], [child, { entries: [file] }]]);
  let reads = 0;
  const load = async path => { reads++; assert.ok(listings.has(path)); return listings.get(path); };
  const verify = createDirectoryVerifier(load, resources);
  assert.equal(await verify(root), true);
  await verify(root);
  await verify(child);
  assert.equal(reads, 2);
  await assert.rejects(verify(undefined), /directory-reference-invalid/);
  await assert.rejects(createDirectoryVerifier(load, new Map())(root), /directory-file-reference-invalid/);
  listings.get(child).entries = [{ ...directory, directory: root }];
  await assert.rejects(createDirectoryVerifier(load, resources)(root), /directory-cycle/);
  listings.get(child).entries = [file, { ...file, name: 'REPORT.md' }];
  await assert.rejects(createDirectoryVerifier(load, resources)(root), /directory-name-conflict/);
  listings.get(child).entries = [];
  await assert.rejects(createDirectoryVerifier(load, resources)(root), /directory-children-invalid/);
});

function fixture() {
  const resource = { path: `resources/${'a'.repeat(64)}`, sha256: 'a'.repeat(64), bytes: 20, sourceBytes: 20 };
  const data = { changeSet: { id: 'set', branchId: 'root', changes: [{ id: 'change', beforeVersion: null,
    afterVersion: { contentHash: 'source-hash', byteLength: 20 } }], attachments: [{ id: 'attachment' }] },
    versions: { 'source-hash': resource }, attachments: { attachment: resource } };
  return { data, resources: new Map([[resource.path, resource]]), sourceVersions: new Map([['source-hash', resource]]) };
}

test('verifies file references against the hashed resource manifest', () => {
  const { data, resources, sourceVersions } = fixture();
  verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions);
  assert.throws(() => verifyArchiveFileReferences('other', data, new Set(['root']), resources, sourceVersions), /change-set-identity/);
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['other']), resources, sourceVersions), /change-set-identity/);
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), new Map(), sourceVersions), /file-reference-invalid/);
  data.changeSet.changes[0].afterVersion.byteLength++;
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-version-size/);
});

test('rejects missing attachments and duplicate identities', () => {
  const { data, resources, sourceVersions } = fixture();
  data.changeSet.attachments.push({ id: 'attachment' });
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /attachment-duplicate/);
  data.changeSet.attachments.pop();
  delete data.attachments.attachment;
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-reference-invalid/);
});

test('checks captured version size against source bytes after public redaction changes the payload size', () => {
  const { data, resources, sourceVersions } = fixture();
  const originalVersion = { contentHash: 'source-hash', byteLength: 43 };
  data.changeSet.changes[0].afterVersion = originalVersion;
  resources.values().next().value.sourceBytes = 43;
  verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions);
  assert.deepEqual(data.changeSet.changes[0].afterVersion, { contentHash: 'source-hash', byteLength: 43 });
  resources.values().next().value.sourceBytes = 44;
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-version-size/);
  resources.values().next().value.sourceBytes = 43;
  data.versions['source-hash'] = { ...data.versions['source-hash'], bytes: 21 };
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-reference-invalid/);
});

test('keeps distinct source versions valid when redaction deduplicates their published payload', () => {
  const { data, resources } = fixture();
  const published = resources.values().next().value;
  published.sourceBytes = 43;
  const sourceVersions = new Map([
    ['first-source', { ...published, sourceBytes: 43 }],
    ['second-source', { ...published, sourceBytes: 64 }],
  ]);
  data.changeSet.changes[0].afterVersion = { contentHash: 'second-source', byteLength: 64 };
  data.versions = { 'second-source': published };
  verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions);
  data.changeSet.changes[0].afterVersion.byteLength = 43;
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-version-size/);
  data.changeSet.changes[0].afterVersion.byteLength = 64;
  sourceVersions.delete('second-source');
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-version-source/);
  sourceVersions.set('second-source', { ...published, path: `resources/${'b'.repeat(64)}`, sourceBytes: 64 });
  assert.throws(() => verifyArchiveFileReferences('set', data, new Set(['root']), resources, sourceVersions), /file-version-source/);
});

test('indexes original content hashes independently of deduplicated public resources', () => {
  const first = 'a'.repeat(64);
  const second = 'b'.repeat(64);
  const resource = { source: `attempt/acp.file-blobs/aa/${first}`, sourceSha256: 'c'.repeat(64), sourceBytes: 43,
    path: `resources/${'d'.repeat(64)}`, sha256: 'd'.repeat(64), bytes: 20 };
  const other = { ...resource, source: `attempt/acp.file-blobs/bb/${second}`, sourceSha256: 'e'.repeat(64), sourceBytes: 64 };
  const indexed = indexArchiveSourceVersions([resource, other, { ...resource, source: `other/acp.file-blobs/aa/${first}` }]);
  assert.equal(indexed.size, 2);
  assert.equal(indexed.get(first).sourceBytes, 43);
  assert.equal(indexed.get(second).sourceBytes, 64);
  assert.throws(() => indexArchiveSourceVersions([resource, { ...resource, sourceBytes: 44 }]), /source-version-conflict/);
  assert.throws(() => indexArchiveSourceVersions([{ ...resource, source: `attempt/acp.file-blobs/bb/${first}` }]), /source-version-path/);
});
