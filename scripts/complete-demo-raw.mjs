import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
import { resolve, join, dirname } from 'node:path';

if (!process.argv[2]) throw new Error('Dataset directory is required');
const root = resolve(process.argv[2]);
const load = async (path) => JSON.parse(await readFile(join(root, path), 'utf8'));
async function publish(path, value) {
  const bytes = Buffer.from(JSON.stringify(value));
  await mkdir(dirname(join(root, path)), { recursive: true }); await writeFile(join(root, path), bytes);
  return { path, byteLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
}
const manifest = await load('manifest.json'); const dataset = await load('dataset.json');
let total = 0;
for (const ref of manifest.sessions.filter((ref) => ref.branchId === 'root')) {
  const prefix = `rounds/${ref.roundId}/nodes/${ref.outerNodeId}/${ref.outerAttemptId}/dynamic/nodes/${ref.nodeId}/${ref.attemptId}/`;
  const archive = manifest.files.find((file) => file.source === prefix + 'acp.raw.jsonl');
  if (!archive) throw new Error(`Missing raw archive for ${ref.nodeId}`);
  const text = gunzipSync(await readFile(join(root, archive.path))).toString('utf8').trimEnd();
  const lines = text ? text.split('\n') : [];
  const frames = lines.map((content, index) => {
    let value; try { value = JSON.parse(content); } catch { value = null; }
    const frame = value?.frame;
    const kind = value === null ? 'parse-error' : frame?.params?.update?.sessionUpdate ?? frame?.method ?? (frame?.error !== undefined ? 'error' : frame?.result !== undefined ? 'result' : 'frame');
    return { id: `raw-${index + 1}`, lineNumber: index + 1, timestamp: value?.timestamp ?? null, direction: value?.direction ?? null, kind, content, contentTruncated: false };
  });
  const key = ref.path.replace('/index.json', ''); const pages = [];
  for (let start = 0; start < frames.length; start += 50) pages.push(await publish(`${key}/raw/${pages.length}.json`, frames.slice(start, start + 50)));
  const raw = await publish(`${key}/raw/index.json`, { frames: frames.map(({ content: _, ...item }, index) => ({ ...item, page: Math.floor(index / 50) })), pages });
  for (const branch of manifest.sessions.filter((branch) => branch.roundId === ref.roundId && branch.nodeId === ref.nodeId && branch.attemptId === ref.attemptId && branch.outerNodeId === ref.outerNodeId && branch.outerAttemptId === ref.outerAttemptId)) {
    const index = await load(branch.path);
    index.raw = raw; index.session.diagnostics.rawFrameCount = frames.length;
    Object.assign(branch, await publish(branch.path, index));
    Object.assign(dataset.sessions.find((item) => item.path === branch.path), branch);
  }
  total += frames.length;
}
await publish('manifest.json', manifest); await publish('dataset.json', dataset);
console.log(JSON.stringify({ rawFrames: total, rootSessions: manifest.sessions.filter((ref) => ref.branchId === 'root').length }));
