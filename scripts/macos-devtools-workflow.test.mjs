import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptsDir, '..');
const workflowPath = path.join(
  repoRoot,
  '.github',
  'workflows',
  'macos-intel-devtools.yml',
);

test('manual Intel macOS diagnostic workflow builds and uploads only a DevTools DMG', async () => {
  const workflow = (await readFile(workflowPath, 'utf8')).replaceAll('\r\n', '\n');

  assert.match(workflow, /^on:\n  workflow_dispatch:\s*$/m);
  assert.match(workflow, /^permissions:\n  contents: read\s*$/m);
  assert.match(workflow, /^    runs-on: macos-15-intel\s*$/m);
  assert.match(workflow, /uses: actions\/checkout@v4/);
  assert.match(workflow, /uses: actions\/setup-node@v4/);
  assert.match(workflow, /node-version: 22/);
  assert.match(workflow, /uses: dtolnay\/rust-toolchain@stable/);
  assert.match(workflow, /run: npm ci/);
  assert.match(workflow, /run: node scripts\/configure-macos-signing\.mjs/);
  assert.match(workflow, /run: npm run build -- --devtools/);
  assert.match(workflow, /uses: actions\/upload-artifact@v4/);
  assert.match(workflow, /target\/release\/bundle\/dmg\/\*\.dmg/);
  assert.match(workflow, /src-tauri\/target\/release\/bundle\/dmg\/\*\.dmg/);
  assert.match(workflow, /if-no-files-found: error/);

  assert.doesNotMatch(workflow, /tauri-apps\/tauri-action/);
  assert.doesNotMatch(workflow, /tagName:|releaseName:|latest\.json|TAURI_SIGNING_PRIVATE_KEY/);
});
