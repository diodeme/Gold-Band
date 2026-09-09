import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';

const [input, reportPath] = process.argv.slice(2);
if (!input || !reportPath) throw new Error('Usage: node scripts/verify-demo-history.mjs DATASET REPORT');
const root = resolve(input);
const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
const report = { dataset: root, source: manifest.source, sessions: manifest.sessions.length, archiveFiles: manifest.files.length,
  sourceBytes: manifest.files.reduce((sum, file) => sum + file.sourceBytes, 0), missing: manifest.missing,
  externalDependencies: manifest.externalDependencies, resourcesChecked: 0, archivesChecked: 0, failures: [],
  privacyReview: 'pending-binary-and-external-reference-audit', structuralComplete: false };
const seen = new Set();
const inventory = new Map();
async function inspect(value) {
  if (!value || typeof value !== 'object') return;
  if (typeof value.path === 'string' && typeof value.sha256 === 'string' && typeof value.byteLength === 'number') {
    if (value.path.startsWith('/') || /\\|\.\.|:/.test(value.path)) throw new Error('Unsafe resource path');
    if (!seen.has(value.path)) {
      seen.add(value.path);
      const bytes = await readFile(join(root, value.path));
      if (bytes.length !== value.byteLength || createHash('sha256').update(bytes).digest('hex') !== value.sha256) throw new Error(`Resource integrity: ${value.path}`);
      report.resourcesChecked++;
      inventory.set(value.path, { path: value.path, byteLength: bytes.length, sha256: value.sha256 });
      await inspect(JSON.parse(bytes));
    }
  }
  if (value.kind === 'image' && typeof value.previewGrant?.token === 'string') {
    const path = value.previewGrant.token.replace(/^demo-history\//, '');
    if (!/^sessions\/[a-f0-9]{24}\/images\/[a-f0-9]{64}\.(png|jpg|gif|webp|bmp|ico|avif)$/.test(path)) throw new Error('Unsafe image path');
    const bytes = await readFile(join(root, path));
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    if (bytes.length !== value.revision.byteLength || sha256 !== value.revision.contentHash) throw new Error(`Image integrity: ${path}`);
    inventory.set(path, { path, byteLength: bytes.length, sha256 });
  }
  for (const child of Object.values(value)) await inspect(child);
}
try { await inspect(JSON.parse(await readFile(join(root, 'dataset.json'), 'utf8'))); }
catch (error) { report.failures.push(String(error)); }
for (const file of manifest.files) {
  try {
    const bytes = await readFile(join(root, file.path));
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    if (bytes.length !== file.byteLength || sha256 !== file.sha256) throw new Error(`Archive integrity: ${file.path}`);
    inventory.set(file.path, { path: file.path, byteLength: bytes.length, sha256 });
    report.archivesChecked++;
  } catch (error) { report.failures.push(String(error)); }
}
report.structuralComplete = !report.missing.length && !report.failures.length;
for (const path of ['dataset.json', 'manifest.json']) {
  const bytes = await readFile(join(root, path));
  inventory.set(path, { path, byteLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });
}
if (!report.failures.length) await writeFile(join(root, 'publish-index.json'), JSON.stringify({ files: [...inventory.values()] }));
await writeFile(resolve(reportPath), JSON.stringify(report, null, 2));
console.log(JSON.stringify({ ...report, missing: report.missing.length, failures: report.failures.length }));
if (!report.structuralComplete) process.exitCode = 1;
