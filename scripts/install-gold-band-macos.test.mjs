import assert from 'node:assert/strict';
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const installer = path.resolve('scripts/install-gold-band-macos.sh');
const bash = resolveBash();
const bashProbe = spawnSync(bash, ['--version'], { encoding: 'utf8' });
const bashAvailable = !bashProbe.error && bashProbe.status === 0;
const digest = 'a'.repeat(64);

function resolveBash() {
  if (process.env.GOLD_BAND_TEST_BASH) return process.env.GOLD_BAND_TEST_BASH;
  if (process.platform !== 'win32') return 'bash';

  const whereGit = spawnSync('where.exe', ['git'], { encoding: 'utf8' });
  const gitPath = whereGit.stdout?.split(/\r?\n/).find(Boolean);
  if (gitPath) {
    const candidate = path.join(path.dirname(path.dirname(gitPath)), 'bin', 'bash.exe');
    if (existsSync(candidate)) return candidate;
  }
  return 'bash';
}

function shellPath(filePath) {
  const normalized = path.resolve(filePath).replaceAll('\\', '/');
  const driveMatch = normalized.match(/^([A-Za-z]):\/(.*)$/);
  if (!driveMatch) return normalized;
  return `/${driveMatch[1].toLowerCase()}/${driveMatch[2]}`;
}

function writeExecutable(filePath, content) {
  writeFileSync(filePath, content, 'utf8');
  chmodSync(filePath, 0o755);
}

function createHarness(t, overrides = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'gold-band-installer-'));
  const bin = path.join(root, 'bin');
  const home = path.join(root, 'home');
  const temporary = path.join(root, 'tmp');
  const installDirectory = path.join(root, 'Applications');
  const existingApp = path.join(installDirectory, 'Gold Band.app');
  mkdirSync(bin, { recursive: true });
  mkdirSync(path.join(home, 'Downloads'), { recursive: true });
  mkdirSync(temporary, { recursive: true });
  mkdirSync(existingApp, { recursive: true });
  writeFileSync(path.join(existingApp, 'stale.txt'), 'old-version');
  t.after(() => rmSync(root, { recursive: true, force: true }));

  writeExecutable(path.join(bin, 'uname'), `#!/usr/bin/env bash
if [[ "\${1:-}" == "-s" ]]; then printf 'Darwin\\n'; else printf 'arm64\\n'; fi
`);

  writeExecutable(path.join(bin, 'curl'), `#!/usr/bin/env bash
set -euo pipefail
output=''
url=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output|-o) output="$2"; shift 2 ;;
    *) url="$1"; shift ;;
  esac
done
printf '%s\\n' "$url" >> "$TEST_CURL_LOG"
if [[ "$url" == *api.github.com* ]]; then
  printf '{}\\n' > "$output"
elif [[ "$url" == *.sha256 ]]; then
  if [[ "\${TEST_MISSING_CHECKSUM:-0}" == "1" ]]; then exit 22; fi
  asset_name="\${url##*/}"
  asset_name="\${asset_name%.sha256}"
  if [[ "\${TEST_BAD_CHECKSUM_NAME:-0}" == "1" ]]; then asset_name='wrong.dmg'; fi
  printf '%s  %s\\n' "$TEST_DIGEST" "$asset_name" > "$output"
else
  printf 'fake-dmg' > "$output"
fi
`);

  writeExecutable(path.join(bin, 'plutil'), `#!/usr/bin/env bash
set -euo pipefail
key=''
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-extract" ]]; then key="$2"; break; fi
  shift
done
case "$key" in
  tag_name) printf 'v1.2.3\\n' ;;
  CFBundleIdentifier) printf '%s\\n' "\${TEST_BUNDLE_ID:-local.gold-band.desktop}" ;;
  *) exit 1 ;;
esac
`);

  writeExecutable(path.join(bin, 'shasum'), `#!/usr/bin/env bash
printf '%s  %s\\n' "\${TEST_ACTUAL_DIGEST:-$TEST_DIGEST}" "\${@: -1}"
`);

  writeExecutable(path.join(bin, 'hdiutil'), `#!/usr/bin/env bash
set -euo pipefail
action="$1"
shift
printf '%s\\n' "$action" >> "$TEST_HDIUTIL_LOG"
case "$action" in
  verify|detach) exit 0 ;;
  attach)
    mount_path=''
    while [[ $# -gt 0 ]]; do
      if [[ "$1" == "-mountpoint" ]]; then mount_path="$2"; break; fi
      shift
    done
    mkdir -p "$mount_path/Gold Band.app/Contents"
    printf '<plist/>\\n' > "$mount_path/Gold Band.app/Contents/Info.plist"
    printf 'new-version\\n' > "$mount_path/Gold Band.app/version.txt"
    ;;
  *) exit 1 ;;
esac
`);

  writeExecutable(path.join(bin, 'codesign'), `#!/usr/bin/env bash
if [[ "\${TEST_FAIL_FINAL_CODESIGN:-0}" == "1" && "\${@: -1}" == "$GOLD_BAND_INSTALL_DIR/Gold Band.app" ]]; then
  exit 1
fi
exit 0
`);
  writeExecutable(path.join(bin, 'xattr'), `#!/usr/bin/env bash
exit 0
`);
  writeExecutable(path.join(bin, 'sudo'), `#!/usr/bin/env bash
exec "$@"
`);
  writeExecutable(path.join(bin, 'mv'), `#!/usr/bin/env bash
if [[ "\${TEST_FAIL_RESTORE:-0}" == "1" && "$1" == "$GOLD_BAND_INSTALL_DIR"/.gold-band-backup.*/"Gold Band.app" && "$2" == "$GOLD_BAND_INSTALL_DIR/Gold Band.app" ]]; then
  exit 1
fi
exec /usr/bin/mv "$@"
`);
  writeExecutable(path.join(bin, 'ditto'), `#!/usr/bin/env bash
set -euo pipefail
while [[ $# -gt 0 && "$1" == -* ]]; do shift; done
source_path="$1"
destination_path="$2"
cp -R "$source_path" "$destination_path"
`);

  const env = {
    ...process.env,
    HOME: shellPath(home),
    TMPDIR: shellPath(temporary),
    GOLD_BAND_INSTALL_DIR: shellPath(installDirectory),
    TEST_CURL_LOG: shellPath(path.join(root, 'curl.log')),
    TEST_HDIUTIL_LOG: shellPath(path.join(root, 'hdiutil.log')),
    TEST_DIGEST: digest,
    TEST_BUNDLE_ID: 'local.gold-band.desktop',
    MSYS2_ARG_CONV_EXCL: '*',
    ...overrides,
  };
  const commandPath = `${shellPath(bin)}:/usr/bin:/bin`;

  return {
    env,
    existingApp,
    installDirectory,
    root,
    run: (...args) =>
      spawnSync(
        bash,
        [
          '-c',
          'export PATH="$1"; shift; exec bash "$@"',
          'gold-band-installer-test',
          commandPath,
          shellPath(installer),
          ...args,
        ],
        { env, encoding: 'utf8' },
      ),
  };
}

