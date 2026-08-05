import { spawnSync } from 'node:child_process';
import { existsSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { randomUUID } from 'node:crypto';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

export function staticDevPaths(repositoryRoot, channel, snapshotId) {
  const srcTauriDir = join(repositoryRoot, 'src-tauri');
  const staticDevRootDir = join(srcTauriDir, 'target', 'static-dev');
  const runDir = join(staticDevRootDir, channel, snapshotId);
  const frontendSnapshotDir = join(runDir, 'frontend');

  return {
    staticDevRootDir,
    runDir,
    frontendSnapshotDir,
    frontendDist: relative(srcTauriDir, frontendSnapshotDir).split(sep).join('/'),
    staticConfigPath: join(runDir, 'tauri.static.conf.json'),
    channelOverlayPath: join(srcTauriDir, 'target', 'channel', `tauri.${channel}.conf.json`),
    cargoTargetDir: join(repositoryRoot, 'target', 'static-dev', channel),
  };
}

export function staticDevBuildConfig(frontendDist) {
  return {
    build: {
      beforeDevCommand: null,
      devUrl: null,
      frontendDist,
    },
  };
}

export function staticDevWebBuildInvocation(npmCliPath, frontendSnapshotDir) {
  return {
    command: process.execPath,
    args: [
      npmCliPath,
      'run',
      'web:build',
      '--',
      '--outDir',
      frontendSnapshotDir,
      '--emptyOutDir',
    ],
  };
}

export function staticDevPort(rawPort) {
  if (rawPort === undefined || rawPort === '') return undefined;
  if (!/^\d+$/u.test(rawPort)) {
    throw new Error(`Invalid GOLD_BAND_STATIC_DEV_PORT: ${rawPort}`);
  }
  const port = Number(rawPort);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65_535) {
    throw new Error(`Invalid GOLD_BAND_STATIC_DEV_PORT: ${rawPort}`);
  }
  return port;
}

export function staticDevTauriArgs(overlayPath, staticConfigPath, port) {
  const args = [
    'dev',
    '--no-watch',
    '--config',
    overlayPath,
    '--config',
    staticConfigPath,
  ];
  if (port !== undefined) {
    args.push('--port', String(port));
  }
  return args;
}

export function staticDevCliInvocation(tauriCliPath, overlayPath, staticConfigPath, port) {
  return {
    command: process.execPath,
    args: [tauriCliPath, ...staticDevTauriArgs(overlayPath, staticConfigPath, port)],
  };
}

export function staticDevEnvironment(baseEnv, channel, cargoTargetDir) {
  return {
    ...baseEnv,
    GOLD_BAND_RELEASE_CHANNEL: channel,
    CARGO_PROFILE_DEV_DEBUG: '0',
    CARGO_TARGET_DIR: cargoTargetDir,
  };
}

export function assertStaticDevRunDirectory(runDir, staticDevRootDir) {
  const relativePath = relative(resolve(staticDevRootDir), resolve(runDir));
  if (
    !relativePath
    || relativePath === '..'
    || relativePath.startsWith(`..${sep}`)
    || isAbsolute(relativePath)
  ) {
    throw new Error(`Refusing to clean unsafe static dev directory: ${runDir}`);
  }
}

export function cleanupStaticDevSnapshot(runDir, staticDevRootDir) {
  assertStaticDevRunDirectory(runDir, staticDevRootDir);
  rmSync(runDir, { recursive: true, force: true });
}

function runInvocation(invocation, env = process.env) {
  const result = spawnSync(invocation.command, invocation.args, {
    env,
    stdio: 'inherit',
    shell: false,
  });

  if (result.error) {
    console.error(result.error.message);
  }

  return result.status ?? 1;
}

function run() {
  const channel = process.argv[2] ?? 'default';
  const snapshotId = `${process.pid}-${randomUUID()}`;
  const paths = staticDevPaths(repoRoot, channel, snapshotId);

  console.log(`Preparing immutable static dev snapshot (channel: ${channel})...`);

  let config;
  try {
    config = readChannelConfig(channel);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }

  const npmCliPath = process.env.npm_execpath;
  if (!npmCliPath) {
    console.error('npm executable path is unavailable. Start this command with npm run dev:static.');
    return 1;
  }

  let port;
  try {
    port = staticDevPort(process.env.GOLD_BAND_STATIC_DEV_PORT);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }

  try {
    const webBuildStatus = runInvocation(
      staticDevWebBuildInvocation(npmCliPath, paths.frontendSnapshotDir),
    );
    if (webBuildStatus !== 0) {
      return webBuildStatus;
    }

    if (!existsSync(join(paths.frontendSnapshotDir, 'index.html'))) {
      console.error(`Static frontend snapshot is incomplete: ${paths.frontendSnapshotDir}`);
      return 1;
    }

    writeTauriConfigOverlay(config, paths.channelOverlayPath);
    writeFileSync(
      paths.staticConfigPath,
      `${JSON.stringify(staticDevBuildConfig(paths.frontendDist), null, 2)}\n`,
    );

    console.log(`Starting static Tauri dev client (channel: ${channel})...`);

    const require = createRequire(import.meta.url);
    const tauriCliPath = require.resolve('@tauri-apps/cli/tauri.js');
    const invocation = staticDevCliInvocation(
      tauriCliPath,
      paths.channelOverlayPath,
      paths.staticConfigPath,
      port,
    );
    const env = staticDevEnvironment(process.env, channel, paths.cargoTargetDir);
    return runInvocation(invocation, env);
  } finally {
    cleanupStaticDevSnapshot(paths.runDir, paths.staticDevRootDir);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(run());
}
