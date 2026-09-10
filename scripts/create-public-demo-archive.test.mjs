import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { capturedBlobHash, exportDemoSession, hash } from './export-demo-session.mjs';
import { prepareDemoSession } from './prepare-demo-session.mjs';
import { verifyDemoSession } from './verify-demo-session.mjs';
import { createPublicDemoArchive } from './create-public-demo-archive.mjs';

const value = 'synthetic-launch-token-for-archive-review';
const reviewed = [{ id: 'launch-1', value }];
const load = async path => JSON.parse(await readFile(path, 'utf8'));
async function fixture({ sensitiveBinary = false, keyCollision = false, compressedPayload } = {}) {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-public-archive-test-'));
  const task = join(directory, 'projects', 'project-a', 'tasks', 'task-a');
  const run = join(task, 'runs', 'run-a');
  const round = join(run, 'rounds', 'round-a');
  const attempt = join(round, 'nodes', 'node-a', 'attempt-a');
  const text = `Original file: ${value}\r\n`;
  const originalHash = capturedBlobHash(Buffer.from(text));
  await mkdir(join(attempt, 'attachments'), { recursive: true });
  await mkdir(join(attempt, 'turn-file-change-sets'));
  await mkdir(join(attempt, 'acp.file-blobs', originalHash.slice(0, 2)), { recursive: true });
  const files = [
    [join(task, 'task.json'), { id: 'task-a', title: 'Original task' }],
    [join(run, 'run.json'), { id: 'run-a', status: 'paused', outcome: null, workflow_snapshot: 'workflow.snapshot.json' }],
    [join(run, 'workflow.snapshot.json'), {}],
    [join(round, 'round.json'), { id: 'round-a', index: 1, status: 'paused', outcome: null }],
    [join(attempt, 'node.json'), { id: 'node-a' }],
    [join(attempt, 'acp.snapshot.json'), { [value]: 'keyed evidence', ['__proto__']: { keep: true },
      ...(keyCollision ? { '[REDACTED:launch-1]': 'different evidence' } : {}) }],
    [join(attempt, 'worker-ref.json'), { provider: 'codex-acp', mode: 'acp', continueRef: { sessionId: 'source-session' } }],
    [join(attempt, 'turn-file-change-sets', 'set.json'), { id: 'set', branchId: 'root', changes: [{ id: 'change', beforeVersion: null,
      afterVersion: { contentHash: originalHash, byteLength: Buffer.byteLength(text) } }], attachments: [] }],
  ];
  for (const [path, data] of files) await writeFile(path, JSON.stringify(data));
  await writeFile(join(attempt, 'acp.raw.jsonl'), [1, 2, 3].map(seq => JSON.stringify({ seq, frame: { result: `raw ${seq}: ${value}` } })).join('\n') + '\n');
  await writeFile(join(attempt, 'acp.diagnostics.jsonl'), JSON.stringify({ level: 'error', message: `diagnostic ${value}`, timestamp: '2Z' }) + '\n');
  await writeFile(join(attempt, 'acp.timeline.jsonl'), JSON.stringify({ item: { id: 'tool-1', seq: 1, kind: 'toolCall', toolCallId: 'tool-1', raw: { rawOutput: `result ${value}` } } }) + '\n');
  await writeFile(join(attempt, 'acp.file-blobs', originalHash.slice(0, 2), originalHash), text);
  const binary = sensitiveBinary ? Buffer.concat([Buffer.from([0, 255]), Buffer.from(value)]) : Buffer.from([0, 255, 1, 2]);
  await writeFile(join(attempt, 'attachments', 'binary.bin'), binary);
  if (compressedPayload) await writeFile(join(attempt, 'attachments', 'compressed.bin'), compressedPayload);
  const input = join(directory, 'private');
  await exportDemoSession(run, input);
  await prepareDemoSession(input);
  return { directory, input, output: join(directory, 'public'), originalHash, text, binary };
}

