import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

export function devChannelEnvironment(baseEnv, channel) {
  return {
    ...baseEnv,
    GOLD_BAND_RELEASE_CHANNEL: channel,
    CARGO_BUILD_JOBS: baseEnv.CARGO_BUILD_JOBS ?? '1',
  };
}

function run() {
  const channel = process.argv[2] ?? 'default';

  console.log(`Starting Tauri dev server (channel: ${channel})...`);

  // Read channel config and apply channel-specific product name / window title etc.
  let config;
  try {
    config = readChannelConfig(channel);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }

  const overlayPath = join(repoRoot, 'src-tauri', 'target', 'channel', `tauri.${channel}.conf.json`);
  writeTauriConfigOverlay(config, overlayPath);

  const env = devChannelEnvironment(process.env, channel);

  const result = spawnSync('npx', ['tauri', 'dev', '--config', overlayPath], {
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });

  return result.status ?? 1;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(run());
}
