import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  findBundleDir,
  possibleBundleDirs,
  readCargoTargetDirectory,
} from './cargo-bundle-dirs.mjs';

function createRoot(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'gold-band-bundle-dirs-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('possibleBundleDirs keeps workspace and src-tauri fallbacks', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  assert.deepEqual(possibleBundleDirs({ repoRoot, env: {} }), [
    path.join(repoRoot, 'target', 'release', 'bundle'),
    path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle'),
  ]);
});

test('possibleBundleDirs includes CARGO_TARGET_DIR so local channel builds can collect updater artifacts', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const cargoTarget = path.resolve('/machine/rust/target');
  const dirs = possibleBundleDirs({
    repoRoot,
    env: { CARGO_TARGET_DIR: cargoTarget },
  });
  assert.equal(dirs[0], path.join(cargoTarget, 'release', 'bundle'));
  assert.ok(dirs.includes(path.join(repoRoot, 'target', 'release', 'bundle')));
  assert.ok(dirs.includes(path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle')));
});

test('possibleBundleDirs includes CARGO_BUILD_TARGET_DIR and host-triple bundle paths', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const cargoTarget = path.resolve('/machine/rust/target');
  const dirs = possibleBundleDirs({
    repoRoot,
    env: {
      CARGO_BUILD_TARGET_DIR: cargoTarget,
      CARGO_BUILD_TARGET: 'x86_64-pc-windows-msvc',
    },
  });
  assert.ok(dirs.includes(path.join(cargoTarget, 'release', 'bundle')));
  assert.ok(
    dirs.includes(path.join(cargoTarget, 'x86_64-pc-windows-msvc', 'release', 'bundle')),
  );
});

test('possibleBundleDirs uses cargo metadata target_directory first so user cargo config works across machines', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const cargoTarget = path.resolve('/shared/rust/target');
  const dirs = possibleBundleDirs({
    repoRoot,
    env: {},
    cargoTargetDirectory: cargoTarget,
  });
  assert.equal(dirs[0], path.join(cargoTarget, 'release', 'bundle'));
});

test('possibleBundleDirs does not duplicate the workspace target when CARGO_TARGET_DIR points at it', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const dirs = possibleBundleDirs({
    repoRoot,
    env: { CARGO_TARGET_DIR: path.join(repoRoot, 'target') },
  });
  assert.deepEqual(dirs, [
    path.join(repoRoot, 'target', 'release', 'bundle'),
    path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle'),
  ]);
});

test('readCargoTargetDirectory prefers cargo metadata target_directory over a local env override', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const fromCargo = path.resolve('/shared/rust/target');
  const actual = readCargoTargetDirectory(
    repoRoot,
    { CARGO_TARGET_DIR: path.resolve('/wrong/target') },
    {
      readMetadata: () => ({ target_directory: fromCargo }),
    },
  );
  assert.equal(actual, path.normalize(fromCargo));
});

test('readCargoTargetDirectory falls back to CARGO_TARGET_DIR when cargo metadata is unavailable', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const cargoTarget = path.resolve('/machine/rust/target');
  const actual = readCargoTargetDirectory(
    repoRoot,
    { CARGO_TARGET_DIR: cargoTarget },
    {
      readMetadata: () => {
        throw new Error('cargo missing');
      },
    },
  );
  assert.equal(actual, path.normalize(cargoTarget));
});

test('readCargoTargetDirectory resolves a relative target dir against the repo root when metadata is unavailable', () => {
  const repoRoot = path.resolve('/gold-band-repo');
  const actual = readCargoTargetDirectory(
    repoRoot,
    { CARGO_TARGET_DIR: 'shared-target' },
    {
      readMetadata: () => {
        throw new Error('cargo missing');
      },
    },
  );
  assert.equal(actual, path.resolve(repoRoot, 'shared-target'));
});

test('findBundleDir prefers cargo target bundle over leftover repo target artifacts', (t) => {
  const root = createRoot(t);
  const repoRoot = path.join(root, 'repo');
  const cargoTarget = path.join(root, 'machine-target');
  mkdirSync(path.join(repoRoot, 'target', 'release', 'bundle'), { recursive: true });
  mkdirSync(path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle'), { recursive: true });
  mkdirSync(path.join(cargoTarget, 'release', 'bundle'), { recursive: true });

  const found = findBundleDir({
    repoRoot,
    env: { CARGO_TARGET_DIR: cargoTarget },
  });
  assert.equal(found, path.join(cargoTarget, 'release', 'bundle'));
});

test('build-channel collects artifacts from cargo-resolved bundle directories', () => {
  const source = readFileSync(path.resolve('scripts/build-channel.mjs'), 'utf8');
  assert.match(source, /readCargoTargetDirectory/);
  assert.match(source, /possibleBundleDirs/);
  assert.match(source, /findBundleDir/);
  assert.doesNotMatch(source, /join\(repoRoot, 'target', 'release', 'bundle'\)/);
});
