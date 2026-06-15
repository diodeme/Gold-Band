import { execSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { basename, join, relative } from 'node:path';

import { channelEnvPrefix, readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

const channel = process.argv[2] ?? 'default';
const isCritical = process.argv[3] === 'critical';

let config;
try {
  config = readChannelConfig(channel);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

// ── channel versioning (non-default channels only) ──
const isDefaultChannel = channel === 'default';
const pkg = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'));
const baseVersion = pkg.version;

let channelVersion;
if (isDefaultChannel) {
  channelVersion = baseVersion;
} else {
  const now = new Date();
  const ts = now.getFullYear().toString() +
    String(now.getMonth() + 1).padStart(2, '0') +
    String(now.getDate()).padStart(2, '0') +
    String(now.getHours()).padStart(2, '0') +
    String(now.getMinutes()).padStart(2, '0');
  const shortSha = execSync('git rev-parse --short=7 HEAD', { encoding: 'utf8', cwd: repoRoot }).trim();
  channelVersion = `${baseVersion}-${channel}.${ts}+${shortSha}`;
  console.log(`Channel build: ${channel} ${channelVersion}`);
}

// ── env ──
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

// ── build ──
const overlayPath = join(repoRoot, 'src-tauri', 'target', 'channel', `tauri.${channel}.conf.json`);
writeTauriConfigOverlay(config, overlayPath, isDefaultChannel ? undefined : channelVersion);

// Clean stale bundle artifacts from both possible target locations
// (workspace root target/ takes precedence after Cargo workspace migration)
for (const dir of possibleBundleDirs()) {
  if (existsSync(dir)) {
    rmSync(dir, { recursive: true, force: true });
  }
}

const result = spawnSync('npx', ['tauri', 'build', '--config', overlayPath], {
  env,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

if (result.status !== 0) {
  console.error('Build failed.');
}

// ── post-build: collect artifacts & generate latest.json ──
if (result.status === 0 && config.releaseBaseUrl) {
  const releaseDir = join(repoRoot, 'release', channel);
  mkdirSync(releaseDir, { recursive: true });

  const bundleDir = findBundleDir();
  if (!bundleDir) {
    console.error('Could not locate bundle artifacts directory.');
    process.exit(1);
  }

  // Find all .sig files recursively and copy artifact + signature
  const sigFiles = [];
  walkDir(bundleDir, (filePath) => {
    if (filePath.endsWith('.sig')) sigFiles.push(filePath);
  });

  for (const sigPath of sigFiles) {
    const artifactPath = sigPath.slice(0, -4); // remove .sig
    if (existsSync(artifactPath)) {
      copyFileSync(artifactPath, join(releaseDir, basename(artifactPath)));
      copyFileSync(sigPath, join(releaseDir, basename(sigPath)));
    }
  }

  // Generate latest.json
  const version = isDefaultChannel ? baseVersion : channelVersion;
  const criticalFlag = isCritical ? ' --critical' : '';
  execSync(
    `node scripts/generate-updater-json.mjs "${releaseDir}" "${join(releaseDir, 'latest.json')}" --base-url "${config.releaseBaseUrl}" --version "${version}"${criticalFlag}`,
    { stdio: 'inherit', cwd: repoRoot },
  );

  console.log(`Release artifacts ready: ${relative(repoRoot, releaseDir)}`);
}

process.exit(result.status ?? 1);

function walkDir(dir, fn) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      walkDir(full, fn);
    } else {
      fn(full);
    }
  }
}

function possibleBundleDirs() {
  return [
    join(repoRoot, 'target', 'release', 'bundle'),
    join(repoRoot, 'src-tauri', 'target', 'release', 'bundle'),
  ];
}

function findBundleDir() {
  for (const dir of possibleBundleDirs()) {
    if (existsSync(dir)) return dir;
  }
  return null;
}
