import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { archiveCatalog, capturedBlobHash, exportDemoSession, indexTimeline, redactPublic, timelineRecord } from './export-demo-session.mjs';
import { verifyDemoSession } from './verify-demo-session.mjs';
import { prepareDemoSession } from './prepare-demo-session.mjs';

test('runtime catalog excludes audit resources and task body while retaining source identity and paused outcome', () => {
  const manifest = { version: 1, projectId: 'project-a', task: { id: 'task-a', title: 'Original task', requirement: 'Large original body' },
    run: { id: 'run-a', status: 'paused', outcome: null, pause_reason: 'process-interrupted' }, workflow: 'assets/workflow.json', sessions: [], resources: ['large audit'] };
  const catalog = archiveCatalog(manifest);
  assert.equal(catalog.task.title, manifest.task.title);
  assert.equal(catalog.run.status, 'paused');
  assert.equal(catalog.run.outcome, null);
  assert.equal(catalog.run.pauseReason, 'process-interrupted');
  assert.equal('resources' in catalog, false);
  assert.equal('requirement' in catalog.task, false);
});

test('exports attachment bytes and independently stored child-agent timelines', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-resource-test-'));
  try {
    const task = join(directory, 'projects', 'project-a', 'tasks', 'task-a');
    const source = join(task, 'runs', 'run-a');
    const round = join(source, 'rounds', 'round-a');
    const attempt = join(round, 'nodes', 'node-a', 'attempt-a');
    const attachments = join(attempt, 'attachments');
    await mkdir(attachments, { recursive: true });
    await writeFile(join(task, 'task.json'), JSON.stringify({ id: 'task-a', title: 'Source task' }));
    await writeFile(join(source, 'run.json'), JSON.stringify({ id: 'run-a', status: 'paused', workflow_snapshot: 'workflow.snapshot.json' }));
    await writeFile(join(source, 'workflow.snapshot.json'), '{}');
    await writeFile(join(round, 'round.json'), JSON.stringify({ id: 'round-a', index: 1, status: 'paused', outcome: null }));
    await writeFile(join(attempt, 'node.json'), JSON.stringify({ id: 'node-a' }));
    await writeFile(join(attempt, 'acp.snapshot.json'), '{}');
    await writeFile(join(attempt, 'acp.raw.jsonl'), JSON.stringify({ timestamp: '1Z', frame: { result: 'Original raw' } }) + '\n');
    await writeFile(join(attempt, 'acp.diagnostics.jsonl'), JSON.stringify({ level: 'error', message: 'Original diagnostic', timestamp: '2Z' }) + '\n');
    await writeFile(join(attempt, 'worker-ref.json'), JSON.stringify({ provider: 'codex-acp', mode: 'acp', continueRef: { sessionId: 'source-session' } }));
    await writeFile(join(attempt, 'acp.timeline.jsonl'), JSON.stringify({ item: { id: 'root-item', seq: 1, kind: 'toolCall', toolCallId: 'source-tool', raw: { rawOutput: 'Full original output' } } }));
    const agent = join(attempt, 'agents', 'agent-a');
    await mkdir(agent, { recursive: true });
    await writeFile(join(agent, 'timeline.jsonl'), JSON.stringify({ item: { id: 'agent-item', seq: 1, kind: 'textDelta', content: 'Agent original' } }));
    const body = 'STATUS=exited EXIT=0\r\n';
    await writeFile(join(attachments, 'output.jsonl'), body);
    const output = join(directory, 'export');
    await exportDemoSession(source, output);
    const manifest = JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8'));
    const attachment = manifest.resources.find(resource => resource.source.endsWith('attachments/output.jsonl'));
    assert.equal(await readFile(join(output, attachment.path), 'utf8'), body);
    assert.equal(attachment.sha256, attachment.sourceSha256);
    assert.equal(manifest.counts.items, 2);
    assert.equal(manifest.counts.records, 2);
    const detail = JSON.parse(await readFile(join(output, manifest.sessions[0].detail), 'utf8'));
    assert.deepEqual(detail.branches.map(branch => branch.id), ['root', 'agent-a']);
    const rootPage = JSON.parse(await readFile(join(output, detail.branches[0].pages[0].path), 'utf8'));
    assert.equal(rootPage[0].raw?._meta?.goldBandConversation?.toolDetailAvailable, true);
    const childPage = JSON.parse(await readFile(join(output, detail.branches[1].pages[0].path), 'utf8'));
    assert.equal(childPage[0].content, 'Agent original');
    assert.match((await verifyDemoSession(output)).integrity, /^verified$/);
    assert.deepEqual(await prepareDemoSession(output), { sessions: 1, rawFrames: 1, runtimeVersion: 1 });
    const preparedCatalog = JSON.parse(await readFile(join(output, 'catalog.json'), 'utf8'));
    const preparedManifest = JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8'));
    assert.deepEqual(preparedCatalog.sessions, preparedManifest.sessions);
    assert.equal(preparedCatalog.runtimeVersion, 1);
    const preparedDetail = JSON.parse(await readFile(join(output, preparedCatalog.sessions[0].detail), 'utf8'));
    assert.equal(preparedDetail.diagnostics.lastError, 'Original diagnostic');
    assert.equal(preparedDetail.workerRef.continueRef.sessionId, 'source-session');
    assert.equal(preparedDetail.rawFrames.count, 1);
    assert.deepEqual(preparedDetail.branches, detail.branches);
    assert.equal((await verifyDemoSession(output)).integrity, 'verified');
    await prepareDemoSession(output);
    assert.deepEqual(JSON.parse(await readFile(join(output, 'catalog.json'), 'utf8')), preparedCatalog);
    assert.deepEqual(JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8')), preparedManifest);
    await writeFile(join(output, detail.branches[1].pages[0].path), 'tampered');
    await assert.rejects(verifyDemoSession(output), /archive.resource-corrupt/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('uses the source storage BLAKE3 checksum for captured file blobs', () => {
  assert.equal(capturedBlobHash(Buffer.from('abc')), '6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85');
});

test('materializes stable identity and revision without losing branches or byte addresses', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-archive-test-'));
  try {
    const item = { id: 'one', seq: 1, kind: 'textDelta', content: '原文' };
    const records = [
      { item },
      { patchType: 'timelinePatch', op: 'upsert', itemId: 'one', revision: 3, item: { ...item, content: '完整原文' } },
      { patchType: 'timelinePatch', op: 'upsert', itemId: 'one', revision: 2, item: { ...item, content: '迟到' } },
      { item: { ...item, raw: { _meta: { goldBandConversation: { branchId: 'child' } } } } },
    ];
    const path = join(directory, 'timeline.jsonl');
    await writeFile(path, records.map(JSON.stringify).join('\r\n'));
    const index = await indexTimeline(path);
    assert.equal(index.rows, 4);
    assert.equal(index.items.length, 2);
    const bytes = await readFile(path);
    const root = index.items.find(item => item.branchId === 'root');
    assert.equal(JSON.parse(bytes.subarray(root.offset, root.offset + root.length)).item.content, '完整原文');
    assert.equal(root.revision, 3);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('redacts credential values with an auditable path while preserving ordinary text', () => {
  const report = [];
  assert.deepEqual(redactPublic({ title: '任务名称', result: false, env: { API_KEY: 'sensitive' }, content: 'ordinary code' }, report), { title: '任务名称', result: false, env: { API_KEY: '[REDACTED]' }, content: 'ordinary code' });
  assert.deepEqual(report, [{ path: '/env/API_KEY', reason: 'credential-field' }]);
});

test('unsupported storage operations fail closed', () => {
  assert.throws(() => timelineRecord({ patchType: 'timelinePatch', op: 'delete', item: { id: 'a', seq: 1 } }));
});
