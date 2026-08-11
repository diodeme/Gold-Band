import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptsDir, '..');

test('release-please declares the pre-1.0 breaking-change policy', async () => {
  const rawConfig = await readFile(
    path.join(repoRoot, 'release-please-config.json'),
    'utf8',
  );
  const config = JSON.parse(rawConfig);

  assert.equal(config.packages?.['.']?.['bump-minor-pre-major'], true);
});
