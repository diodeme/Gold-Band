import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readdir, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';

const RELEASE_ASSET_PATTERNS = [
  /\.dmg$/i,
  /\.app\.tar\.gz$/i,
  /\.appimage$/i,
  /\.deb$/i,
  /\.rpm$/i,
  /\.exe$/i,
  /\.msi$/i,
];

const assetDirectory = path.resolve(process.argv[2] ?? 'release-assets');

try {
  const entries = await readdir(assetDirectory, { withFileTypes: true });
  const releaseAssets = entries
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .filter((name) => RELEASE_ASSET_PATTERNS.some((pattern) => pattern.test(name)))
    .sort((left, right) => left.localeCompare(right));

  if (releaseAssets.length === 0) {
    throw new Error(`No supported release assets found in ${assetDirectory}`);
  }

  await Promise.all(
    entries
      .filter((entry) => entry.isFile() && entry.name.endsWith('.sha256'))
      .map((entry) => rm(path.join(assetDirectory, entry.name), { force: true })),
  );

  for (const assetName of releaseAssets) {
    const assetPath = path.join(assetDirectory, assetName);
    const checksumPath = `${assetPath}.sha256`;
    const temporaryPath = `${checksumPath}.${process.pid}.tmp`;
    const digest = await sha256File(assetPath);
    await writeFile(temporaryPath, `${digest}  ${assetName}\n`, 'utf8');
    await rename(temporaryPath, checksumPath);
    console.log(`Wrote ${path.basename(checksumPath)}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

async function sha256File(filePath) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest('hex');
}
