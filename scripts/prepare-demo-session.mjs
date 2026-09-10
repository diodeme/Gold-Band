import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, realpath, rename, writeFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { projectArchiveRun } from './project-demo-run.mjs';
import { archiveToolSummary } from './export-demo-session.mjs';
import { prepareArchiveChangeSet } from './prepare-demo-files.mjs';
import { prepareArchiveDirectories } from './prepare-demo-directories.mjs';

export const RAW_PAGE_SIZE = 96;
export function attemptResourcePrefix(session) {
  const node = session.outerNodeId ?? session.nodeId;
  const attempt = session.outerAttemptId ?? session.attemptId;
  const base = `rounds/${session.roundId}/nodes/${node}/${attempt}`;
  return session.outerNodeId ? `${base}/dynamic/nodes/${session.nodeId}/${session.attemptId}` : base;
}

export async function inspectAttemptLogs(rawPath, diagnosticsPath) {
  const pages = [];
  let rawFrameCount = 0;
  let offset = 0;
  let start = 0;
  let count = 0;
  // Exported structured resources are canonical UTF-8 JSONL with one LF per record.
  for await (const line of createInterface({ input: createReadStream(rawPath), crlfDelay: Infinity })) {
    if (!line.trim()) throw new Error('archive.raw-empty-record');
    JSON.parse(line);
    offset += Buffer.byteLength(line) + 1;
    count++;
    rawFrameCount++;
    if (count === RAW_PAGE_SIZE) {
      pages.push({ offset: start, length: offset - start, count, firstLine: rawFrameCount - count + 1 });
      start = offset;
      count = 0;
    }
  }
  if (count) pages.push({ offset: start, length: offset - start, count, firstLine: rawFrameCount - count + 1 });
  const diagnostics = { rawFrameCount, eventCount: 0, errorCount: 0, lastError: null, lastErrorTimestamp: null };
  for await (const line of createInterface({ input: createReadStream(diagnosticsPath), crlfDelay: Infinity })) {
    if (!line.trim()) continue;
    const value = JSON.parse(line);
    if (value.level === 'error') {
      diagnostics.errorCount++;
      if (typeof value.message === 'string') {
        diagnostics.lastError = value.message;
        diagnostics.lastErrorTimestamp = typeof value.timestamp === 'string' ? value.timestamp : null;
      }
    }
  }
  return { diagnostics, pages, bytes: offset };
}

