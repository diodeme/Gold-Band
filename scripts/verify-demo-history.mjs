import { readFile, writeFile, stat } from 'node:fs/promises';
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
async function inspect(value) {
  if (!value || typeof value !== 'object') return;
  if (typeof value.path === 'string' && typeof value.sha256 === 'string' && typeof value.byteLength === 'number') {
    if (value.path.startsWith('/') || /\\|\.\.|:/.test(value.path)) throw new Error('Unsafe resource path');
    if (!seen.has(value.path)) {
      seen.add(value.path);
      const bytes = await readFile(join(root, value.path));
      if (bytes.length !== value.byteLength || createHash('sha256').update(bytes).digest('hex') !== value.sha256) throw new Error(`Resource integrity: ${value.path}`);
      report.resourcesChecked++;
      await inspect(JSON.parse(bytes));
    }
  }
  for (const child of Object.values(value)) await inspect(child);
}
try { await inspect(JSON.parse(await readFile(join(root, 'dataset.json'), 'utf8'))); }
catch (error) { report.failures.push(String(error)); }
for (const file of manifest.files) {
  try {
    const info = await stat(join(root, file.path));
    if (info.size !== file.byteLength) throw new Error(`Archive size: ${file.path}`);
    report.archivesChecked++;
  } catch (error) { report.failures.push(String(error)); }
}
report.structuralComplete = !report.missing.length && !report.failures.length;
await writeFile(resolve(reportPath), JSON.stringify(report, null, 2));
console.log(JSON.stringify({ ...report, missing: report.missing.length, failures: report.failures.length }));
if (!report.structuralComplete) process.exitCode = 1;
