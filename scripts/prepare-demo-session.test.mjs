import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { attemptResourcePrefix, inspectAttemptLogs, RAW_PAGE_SIZE } from './prepare-demo-session.mjs';

test('preserves nested attempt scope in source-resource ownership', () => {
  const session = { roundId: 'r', nodeId: 'n', attemptId: 'a' };
  assert.equal(attemptResourcePrefix(session), 'rounds/r/nodes/n/a');
  assert.equal(attemptResourcePrefix({ ...session, outerNodeId: 'outer', outerAttemptId: 'outer-a' }), 'rounds/r/nodes/outer/outer-a/dynamic/nodes/n/a');
});

test('indexes original raw bytes and independent error diagnostics without materializing all frames', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gold-band-log-index-'));
  try {
    const lines = Array.from({ length: RAW_PAGE_SIZE + 2 }, (_, i) => JSON.stringify({ timestamp: `${i}Z`, frame: { result: `原始帧 ${i}` } }));
    const rawPath = join(root, 'raw.jsonl');
    const diagnosticsPath = join(root, 'diagnostics.jsonl');
    await writeFile(rawPath, `${lines.join('\n')}\n`);
    await writeFile(diagnosticsPath, [{ level: 'info' }, { level: 'error', message: 'Original error', timestamp: '2Z' }, { level: 'error' }].map(JSON.stringify).join('\n'));
    const index = await inspectAttemptLogs(rawPath, diagnosticsPath);
    assert.deepEqual(index.diagnostics, { rawFrameCount: 98, eventCount: 0, errorCount: 2, lastError: 'Original error', lastErrorTimestamp: '2Z' });
    assert.deepEqual(index.pages.map(page => page.count), [96, 2]);
    const bytes = await readFile(rawPath);
    assert.equal(index.bytes, bytes.length);
    const restored = index.pages.flatMap(page => bytes.subarray(page.offset, page.offset + page.length).toString('utf8').trimEnd().split('\n'));
    assert.deepEqual(restored, lines);
  } finally { await rm(root, { recursive: true, force: true }); }
});
