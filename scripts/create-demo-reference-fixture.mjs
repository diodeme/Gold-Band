import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { projectArchiveRun } from './project-demo-run.mjs';

// An isolated browser acceptance fixture, never a replacement for the source archive.
const output = resolve('.codex-temp/demo-reference-fixture');
await mkdir(resolve(output, 'assets'), { recursive: true });
await mkdir(resolve(output, 'resources'), { recursive: true });
const publish = async (bytes, directory, extension = '') => {
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const path = `${directory}/${sha256}${extension}`;
  await writeFile(resolve(output, path), bytes);
  return { path, sha256, bytes: Buffer.byteLength(bytes) };
};
const asset = async value => (await publish(JSON.stringify(value), 'assets', '.json')).path;
const file = async (name, content, image = null) => ({ name, kind: 'file', resource: await publish(content, 'resources'), image });
const png = await readFile('marketing/site/media/zh-before.png');
const image = { mimeType: 'image/png', width: png.readUInt32BE(16), height: png.readUInt32BE(20), animated: false };
const reports = await asset({ entries: [
  await file('report.md', '# Reference acceptance\n\n![Product view](product.png)\n\n[Open notes](notes.md#L3)\n\n[Missing file](missing.md)\n\n[Project source](https://github.com/diodeme/Gold-Band)\n'),
  await file('notes.md', '# Notes\n\nLinked file content.\n'),
  await file('product.png', png, image),
] });
const directoryRoot = await asset({ entries: [{ name: 'reports', kind: 'directory', directory: reports, hasChildren: true }] });
const events = await asset([{ id: 'fixture-event', seq: 1, kind: 'textDelta', content: 'Markdown reference acceptance fixture.', timestamp: '1788960000Z', sessionId: 'fixture-session' }]);
const empty = await publish('', 'resources');
const detail = await asset({ directoryRoot, changeSets: {}, snapshot: { sessionId: 'fixture-session', latestTurnStatus: 'paused' }, workerRef: { provider: 'codex-acp' },
  diagnostics: { rawFrameCount: 0, eventCount: 1, errorCount: 0, lastError: null, lastErrorTimestamp: null },
  rawFrames: { resource: empty.path, bytes: 0, count: 0, pages: [] },
  branches: [{ id: 'root', count: 1, tools: {}, pages: [{ path: events, firstSeq: 1, lastSeq: 1, count: 1 }] }] });
const session = { projectId: 'reference-fixture', taskId: 'reference-test', runId: 'run-1', roundId: 'round-1', nodeId: 'review', attemptId: 'attempt-1',
  node: { id: 'review', title: 'Reference test', status: 'paused', outcome: null, startedAt: '1788960000Z' }, detail, itemCount: 1, recordCount: 1 };
const catalog = { version: 1, runtimeVersion: 1, projectId: session.projectId, task: { id: session.taskId, title: 'Markdown reference acceptance fixture' },
  run: { id: session.runId, status: 'paused', outcome: null, startedAt: '1788960000Z', updatedAt: '1788960000Z', current_round: 'round-1', current_node: 'review', current_attempt: 'attempt-1' },
  workflow: await asset({}), sessions: [session] };
catalog.runView = await asset(projectArchiveRun(catalog, [{ id: 'round-1', index: 1, status: 'paused', outcome: null }], []));
await writeFile(resolve(output, 'catalog.json'), JSON.stringify(catalog));
console.log(JSON.stringify({ output, projectId: session.projectId, taskId: session.taskId, runId: session.runId }));
