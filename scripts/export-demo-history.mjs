import { createReadStream, createWriteStream } from 'node:fs';
import { readFile, writeFile, mkdir, readdir, stat, realpath, open } from 'node:fs/promises';
import { dirname, resolve, relative, join } from 'node:path';
import { createHash } from 'node:crypto';
import { createGzip } from 'node:zlib';
import { pipeline } from 'node:stream/promises';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
import { blake3 } from '@noble/hashes/blake3.js';

const digest = (value) => createHash('sha256').update(value).digest('hex');
const json = async (path) => JSON.parse(await readFile(path, 'utf8'));
const exists = async (path) => stat(path).then(() => true, (error) => { if (error.code === 'ENOENT') return false; throw error; });
export function sanitize(text, counts = {}) {
  const trimmed = text.trimStart();
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    let value;
    try { value = JSON.parse(text); } catch { /* Tool output can start with JSON punctuation. */ }
    if (value && typeof value === 'object') {
      const project = (item) => {
        if (typeof item === 'string') return sanitize(item, counts);
        if (!item || typeof item !== 'object') return item;
        if (Array.isArray(item)) return item.map(project);
        return Object.fromEntries(Object.entries(item).map(([key, child]) => {
          if (typeof child === 'string' && /(?:token$|^(?:api[_-]?key|password|client[_-]?secret)$)/i.test(key)) {
            counts['credential-field'] = (counts['credential-field'] ?? 0) + 1;
            return [key, '[REDACTED_CREDENTIAL]'];
          }
          return [key, project(child)];
        }));
      };
      return JSON.stringify(project(value));
    }
    const lines = text.split('\n');
    if (lines.length > 1 && lines.filter((line) => line.trim()).every((line) => {
      try { JSON.parse(line); return true; } catch { return false; }
    })) return lines.map((line) => line.trim() ? sanitize(line, counts) : line).join('\n');
  }
  const replace = (pattern, value, key) => { text = text.replace(pattern, () => { counts[key] = (counts[key] ?? 0) + 1; return value; }); };
  replace(/[A-Z]:[\\/]+Users[\\/]+[^\\/\s"<>]+/gi, '/export/user', 'user-root');
  replace(/\b(?:sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b/g, '[REDACTED_CREDENTIAL]', 'credential');
  replace(/(?<=Bearer\s)[A-Za-z0-9._~-]{16,}/gi, '[REDACTED_CREDENTIAL]', 'bearer');
  replace(/(?<=Basic\s)[A-Za-z0-9+/=]{8,}/gi, '[REDACTED_CREDENTIAL]', 'basic');
  replace(/-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----[\s\S]*?-----END (?:[A-Z]+ )?PRIVATE KEY-----/g, '[REDACTED_PRIVATE_KEY]', 'private-key');
  replace(/(?<=\b[A-Z0-9_]*(?:API[_-]?KEY|ACCESS[_-]?TOKEN|REFRESH[_-]?TOKEN|PASSWORD|CLIENT[_-]?SECRET)\s*=\s*["']?)[^\r\n"'\s]+/gi, '[REDACTED_CREDENTIAL]', 'dotenv-credential');
  replace(/(?<=\b(?:[A-Z0-9_]*TOKEN|KEY)\s*=\s*["']?)[^\r\n"'&\s]+/gi, '[REDACTED_CREDENTIAL]', 'scoped-token');
  replace(/(?<=["'][A-Za-z0-9_-]*token["']\s*:\s*["'])[^"'\r\n]+/gi, '[REDACTED_CREDENTIAL]', 'token-field');
  replace(/(?<=["'](?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|client[_-]?secret)["']\s*:\s*["'])[^"'\r\n]+/gi, '[REDACTED_CREDENTIAL]', 'credential-field');
  return text;
}
export async function archiveSourceFile(path, target, counts = {}) {
  const before = await stat(path);
  const hash = createHash('sha256');
  const textFile = /\.(jsonl?|ndjson|md|txt|log|ya?ml|toml|xml|csv|tsv|[cm]?js|tsx?|css|html|sh|ps1|java|rs|properties|gradle|kts|lock|env|pem|key|ini|conf|patch|diff|bat|cmd)$/i.test(path) || path.includes('acp.file-blobs');
  if (/\.json$/i.test(path) || path.includes('acp.file-blobs')) {
    const bytes = await readFile(path); hash.update(bytes);
    let text;
    try { text = new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { text = null; }
    const projected = text != null && !text.includes('\0') ? sanitize(text, counts) : bytes;
    await pipeline([projected], createGzip(), createWriteStream(target));
  } else {
    const stream = createReadStream(path); stream.on('data', (data) => hash.update(data));
    if (textFile) {
      async function* redacted() {
        let privateKey = false;
        for await (const line of createInterface({ input: stream, crlfDelay: Infinity })) {
          if (/^-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----$/.test(line.trim())) { privateKey = true; counts['private-key'] = (counts['private-key'] ?? 0) + 1; yield '[REDACTED_PRIVATE_KEY]\n'; }
          else if (privateKey) { if (/^-----END (?:[A-Z]+ )?PRIVATE KEY-----$/.test(line.trim())) privateKey = false; }
          else yield sanitize(line, counts) + '\n';
        }
        if (privateKey) throw new Error('Unterminated private key in source');
      }
      await pipeline(redacted(), createGzip(), createWriteStream(target));
    } else await pipeline(stream, createGzip(), createWriteStream(target));
  }
  const after = await stat(path);
  if (after.size !== before.size || after.mtimeMs !== before.mtimeMs) throw new Error('Source changed during archive');
  return { sourceBytes: before.size, sourceSha256: hash.digest('hex'), byteLength: (await stat(target)).size };
}
export async function exportHistory(source, output) {
  source = resolve(source); output = resolve(output);
  const root = dirname(source);
  const runBytes = await readFile(source);
  const run = JSON.parse(runBytes);
  if (run.task_uuid !== '92900d5e127a4bc79239514a7e561a46' || run.uuid !== 'fb981f32d074405aad5321c9daed98eb' || run.execution.revision !== 118 || run.status !== 'paused' || run.outcome !== null || run.pause_reason !== 'process-interrupted') throw new Error('Source identity/revision mismatch');
  const projectId = 'e-projects-code-ai-ji--a40d4379';
  const task = await json(join(root, '../../task.json'));
  const manifest = { version: 1, projectId, taskId: run.task_id, taskUuid: run.task_uuid, runId: run.id, title: task.title, source: { runSha256: digest(runBytes), revision: run.execution.revision, status: run.status, outcome: run.outcome, pauseReason: run.pause_reason, startedAt: run.started_at, updatedAt: run.updated_at }, sessions: [], files: [], missing: [], externalDependencies: [], redactions: {} };
  const publish = async (path, value) => { const data = Buffer.from(JSON.stringify(value)); await mkdir(dirname(join(output, path)), { recursive: true }); await writeFile(join(output, path), data); return { path, byteLength: data.length, sha256: digest(data) }; };
  const archive = async (path) => {
    const rel = relative(root, path).replaceAll('\\', '/');
    const target = `archive/${rel.replaceAll('../', 'parent/')}.gz`;
    await mkdir(dirname(join(output, target)), { recursive: true });
    manifest.files.push({ source: rel, path: target, ...await archiveSourceFile(path, join(output, target), manifest.redactions) });
  };
  const seen = new Set();
  const archiveTree = async (path) => {
    if (!(await exists(path))) { manifest.missing.push(relative(root, path).replaceAll('\\', '/')); return; }
    const canonical = await realpath(path);
    if (seen.has(canonical)) return;
    seen.add(canonical);
    if (!canonical.toLowerCase().startsWith((await realpath(root)).toLowerCase() + '\\') && canonical !== await realpath(root)) { manifest.externalDependencies.push(relative(root, path)); return; }
    const info = await stat(path);
    if (info.isDirectory()) {
      for (const entry of await readdir(path, { withFileTypes: true })) {
        if (entry.isSymbolicLink() || ['node_modules', '.git', 'worktrees', 'workspaces'].includes(entry.name)) { manifest.externalDependencies.push(relative(root, join(path, entry.name))); continue; }
        if (entry.name === 'provider.pid') continue;
        await archiveTree(join(path, entry.name));
      }
    } else await archive(path);
  };
  const display = (node) => ({ code: node.status === 'completed' ? node.outcome ?? 'completed' : node.status, tone: node.status === 'paused' ? 'warning' : node.outcome === 'failure' ? 'danger' : node.outcome === 'success' ? 'success' : 'neutral', icon: node.status === 'paused' ? 'pause' : node.outcome === 'failure' ? 'error' : node.outcome === 'success' ? 'check' : 'dot', terminal: node.status === 'completed', resumable: false, blockingError: false });
  const tree = [];
  const graphNodes = []; const graphEdges = [];
  await archive(source); await archiveTree(join(root, run.workflow_snapshot));
  await archiveTree(join(root, 'events.jsonl'));
  manifest.task = await publish('task.json', JSON.parse(sanitize(JSON.stringify(task), manifest.redactions)));
  manifest.workflow = await publish('workflow.json', JSON.parse(sanitize(JSON.stringify(await json(join(root, run.workflow_snapshot))), manifest.redactions)));
  const round = await json(join(root, 'rounds', run.current_round, 'round.json'));
  await archiveTree(join(root, 'rounds', round.id, 'round.json'));
  for (const trace of round.trace) {
    const outer = join(root, 'rounds', round.id, 'nodes', trace.node_id, trace.attempt_id);
    await archiveTree(join(outer, 'node.json'));
    const dynamic = join(outer, 'dynamic');
    const graph = await json(join(dynamic, 'graph.json'));
    await archiveTree(join(dynamic, 'graph.json')); await archiveTree(join(dynamic, 'events.jsonl'));
    for (const [position, node] of graph.nodes.entries()) {
      const nodeDir = join(dynamic, 'nodes', node.id);
      await archiveTree(join(nodeDir, 'node.json'));
      const attempts = [];
      for (const entry of await readdir(nodeDir, { withFileTypes: true })) {
        if (!entry.isDirectory() || !/^attempt-\d+$/.test(entry.name)) continue;
        const attemptDir = join(nodeDir, entry.name);
        await archiveTree(attemptDir);
        const sessionDirs = [{ path: attemptDir, branchId: 'root' }];
        const agentsDir = join(attemptDir, 'agents');
        if (await exists(agentsDir)) for (const agent of await readdir(agentsDir, { withFileTypes: true })) if (agent.isDirectory()) sessionDirs.push({ path: join(agentsDir, agent.name), branchId: agent.name });
        let rootSession;
        for (const branch of sessionDirs) {
          const snapshotPath = join(attemptDir, 'acp.snapshot.json');
          const timelinePath = join(branch.path, branch.branchId === 'root' ? 'acp.timeline.jsonl' : 'timeline.jsonl');
          const indexPath = join(branch.path, branch.branchId === 'root' ? 'acp.timeline.index.json' : 'timeline.index.json');
          if (!(await exists(snapshotPath)) || !(await exists(indexPath))) { manifest.missing.push(relative(root, snapshotPath)); continue; }
          const snapshot = await json(snapshotPath); const index = await json(indexPath);
          if (index.coveredOffset !== (await stat(timelinePath)).size) throw new Error(`Stale timeline index: ${node.id}`);
          const key = `${round.id}/${trace.node_id}/${trace.attempt_id}/${node.id}/${entry.name}/${branch.branchId}`;
          const prefix = `sessions/${digest(key).slice(0, 24)}`;
          const handle = await open(timelinePath, 'r');
          const hydrate = async (value) => {
            if (!value || typeof value !== 'object') return value;
            if (value.$goldBandBlob) {
              const ref = value.$goldBandBlob;
              const blob = join(attemptDir, 'acp.file-blobs', ref.contentHash.slice(0, 2), ref.contentHash);
              if (!(await exists(blob))) { manifest.missing.push(relative(root, blob)); return '[Unavailable source blob]'; }
              const bytes = await readFile(blob);
              if (Buffer.from(blake3(bytes)).toString('hex') !== ref.contentHash || bytes.length !== ref.byteLength) throw new Error('Blob integrity mismatch');
              return sanitize(bytes.toString('utf8'), manifest.redactions);
            }
            for (const key of Object.keys(value)) value[key] = await hydrate(value[key]);
            return value;
          };
          const eventRefs = {};
          try {
            for (const [id, loc] of Object.entries(index.itemLocators)) {
              const bytes = Buffer.alloc(loc.lineLength); await handle.read(bytes, 0, bytes.length, loc.offset);
              const record = JSON.parse(bytes.toString('utf8'));
              if (record.item.id !== id || (record.revision ?? record.item.endedSeq ?? record.item.startedSeq ?? record.item.seq) !== loc.revision) throw new Error(`Timeline identity mismatch: ${node.id}, ${id}`);
              const event = await hydrate(JSON.parse(sanitize(JSON.stringify(record.item), manifest.redactions)));
              const ref = await publish(`${prefix}/events/${digest(id).slice(0, 24)}.json`, event);
              eventRefs[id] = { ...ref, id, seq: loc.seq, startedSeq: loc.startedSeq, endedSeq: loc.endedSeq, revision: loc.revision, kind: loc.kind, toolCallId: loc.toolCallId, branchId: loc.branchId, sessionId: event.sessionId };
            }
          } finally { await handle.close(); }
          const blocks = index.semanticBlocks.map((block) => ({ ...block, summary: block.summary ? JSON.parse(sanitize(JSON.stringify(block.summary), manifest.redactions)) : undefined }));
          const lightEvent = (event) => {
            if (event.kind !== 'toolCall') return event;
            return { ...event, content: null, raw: { toolCallId: event.toolCallId, title: event.title, status: event.status, _meta: { ...event.raw?._meta, goldBandConversation: { ...event.raw?._meta?.goldBandConversation, toolOutput: null, branchId: branch.branchId, toolDetailAvailable: true } } } };
          };
          const pages = [];
          for (let offset = 0; offset < blocks.length; offset += 50) {
            const events = [];
            for (const block of blocks.slice(offset, offset + 50)) {
              if (block.summary) events.push(block.summary);
              else for (const id of block.itemIds) {
                if (!eventRefs[id]) throw new Error(`Missing semantic item: ${id}`);
                events.push(lightEvent(await json(join(output, eventRefs[id].path))));
              }
            }
            pages.push(await publish(`${prefix}/pages/${pages.length}.json`, events));
          }
          const activityPages = [];
          const refs = Object.values(eventRefs).sort((a, b) => a.startedSeq - b.startedSeq || a.seq - b.seq);
          for (let offset = 0; offset < refs.length; offset += 50) {
            const events = [];
            for (const ref of refs.slice(offset, offset + 50)) {
              ref.activityPage = activityPages.length;
              events.push(lightEvent(await json(join(output, ref.path))));
            }
            activityPages.push(await publish(`${prefix}/activity/${activityPages.length}.json`, events));
          }
          const session = { branchId: branch.branchId, readOnly: true, sessionId: snapshot.sessionId, title: node.title, roundId: round.id, nodeId: node.id, attemptId: entry.name, outerNodeId: trace.node_id, outerAttemptId: trace.attempt_id, provider: node.provider, adapterId: snapshot.adapterId, adapterDisplayName: snapshot.adapterDisplayName, status: snapshot.latestTurnStatus ?? node.status, sessionStartedAt: snapshot.createdAt, sessionUpdatedAt: snapshot.updatedAt, restored: snapshot.restored ?? false, stopReason: snapshot.stopReason, timing: index.timing, config: { models: snapshot.models, modes: snapshot.modes, configOptions: snapshot.configOptions }, events: [], eventPage: { generation: index.generation, coveredRevision: index.coveredRevision, newestRevision: index.coveredRevision, loadedCount: 0, total: blocks.length, hasOlder: blocks.length > 0, hasNewer: false }, timelineProjection: null, pendingInteractions: [], usage: index.usage, diagnostics: { eventCount: index.eventCount, rawFrameCount: 0, errorCount: 0 } };
          const metadata = await publish(`${prefix}/index.json`, { session, blocks, eventRefs, pages, activityPages });
          manifest.sessions.push({ key, ...metadata, roundId: round.id, nodeId: node.id, attemptId: entry.name, outerNodeId: trace.node_id, outerAttemptId: trace.attempt_id, branchId: branch.branchId, sessionId: snapshot.sessionId, eventCount: index.eventCount, logicalCount: blocks.length, revision: index.coveredRevision });
          if (branch.branchId === 'root') rootSession = session;
        }
        attempts.push({ roundId: round.id, nodeId: node.id, attemptId: entry.name, outerNodeId: trace.node_id, outerAttemptId: trace.attempt_id, pathLabel: node.title, status: node.status, outcome: node.outcome, runtimeDisplay: display(node), current: node.status === 'paused', manualCheckPending: false, startedAt: node.startedAt, finishedAt: node.finishedAt, sessionId: rootSession?.sessionId, sessionEstablished: !!rootSession, artifactCount: 0, attachmentCount: 0 });
      }
      tree.push({ nodeId: node.id, label: node.title, nodeType: node.kind, status: node.status, runtimeDisplay: display(node), attempts });
      graphNodes.push({ id: node.id, nodeId: node.id, sequence: position + 1, label: node.title, nodeType: node.kind, status: node.status, outcome: node.outcome, runtimeDisplay: display(node), outerNodeId: trace.node_id, outerAttemptId: trace.attempt_id, attemptId: attempts.at(-1)?.attemptId, artifactCount: 0, attachmentCount: 0, current: node.status === 'paused' });
      graphEdges.push(...node.dependsOn.map((from) => ({ from, to: node.id, label: '' })));
    }
  }
  const selected = tree.flatMap((node) => node.attempts).find((leaf) => leaf.current) ?? tree[0].attempts[0];
  manifest.run = await publish('run.json', { projectId, taskId: run.task_id, taskUuid: run.task_uuid, runId: run.id, runMode: 'workflow', runStatus: run.status, runOutcome: run.outcome, pauseReason: run.pause_reason, lastActivityAt: run.updated_at, sessionTree: { rounds: [{ roundId: round.id, index: round.index, label: round.id, status: round.status, runtimeDisplay: display(round), nodes: tree }], selectedSessionKey: `${selected.roundId}/${selected.outerNodeId}/${selected.outerAttemptId}/${selected.nodeId}/${selected.attemptId}` }, selectedSession: null, activeSessions: [], inputAttachments: [], workflowStatus: 'valid', workflowValid: true, workflowJson: null, workflowGraph: { nodes: graphNodes, edges: graphEdges }, resumable: false });
  if (digest(await readFile(source)) !== digest(runBytes)) throw new Error('Run changed during export');
  manifest.missing = [...new Set(manifest.missing)];
  await publish('dataset.json', { version: manifest.version, projectId, taskId: run.task_id, taskUuid: run.task_uuid, runId: run.id, title: task.title, source: manifest.source, sessions: manifest.sessions, run: manifest.run, task: manifest.task, workflow: manifest.workflow });
  await publish('manifest.json', manifest);
  return { sessions: manifest.sessions.length, files: manifest.files.length, sourceBytes: manifest.files.reduce((sum, file) => sum + file.sourceBytes, 0), missing: manifest.missing, externalDependencies: manifest.externalDependencies, redactions: manifest.redactions };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2] || !process.argv[3]) throw new Error('Usage: export-demo-history.mjs SOURCE_RUN_JSON OUTPUT_DIRECTORY');
  console.log(JSON.stringify(await exportHistory(process.argv[2], process.argv[3]), null, 2));
}