test('downloads latest DMG and sidecar before replacing the existing app', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t);
  const result = harness.run('--yes');
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);

  assert.equal(existsSync(path.join(harness.existingApp, 'stale.txt')), false);
  assert.equal(readFileSync(path.join(harness.existingApp, 'version.txt'), 'utf8'), 'new-version\n');

  const curlLog = readFileSync(path.join(harness.root, 'curl.log'), 'utf8');
  assert.match(curlLog, /releases\/latest/);
  assert.match(curlLog, /Gold\.Band_1\.2\.3_aarch64\.dmg$/m);
  assert.match(curlLog, /Gold\.Band_1\.2\.3_aarch64\.dmg\.sha256$/m);
});

test('explicit version skips latest lookup and still requires its sidecar', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, { GOLD_BAND_VERSION: '1.2.3' });
  const result = harness.run('--yes');
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);

  const curlLog = readFileSync(path.join(harness.root, 'curl.log'), 'utf8');
  assert.doesNotMatch(curlLog, /api\.github\.com/);
  assert.match(curlLog, /Gold\.Band_1\.2\.3_aarch64\.dmg\.sha256$/m);
});

test('missing checksum fails before mounting and preserves the old app', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, { TEST_MISSING_CHECKSUM: '1' });
  const result = harness.run('--yes');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /只支持包含 checksum 的新版本/);
  assert.equal(readFileSync(path.join(harness.existingApp, 'stale.txt'), 'utf8'), 'old-version');
  assert.equal(existsSync(path.join(harness.root, 'hdiutil.log')), false);
});

test('checksum mismatch fails before mounting and preserves the old app', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, { TEST_ACTUAL_DIGEST: 'b'.repeat(64) });
  const result = harness.run('--yes');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /SHA256 不匹配/);
  assert.equal(readFileSync(path.join(harness.existingApp, 'stale.txt'), 'utf8'), 'old-version');
  assert.equal(existsSync(path.join(harness.root, 'hdiutil.log')), false);
});

test('unexpected app identity aborts without replacing the old app', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, { TEST_BUNDLE_ID: 'example.invalid' });
  const result = harness.run('--yes');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /App 标识不匹配/);
  assert.equal(readFileSync(path.join(harness.existingApp, 'stale.txt'), 'utf8'), 'old-version');
});

test('failed post-install verification restores the previous app', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, { TEST_FAIL_FINAL_CODESIGN: '1' });
  const result = harness.run('--yes');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /已恢复旧版本/);
  assert.equal(readFileSync(path.join(harness.existingApp, 'stale.txt'), 'utf8'), 'old-version');
  assert.equal(existsSync(path.join(harness.existingApp, 'version.txt')), false);
});

test('failed automatic restore preserves a recoverable backup', { skip: !bashAvailable }, (t) => {
  const harness = createHarness(t, {
    TEST_FAIL_FINAL_CODESIGN: '1',
    TEST_FAIL_RESTORE: '1',
  });
  const result = harness.run('--yes');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /备份保留在/);

  const backupDirectory = readdirSync(harness.installDirectory).find((name) =>
    name.startsWith('.gold-band-backup.'),
  );
  assert.ok(backupDirectory);
  assert.equal(
    readFileSync(
      path.join(harness.installDirectory, backupDirectory, 'Gold Band.app', 'stale.txt'),
      'utf8',
    ),
    'old-version',
  );
});
