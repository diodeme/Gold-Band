import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

import { readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

const channel = process.argv[2] ?? 'default';

console.log(`Starting Tauri dev server (channel: ${channel})...`);

// Read channel config and apply channel-specific product name / window title etc.
let config;
try {
  config = readChannelConfig(channel);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

const overlayPath = join(repoRoot, 'src-tauri', 'target', 'channel', `tauri.${channel}.conf.json`);
writeTauriConfigOverlay(config, overlayPath);

const env = { ...process.env, GOLD_BAND_RELEASE_CHANNEL: channel };

const result = spawnSync('npx', ['tauri', 'dev', '--config', overlayPath], {
  env,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

process.exit(result.status ?? 1);
