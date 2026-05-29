import { readdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';

const [assetDirArg, outputArg, ...rest] = process.argv.slice(2);
const assetDir = assetDirArg ?? 'release-assets';
const outputPath = outputArg ?? 'latest.json';

// ── local mode (--base-url + --version) ──
const baseUrlFlagIdx = rest.indexOf('--base-url');
const versionFlagIdx = rest.indexOf('--version');
const baseUrl = baseUrlFlagIdx >= 0 ? rest[baseUrlFlagIdx + 1] : process.env.RELEASE_BASE_URL;
const version = versionFlagIdx >= 0 ? rest[versionFlagIdx + 1] : process.env.RELEASE_VERSION;

if (baseUrl) {
  // local mode: use custom base URL, skip GitHub deps
  if (!version) {
    console.error('--version is required in local mode.');
    process.exit(1);
  }
  await generateLocal(baseUrl, version, assetDir, outputPath);
} else {
  // CI mode: GitHub Releases
  await generateGitHub(assetDir, outputPath);
}

async function generateLocal(baseUrl, version, assetDir, outputPath) {
  const normalizedBase = baseUrl.replace(/\/+$/, '');
  const files = await readdir(assetDir);
  const signatures = new Map(
    files.filter((f) => f.endsWith('.sig')).map((f) => [f.slice(0, -4), f]),
  );

  const platforms = {};
  for (const file of files) {
    if (!signatures.has(file)) continue;
    const plat = platformKey(file);
    if (!plat) continue;
    if (platforms[plat]) {
      const existingFile = findExisting(platforms, plat, files);
      const existingScore = score(existingFile);
      const newScore = score(file);
      if (newScore < existingScore) continue;
      if (newScore === existingScore && cmpSemver(extractSemver(file), extractSemver(existingFile)) <= 0) continue;
    }

    const sig = await readFile(path.join(assetDir, signatures.get(file)), 'utf8');
    platforms[plat] = {
      url: `${normalizedBase}/${encodeURIComponent(file)}`,
      signature: sig.trim(),
    };
  }

  if (Object.keys(platforms).length === 0) {
    console.error(`No updater artifacts with .sig files found in ${assetDir}.`);
    process.exit(1);
  }

  const changelog = await extractChangelog(version);
  const notes = changelog || `Gold Band ${version}`;

  const latest = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms,
  };

  await writeFile(outputPath, `${JSON.stringify(latest, null, 2)}\n`);
  console.log(`Wrote ${outputPath} (base: ${normalizedBase}) platforms: ${Object.keys(platforms).join(', ')} (changelog: ${changelog ? 'yes' : 'no'})`);
}

async function generateGitHub(assetDir, outputPath) {
  const repository = process.env.GITHUB_REPOSITORY;
  const tag = process.env.RELEASE_TAG ?? process.env.GITHUB_REF_NAME;
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
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    console.error(`Invalid release version: ${version}. Set RELEASE_TAG to a semver tag such as v0.2.0.`);
    process.exit(1);
  }

  const files = await readdir(assetDir);
  const signatures = new Map(files.filter((file) => file.endsWith('.sig')).map((file) => [file.slice(0, -4), file]));
  const candidates = files
    .filter((file) => signatures.has(file))
    .map((file) => ({ file, platform: platformKey(file) }))
    .filter((item) => item.platform)
    .sort((left, right) => {
      const scoreDiff = score(right.file) - score(left.file);
      if (scoreDiff !== 0) return scoreDiff;
      return cmpSemver(extractSemver(right.file), extractSemver(left.file));
    });

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

  const changelog = await extractChangelog(version);
  const notes = changelog || `Gold Band ${tag}`;

  const latest = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms,
  };

  await writeFile(outputPath, `${JSON.stringify(latest, null, 2)}\n`);
  console.log(`Wrote ${outputPath} with platforms: ${Object.keys(platforms).join(', ')} (changelog: ${changelog ? 'yes' : 'no'})`);
}

function findExisting(platforms, plat, files) {
  for (const f of files) {
    if (platformKey(f) === plat && platforms[plat]) {
      const existingUrl = platforms[plat].url;
      if (existingUrl) {
        const lastPart = existingUrl.split('/').pop();
        if (lastPart) return decodeURIComponent(lastPart);
      }
    }
  }
  return '';
}

function platformKey(file) {
  const lower = file.toLowerCase();
  const arch = lower.includes('aarch64') || lower.includes('arm64') ? 'aarch64' : 'x86_64';
  if (lower.endsWith('.appimage') || lower.includes('linux')) return `linux-${arch}`;
  if (lower.endsWith('.dmg') || lower.endsWith('.app.tar.gz') || lower.includes('darwin') || lower.includes('macos')) return `darwin-${arch}`;
  if (lower.endsWith('.msi') || lower.endsWith('.exe') || lower.includes('windows')) return `windows-${arch}`;
  return null;
}

async function extractChangelog(version) {
  const changelogPath = path.resolve('CHANGELOG.md');
  if (!existsSync(changelogPath)) return null;

  const content = await readFile(changelogPath, 'utf8');
  // Match the section for this version: "## [0.3.1](...) (date)" or "## 0.3.1 (date)"
  const headingPattern = new RegExp(
    `^## \\[?${version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\]?[^\n]*\n`,
    'm',
  );
  const match = content.match(headingPattern);
  if (!match || match.index === undefined) return null;

  const start = match.index + match[0].length;
  // Find the next version heading or end of file
  const nextHeading = new RegExp('^## \\[?\\d+\\.\\d+\\.\\d+', 'm');
  nextHeading.lastIndex = start;
  const nextMatch = nextHeading.exec(content);
  const end = nextMatch ? nextMatch.index : content.length;

  return content.slice(start, end).trim();
}

function extractSemver(file) {
  const match = file.match(/(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;
  return [parseInt(match[1], 10), parseInt(match[2], 10), parseInt(match[3], 10)];
}

function cmpSemver(a, b) {
  if (!a && !b) return 0;
  if (!a) return -1;
  if (!b) return 1;
  for (let i = 0; i < 3; i++) {
    if (a[i] > b[i]) return 1;
    if (a[i] < b[i]) return -1;
  }
  return 0;
}

function score(file) {
  const lower = file.toLowerCase();
  if (lower.endsWith('-setup.exe') || lower.endsWith('_setup.exe')) return 50;
  if (lower.endsWith('.app.tar.gz')) return 40;
  if (lower.endsWith('.appimage')) return 35;
  if (lower.endsWith('.msi')) return 30;
  if (lower.endsWith('.exe')) return 25;
  if (lower.endsWith('.dmg')) return 10;
  return 0;
}
