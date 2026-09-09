import test from 'node:test';
import assert from 'node:assert/strict';
import { rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { createAfterRepository, AFTER_FILES } from './after-repository.mjs';

test('after recording saves, stages and commits actual isolated Git files without changing captured snapshots', async () => {
  const repo = await createAfterRepository(process.env.RECORDING_ATTACHMENTS || tmpdir(), 'en');
  try {
    const snapshot = await repo.snapshot();
    assert.equal(snapshot.changes.length, 2);
    assert.deepEqual(snapshot.changes.map(row => row.path).sort(), [...AFTER_FILES].sort());
    const original = await repo.read(AFTER_FILES[1]);
    await repo.save(AFTER_FILES[1], original.content + 'Keep reviews local.\n', original.revision.contentHash);
    assert.equal(repo.captured[AFTER_FILES[1]], original.content);
    await assert.rejects(repo.save(AFTER_FILES[1], 'stale', original.revision.contentHash), { code: 'recording.after.revision-conflict' });
    await assert.rejects(repo.read('../outside'), { code: 'recording.after.path-invalid' });
    await assert.rejects(repo.mutate({ kind: 'push' }), { code: 'recording.after.operation-unavailable' });
    await repo.mutate({ kind: 'stage-all', expectedRevision: (await repo.snapshot()).revision });
    const staged = await repo.comparison(AFTER_FILES[1], 'staged');
    assert(staged.after.includes('Keep reviews local.'));
    assert.equal((await repo.snapshot()).changes.filter(row => ['A', 'M'].includes(row.index)).length, 2);
    const clean = await repo.mutate({ kind: 'commit', subject: 'Review workspace files', expectedRevision: (await repo.snapshot()).revision });
    assert.equal(clean.changes.length, 0); assert.notEqual(clean.head, snapshot.head); assert.equal(clean.subject, 'Review workspace files');
  } finally { await rm(repo.directory, { recursive: true, force: true }); }
});
