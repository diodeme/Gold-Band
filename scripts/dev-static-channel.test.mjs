import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';

import { repoRoot } from './channel-config.mjs';
import {
  assertStaticDevRunDirectory,
  cleanupStaticDevSnapshot,
  staticDevBuildConfig,
  staticDevCliInvocation,
  staticDevEnvironment,
  staticDevPaths,
  staticDevPort,
  staticDevTauriArgs,
  staticDevWebBuildInvocation,
} from './dev-static-channel.mjs';

test('static dev derives an isolated frontend snapshot and Cargo target', () => {
  const repositoryRoot = join('workspace', 'repo');
  const paths = staticDevPaths(repositoryRoot, 'default', 'snapshot-1');

  assert.equal(
    paths.frontendSnapshotDir,
    join(repositoryRoot, 'src-tauri', 'target', 'static-dev', 'default', 'snapshot-1', 'frontend'),
  );
  assert.equal(paths.frontendDist, 'target/static-dev/default/snapshot-1/frontend');
  assert.equal(paths.cargoTargetDir, join(repositoryRoot, 'target', 'static-dev', 'default'));
});

test('static dev serves only the immutable snapshot without another build command', () => {
  assert.deepEqual(staticDevBuildConfig('target/static-dev/default/snapshot/frontend'), {
    build: {
      beforeDevCommand: null,
      devUrl: null,
      frontendDist: 'target/static-dev/default/snapshot/frontend',
    },
  });
});

test('static dev web build writes directly to the immutable snapshot', () => {
  assert.deepEqual(
    staticDevWebBuildInvocation('npm-cli.js', 'snapshot/frontend'),
    {
      command: process.execPath,
      args: [
        'npm-cli.js',
        'run',
        'web:build',
        '--',
        '--outDir',
        'snapshot/frontend',
        '--emptyOutDir',
      ],
    },
  );
});

test('static dev disables the Tauri source watcher and applies both configs', () => {
  assert.deepEqual(
    staticDevTauriArgs('channel.json', 'static.json'),
    [
      'dev',
      '--no-watch',
      '--config',
      'channel.json',
      '--config',
      'static.json',
    ],
  );
});

test('static dev supports an isolated validation port and rejects invalid ports', () => {
  assert.equal(staticDevPort(undefined), undefined);
  assert.equal(staticDevPort('1431'), 1431);
  assert.throws(() => staticDevPort('0'), /Invalid GOLD_BAND_STATIC_DEV_PORT/);
  assert.throws(() => staticDevPort('not-a-port'), /Invalid GOLD_BAND_STATIC_DEV_PORT/);
  assert.deepEqual(
    staticDevTauriArgs('channel.json', 'static.json', 1431),
    [
      'dev',
      '--no-watch',
      '--config',
      'channel.json',
      '--config',
      'static.json',
      '--port',
      '1431',
    ],
  );
});

test('static dev invokes the local Tauri CLI through Node without a Windows cmd shim', () => {
  assert.deepEqual(
    staticDevCliInvocation('tauri.js', 'channel.json', 'static.json'),
    {
      command: process.execPath,
      args: [
        'tauri.js',
        'dev',
        '--no-watch',
        '--config',
        'channel.json',
        '--config',
        'static.json',
      ],
    },
  );
});

test('static dev isolates Cargo output and disables oversized Windows PDB files', () => {
  assert.deepEqual(
    staticDevEnvironment({ EXISTING: 'kept' }, 'default', 'static-cargo'),
    {
      EXISTING: 'kept',
      GOLD_BAND_RELEASE_CHANNEL: 'default',
      CARGO_PROFILE_DEV_DEBUG: '0',
      CARGO_TARGET_DIR: 'static-cargo',
    },
  );
});

test('static dev cleanup only removes a child snapshot directory', () => {
  const staticDevRootDir = mkdtempSync(join(tmpdir(), 'gold-band-static-dev-'));
  const runDir = join(staticDevRootDir, 'default', 'snapshot-1');
  try {
    mkdirSync(runDir, { recursive: true });
    writeFileSync(join(runDir, 'marker'), 'snapshot');

    assert.doesNotThrow(() => assertStaticDevRunDirectory(runDir, staticDevRootDir));
    assert.throws(
      () => assertStaticDevRunDirectory(staticDevRootDir, staticDevRootDir),
      /Refusing to clean unsafe static dev directory/,
    );
    assert.throws(
      () => assertStaticDevRunDirectory(join(staticDevRootDir, '..', 'outside'), staticDevRootDir),
      /Refusing to clean unsafe static dev directory/,
    );

    cleanupStaticDevSnapshot(runDir, staticDevRootDir);
    assert.equal(existsSync(runDir), false);
  } finally {
    rmSync(staticDevRootDir, { recursive: true, force: true });
  }
});

test('package exposes the default-channel static dev command', () => {
  const packageJson = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'));

  assert.equal(
    packageJson.scripts['dev:static'],
    'node scripts/dev-static-channel.mjs default',
  );
});
