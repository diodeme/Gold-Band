import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, open, readFile, readdir, realpath, stat, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { blake3 } from '@noble/hashes/blake3.js';

export const ARCHIVE_VERSION = 1;
export const ARCHIVE_PAGE_SIZE = 96;
export const hash = value => createHash('sha256').update(value).digest('hex');
export const capturedBlobHash = value => Buffer.from(blake3(value)).toString('hex');
export function archiveToolSummary(event) {
  const meta = { ...event.raw?._meta };
  delete meta.claudeCode;
  delete meta.agentTranscript;
  const conversation = { ...meta.goldBandConversation, toolDetailAvailable: true };
  delete conversation.toolOutput;
  return { ...event, raw: { toolCallId: event.toolCallId, title: event.title, status: event.status,
    _meta: { ...meta, goldBandConversation: conversation } }, content: null };
}
export function archiveCatalog(manifest) {
  return {
    version: manifest.version, projectId: manifest.projectId,
    task: { id: manifest.task.id, title: manifest.task.title, uuid: manifest.task.uuid },
    run: { id: manifest.run.id, status: manifest.run.status, outcome: manifest.run.outcome ?? null,
      startedAt: manifest.run.started_at, updatedAt: manifest.run.updated_at, pauseReason: manifest.run.pause_reason ?? null },
    workflow: manifest.workflow, sessions: manifest.sessions,
  };
}
const json = async path => JSON.parse(await readFile(path, 'utf8'));
const entries = async path => { try { return await readdir(path, { withFileTypes: true }); } catch (error) { if (error.code === 'ENOENT') return []; throw error; } };
const children = async path => (await entries(path)).filter(e => e.isDirectory()).map(e => join(path, e.name));
const branchId = (item, fallback = 'root') => item.raw?._meta?.goldBandConversation?.branchId || fallback;

// Same persisted formats and revision rule as src/acp/timeline.rs::parse_timeline_record.
export function timelineRecord(record, defaultBranch = 'root') {
  if (record.patchType && (record.patchType !== 'timelinePatch' || record.op !== 'upsert')) throw new Error('archive.timeline-op-unsupported');
  const item = record.item;
  if (!item || typeof item.id !== 'string' || !Number.isSafeInteger(item.seq)) throw new Error('archive.timeline-item-invalid');
  const revision = record.revision ?? item.endedSeq ?? item.startedSeq ?? item.seq;
  return { item, revision, key: `${branchId(item, defaultBranch)}:${item.id}` };
}

export async function indexTimeline(path, defaultBranch = 'root') {
  const items = new Map();
  let offset = 0;
  let rows = 0;
  const digest = createHash('sha256');
  const stream = createReadStream(path);
  let pending = Buffer.alloc(0);
  const consume = line => {
    const length = line.length;
    if (line.toString('utf8').trim()) {
      const { item, revision, key } = timelineRecord(JSON.parse(line.toString('utf8')), defaultBranch);
      const prior = items.get(key);
      if (!prior || revision >= prior.revision) items.set(key, { id: item.id, seq: item.seq, branchId: branchId(item, defaultBranch), revision, offset, length });
      rows++;
    }
    offset += length;
  };
  for await (const chunk of stream) {
    digest.update(chunk);
    pending = Buffer.concat([pending, chunk]);
    let start = 0;
    for (let end = pending.indexOf(10); end >= 0; end = pending.indexOf(10, start)) {
      consume(pending.subarray(start, end + 1));
      start = end + 1;
    }
    pending = pending.subarray(start);
  }
  if (pending.length) consume(pending);
  return { rows, sha256: digest.digest('hex'), items: [...items.values()].sort((a, b) => a.seq - b.seq || a.id.localeCompare(b.id)) };
}

export function redactPublic(value, report, path = '') {
  if (Array.isArray(value)) return value.map((item, i) => redactPublic(item, report, `${path}/${i}`));
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => {
    if (/^(api[_-]?key|access[_-]?token|refresh[_-]?token|password|authorization|private[_-]?key|client[_-]?secret)$/i.test(key) && typeof item === 'string' && item) {
      report.push({ path: `${path}/${key}`, reason: 'credential-field' });
      return [key, '[REDACTED]'];
    }
    return [key, redactPublic(item, report, `${path}/${key}`)];
  }));
  if (typeof value !== 'string') return value;
  return value.replace(/\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{30,})\b/g, () => {
    report.push({ path, reason: 'credential-token' }); return '[REDACTED]';
  });
}

