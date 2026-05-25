import fs from 'node:fs';
import path from 'node:path';

const rawVersion = process.argv[2];

if (!rawVersion) {
  console.error('Usage: node scripts/sync-version.mjs <version>');
  process.exit(1);
}

const version = rawVersion.replace(/^v/, '');

if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid semver version: ${rawVersion}`);
  process.exit(1);
}

const root = process.cwd();

function writeIfChanged(filePath, content, nextContent) {
  if (nextContent !== content) {
    fs.writeFileSync(filePath, nextContent);
  }
}

function updateJsonVersion(relativePath, updateLockRoot = false) {
  const filePath = path.join(root, relativePath);

  if (!fs.existsSync(filePath)) {
    return;
  }

  const content = fs.readFileSync(filePath, 'utf8');
  const newline = content.includes('\r\n') ? '\r\n' : '\n';
  const json = JSON.parse(content);
  json.version = version;

  if (updateLockRoot && json.packages?.['']) {
    json.packages[''].version = version;
  }

  const nextContent = `${JSON.stringify(json, null, 2).replace(/\n/g, newline)}${newline}`;
  writeIfChanged(filePath, content, nextContent);
}

function updateCargoToml(relativePath) {
  const filePath = path.join(root, relativePath);
  const content = fs.readFileSync(filePath, 'utf8');
  const packageVersionPattern = /(\[package\][\s\S]*?\nversion\s*=\s*)"[^"]+"/;

  if (!packageVersionPattern.test(content)) {
    console.error(`Could not find package version in ${relativePath}`);
    process.exit(1);
  }

  const nextContent = content.replace(packageVersionPattern, `$1"${version}"`);
  writeIfChanged(filePath, content, nextContent);
}

function updateCargoLock(relativePath, packageNames) {
  const filePath = path.join(root, relativePath);

  if (!fs.existsSync(filePath)) {
    return;
  }

  const content = fs.readFileSync(filePath, 'utf8');
  const nextContent = content.replace(
    /(\[\[package\]\]\nname = "([^"]+)"\nversion = ")([^"]+)(")/g,
    (match, prefix, packageName, currentVersion, suffix) => {
      if (!packageNames.includes(packageName)) {
        return match;
      }

      return `${prefix}${version}${suffix}`;
    },
  );

  writeIfChanged(filePath, content, nextContent);
}

updateJsonVersion('package.json');
updateJsonVersion('package-lock.json', true);
updateJsonVersion(path.join('src-tauri', 'tauri.conf.json'));
updateCargoToml('Cargo.toml');
updateCargoToml(path.join('src-tauri', 'Cargo.toml'));
updateCargoLock('Cargo.lock', ['gold-band']);
updateCargoLock(path.join('src-tauri', 'Cargo.lock'), ['gold-band', 'gold-band-desktop']);

console.log(`Synced release version ${version}`);
