import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const scriptPath = path.join(repoRoot, 'scripts', 'generate-release-assets.mjs');
const locales = ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'];

test('generates localized default manifests and a bilingual release body', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'gold-band-release-assets-'));
  const assets = path.join(root, 'assets');
  const output = path.join(root, 'output');
  const notesRoot = path.join(root, 'release-notes');
  const versionDir = path.join(notesRoot, '1.2.3');
  await mkdir(assets, { recursive: true });
  await mkdir(versionDir, { recursive: true });
  await writeFile(path.join(assets, 'Gold-Band_1.2.3_x64-setup.exe'), 'installer');
  await writeFile(path.join(assets, 'Gold-Band_1.2.3_x64-setup.exe.sig'), 'signature');
  for (const locale of locales) {
    await writeFile(path.join(versionDir, `${locale}.md`), `## ${locale}\n\nNotes for ${locale}.\n`);
  }

  const result = spawnSync(
    process.execPath,
    [scriptPath, assets, output, '--version', '1.2.3', '--notes-root', notesRoot],
    {
      cwd: root,
      env: {
        ...process.env,
        GITHUB_REPOSITORY: 'diodeme/Gold-Band',
        RELEASE_TAG: 'v1.2.3',
      },
      encoding: 'utf8',
    },
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);
  for (const locale of locales) {
    const manifest = JSON.parse(await readFile(path.join(output, `latest.${locale}.json`), 'utf8'));
    assert.equal(manifest.version, '1.2.3');
    assert.equal(manifest.notes, `## ${locale}\n\nNotes for ${locale}.`);
    assert.equal(manifest.platforms['windows-x86_64'].signature, 'signature');
  }
  assert.deepEqual(
    JSON.parse(await readFile(path.join(output, 'latest.json'), 'utf8')),
    JSON.parse(await readFile(path.join(output, 'latest.en.json'), 'utf8')),
  );
  const releaseBody = await readFile(path.join(output, 'release-body.md'), 'utf8');
  assert.match(releaseBody, /^# 简体中文/m);
  assert.match(releaseBody, /Notes for zh-CN\./);
  assert.match(releaseBody, /^# English/m);
  assert.match(releaseBody, /Notes for en\./);
});

test('fails when the current release notes directory is missing', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'gold-band-release-assets-missing-'));
  const result = spawnSync(
    process.execPath,
    [scriptPath, path.join(root, 'assets'), path.join(root, 'output'), '--version', '1.2.3', '--notes-root', path.join(root, 'release-notes')],
    { cwd: root, encoding: 'utf8' },
  );

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Release notes directory not found/);
});
