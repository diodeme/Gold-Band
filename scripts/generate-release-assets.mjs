import { access, copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'];
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const generatorPath = path.join(scriptDir, 'generate-updater-json.mjs');
const [assetDirArg = 'release-assets', outputDirArg = 'release-output', ...rest] = process.argv.slice(2);
const versionFlagIndex = rest.indexOf('--version');
const notesRootFlagIndex = rest.indexOf('--notes-root');
const releaseTag = process.env.RELEASE_TAG ?? process.env.GITHUB_REF_NAME ?? '';
const version = versionFlagIndex >= 0
  ? rest[versionFlagIndex + 1]
  : (process.env.RELEASE_VERSION ?? releaseTag).replace(/^v/, '');
const notesRoot = path.resolve(notesRootFlagIndex >= 0 ? rest[notesRootFlagIndex + 1] : 'release-notes');
const assetDir = path.resolve(assetDirArg);
const outputDir = path.resolve(outputDirArg);

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error('A semver release version is required. Pass --version X.Y.Z.');
  process.exit(1);
}

const versionNotesDir = path.join(notesRoot, version);
try {
  await access(versionNotesDir);
} catch {
  console.error(`Release notes directory not found: ${versionNotesDir}`);
  process.exit(1);
}

await mkdir(outputDir, { recursive: true });
const notesByLocale = new Map();

for (const locale of LOCALES) {
  const notesPath = path.join(versionNotesDir, `${locale}.md`);
  let notes;
  try {
    notes = (await readFile(notesPath, 'utf8')).trim();
  } catch {
    console.error(`Release notes file not found: ${notesPath}`);
    process.exit(1);
  }
  if (!notes) {
    console.error(`Release notes file is empty: ${notesPath}`);
    process.exit(1);
  }
  notesByLocale.set(locale, notes);

  const outputPath = path.join(outputDir, `latest.${locale}.json`);
  const result = spawnSync(
    process.execPath,
    [generatorPath, assetDir, outputPath, '--version', version, '--notes-file', notesPath],
    {
      cwd: process.cwd(),
      env: { ...process.env, RELEASE_VERSION: version },
      stdio: 'inherit',
    },
  );
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

await copyFile(path.join(outputDir, 'latest.en.json'), path.join(outputDir, 'latest.json'));
await writeFile(
  path.join(outputDir, 'release-body.md'),
  `# 简体中文\n\n${notesByLocale.get('zh-CN')}\n\n---\n\n# English\n\n${notesByLocale.get('en')}\n`,
);

console.log(`Wrote localized updater manifests and release body for ${version}.`);
