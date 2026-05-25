import { spawnSync } from 'node:child_process';
import process from 'node:process';

const rawVersion = process.argv[2];

if (!rawVersion) {
  console.error('Usage: npm run release:tag -- <version>');
  process.exit(1);
}

const version = rawVersion.replace(/^v/, '');
const tag = `v${version}`;

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid semver version: ${rawVersion}`);
  process.exit(1);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    stdio: options.capture ? 'pipe' : 'inherit',
  });

  if (result.status !== 0) {
    if (options.capture && result.stderr) {
      process.stderr.write(result.stderr);
    }
    process.exit(result.status ?? 1);
  }

  return result.stdout?.trim() ?? '';
}

function runAllowFailure(command, args) {
  return spawnSync(command, args, {
    encoding: 'utf8',
    stdio: 'pipe',
  });
}

const branch = run('git', ['branch', '--show-current'], { capture: true });

if (branch !== 'main') {
  console.error(`Release tags must be created from main. Current branch: ${branch || '(detached)'}`);
  process.exit(1);
}

const existingTag = runAllowFailure('git', ['rev-parse', '--verify', '--quiet', `refs/tags/${tag}`]);

if (existingTag.status === 0) {
  console.error(`Tag already exists locally: ${tag}`);
  process.exit(1);
}

const statusBefore = run('git', ['status', '--porcelain'], { capture: true });

if (statusBefore) {
  console.error('Working tree must be clean before creating a release tag. Commit or stash current changes first.');
  process.exit(1);
}

run('node', ['scripts/sync-version.mjs', version]);

const dirtyAfter = runAllowFailure('git', ['diff', '--quiet']);

if (dirtyAfter.status !== 0) {
  run('git', [
    'add',
    'package.json',
    'package-lock.json',
    'Cargo.toml',
    'Cargo.lock',
    'src-tauri/tauri.conf.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
  ]);
  run('git', ['commit', '-m', `chore(release): ${tag}`]);
}

run('git', ['tag', tag]);

console.log(`Created local release tag ${tag}.`);
console.log('Push it when ready:');
console.log('  git push origin main');
console.log(`  git push origin ${tag}`);
