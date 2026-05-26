import { execSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { basename, join, relative } from 'node:path';

import { channelEnvPrefix, readChannelConfig, repoRoot, writeTauriConfigOverlay } from './channel-config.mjs';

const channel = process.argv[2] ?? 'default';

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
let channelTag;
if (isDefaultChannel) {
  channelVersion = baseVersion;
} else {
  const tagPattern = `v${baseVersion}-${channel}.*`;
  let existingTags = [];
  try {
    existingTags = execSync(`git tag -l "${tagPattern}"`, { encoding: 'utf8', cwd: repoRoot })
      .trim()
      .split('\n')
      .filter(Boolean);
  } catch {
    // no tags yet
  }

  let maxCounter = 0;
  const escapedBase = baseVersion.replace(/\./g, '\\.');
  const escapedChannel = channel.replace(/\./g, '\\.');
  const tagRegex = new RegExp(`^v${escapedBase}-${escapedChannel}\\.(\\d+)$`);
  for (const tag of existingTags) {
    const match = tag.trim().match(tagRegex);
    if (match) {
      const n = parseInt(match[1], 10);
      if (n > maxCounter) maxCounter = n;
    }
  }

  const counter = String(maxCounter + 1);
  channelVersion = `${baseVersion}-${channel}.${counter}`;
  channelTag = `v${channelVersion}`;
  console.log(`Channel build: ${channel} ${channelVersion} (tag: ${channelTag})`);
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

const result = spawnSync('npx', ['tauri', 'build', '--config', overlayPath], {
  env,
  stdio: 'inherit',
  shell: process.platform === 'win32',
});

// ── tag on success ──
if (result.status === 0 && !isDefaultChannel) {
  execSync(`git tag "${channelTag}"`, { stdio: 'inherit', cwd: repoRoot });
  try {
    execSync(`git push origin "${channelTag}"`, { stdio: 'inherit', cwd: repoRoot });
  } catch {
    console.error(`Tag ${channelTag} created locally but push failed. Push manually: git push origin "${channelTag}"`);
  }
} else if (result.status !== 0) {
  console.error('Build failed — tag skipped.');
}

// ── post-build: collect artifacts & generate latest.json ──
if (result.status === 0 && config.releaseBaseUrl) {
  const bundleDir = join(repoRoot, 'src-tauri', 'target', 'release', 'bundle');
  const releaseDir = join(repoRoot, 'release', channel);
  mkdirSync(releaseDir, { recursive: true });

  // Find all .sig files recursively and copy artifact + signature
  const sigFiles = [];
  if (existsSync(bundleDir)) {
    walkDir(bundleDir, (filePath) => {
      if (filePath.endsWith('.sig')) sigFiles.push(filePath);
    });
  }

  for (const sigPath of sigFiles) {
    const artifactPath = sigPath.slice(0, -4); // remove .sig
    if (existsSync(artifactPath)) {
      copyFileSync(artifactPath, join(releaseDir, basename(artifactPath)));
      copyFileSync(sigPath, join(releaseDir, basename(sigPath)));
    }
  }

  // Generate latest.json
  const version = isDefaultChannel ? baseVersion : channelVersion;
  execSync(
    `node scripts/generate-updater-json.mjs "${releaseDir}" "${join(releaseDir, 'latest.json')}" --base-url "${config.releaseBaseUrl}" --version "${version}"`,
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
