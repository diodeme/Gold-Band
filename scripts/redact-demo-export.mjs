import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gzipSync, gunzipSync } from 'node:zlib';
import { resolve, join } from 'node:path';
import { sanitize } from './export-demo-history.mjs';
import { fileSnapshot } from './demo-file-snapshot.mjs';

if (!process.argv[2]) throw new Error('Dataset directory is required');
const root = resolve(process.argv[2]);
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const load = async (path) => JSON.parse(await readFile(join(root, path), 'utf8'));
const manifest = await load('manifest.json');
const counts = {}; let changedArchives = 0; let changedResources = 0;
for (const [index, file] of (process.argv.includes('--resources-only') ? [] : manifest.files).entries()) {
  let compressed = await readFile(join(root, file.path));
  const bytes = gunzipSync(compressed);
  let text; try { text = new TextDecoder('utf8', { fatal: true }).decode(bytes); } catch { text = null; }
  if (text != null && !text.includes('\0')) {
    const next = sanitize(text, counts);
    if (next !== text) { compressed = gzipSync(Buffer.from(next)); await writeFile(join(root, file.path), compressed); changedArchives++; }
  }
  file.sha256 = hash(compressed); file.byteLength = compressed.length;
  if (index % 1000 === 0) console.log(JSON.stringify({ archives: index, changedArchives }));
}
const seen = new Map();
const rebuiltSnapshots = new Map();
if (process.argv.includes('--restore-json-snapshots')) {
  for (const ref of manifest.sessions.filter((item) => item.branchId === 'root')) {
    const prefix = `rounds/${ref.roundId}/nodes/${ref.outerNodeId}/${ref.outerAttemptId}/dynamic/nodes/${ref.nodeId}/${ref.attemptId}/`;
    const key = ref.path.replace('/index.json', '');
    for (const file of manifest.files.filter((item) => item.source.startsWith(prefix) && /\.(json|jsonl)$/.test(item.source))) {
      const relativePath = file.source.slice(prefix.length);
      rebuiltSnapshots.set(`${key}/files/${hash(Buffer.from(relativePath))}.json`, { file, key, relativePath });
    }
  }
}
async function project(value) {
  if (typeof value === 'string') return sanitize(value, counts);
  if (!value || typeof value !== 'object') return value;
  if (typeof value.path === 'string' && typeof value.sha256 === 'string' && typeof value.byteLength === 'number') {
    if (!seen.has(value.path)) {
      if (/\\|\.\.|:/.test(value.path) || value.path.startsWith('/')) throw new Error('Unsafe resource path');
      const old = await readFile(join(root, value.path));
      if (hash(old) !== value.sha256 || old.length !== value.byteLength) throw new Error(`Resource integrity: ${value.path}`);
      const replacement = rebuiltSnapshots.get(value.path);
      const input = replacement ? fileSnapshot(gunzipSync(await readFile(join(root, replacement.file.path))), manifest.projectId, replacement.key, replacement.relativePath).snapshot : JSON.parse(old);
      const next = Buffer.from(JSON.stringify(await project(input)));
      if (!old.equals(next)) { await writeFile(join(root, value.path), next); changedResources++; }
      seen.set(value.path, { path: value.path, byteLength: next.length, sha256: hash(next) });
      if (seen.size % 1000 === 0) console.log(JSON.stringify({ resources: seen.size, changedResources }));
    }
    const { path: _path, byteLength: _length, sha256: _hash, ...metadata } = value;
    return { ...await project(metadata), ...seen.get(value.path) };
  }
  if (Array.isArray(value)) {
    const result = [];
    for (const child of value) result.push(await project(child));
    return result;
  }
  const result = {};
  for (const [key, child] of Object.entries(value)) {
    const credential = typeof child === 'string' && /(?:token$|^(?:api[_-]?key|password|client[_-]?secret)$)/i.test(key)
      && !(key === 'token' && /^demo-history\/sessions\/[a-f0-9]{24}\/images\/[a-f0-9]{64}\./.test(child));
    result[key] = credential ? '[REDACTED_CREDENTIAL]' : await project(child);
  }
  if (result.kind === 'text' && typeof result.content === 'string' && result.revision) {
    result.revision = { ...result.revision, contentHash: hash(Buffer.from(result.content)), byteLength: Buffer.byteLength(result.content) };
  }
  return result;
}
const dataset = await project(await load('dataset.json'));
manifest.sessions = dataset.sessions; manifest.run = dataset.run;
manifest.task = dataset.task; manifest.workflow = dataset.workflow;
manifest.redactions = { ...manifest.redactions, exportPass: counts };
await writeFile(join(root, 'dataset.json'), JSON.stringify(dataset));
await writeFile(join(root, 'manifest.json'), JSON.stringify(manifest));
console.log(JSON.stringify({ changedArchives, changedResources, resources: seen.size, rules: counts }));
