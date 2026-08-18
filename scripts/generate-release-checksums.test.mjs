import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const script = path.resolve('scripts/generate-release-checksums.mjs');

function createDirectory(t) {
  const directory = mkdtempSync(path.join(tmpdir(), 'gold-band-checksums-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

test('writes deterministic sidecars for every supported platform artifact', (t) => {
  const directory = createDirectory(t);
  const assets = new Map([
    ['Gold.Band_1.2.3_aarch64.dmg', 'mac-arm'],
    ['Gold.Band_1.2.3_x64.dmg', 'mac-intel'],
    ['Gold.Band_1.2.3_x64-setup.exe', 'windows-exe'],
    ['Gold.Band_1.2.3_x64_en-US.msi', 'windows-msi'],
    ['Gold.Band_1.2.3_amd64.AppImage', 'linux-appimage'],
    ['Gold.Band_1.2.3_amd64.deb', 'linux-deb'],
    ['Gold.Band-1.2.3-1.x86_64.rpm', 'linux-rpm'],
    ['Gold.Band_aarch64.app.tar.gz', 'updater-macos'],
  ]);

  for (const [name, content] of assets) {
    writeFileSync(path.join(directory, name), content);
  }
  writeFileSync(path.join(directory, 'Gold.Band_1.2.3_amd64.deb.sig'), 'signature');
  writeFileSync(path.join(directory, 'latest.json'), '{}');
  writeFileSync(path.join(directory, 'notes.txt'), 'notes');
  writeFileSync(path.join(directory, 'obsolete.sha256'), 'stale');

  const result = spawnSync(process.execPath, [script, directory], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);

  for (const [name, content] of assets) {
    const digest = createHash('sha256').update(content).digest('hex');
    assert.equal(readFileSync(path.join(directory, `${name}.sha256`), 'utf8'), `${digest}  ${name}\n`);
  }

  const generated = readdirSync(directory).filter((name) => name.endsWith('.sha256')).sort();
  assert.deepEqual(generated, [...assets.keys()].map((name) => `${name}.sha256`).sort());
});

test('fails when the release contains no supported platform artifacts', (t) => {
  const directory = createDirectory(t);
  writeFileSync(path.join(directory, 'latest.json'), '{}');
  writeFileSync(path.join(directory, 'artifact.sig'), 'signature');

  const result = spawnSync(process.execPath, [script, directory], { encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No supported release assets/);
});

test('both release entry points publish the installer and checksums after the updater manifest', () => {
  for (const workflow of ['.github/workflows/release.yml', '.github/workflows/release-please.yml']) {
    const source = readFileSync(path.resolve(workflow), 'utf8');
    const manifestIndex = source.indexOf('node scripts/generate-updater-json.mjs release-assets latest.json');
    const checksumIndex = source.indexOf('node scripts/generate-release-checksums.mjs release-assets');
    const uploadIndex = source.indexOf(
      'gh release upload "${RELEASE_TAG}" latest.json scripts/install-gold-band-macos.sh release-assets/*.sha256 --clobber',
    );

    assert.notEqual(manifestIndex, -1, `${workflow} must generate latest.json`);
    assert.ok(checksumIndex > manifestIndex, `${workflow} must generate checksums after latest.json`);
    assert.ok(uploadIndex > checksumIndex, `${workflow} must upload the installer and generated checksums`);
  }
});
