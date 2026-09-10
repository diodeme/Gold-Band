import { test } from 'node:test';
import assert from 'node:assert/strict';
import { access } from 'node:fs/promises';
import { createReviewRepository } from './site-review-repository.mjs';

test('records actual isolated Git edits, index snapshots and a clean local commit', async () => {
  const repository = await createReviewRepository('en');
  try {
    assert.notEqual(repository.root, process.cwd());
    const config = await repository.run({ action: 'read', path: 'src/config.json' });
    assert.equal(JSON.parse(config.content).captureFileChanges, true);
    const document = await repository.run({ action: 'read', path: 'docs/workspace-notes.md' });
    const updated = await repository.run({ action: 'write', path: document.path, expectedHash: document.hash, content: `${document.content}\nReviewed locally.\n` });
    await assert.rejects(repository.run({ action: 'write', path: document.path, expectedHash: document.hash, content: 'stale' }), { code: 'workspace-file.changed-on-disk' });
    await repository.run({ action: 'stage', paths: [config.path, document.path] });
    const staged = await repository.run({ action: 'comparison', area: 'staged', path: document.path });
    assert.equal(staged.before, null);
    assert.equal(staged.after, updated.content);
    const result = await repository.run({ action: 'commit', subject: 'Document workspace review' });
    assert.notEqual(result.head, repository.initialHead);
    assert.deepEqual(result.changes, []);
    assert.equal((await repository.run({ action: 'read', path: document.path })).content, updated.content);
    await assert.rejects(repository.run({ action: 'read', path: '../outside' }), { code: 'site.review-path-invalid' });
    await assert.rejects(repository.run({ action: 'push' }), { code: 'site.review-operation-rejected' });
  } finally { await repository.dispose(); }
  await assert.rejects(access(repository.root));
});

test('rejects a stale Git mutation and distinguishes index content with the same status letters', async () => {
  const repository = await createReviewRepository('en');
  try {
    const before = await repository.run({ action: 'status' });
    const path = 'src/config.json';
    await repository.run({ action: 'stage', paths: [path], expectedRevision: before.revision });
    await assert.rejects(repository.run({ action: 'commit', subject: 'Stale review', expectedRevision: before.revision }), { code: 'git.snapshot-stale' });
    const file = await repository.run({ action: 'read', path });
    await repository.run({ action: 'write', path, expectedHash: file.hash, content: `${file.content}\n\n` });
    const first = await repository.run({ action: 'status' });
    const interim = await repository.run({ action: 'read', path });
    await repository.run({ action: 'write', path, expectedHash: interim.hash, content: `${file.content}\n` });
    await repository.run({ action: 'stage', paths: [path] });
    const next = await repository.run({ action: 'read', path });
    await repository.run({ action: 'write', path, expectedHash: next.hash, content: `${file.content}\n\n` });
    const second = await repository.run({ action: 'status' });
    assert.deepEqual(first.changes, second.changes);
    assert.notEqual(first.revision, second.revision);
  } finally { await repository.dispose(); }
});
