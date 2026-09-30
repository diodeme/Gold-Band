import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const repoRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const scriptPath = path.join(repoRoot, 'scripts', 'generate-release-assets.mjs');
const locales = ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'];
const notesFor = (locale) => `## Features\n\n### 1. ${locale}\n\n\`\`\`sh\n# ${locale} comment\n\`\`\``;

async function fixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'gold-band-release-assets-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const assets = path.join(root, 'assets');
  const output = path.join(root, 'output');
  const notesRoot = path.join(root, 'release-notes');
  const versionDir = path.join(notesRoot, '1.2.3');
  await mkdir(assets, { recursive: true });
  await mkdir(versionDir, { recursive: true });
  await writeFile(path.join(assets, 'Gold-Band_1.2.3_x64-setup.exe'), 'installer');
  await writeFile(path.join(assets, 'Gold-Band_1.2.3_x64-setup.exe.sig'), 'signature');
  for (const locale of locales) {
    await writeFile(path.join(versionDir, `${locale}.md`), `${notesFor(locale)}\n`);
  }

  const run = () => spawnSync(
    process.execPath,
    [scriptPath, assets, output, '--version', '1.2.3', '--notes-root', notesRoot],
    {
      cwd: root,
      env: {
        ...process.env,
        GITHUB_REPOSITORY: 'diodeme/Gold-Band',
        RELEASE_TAG: 'v1.2.3',
        RELEASE_BASE_URL: '',
      },
      encoding: 'utf8',
    },
  );

  return { output, metadataPath: path.join(versionDir, 'release.json'), run };
}

for (const critical of [undefined, false, true]) {
  test(`generates localized manifests and bilingual body with critical=${critical ?? 'absent'}`, async (t) => {
    const { output, metadataPath, run } = await fixture(t);
    if (critical !== undefined) await writeFile(metadataPath, JSON.stringify({ critical }));
    const result = run();
    assert.equal(result.status, 0, result.stderr || result.stdout);
    for (const locale of locales) {
      const manifest = JSON.parse(await readFile(path.join(output, `latest.${locale}.json`), 'utf8'));
      assert.equal(Object.hasOwn(manifest, 'critical'), critical === true);
      if (critical === true) assert.equal(manifest.critical, true);
      assert.equal(manifest.version, '1.2.3');
      assert.equal(manifest.notes, notesFor(locale));
      assert.equal(manifest.platforms['windows-x86_64'].signature, 'signature');
    }
    assert.deepEqual(
      JSON.parse(await readFile(path.join(output, 'latest.json'), 'utf8')),
      JSON.parse(await readFile(path.join(output, 'latest.zh-CN.json'), 'utf8')),
    );
    const releaseBody = await readFile(path.join(output, 'release-body.md'), 'utf8');
    assert.equal(
      releaseBody,
      [
        '# Gold Band v1.2.3',
        '[中文](#user-content-zh) · [English](#user-content-en)',
        '<a id="zh"></a>',
        '## 中文',
        '### Features\n\n#### 1. zh-CN\n\n```sh\n# zh-CN comment\n```',
        '---',
        '<a id="en"></a>',
        '## English',
        '### Features\n\n#### 1. en\n\n```sh\n# en comment\n```',
      ].join('\n\n') + '\n',
    );
  });
}

for (const source of ['{', '', 'null', '[]', 'true', '1', '"critical"', '{}', '{"critical":"true"}', '{"critical":1}', '{"critical":null}', '{"critical":false,"extra":true}']) {
  test(`rejects invalid metadata before creating output: ${JSON.stringify(source)}`, async (t) => {
    const { output, metadataPath, run } = await fixture(t);
    await writeFile(metadataPath, source);
    const result = run();
    assert.notEqual(result.status, 0);
    assert.ok(result.stderr.includes(`Invalid release metadata: ${metadataPath}`), result.stderr);
    await assert.rejects(readdir(output), { code: 'ENOENT' });
  });
}

test('invalid metadata leaves existing output untouched', async (t) => {
  const { output, metadataPath, run } = await fixture(t);
  await mkdir(output);
  await writeFile(path.join(output, 'latest.json'), 'existing output');
  await writeFile(metadataPath, '{"critical":"true"}');
  assert.notEqual(run().status, 0);
  assert.deepEqual(await readdir(output), ['latest.json']);
  assert.equal(await readFile(path.join(output, 'latest.json'), 'utf8'), 'existing output');
});

test('metadata read errors are not treated as an absent file', async (t) => {
  const { output, metadataPath, run } = await fixture(t);
  await mkdir(metadataPath);
  const result = run();
  assert.notEqual(result.status, 0);
  assert.ok(result.stderr.includes(`Invalid release metadata: ${metadataPath}`), result.stderr);
  await assert.rejects(readdir(output), { code: 'ENOENT' });
});

test('fails when the current release notes directory is missing', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'gold-band-release-assets-missing-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const result = spawnSync(
    process.execPath,
    [scriptPath, path.join(root, 'assets'), path.join(root, 'output'), '--version', '1.2.3', '--notes-root', path.join(root, 'release-notes')],
    { cwd: root, encoding: 'utf8' },
  );

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Release notes directory not found/);
});
