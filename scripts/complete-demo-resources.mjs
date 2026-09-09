import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { dirname, resolve, join } from 'node:path';
import { createHash } from 'node:crypto';
import { blake3 } from '@noble/hashes/blake3.js';
import { sanitize } from './export-demo-history.mjs';
import { gunzipSync } from 'node:zlib';
import { indexResources } from './index-demo-resources.mjs';
import { fileSnapshot } from './demo-file-snapshot.mjs';

const [source, output] = process.argv.slice(2).map((path) => resolve(path));
if (!source || !output) throw new Error('Source run and output are required');
const root = dirname(source);
const parse = async (path) => JSON.parse(await readFile(path, 'utf8'));
const manifest = await parse(join(output, 'manifest.json'));
const resourceIndex = {};
const files = {};
const counts = new Map();
const archivedFiles = new Map(manifest.files.map((file) => [file.source, file]));
const archived = async (file) => gunzipSync(await readFile(join(output, file.path)));
const hash = (data) => createHash('sha256').update(data).digest('hex');
async function publish(path, value) {
  const bytes = Buffer.from(JSON.stringify(value));
  await mkdir(dirname(join(output, path)), { recursive: true }); await writeFile(join(output, path), bytes);
  return { path, sha256: hash(bytes), byteLength: bytes.length };
}
for (const ref of manifest.sessions.filter((ref) => ref.branchId === 'root')) {
  const prefix = `rounds/${ref.roundId}/nodes/${ref.outerNodeId}/${ref.outerAttemptId}/dynamic/nodes/${ref.nodeId}/${ref.attemptId}/`;
  const key = ref.path.replace('/index.json', '');
  const changes = {};
  const directoryFiles = {};
  const attemptFiles = manifest.files.filter((file) => file.source.startsWith(prefix));
  const directCount = (name) => attemptFiles.filter((file) => file.source.startsWith(prefix + name + '/') && !file.source.slice((prefix + name + '/').length).includes('/')).length;
  counts.set(key, { artifactCount: directCount('artifacts'), attachmentCount: directCount('attachments') });
  for (const file of attemptFiles) {
    const bytes = await archived(file);
    const relativePath = file.source.slice(prefix.length);
    const { snapshot, imagePath } = fileSnapshot(bytes, manifest.projectId, key, relativePath);
    if (imagePath) {
      await mkdir(dirname(join(output, imagePath)), { recursive: true }); await writeFile(join(output, imagePath), bytes);
    }
    const published = await publish(`${key}/files/${hash(relativePath)}.json`, snapshot);
    const canonicalPath = snapshot.locator.canonicalPath;
    files[canonicalPath] = published; directoryFiles[relativePath] = { canonicalPath, fileByteLength: bytes.length, ...published };
  }
  for (const file of manifest.files.filter((file) => file.source.startsWith(prefix + 'turn-file-change-sets/') && file.source.endsWith('.json'))) {
    const changeSet = JSON.parse((await archived(file)).toString('utf8'));
    const comparisons = {};
    for (const change of changeSet.changes) {
      const snapshot = async (version) => {
        if (!version) return null;
        const path = join(root, prefix, 'acp.file-blobs', version.contentHash.slice(0, 2), version.contentHash);
        let bytes;
        try {
          bytes = await readFile(path);
          if (Buffer.from(blake3(bytes)).toString('hex') !== version.contentHash) throw new Error('File version hash mismatch');
        } catch (error) {
          if (error.code !== 'ENOENT') throw error;
          const file = archivedFiles.get(prefix + 'acp.file-blobs/' + version.contentHash.slice(0, 2) + '/' + version.contentHash);
          if (!file) { manifest.missing.push(prefix + 'acp.file-blobs/' + version.contentHash); return null; }
          bytes = await archived(file);
        }
        return { version, content: sanitize(bytes.toString('utf8'), manifest.redactions) };
      };
      comparisons[change.id] = await publish(`${key}/comparisons/${hash(changeSet.id + change.id)}.json`, { changeSetId: changeSet.id, changeId: change.id, path: change.logicalPath, stats: { addedLines: change.addedLines, deletedLines: change.deletedLines }, before: await snapshot(change.beforeVersion), after: await snapshot(change.afterVersion), limitationCode: change.limitationCode });
    }
    changes[changeSet.id] = { ...(await publish(`${key}/changes/${hash(changeSet.id)}.json`, changeSet)), comparisons };
  }
  resourceIndex[key] = { changes, files: directoryFiles };
  console.log(JSON.stringify({ node: ref.nodeId, files: Object.keys(directoryFiles).length }));
}
const dataset = await parse(join(output, 'dataset.json'));
const run = await parse(join(output, dataset.run.path));
for (const round of run.sessionTree.rounds) for (const node of round.nodes) for (const leaf of node.attempts) {
  const ref = manifest.sessions.find((ref) => ref.branchId === 'root' && ref.roundId === leaf.roundId && ref.nodeId === leaf.nodeId && ref.attemptId === leaf.attemptId && ref.outerNodeId === leaf.outerNodeId && ref.outerAttemptId === leaf.outerAttemptId);
  if (!ref) throw new Error('Unresolved session leaf');
  const count = counts.get(ref.path.replace('/index.json', ''));
  Object.assign(leaf, count);
  const graphNode = run.workflowGraph.nodes.find((item) => item.nodeId === leaf.nodeId && item.attemptId === leaf.attemptId && item.outerNodeId === leaf.outerNodeId && item.outerAttemptId === leaf.outerAttemptId);
  if (graphNode) Object.assign(graphNode, count);
}
dataset.run = await publish('run.json', run); manifest.run = dataset.run;
dataset.resources = await publish('resources.json', { sessions: resourceIndex, files });
manifest.missing = [...new Set(manifest.missing)];
await publish('dataset.json', dataset); await publish('manifest.json', manifest);
await indexResources(output, { sessions: resourceIndex, files });
console.log(JSON.stringify({ sessions: Object.keys(resourceIndex).length, textAttachments: Object.keys(files).length, missing: manifest.missing }, null, 2));
