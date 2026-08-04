import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test, { after, before } from 'node:test';

import { repoRoot } from './channel-config.mjs';

const ignoreFile = join(repoRoot, '.taurignore');
let isolatedRepository;

before(() => {
  isolatedRepository = mkdtempSync(join(tmpdir(), 'gold-band-dev-watch-'));
  const result = spawnSync('git', ['init', '--quiet'], {
    cwd: isolatedRepository,
    encoding: 'utf8',
    shell: false,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(result.stderr || `git init exited with status ${result.status}`);
  }
});

after(() => {
  rmSync(isolatedRepository, { recursive: true, force: true });
});

function isIgnoredByTauri(relativePath) {
  const result = spawnSync(
    'git',
    ['-c', `core.excludesFile=${ignoreFile}`, 'check-ignore', '--no-index', relativePath],
    {
      cwd: isolatedRepository,
      encoding: 'utf8',
      shell: false,
    },
  );

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && result.status !== 1) {
    throw new Error(result.stderr || `git check-ignore exited with status ${result.status}`);
  }

  return result.status === 0;
}

test('Tauri dev watcher ignores documentation-only changes', () => {
  for (const path of [
    'docs/gold-band/product-design.md',
    'docs/nested/README.md',
    'README.md',
    'README.en.md',
  ]) {
    assert.equal(isIgnoredByTauri(path), true, `${path} should be ignored`);
  }
});

test('Tauri dev watcher keeps runtime and build inputs observable', () => {
  for (const path of [
    'Cargo.toml',
    'src/lib.rs',
    'src-tauri/src/main.rs',
    'web/src/main.tsx',
    'package.json',
  ]) {
    assert.equal(isIgnoredByTauri(path), false, `${path} should remain observable`);
  }
});
