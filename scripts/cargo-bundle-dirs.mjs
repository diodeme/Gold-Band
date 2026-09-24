import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { isAbsolute, join, normalize, resolve } from 'node:path';

export function readCargoTargetDirectory(repoRoot, env = process.env, { readMetadata } = {}) {
  const reader = readMetadata ?? defaultReadCargoMetadata;
  try {
    const metadata = reader(repoRoot, env);
    const fromMetadata = resolveTargetDirectory(metadata?.target_directory, repoRoot);
    if (fromMetadata) return fromMetadata;
  } catch {
    // Fall back to env so a missing cargo binary still honors this machine's target dir.
  }
  return cargoTargetDirectoryFromEnv(env, repoRoot);
}

export function possibleBundleDirs({
  repoRoot,
  env = process.env,
  cargoTargetDirectory,
} = {}) {
  const targetDir = resolveTargetDirectory(cargoTargetDirectory, repoRoot)
    ?? cargoTargetDirectoryFromEnv(env, repoRoot);
  const dirs = [];
  if (targetDir) {
    dirs.push(join(targetDir, 'release', 'bundle'));
    const triple = typeof env.CARGO_BUILD_TARGET === 'string' ? env.CARGO_BUILD_TARGET.trim() : '';
    if (triple) {
      dirs.push(join(targetDir, triple, 'release', 'bundle'));
    }
  }
  dirs.push(
    join(repoRoot, 'target', 'release', 'bundle'),
    join(repoRoot, 'src-tauri', 'target', 'release', 'bundle'),
  );
  return uniqueNormalized(dirs);
}

export function findBundleDir(options) {
  for (const dir of possibleBundleDirs(options)) {
    if (existsSync(dir)) return dir;
  }
  return null;
}

export function defaultReadCargoMetadata(repoRoot, env) {
  const output = execSync('cargo metadata --format-version 1 --no-deps', {
    cwd: repoRoot,
    env,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
    windowsHide: true,
  });
  return JSON.parse(output);
}

function cargoTargetDirectoryFromEnv(env, repoRoot) {
  return resolveTargetDirectory(env?.CARGO_TARGET_DIR || env?.CARGO_BUILD_TARGET_DIR, repoRoot);
}

function resolveTargetDirectory(targetDir, repoRoot) {
  if (typeof targetDir !== 'string') return null;
  const trimmed = targetDir.trim();
  if (!trimmed) return null;
  return normalize(isAbsolute(trimmed) ? trimmed : resolve(repoRoot, trimmed));
}

function uniqueNormalized(paths) {
  const seen = new Set();
  const result = [];
  for (const value of paths) {
    const normalized = normalize(value);
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    result.push(normalized);
  }
  return result;
}
