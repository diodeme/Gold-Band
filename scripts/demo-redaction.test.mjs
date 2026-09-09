import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sanitize, archiveSourceFile } from './export-demo-history.mjs';
import { gunzipSync } from 'node:zlib';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

test('redacts dotenv, authorization and PEM credentials while retaining public keys', () => {
  const text = 'TEST_PROVIDER_API_KEY="DUMMY_TEST_VALUE"\nAuthorization: Basic RFVNTVk=\n-----BEGIN PRIVATE KEY-----\nRFVNTVk=\n-----END PRIVATE KEY-----\n';
  const result = sanitize(text);
  assert.ok(!result.includes('DUMMY_TEST_VALUE'));
  assert.ok(!result.includes('RFVNTVk='));
  assert.equal(sanitize('-----BEGIN PUBLIC KEY-----\nRFVNTVk=\n-----END PUBLIC KEY-----'), '-----BEGIN PUBLIC KEY-----\nRFVNTVk=\n-----END PUBLIC KEY-----');
});
test('redaction remains idempotent and keeps noncredential state', () => {
  const text = '{"status":"paused","revision":118,"outcome":null,"api_key":"DUMMY_TEST_VALUE"}';
  const once = sanitize(text);
  assert.equal(sanitize(once), once);
  assert.deepEqual(JSON.parse(once), { status: 'paused', revision: 118, outcome: null, api_key: '[REDACTED_CREDENTIAL]' });
});
test('removes scoped test tokens and recovery grants without rewriting idempotency identities', () => {
  const text = 'JI_STAGE1_TOKEN="DUMMY_TOKEN_VALUE"\nKEY="DUMMY_KEY_VALUE"\n{"recoveryCandidateToken":"DUMMY_RECOVERY_VALUE","idempotencyKey":"0123456789abcdef0123456789abcdef"}';
  const value = sanitize(text);
  assert.ok(!value.includes('DUMMY_'));
  assert.ok(value.includes('0123456789abcdef0123456789abcdef'));
});

test('preserves JSON escaping when credentials occur inside quoted tool commands', () => {
  const value = { kind: 'toolCall', command: 'TOKEN="DUMMY_VALUE"; echo done', nested: { recoveryCandidateToken: 'DUMMY_RECOVERY' } };
  const result = JSON.parse(sanitize(JSON.stringify(value)));
  assert.equal(result.command, 'TOKEN="[REDACTED_CREDENTIAL]"; echo done');
  assert.equal(result.nested.recoveryCandidateToken, '[REDACTED_CREDENTIAL]');
});

test('redacts resource references attached to another resource descriptor', () => {
  const root = mkdtempSync(join(tmpdir(), 'demo-redaction-'));
  try {
    const publish = (path, value) => {
      const bytes = Buffer.from(JSON.stringify(value)); writeFileSync(join(root, path), bytes);
      return { path, byteLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
    };
    const comparisonIndex = publish('comparison.json', { api_key: 'DUMMY_SECRET_VALUE' });
    const run = { ...publish('run.json', { status: 'paused' }), comparisonIndex };
    publish('dataset.json', { sessions: [], run });
    publish('manifest.json', { sessions: [], files: [] });
    const result = spawnSync(process.execPath, [resolve('scripts/redact-demo-export.mjs'), root, '--resources-only'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(readFileSync(join(root, 'comparison.json'))).api_key, '[REDACTED_CREDENTIAL]');
    assert.notEqual(JSON.parse(readFileSync(join(root, 'dataset.json'))).run.comparisonIndex.sha256, comparisonIndex.sha256);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('fresh archives preserve pretty-printed JSON structure and source hashes', async () => {
  const root = mkdtempSync(join(tmpdir(), 'demo-archive-'));
  try {
    const source = join(root, 'source.json'); const target = join(root, 'source.gz');
    const text = JSON.stringify({ command: 'TOKEN="DUMMY_VALUE"; echo done', status: 'paused', revision: 118 }, null, 2);
    writeFileSync(source, text);
    const result = await archiveSourceFile(source, target);
    const value = JSON.parse(gunzipSync(readFileSync(target)));
    assert.equal(value.command, 'TOKEN="[REDACTED_CREDENTIAL]"; echo done');
    assert.equal(value.status, 'paused');
    assert.equal(result.sourceSha256, createHash('sha256').update(text).digest('hex'));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('content-addressed binary blobs retain exact bytes', async () => {
  const root = mkdtempSync(join(tmpdir(), 'demo-blob-'));
  try {
    mkdirSync(join(root, 'acp.file-blobs'));
    const source = join(root, 'acp.file-blobs', 'test'); const target = join(root, 'blob.gz');
    const bytes = Buffer.from([65, 0, 66, 13, 10, 67]);
    writeFileSync(source, bytes);
    await archiveSourceFile(source, target);
    assert.deepEqual(gunzipSync(readFileSync(target)), bytes);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
