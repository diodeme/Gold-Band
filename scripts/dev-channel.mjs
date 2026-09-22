import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

export function devChannelEnvironment(baseEnv, channel, lowMemory = false) {
  const environment = {
    ...baseEnv,
    GOLD_BAND_RELEASE_CHANNEL: channel,
  };
  if (lowMemory && environment.CARGO_BUILD_JOBS == null) {
    environment.CARGO_BUILD_JOBS = '1';
  }
  return environment;
}

function run() {
  const args = process.argv.slice(2);
  const lowMemory = args.includes('--low-memory');
  const channel = args.find((arg) => !arg.startsWith('--')) ?? 'default';

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

  const env = devChannelEnvironment(process.env, channel, lowMemory);

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