test('builds a verified public copy with new byte pages and immutable source version identity', async () => {
  const f = await fixture();
  try {
    const originalManifest = await readFile(join(f.input, 'manifest.json'));
    const source = JSON.parse(originalManifest);
    const result = await createPublicDemoArchive(f.input, f.output, reviewed);
    assert.equal(result.integrity, 'verified');
    assert.equal(result.publication, 'pending-disclosure-review');
    assert(result.replacements[0].count > 0);
    assert.deepEqual(await readFile(join(f.input, 'manifest.json')), originalManifest);
    const manifest = await load(join(f.output, 'manifest.json'));
    assert.deepEqual(manifest.task, source.task);
    assert.deepEqual(manifest.run, source.run);
    assert.equal(manifest.counts.items, source.counts.items);
    assert.equal(manifest.counts.records, source.counts.records);
    const detail = await load(join(f.output, manifest.sessions[0].detail));
    assert.equal(Object.hasOwn(detail.snapshot, '__proto__'), true);
    assert.deepEqual(detail.snapshot.__proto__, { keep: true });
    const beforeDetail = await load(join(f.input, source.sessions[0].detail));
    assert.equal(detail.rawFrames.count, 3);
    assert.notEqual(detail.rawFrames.bytes, beforeDetail.rawFrames.bytes);
    const raw = await readFile(join(f.output, detail.rawFrames.resource));
    let count = 0;
    for (const page of detail.rawFrames.pages) {
      const lines = raw.subarray(page.offset, page.offset + page.length).toString().trimEnd().split('\n');
      assert.equal(lines.length, page.count);
      for (const line of lines) { assert.match(JSON.parse(line).frame.result, /\[REDACTED:launch-1\]/); count++; }
    }
    assert.equal(count, 3);
    const changes = await load(join(f.output, detail.changeSets.set));
    assert.deepEqual(changes.changeSet.changes[0].afterVersion, { contentHash: f.originalHash, byteLength: Buffer.byteLength(f.text) });
    assert.notEqual(changes.versions[f.originalHash].bytes, Buffer.byteLength(f.text));
    const original = source.resources.find(item => item.source.endsWith(f.originalHash));
    assert.equal(await readFile(join(f.input, original.path), 'utf8'), f.text);
    const binary = manifest.resources.find(item => item.source.endsWith('/binary.bin'));
    assert.deepEqual(await readFile(join(f.output, binary.path)), f.binary);
    const tool = await load(join(f.output, detail.branches[0].tools['tool-1']));
    assert.equal(tool.id, 'tool-1');
    assert.equal(tool.seq, 1);
    assert.equal(tool.raw.rawOutput, 'result [REDACTED:launch-1]');
    for (const folder of ['resources', 'assets']) for (const name of await readdir(join(f.output, folder))) {
      assert.equal((await readFile(join(f.output, folder, name))).includes(Buffer.from(value)), false);
    }
    assert.equal((await readFile(join(f.output, 'disclosure-review.json'), 'utf8')).includes(value), false);
    assert.equal((await verifyDemoSession(f.output)).integrity, 'verified');
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed), /public-output-exists/);
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('rejects binary replacement without exposing an output catalog', async () => {
  const f = await fixture({ sensitiveBinary: true });
  try {
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed), /public-sensitive-binary/);
    await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('rejects changed input resources and invalid reviewed values', async () => {
  const f = await fixture();
  try {
    for (const values of [[], [{ id: 'bad', value: 'short' }], [reviewed[0], reviewed[0]]]) {
      await assert.rejects(createPublicDemoArchive(f.input, f.output, values), /reviewed-value/);
    }
    const manifest = await load(join(f.input, 'manifest.json'));
    const resource = manifest.resources[0];
    await writeFile(join(f.input, resource.path), 'changed');
    assert.notEqual(hash(Buffer.from('changed')), resource.sha256);
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed), /public-source-changed/);
    await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('rejects colliding redacted dictionary keys instead of discarding evidence', async () => {
  const f = await fixture({ keyCollision: true });
  try {
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed), /public-key-collision/);
    await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('rejects reviewed values hidden inside gzip bytes regardless of filename', async () => {
  const f = await fixture({ compressedPayload: gzipSync(Buffer.from('x'.repeat(16374) + value)) });
  try {
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed), /public-sensitive-compressed/);
    await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('preserves benign gzip bytes and rejects an incomplete compressed stream', async () => {
  const payload = gzipSync(Buffer.from('benign captured attachment'));
  const f = await fixture({ compressedPayload: payload });
  const broken = await fixture({ compressedPayload: payload.subarray(0, payload.length - 4) });
  try {
    await createPublicDemoArchive(f.input, f.output, reviewed);
    const manifest = await load(join(f.output, 'manifest.json'));
    const resource = manifest.resources.find(entry => entry.source.endsWith('/compressed.bin'));
    assert.deepEqual(await readFile(join(f.output, resource.path)), payload);
    await assert.rejects(createPublicDemoArchive(broken.input, broken.output, reviewed), /public-compressed-invalid/);
    await assert.rejects(readFile(join(broken.output, 'catalog.json')), { code: 'ENOENT' });
  } finally {
    await rm(f.directory, { recursive: true, force: true });
    await rm(broken.directory, { recursive: true, force: true });
  }
});

test('preserves exactly reviewed truncated source bytes and records their limitation', async () => {
  const content = Buffer.from('captured incomplete artifact');
  const payload = gzipSync(content).subarray(0, -4);
  const evidence = { sha256: hash(payload), bytes: payload.length, expandedSha256: hash(content),
    expandedBytes: content.length, reason: 'captured-truncated-gzip' };
  const f = await fixture({ compressedPayload: payload });
  try {
    const result = await createPublicDemoArchive(f.input, f.output, reviewed, undefined, [evidence]);
    assert.equal(result.publication, 'pending-disclosure-review');
    const disclosure = await load(join(f.output, 'disclosure-review.json'));
    assert.deepEqual(disclosure.incompleteGzip, [evidence]);
    const manifest = await load(join(f.output, 'manifest.json'));
    const resource = manifest.resources.find(entry => entry.source.endsWith('/compressed.bin'));
    assert.deepEqual(await readFile(join(f.output, resource.path)), payload);
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});

test('rejects stale, incorrect and unused gzip review evidence', async () => {
  const content = Buffer.from('captured incomplete artifact');
  const payload = gzipSync(content).subarray(0, -4);
  const evidence = { sha256: hash(payload), bytes: payload.length, expandedSha256: hash(content),
    expandedBytes: content.length, reason: 'captured-truncated-gzip' };
  const f = await fixture({ compressedPayload: payload });
  try {
    for (const change of [{ sha256: '0'.repeat(64) }, { bytes: payload.length + 1 },
      { expandedSha256: '0'.repeat(64) }, { expandedBytes: content.length + 1 }, { reason: 'ignore' }]) {
      await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed, undefined, [{ ...evidence, ...change }]), /gzip-review/);
      await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
    }
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed, undefined, [evidence, evidence]), /gzip-review-invalid/);
  } finally { await rm(f.directory, { recursive: true, force: true }); }
  const complete = gzipSync(content);
  const valid = await fixture({ compressedPayload: complete });
  try {
    await assert.rejects(createPublicDemoArchive(valid.input, valid.output, reviewed, undefined,
      [{ ...evidence, sha256: hash(complete), bytes: complete.length }]), /gzip-review-mismatch/);
  } finally { await rm(valid.directory, { recursive: true, force: true }); }
});

test('a reviewed truncated gzip cannot conceal a sensitive value in its last output block', async () => {
  const content = Buffer.from('x'.repeat(16374) + value);
  const payload = gzipSync(content).subarray(0, -4);
  const f = await fixture({ compressedPayload: payload });
  try {
    await assert.rejects(createPublicDemoArchive(f.input, f.output, reviewed, undefined,
      [{ sha256: hash(payload), bytes: payload.length, expandedSha256: hash(content), expandedBytes: content.length,
        reason: 'captured-truncated-gzip' }]), /public-sensitive-compressed/);
    await assert.rejects(readFile(join(f.output, 'catalog.json')), { code: 'ENOENT' });
  } finally { await rm(f.directory, { recursive: true, force: true }); }
});