export async function prepareDemoSession(directory) {
  const root = await realpath(directory);
  const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
  const catalog = JSON.parse(await readFile(join(root, 'catalog.json'), 'utf8'));
  const resources = new Map(manifest.resources.map(resource => [resource.source, resource]));
  const assets = new Map(manifest.assets.map(asset => [asset.path, asset]));
  async function emit(value) {
    const content = Buffer.from(JSON.stringify(value));
    const sha256 = createHash('sha256').update(content).digest('hex');
    const path = `assets/${sha256}.json`;
    await writeFile(join(root, path), content);
    assets.set(path, { path, bytes: content.length, sha256 });
    return path;
  }
  async function resource(source) {
    const entry = resources.get(source);
    if (!entry || !/^resources\/[a-f0-9]{64}$/.test(entry.path)) throw new Error('archive.required-resource-missing');
    const path = await realpath(join(root, entry.path));
    if (!path.startsWith(`${root}${sep}`)) throw new Error('archive.resource-outside-root');
    return { ...entry, absolutePath: path };
  }
  let rawFrames = 0;
  const rounds = [];
  const dynamicGraphs = [];
  const changeSetSources = new Map();
  for (const source of resources.keys()) {
    const fileSet = /^(.*)\/turn-file-change-sets\/[^/]+\.json$/.exec(source);
    if (fileSet) {
      const list = changeSetSources.get(fileSet[1]) ?? [];
      list.push(source);
      changeSetSources.set(fileSet[1], list);
    }
    if (/^rounds\/[^/]+\/round\.json$/.test(source)) {
      rounds.push(JSON.parse(await readFile((await resource(source)).absolutePath, 'utf8')));
    }
    const match = /^rounds\/([^/]+)\/nodes\/([^/]+)\/([^/]+)\/dynamic\/graph\.json$/.exec(source);
    if (match) dynamicGraphs.push({ roundId: match[1], nodeId: match[2], attemptId: match[3],
      graph: JSON.parse(await readFile((await resource(source)).absolutePath, 'utf8')) });
  }
  const directoryRoots = await prepareArchiveDirectories(manifest.resources, emit, resource);
  for (const session of manifest.sessions) {
    const prefix = attemptResourcePrefix(session);
    const raw = await resource(`${prefix}/acp.raw.jsonl`);
    const diagnostic = await resource(`${prefix}/acp.diagnostics.jsonl`);
    const worker = await resource(`${prefix}/worker-ref.json`);
    const index = await inspectAttemptLogs(raw.absolutePath, diagnostic.absolutePath);
    if (index.bytes !== raw.bytes) throw new Error('archive.raw-byte-boundary');
    if (!/^assets\/[a-f0-9]{64}\.json$/.test(session.detail)) throw new Error('archive.detail-path-invalid');
    const detail = JSON.parse(await readFile(join(root, session.detail), 'utf8'));
    detail.directoryRoot = directoryRoots.get(prefix);
    if (!detail.directoryRoot) throw new Error('archive.session-directory-missing');
    detail.changeSets = {};
    const attachmentPaths = new Set();
    for (const source of changeSetSources.get(prefix) ?? []) {
      const files = await prepareArchiveChangeSet(source, resource);
      if (detail.changeSets[files.changeSet.id]) throw new Error('archive.change-set-duplicate');
      detail.changeSets[files.changeSet.id] = await emit(files);
      files.changeSet.attachments.forEach(attachment => attachmentPaths.add(attachment.relativePath));
      for (const missing of files.missing) if (!manifest.missing.some(entry => entry.path === missing.path && entry.reason === missing.reason)) manifest.missing.push(missing);
    }
    session.node.attachmentCount = attachmentPaths.size;
    for (const branch of detail.branches) for (const page of branch.pages) {
      if (!/^assets\/[a-f0-9]{64}\.json$/.test(page.path)) throw new Error('archive.page-path-invalid');
      const events = JSON.parse(await readFile(join(root, page.path), 'utf8'));
      if (events.some(event => event.kind === 'toolCall')) page.path = await emit(events.map(event => event.kind === 'toolCall' ? archiveToolSummary(event) : event));
    }
    const workerSource = JSON.parse(await readFile(worker.absolutePath, 'utf8'));
    const workerRef = { provider: workerSource.provider, mode: workerSource.mode,
      continueRef: workerSource.continueRef == null ? null : Object.fromEntries(
        ['sessionId', 'acpSessionId', 'adapterId', 'adapterDisplayName', 'cwd'].filter(key => workerSource.continueRef[key] !== undefined)
          .map(key => [key, workerSource.continueRef[key]])) };
    const content = Buffer.from(JSON.stringify({ ...detail, diagnostics: index.diagnostics, workerRef,
      rawFrames: { resource: raw.path, bytes: raw.bytes, count: index.diagnostics.rawFrameCount, pages: index.pages } }));
    const sha256 = createHash('sha256').update(content).digest('hex');
    const path = `assets/${sha256}.json`;
    await writeFile(join(root, path), content);
    assets.set(path, { path, bytes: content.length, sha256 });
    session.detail = path;
    rawFrames += index.diagnostics.rawFrameCount;
  }
  const runContent = Buffer.from(JSON.stringify(projectArchiveRun(manifest, rounds, dynamicGraphs)));
  const runHash = createHash('sha256').update(runContent).digest('hex');
  const runView = `assets/${runHash}.json`;
  await writeFile(join(root, runView), runContent);
  assets.set(runView, { path: runView, bytes: runContent.length, sha256: runHash });
  manifest.runView = runView;
  catalog.runView = runView;
  manifest.assets = [...assets.values()];
  manifest.runtimeVersion = 1;
  manifest.counts.bytes = manifest.assets.reduce((sum, asset) => sum + asset.bytes, 0);
  catalog.sessions = manifest.sessions;
  catalog.runtimeVersion = 1;
  // Content-addressed details are written first; publish the lightweight catalog last.
  await writeFile(join(root, 'manifest.json.tmp'), JSON.stringify(manifest, null, 2));
  await rename(join(root, 'manifest.json.tmp'), join(root, 'manifest.json'));
  await writeFile(join(root, 'catalog.json.tmp'), JSON.stringify(catalog));
  await rename(join(root, 'catalog.json.tmp'), join(root, 'catalog.json'));
  return { sessions: manifest.sessions.length, rawFrames, runtimeVersion: 1 };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2]) throw new Error('Usage: node scripts/prepare-demo-session.mjs <archive-directory>');
  console.log(JSON.stringify(await prepareDemoSession(process.argv[2]), null, 2));
}
