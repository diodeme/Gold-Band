import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { sanitize } from './export-demo-history.mjs';

const [source, output] = process.argv.slice(2);
if (!source || !output) throw new Error('Source run and dataset directory are required');
const root = resolve(output);
const load = async (path) => JSON.parse(await readFile(path, 'utf8'));
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sourceBytes = await readFile(resolve(source));
const run = JSON.parse(sourceBytes);
const manifest = await load(join(root, 'manifest.json'));
const dataset = await load(join(root, 'dataset.json'));
if (hash(sourceBytes) !== manifest.source.runSha256) throw new Error('Source revision changed');
for (const [key, path] of [['task', join(dirname(source), '../../task.json')], ['workflow', join(dirname(source), run.workflow_snapshot)]]) {
  const bytes = await readFile(path);
  const published = Buffer.from(sanitize(JSON.stringify(JSON.parse(bytes))));
  const descriptor = { path: `${key}.json`, byteLength: published.length, sha256: hash(published), sourceSha256: hash(bytes) };
  await writeFile(join(root, descriptor.path), published);
  dataset[key] = descriptor; manifest[key] = descriptor;
}
await writeFile(join(root, 'dataset.json'), JSON.stringify(dataset));
await writeFile(join(root, 'manifest.json'), JSON.stringify(manifest));
console.log(JSON.stringify({ task: dataset.task, workflow: dataset.workflow }));
