import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

import { channelEnvPrefix, readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

const channel = process.argv[2] ?? 'default';
let config;
try {
  config = readChannelConfig(channel);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

const env = {
  ...process.env,
  GOLD_BAND_RELEASE_CHANNEL: channel,
};
const upper = channelEnvPrefix(channel);
const privateKey = env[`${upper}_TAURI_SIGNING_PRIVATE_KEY`] || env.TAURI_SIGNING_PRIVATE_KEY;
const password = env[`${upper}_TAURI_SIGNING_PRIVATE_KEY_PASSWORD`];

if (privateKey) {
  env.TAURI_SIGNING_PRIVATE_KEY = privateKey;
}

if (password) {
  env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD = password;
} else {
  delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
}

const overlayPath = join(repoRoot, 'src-tauri', 'target', 'channel', `tauri.${channel}.conf.json`);
writeTauriConfigOverlay(config, overlayPath);

const result = spawnSync('npx', ['tauri', 'build', '--config', overlayPath], {
  env,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

process.exit(result.status ?? 1);
