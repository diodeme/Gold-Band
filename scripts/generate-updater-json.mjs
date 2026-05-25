import { readdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [assetDirArg, outputArg] = process.argv.slice(2);
const assetDir = assetDirArg ?? 'release-assets';
const outputPath = outputArg ?? 'latest.json';
const repository = process.env.GITHUB_REPOSITORY;
const tag = process.env.GITHUB_REF_NAME ?? process.env.RELEASE_TAG;
const version = (process.env.RELEASE_VERSION ?? tag ?? '').replace(/^v/, '');

if (!repository) {
  console.error('GITHUB_REPOSITORY is required.');
  process.exit(1);
}
if (!tag) {
  console.error('GITHUB_REF_NAME or RELEASE_TAG is required.');
  process.exit(1);
}
if (!version) {
  console.error('Could not infer release version.');
  process.exit(1);
}

const files = await readdir(assetDir);
const signatures = new Map(files.filter((file) => file.endsWith('.sig')).map((file) => [file.slice(0, -4), file]));
const candidates = files
  .filter((file) => signatures.has(file))
  .map((file) => ({ file, platform: platformKey(file) }))
  .filter((item) => item.platform)
  .sort((left, right) => score(right.file) - score(left.file));

const platforms = {};
for (const candidate of candidates) {
  if (platforms[candidate.platform]) continue;
  const signature = await readFile(path.join(assetDir, signatures.get(candidate.file)), 'utf8');
  platforms[candidate.platform] = {
    url: `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(candidate.file)}`,
    signature: signature.trim(),
  };
}

if (Object.keys(platforms).length === 0) {
  console.error(`No updater artifacts with .sig files found in ${assetDir}.`);
  process.exit(1);
}

const latest = {
  version,
  notes: `Gold Band ${tag}`,
  pub_date: new Date().toISOString(),
  platforms,
};

await writeFile(outputPath, `${JSON.stringify(latest, null, 2)}\n`);
console.log(`Wrote ${outputPath} with platforms: ${Object.keys(platforms).join(', ')}`);

function platformKey(file) {
  const lower = file.toLowerCase();
  const arch = lower.includes('aarch64') || lower.includes('arm64') ? 'aarch64' : 'x86_64';
  if (lower.endsWith('.appimage') || lower.includes('linux')) return `linux-${arch}`;
  if (lower.endsWith('.dmg') || lower.endsWith('.app.tar.gz') || lower.includes('darwin') || lower.includes('macos')) return `darwin-${arch}`;
  if (lower.endsWith('.msi') || lower.endsWith('.exe') || lower.includes('windows')) return `windows-${arch}`;
  return null;
}

function score(file) {
  const lower = file.toLowerCase();
  if (lower.endsWith('.msi')) return 40;
  if (lower.endsWith('.app.tar.gz')) return 35;
  if (lower.endsWith('.appimage')) return 30;
  if (lower.endsWith('.exe')) return 20;
  if (lower.endsWith('.dmg')) return 10;
  return 0;
}
