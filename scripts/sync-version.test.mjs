import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const syncScript = path.join(scriptsDir, 'sync-version.mjs');

function crlf(content) {
  return `${content.replace(/\n/g, '\r\n')}`;
}

test('version sync updates every workspace package in a CRLF Cargo.lock', async () => {
  const tempDir = await mkdtemp(path.join(tmpdir(), 'sync-version-'));

  try {
    await writeFile(path.join(tempDir, 'package.json'), '{"version":"0.20.0"}\n');
    await writeFile(
      path.join(tempDir, 'Cargo.toml'),
      '[package]\nname = "gold-band"\nversion = "0.20.0"\n',
    );
    await mkdir(path.join(tempDir, 'src-tauri'), { recursive: true });
    await writeFile(
      path.join(tempDir, 'src-tauri', 'Cargo.toml'),
      '[package]\nname = "gold-band-desktop"\nversion = "0.20.0"\n',
    );
    await writeFile(
      path.join(tempDir, 'Cargo.lock'),
      crlf(`[[package]]
name = "gold-band"
version = "0.20.0"

[[package]]
name = "gold-band-desktop"
version = "0.20.0"

[[package]]
name = "third-party"
version = "0.20.0"
`),
    );

    const result = spawnSync(process.execPath, [syncScript, '0.21.0'], {
      cwd: tempDir,
      encoding: 'utf8',
    });

    assert.equal(result.status, 0, result.stderr);
    const lock = await readFile(path.join(tempDir, 'Cargo.lock'), 'utf8');
    assert.ok(lock.includes('name = "gold-band"\r\nversion = "0.21.0"'));
    assert.ok(lock.includes('name = "gold-band-desktop"\r\nversion = "0.21.0"'));
    assert.ok(lock.includes('name = "third-party"\r\nversion = "0.20.0"'));
  } finally {
    await rm(tempDir, { recursive: true, force: true });
  }
});
