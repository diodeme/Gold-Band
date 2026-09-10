import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { prepareDemoSourceReferences } from './prepare-demo-source-references.mjs';
import { verifyDemoSession } from './verify-demo-session.mjs';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const json = async path => JSON.parse(await readFile(path, 'utf8'));
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'gold-band-source-reference-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = join(root, 'archive');
  const supplement = join(root, 'supplement');
  const output = join(root, 'output');
  for (const directory of [source, supplement]) {
    await mkdir(join(directory, 'assets'), { recursive: true });
    await mkdir(join(directory, 'resources'));
  }
  const assets = [];
  const asset = async value => {
    const bytes = Buffer.from(JSON.stringify(value));
    const sha256 = digest(bytes);
    const path = `assets/${sha256}.json`;
    await writeFile(join(source, path), bytes);
    if (!assets.some(asset => asset.path === path)) assets.push({ path, sha256, bytes: bytes.length });
    return path;
  };
  const href = '/E:/code/main.ts:3';
  const page = await asset([{ id: 'message', seq: 2, kind: 'textDelta', content: `[Source](${href})` }]);
  const evidence = await asset({ id: 'tool', seq: 1, raw: { rawOutput: 'Captured Git HEAD' } });
  const directoryRoot = await asset({ entries: [] });
  const detail = await asset({ directoryRoot, branches: [{ id: 'root', count: 1, tools: {}, pages: [{ path: page, firstSeq: 2, lastSeq: 2, count: 1 }] }] });
  const sessions = ['outer-a', 'outer-b'].map(outerNodeId => ({ projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round',
    nodeId: 'node', attemptId: 'attempt', outerNodeId, outerAttemptId: 'outer-attempt', node: { id: 'node', title: 'Source', status: 'completed', outcome: 'success' }, detail, itemCount: 1, recordCount: 1 }));
  const manifest = { version: 1, runtimeVersion: 1, projectId: 'project', task: { id: 'task' }, run: { id: 'run', status: 'paused', outcome: null },
    workflow: await asset({}), sessions, assets, resources: [], missing: [], redactions: [],
    sourceFiles: sessions.map(() => ({ rows: 1, items: 1 })), counts: { sessions: 2, items: 2, records: 2 } };
  const original = JSON.stringify(manifest);
  await writeFile(join(source, 'manifest.json'), original);
  await writeFile(join(source, 'catalog.json'), JSON.stringify({ version: 1, runtimeVersion: 1, sessions }));
  await writeFile(join(source, 'disclosure-review.json'), JSON.stringify({ publication: 'pending-disclosure-review' }));
  const input = { version: 1, kind: 'reviewed-source-reference-supplement', sourceManifestSha256: digest(original),
    publication: 'pending-disclosure-review', references: [], resources: [], unresolved: [] };
  for (const [index, session] of sessions.entries()) {
    const bytes = Buffer.from(`Version ${index}\r\n`);
    const sha256 = digest(bytes);
    const resource = { path: `resources/${sha256}`, sha256, bytes: bytes.length };
    await writeFile(join(supplement, resource.path), bytes);
    input.resources.push(resource);
    const { node, detail, itemCount, recordCount, ...locator } = session;
    input.references.push({ locator, branchId: 'root', eventId: 'message', href, sourcePath: 'E:/code/main.ts', resource, evidence: [evidence],
      provenance: { kind: 'git-blob', commit: 'a'.repeat(40), path: 'main.ts', blobOid: createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex') } });
  }
  const save = () => writeFile(join(supplement, 'manifest.json'), JSON.stringify(input));
  await save();
  return { root, source, supplement, output, input, original, save, asset, manifest };
}

test('imports full scoped source versions without changing original messages or archives', async t => {
  const f = await fixture(t);
  f.input.references.push(structuredClone(f.input.references[0]));
  const supplementBytes = JSON.stringify(f.input, null, 2) + '\n';
  await writeFile(join(f.supplement, 'manifest.json'), supplementBytes);
  const result = await prepareDemoSourceReferences(f.source, f.supplement, f.output);
  assert.equal(result.referenceOccurrences, 3);
  assert.equal(result.uniqueReferences, 2);
  assert.equal(result.sourceReferences, 2);
  assert.equal(result.unresolvedReferences, 0);
  assert.equal(await readFile(join(f.source, 'manifest.json'), 'utf8'), f.original);
  const manifest = await json(join(f.output, 'manifest.json'));
  assert.equal(await readFile(join(f.output, manifest.sourceReferenceSupplement.audit), 'utf8'), supplementBytes);
  assert.deepEqual(manifest.sessions.map(s => s.node), f.manifest.sessions.map(s => s.node));
  for (const [i, session] of manifest.sessions.entries()) {
    const detail = await json(join(f.output, session.detail));
    const originalDetail = await json(join(f.source, f.manifest.sessions[i].detail));
    assert.deepEqual(detail.branches, originalDetail.branches);
    const index = await json(join(f.output, detail.sourceReferences));
    assert.equal(index.locator.outerNodeId, session.outerNodeId);
    assert.equal(index.references.length, 1);
    assert.equal(await readFile(join(f.output, index.references[0].resource.path), 'utf8'), `Version ${i}\r\n`);
  }
  assert.deepEqual((await json(join(f.output, 'catalog.json'))).sessions, manifest.sessions);
  assert.equal((await json(join(f.output, 'disclosure-review.json'))).publication, 'pending-disclosure-review');
  await assert.rejects(prepareDemoSourceReferences(f.source, f.supplement, f.output), /output-exists/);
});

test('rejects conflicts, absent source events, crossed scopes and stale manifests', async t => {
  const f = await fixture(t);
  const original = structuredClone(f.input);
  for (const [change, error] of [
    [input => { input.references.push({ ...input.references[0], resource: input.references[1].resource }); }, /reference-conflict/],
    [input => { input.references[0].eventId = 'absent'; }, /reference-event/],
    [input => { input.references[0].branchId = 'other'; }, /reference-branch/],
    [input => { input.references[0].href = '/E:/other.ts'; }, /reference-message/],
    [input => { input.references[0].locator.outerNodeId = 'other'; }, /reference-scope/],
    [input => { input.references[0].locator.projectId = 'other'; }, /reference-scope/],
    [input => { input.references[0].sourcePath = 'E:/code/../other.ts'; }, /reference-path/],
    [input => { input.sourceManifestSha256 = '0'.repeat(64); }, /supplement-mismatch/],
    [input => { input.resources[0].path = '../outside'; }, /resource-invalid/],
    [input => { input.references[0].provenance.blobOid = '0'.repeat(40); }, /git-blob-mismatch/],
  ]) {
    Object.assign(f.input, structuredClone(original));
    change(f.input);
    await f.save();
    await assert.rejects(prepareDemoSourceReferences(f.source, f.supplement, f.output), error);
  }
  Object.assign(f.input, original);
  await f.save();
  await writeFile(join(f.supplement, original.resources[0].path), 'changed');
  await assert.rejects(prepareDemoSourceReferences(f.source, f.supplement, f.output), /resource-corrupt/);
});

test('preserves explicit missing references and the verifier rejects changed event identity', async t => {
  const f = await fixture(t);
  const reference = f.input.references.pop();
  f.input.resources.pop();
  const { resource, evidence, provenance, sourcePath, ...unresolved } = reference;
  f.input.unresolved.push({ ...unresolved, reason: 'source-version-not-yet-established' });
  await f.save();
  const result = await prepareDemoSourceReferences(f.source, f.supplement, f.output);
  assert.equal(result.unresolvedSourceReferences, 1);
  const manifest = await json(join(f.output, 'manifest.json'));
  const session = manifest.sessions[0];
  const detail = await json(join(f.output, session.detail));
  const index = await json(join(f.output, detail.sourceReferences));
  index.references[0].eventId = 'wrong';
  const emit = async value => {
    const bytes = Buffer.from(JSON.stringify(value));
    const sha256 = digest(bytes);
    const path = `assets/${sha256}.json`;
    await writeFile(join(f.output, path), bytes);
    manifest.assets.push({ path, sha256, bytes: bytes.length });
    return path;
  };
  detail.sourceReferences = await emit(index);
  session.detail = await emit(detail);
  await writeFile(join(f.output, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(verifyDemoSession(f.output), /source-reference-event/);
  delete detail.sourceReferences;
  session.detail = await emit(detail);
  await writeFile(join(f.output, 'manifest.json'), JSON.stringify(manifest));
  await assert.rejects(verifyDemoSession(f.output), /source-reference-audit-mismatch/);
});

test('requires an exact complete captured read that precedes its referring message', async t => {
  const f = await fixture(t);
  const reference = f.input.references[0];
  const content = await readFile(join(f.supplement, reference.resource.path), 'utf8');
  const captured = { id: 'read', seq: 1, raw: { _meta: { claudeCode: { toolResponse: {
    file: { filePath: reference.sourcePath, content, startLine: 1, numLines: 1, totalLines: 1 },
  } } } } };
  const evidence = await f.asset(captured);
  const manifestBytes = JSON.stringify(f.manifest);
  await writeFile(join(f.source, 'manifest.json'), manifestBytes);
  f.input.sourceManifestSha256 = digest(manifestBytes);
  reference.evidence = [evidence];
  reference.provenance = { kind: 'captured-complete-read', asset: evidence, seq: 1, startLine: 1, totalLines: 1 };
  const provenance = structuredClone(reference.provenance);
  reference.provenance.seq = 3;
  await f.save();
  await assert.rejects(prepareDemoSourceReferences(f.source, f.supplement, f.output), /future-read/);
  reference.provenance = { ...provenance, totalLines: 2 };
  await f.save();
  await assert.rejects(prepareDemoSourceReferences(f.source, f.supplement, f.output), /complete-read-mismatch/);
  reference.provenance = provenance;
  await f.save();
  assert.equal((await prepareDemoSourceReferences(f.source, f.supplement, f.output)).uniqueReferences, 2);
});