export async function exportDemoSession(sourcePath, outputPath) {
  const source = await realpath(sourcePath);
  const output = resolve(outputPath);
  if (output === source || output.startsWith(`${source}${sep}`)) throw new Error('archive.output-inside-source');
  await mkdir(output, { recursive: true });
  const assets = [];
  const assetHashes = new Set();
  const missing = [];
  const redactions = [];
  const sourceFiles = [];
  const resources = [];
  const audited = new Set();
  const task = await json(join(dirname(dirname(source)), 'task.json'));
  const run = await json(join(source, 'run.json'));
  const projectId = dirname(dirname(dirname(dirname(source)))).split(sep).at(-1);
  async function emit(value) {
    const content = Buffer.from(JSON.stringify(redactPublic(value, redactions)));
    const sha256 = hash(content);
    const path = `assets/${sha256}.json`;
    await mkdir(join(output, 'assets'), { recursive: true });
    await writeFile(join(output, path), content);
    if (!assetHashes.has(sha256)) { assetHashes.add(sha256); assets.push({ path, bytes: content.length, sha256 }); }
    return path;
  }
  async function auditFile(path, structured = true) {
    const canonical = await realpath(path);
    if (!canonical.startsWith(`${source}${sep}`)) { missing.push({ path: relative(source, path), reason: 'external-resource' }); return; }
    if (audited.has(canonical)) return;
    audited.add(canonical);
    const bytes = await readFile(canonical);
    let published = bytes;
    if (structured && /\.json$/i.test(path)) published = Buffer.from(JSON.stringify(redactPublic(JSON.parse(bytes.toString('utf8')), redactions, relative(source, path))));
    else if (structured && /\.(?:jsonl|ndjson)$/i.test(path)) published = Buffer.from(bytes.toString('utf8').split('\n').filter(line => line.trim()).map(line => JSON.stringify(redactPublic(JSON.parse(line), redactions, relative(source, path)))).join('\n') + '\n');
    else {
      try { published = Buffer.from(redactPublic(new TextDecoder('utf-8', { fatal: true }).decode(bytes), redactions, relative(source, path))); }
      catch { /* Binary attachments are preserved byte-for-byte. */ }
    }
    const sha256 = hash(published);
    const deployed = `resources/${sha256}`;
    await mkdir(join(output, 'resources'), { recursive: true });
    await writeFile(join(output, deployed), published);
    resources.push({ source: relative(source, path).replaceAll('\\', '/'), sourceSha256: hash(bytes), sourceBytes: bytes.length, path: deployed, sha256, bytes: published.length });
  }
  async function auditDirectory(path, structured = true) {
    for (const entry of await entries(path)) {
      const next = join(path, entry.name);
      if (entry.isSymbolicLink()) { missing.push({ path: relative(source, next), reason: 'symbolic-resource-requires-review' }); continue; }
      if (entry.isDirectory()) await auditDirectory(next, structured);
      else if (entry.isFile()) await auditFile(next, structured);
    }
  }
  async function hydrate(value, directory) {
    if (value && typeof value === 'object' && value.$goldBandBlob) {
      const ref = value.$goldBandBlob;
      if (!/^[a-f0-9]{64}$/.test(ref.contentHash)) throw new Error('archive.blob-reference-invalid');
      const path = join(directory, 'acp.file-blobs', ref.contentHash.slice(0, 2), ref.contentHash);
      try {
        const content = await readFile(path);
        if (capturedBlobHash(content) !== ref.contentHash || content.length !== ref.byteLength) throw new Error('archive.blob-corrupt');
        return content.toString('utf8');
      } catch (error) {
        missing.push({ path: relative(source, path).replaceAll('\\', '/'), reason: error.code || error.message });
        return value;
      }
    }
    if (Array.isArray(value)) {
      const result = [];
      for (const item of value) result.push(await hydrate(item, directory));
      return result;
    }
    if (value && typeof value === 'object') {
      const result = {};
      for (const [key, item] of Object.entries(value)) result[key] = await hydrate(item, directory);
      return result;
    }
    return value;
  }
  const sessions = [];
  async function collectAttempt(directory, locator, node) {
    const files = new Set((await entries(directory)).map(e => e.name));
    for (const name of ['acp.snapshot.json', 'acp.raw.jsonl', 'acp.diagnostics.jsonl', 'acp.prompt-usage.jsonl', 'acp.timeline.jsonl', 'acp.turn-file-mutations.jsonl', 'worker-ref.json', 'artifact-emission.json', 'node.json']) if (files.has(name)) await auditFile(join(directory, name));
    for (const name of ['attachments', 'artifacts', 'turn-file-change-sets', 'turn-attachment-baselines', 'acp.file-blobs']) if (files.has(name)) await auditDirectory(join(directory, name), name === 'turn-file-change-sets' || name === 'turn-attachment-baselines');
    const timelines = files.has('acp.timeline.jsonl') ? [{ path: join(directory, 'acp.timeline.jsonl'), branch: 'root' }] : [];
    for (const agent of await children(join(directory, 'agents'))) {
      const agentFiles = new Set((await entries(agent)).map(entry => entry.name));
      for (const name of ['snapshot.json', 'timeline.jsonl']) if (agentFiles.has(name)) await auditFile(join(agent, name));
      if (agentFiles.has('timeline.jsonl')) timelines.push({ path: join(agent, 'timeline.jsonl'), branch: agent.split(sep).at(-1) });
    }
    if (timelines.length) {
      const snapshot = files.has('acp.snapshot.json') ? await json(join(directory, 'acp.snapshot.json')) : null;
      if (!snapshot) missing.push({ path: relative(source, directory), reason: 'snapshot-missing' });
      const branches = [];
      let itemCount = 0;
      let recordCount = 0;
      for (const { path: timeline, branch: defaultBranch } of timelines) {
      const before = await stat(timeline);
      const index = await indexTimeline(timeline, defaultBranch);
      sourceFiles.push({ path: relative(source, timeline).replaceAll('\\', '/'), bytes: before.size, sha256: index.sha256, rows: index.rows, items: index.items.length });
      const handle = await open(timeline, 'r');
      try {
        for (const branch of new Set(index.items.map(i => i.branchId))) {
          if (branches.some(existing => existing.id === branch)) throw new Error('archive.branch-storage-conflict');
          const ordered = index.items.filter(i => i.branchId === branch);
          const pages = [];
          const tools = {};
          for (let start = 0; start < ordered.length; start += ARCHIVE_PAGE_SIZE) {
            const window = ordered.slice(start, start + ARCHIVE_PAGE_SIZE);
            const events = [];
            for (const entry of window) {
              const buffer = Buffer.alloc(entry.length);
              const { bytesRead } = await handle.read(buffer, 0, entry.length, entry.offset);
              const event = await hydrate(timelineRecord(JSON.parse(buffer.subarray(0, bytesRead).toString('utf8'))).item, directory);
              if (event.kind === 'toolCall') {
                tools[event.id] = await emit(event);
                events.push(archiveToolSummary(event));
              } else events.push(event);
            }
            pages.push({ path: await emit(events), count: events.length, firstSeq: window[0].seq, lastSeq: window.at(-1).seq });
          }
          branches.push({ id: branch, count: ordered.length, pages, tools });
        }
      } finally { await handle.close(); }
      const after = await stat(timeline);
      if (before.size !== after.size || before.mtimeMs !== after.mtimeMs) throw new Error('archive.source-changed');
      itemCount += index.items.length;
      recordCount += index.rows;
      }
      sessions.push({ ...locator, node: { id: node.id, title: node.title ?? node.id, status: node.status, outcome: node.outcome ?? null, provider: node.provider, startedAt: node.startedAt ?? node.started_at, finishedAt: node.finishedAt ?? node.finished_at }, detail: await emit({ snapshot, branches }), itemCount, recordCount });
    }
    if (files.has('dynamic')) {
      const dynamic = join(directory, 'dynamic');
      for (const entry of await entries(dynamic)) if (entry.isFile() && /\.(json|jsonl)$/.test(entry.name)) await auditFile(join(dynamic, entry.name));
      for (const name of ['proposals', 'groups']) await auditDirectory(join(dynamic, name));
      for (const child of await children(join(dynamic, 'nodes'))) {
        await auditFile(join(child, 'node.json'));
        const childNode = await json(join(child, 'node.json'));
        for (const attempt of await children(child)) {
          if (!attempt.split(sep).at(-1).startsWith('attempt-')) continue;
          await collectAttempt(attempt, { ...locator, nodeId: childNode.id, attemptId: attempt.split(sep).at(-1), outerNodeId: locator.nodeId, outerAttemptId: locator.attemptId }, childNode);
        }
      }
    }
  }
  for (const roundDirectory of await children(join(source, 'rounds'))) {
    await auditFile(join(roundDirectory, 'round.json'));
    for (const nodeDirectory of await children(join(roundDirectory, 'nodes'))) {
      for (const attemptDirectory of await children(nodeDirectory)) {
        if (!attemptDirectory.split(sep).at(-1).startsWith('attempt-')) continue;
        const node = await json(join(attemptDirectory, 'node.json'));
        await collectAttempt(attemptDirectory, { projectId, taskId: task.id, runId: run.id, roundId: roundDirectory.split(sep).at(-1), nodeId: node.id ?? nodeDirectory.split(sep).at(-1), attemptId: attemptDirectory.split(sep).at(-1) }, node);
      }
    }
  }
  for (const entry of await entries(source)) if (entry.isFile() && /\.(json|jsonl)$/.test(entry.name)) await auditFile(join(source, entry.name));
  const manifest = { version: ARCHIVE_VERSION, projectId, task, run, workflow: await emit(await json(join(source, run.workflow_snapshot))), sessions, sourceFiles, resources, assets, missing, redactions,
    completeness: 'pending-resource-audit', counts: { sessions: sessions.length, items: sessions.reduce((n, s) => n + s.itemCount, 0), records: sessions.reduce((n, s) => n + s.recordCount, 0), bytes: assets.reduce((n, a) => n + a.bytes, 0) } };
  await writeFile(join(output, 'manifest.json'), JSON.stringify(manifest, null, 2));
  await writeFile(join(output, 'catalog.json'), JSON.stringify(archiveCatalog(manifest)));
  return { counts: manifest.counts, missing: missing.length, redactions: redactions.length, completeness: manifest.completeness };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [source, output] = process.argv.slice(2);
  if (!source || !output) throw new Error('Usage: node scripts/export-demo-session.mjs <run-directory> <output-directory>');
  console.log(JSON.stringify(await exportDemoSession(source, output), null, 2));
}
