import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gzipSync, gunzipSync } from 'node:zlib';
import { dirname, join, resolve } from 'node:path';
import { sanitize } from './export-demo-history.mjs';

const [source, output, reportPath] = process.argv.slice(2);
if (!source || !output || !reportPath) throw new Error('Source run, dataset and report are required');
const root = resolve(output); const sourceRoot = dirname(resolve(source));
const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
if (hash(await readFile(source)) !== manifest.source.runSha256) throw new Error('Source revision changed');
const report = { files: 0, sourceInvalidJson: [], retainedArchiveSources: [], failures: [], imageObjects: 0, imageRefs: 0, externalImages: 0, rules: {} };
function inspect(value) {
  if (!value || typeof value !== 'object') return;
  if (value.type === 'image' || value.kind === 'image') { report.imageObjects++; if (value.url) report.externalImages++; }
  if (value.imageRef) report.imageRefs++;
  for (const child of Object.values(value)) inspect(child);
}
for (const file of manifest.files.filter((file) => /\.(json|jsonl)$/.test(file.source))) {
  try {
    const path = resolve(sourceRoot, file.source);
    if (!path.startsWith(sourceRoot + '\\')) throw new Error('Source path outside authorized run');
    let bytes;
    try {
      bytes = await readFile(path);
      if (hash(bytes) !== file.sourceSha256) throw new Error('Source archive hash changed');
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      const saved = await readFile(join(root, file.path));
      if (hash(saved) !== file.sha256) throw new Error('Retained archive hash changed');
      bytes = gunzipSync(saved);
      JSON.parse(bytes);
      report.retainedArchiveSources.push(file.source);
    }
    const text = bytes.toString('utf8');
    const lines = file.source.endsWith('.jsonl') ? text.split('\n') : [text];
    const next = lines.map((line, index) => {
      if (!line.trim()) return line;
      let parsed;
      try { parsed = JSON.parse(line); } catch { report.sourceInvalidJson.push({ path: file.source, line: index + 1 }); }
      if (file.source.endsWith('acp.timeline.jsonl')) inspect(parsed);
      const result = sanitize(line, report.rules);
      if (parsed !== undefined) JSON.parse(result);
      return result;
    }).join('\n');
    const compressed = gzipSync(Buffer.from(next));
    await writeFile(join(root, file.path), compressed);
    file.sha256 = hash(compressed); file.byteLength = compressed.length;
    report.files++;
  } catch (error) { report.failures.push({ path: file.source, error: error.message }); }
}
await writeFile(join(root, 'manifest.json'), JSON.stringify(manifest));
await writeFile(resolve(reportPath), JSON.stringify(report, null, 2));
console.log(JSON.stringify({ ...report, sourceInvalidJson: report.sourceInvalidJson.length }));
if (report.failures.length) process.exitCode = 1;
