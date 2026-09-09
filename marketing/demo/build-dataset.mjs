import { readFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, dirname } from 'node:path';

export function checkDataset(source, allowIncomplete = false) {
  if (!existsSync(join(source, 'dataset.json')) || !existsSync(join(source, 'manifest.json'))) {
    throw new Error('Demo dataset is required: set DEMO_DATASET_PATH.');
  }
  const manifest = JSON.parse(readFileSync(join(source, 'manifest.json'), 'utf8'));
  if (manifest.missing.length && !allowIncomplete) {
    throw new Error(`Demo source has ${manifest.missing.length} missing references. DEMO_ALLOW_INCOMPLETE_DATASET=1 is for local verification only.`);
  }
  if (!existsSync(join(source, 'publish-index.json'))) throw new Error('Run verify-demo-history.mjs to create the verified publication inventory.');
}

export function copyDataset(source, destination) {
  const inventory = JSON.parse(readFileSync(join(source, 'publish-index.json'), 'utf8'));
  for (const file of inventory.files) {
    if (file.path.startsWith('/') || /\\|\.\.|:/.test(file.path)) throw new Error('Unsafe dataset publication path');
    const target = join(destination, file.path);
    const bytes = readFileSync(join(source, file.path));
    if (bytes.length !== file.byteLength || createHash('sha256').update(bytes).digest('hex') !== file.sha256) throw new Error(`Dataset publication integrity: ${file.path}`);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, bytes);
  }
}
