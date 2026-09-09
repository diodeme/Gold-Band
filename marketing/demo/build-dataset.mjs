import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

export function checkDataset(source, allowIncomplete = false) {
  if (!existsSync(join(source, 'dataset.json')) || !existsSync(join(source, 'manifest.json'))) {
    throw new Error('Demo dataset is required: set DEMO_DATASET_PATH.');
  }
  const manifest = JSON.parse(readFileSync(join(source, 'manifest.json'), 'utf8'));
  if (manifest.missing.length && !allowIncomplete) {
    throw new Error(`Demo source has ${manifest.missing.length} missing references. DEMO_ALLOW_INCOMPLETE_DATASET=1 is for local verification only.`);
  }
}
