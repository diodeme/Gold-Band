import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, lstat, mkdir, mkdtemp, readFile, realpath, rename, unlink, writeFile } from 'node:fs/promises';
import { dirname, join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createGunzip, constants as zlibConstants } from 'node:zlib';
import { Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { archiveCatalog, ARCHIVE_VERSION } from './export-demo-session.mjs';
import { prepareDemoSession } from './prepare-demo-session.mjs';
import { verifyDemoSession } from './verify-demo-session.mjs';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const assetPattern = /^assets\/[a-f0-9]{64}\.json$/;
const resourcePattern = /^resources\/[a-f0-9]{64}$/;
export const PUBLIC_ARCHIVE_LIMITS = { reviewedValues: 32, sensitiveTextBytes: 32 * 1024 * 1024,
  expandedGzipBytes: 2 * 1024 * 1024 * 1024, incompleteGzipResources: 32 };
const within = (root, path) => path === root || path.startsWith(`${root}${sep}`);
function parseJson(bytes) {
  try { return JSON.parse(bytes.toString()); } catch { throw new Error('archive.public-json-invalid'); }
}
function indexEntries(entries, pattern) {
  const result = new Map();
  for (const entry of entries) {
    if (!pattern.test(entry.path) || entry.sha256 !== entry.path.split('/').at(-1).replace(/\.json$/, '')
      || !Number.isSafeInteger(entry.bytes) || entry.bytes < 0) throw new Error('archive.resource-path-invalid');
    const prior = result.get(entry.path);
    if (prior && prior.bytes !== entry.bytes) throw new Error('archive.resource-conflict');
    result.set(entry.path, entry);
  }
  return result;
}
async function absent(path) {
  try { await lstat(path); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
  throw new Error('archive.public-output-exists');
}

export async function createPublicDemoArchive(sourceDirectory, outputDirectory, reviewedValues, onProgress = () => {}, incompleteGzipReviews = []) {
  if (!Array.isArray(reviewedValues) || !reviewedValues.length || reviewedValues.length > PUBLIC_ARCHIVE_LIMITS.reviewedValues) throw new Error('archive.reviewed-values-required');
  const ids = new Set();
  const secrets = new Set();
  const replacements = reviewedValues.map(({ id, value }) => {
    if (!/^[a-z0-9-]{1,48}$/.test(id ?? '') || !/^[A-Za-z0-9_-]{20,256}$/.test(value ?? '') || ids.has(id) || secrets.has(value)) {
      throw new Error('archive.reviewed-value-invalid');
    }
    ids.add(id); secrets.add(value);
    return { id, value, replacement: `[REDACTED:${id}]` };
  });
  if (replacements.some(a => replacements.some(b => a !== b && a.value.includes(b.value)))) throw new Error('archive.reviewed-values-overlap');
  if (replacements.some(a => replacements.some(b => a.replacement.includes(b.value)))) throw new Error('archive.reviewed-value-invalid');
  const source = await realpath(sourceDirectory);
  const outputParent = await realpath(dirname(resolve(outputDirectory)));
  const output = join(outputParent, resolve(outputDirectory).split(sep).at(-1));
  if (within(source, output) || within(output, source)) throw new Error('archive.public-output-overlap');
  await absent(output);
  const manifestBytes = await readFile(join(source, 'manifest.json'));
  const manifest = parseJson(manifestBytes);
  if (manifest.version !== ARCHIVE_VERSION) throw new Error('archive.version-unsupported');
  if (manifest.runtimeVersion !== undefined && manifest.runtimeVersion !== 1) throw new Error('archive.version-unsupported');
  const inputAssets = indexEntries(manifest.assets, assetPattern);
  const inputResources = indexEntries(manifest.resources, resourcePattern);
  if (!Array.isArray(incompleteGzipReviews) || incompleteGzipReviews.length > PUBLIC_ARCHIVE_LIMITS.incompleteGzipResources) throw new Error('archive.gzip-review-invalid');
  const gzipReviews = new Map();
  for (const review of incompleteGzipReviews) {
    if (!review || !/^[a-f0-9]{64}$/.test(review.sha256) || !/^[a-f0-9]{64}$/.test(review.expandedSha256)
      || !Number.isSafeInteger(review.bytes) || review.bytes < 1
      || !Number.isSafeInteger(review.expandedBytes) || review.expandedBytes < 0
      || review.expandedBytes > PUBLIC_ARCHIVE_LIMITS.expandedGzipBytes
      || review.reason !== 'captured-truncated-gzip' || gzipReviews.has(review.sha256)) throw new Error('archive.gzip-review-invalid');
    if (inputResources.get(`resources/${review.sha256}`)?.bytes !== review.bytes) throw new Error('archive.gzip-review-mismatch');
    gzipReviews.set(review.sha256, { sha256: review.sha256, bytes: review.bytes, expandedSha256: review.expandedSha256,
      expandedBytes: review.expandedBytes, reason: review.reason });
  }
  const staging = await mkdtemp(join(outputParent, '.public-archive-'));
  await mkdir(join(staging, 'resources'));
  await mkdir(join(staging, 'assets'));
  const report = { version: 1, publication: 'pending-disclosure-review', sourceManifestSha256: digest(manifestBytes), replacements: [], incompleteGzip: [] };
  const resourceMap = new Map();
  const assetMap = new Map();
  const active = new Set();
  const metadata = (path, bytes) => ({ path, sha256: digest(bytes), bytes: bytes.length });
  const countById = new Map(replacements.map(item => [item.id, 0]));
  function redact(text, path) {
    let result = text;
    for (const item of replacements) {
      const parts = result.split(item.value);
      if (parts.length === 1) continue;
      const count = parts.length - 1;
      report.replacements.push({ path, id: item.id, count });
      countById.set(item.id, countById.get(item.id) + count);
      result = parts.join(item.replacement);
    }
    return result;
  }
  async function inputPath(path) {
    if (!assetPattern.test(path) && !resourcePattern.test(path)) throw new Error('archive.resource-path-invalid');
    const target = await realpath(join(source, path));
    if (!within(source, target)) throw new Error('archive.resource-outside-root');
    return target;
  }
  async function checkedBytes(entry) {
    const bytes = await readFile(await inputPath(entry.path));
    if (bytes.length !== entry.bytes || digest(bytes) !== entry.sha256) throw new Error('archive.public-source-changed');
    return bytes;
  }
  async function scanGzip(target, needles, overlap, options) {
    let tail = Buffer.alloc(0);
    let expanded = 0;
    const hash = createHash('sha256');
    await pipeline(createReadStream(target), createGunzip(options), new Writable({
      write(chunk, encoding, done) {
        try {
          expanded += chunk.length;
          if (expanded > PUBLIC_ARCHIVE_LIMITS.expandedGzipBytes) throw new Error('archive.public-compressed-budget');
          hash.update(chunk);
          const window = Buffer.concat([tail, chunk]);
          if (needles.some(needle => window.includes(needle))) throw new Error('archive.public-sensitive-compressed');
          tail = window.subarray(Math.max(0, window.length - overlap));
          done();
        } catch (error) { done(error); }
      },
    }));
    return { expandedBytes: expanded, expandedSha256: hash.digest('hex') };
  }
  async function inspectGzip(target, needles, overlap, review) {
    try {
      await scanGzip(target, needles, overlap);
    } catch (error) {
      if (error.message === 'archive.public-compressed-budget' || error.message === 'archive.public-sensitive-compressed') throw error;
      if (review && error.code === 'Z_BUF_ERROR') {
        // Inspect all recoverable bytes, including the final partial output block.
        const recovered = await scanGzip(target, needles, overlap, { finishFlush: zlibConstants.Z_SYNC_FLUSH });
        if (recovered.expandedBytes !== review.expandedBytes || recovered.expandedSha256 !== review.expandedSha256) throw new Error('archive.gzip-review-mismatch');
        report.incompleteGzip.push(review);
        return;
      }
      throw new Error('archive.public-compressed-invalid');
    }
    if (review) throw new Error('archive.gzip-review-mismatch');
  }
  async function emitAsset(value) {
    const bytes = Buffer.from(JSON.stringify(value));
    if (replacements.some(item => bytes.includes(Buffer.from(item.value)))) throw new Error('archive.public-value-remains');
    const entry = metadata(`assets/${digest(bytes)}.json`, bytes);
    await writeFile(join(staging, entry.path), bytes);
    return entry;
  }
  async function transform(value, path) {
    if (typeof value === 'string') {
      if (resourceMap.has(value)) return resourceMap.get(value).path;
      if (inputAssets.has(value)) return (await asset(value)).path;
      return redact(value, path);
    }
    if (Array.isArray(value)) {
      const result = [];
      for (let i = 0; i < value.length; i++) result.push(await transform(value[i], `${path}/${i}`));
      return result;
    }
    if (!value || typeof value !== 'object') return value;
    const result = Object.create(null);
    for (const [key, item] of Object.entries(value)) {
      const nextKey = redact(key, `${path}/$key`);
      if (Object.hasOwn(result, nextKey)) throw new Error('archive.public-key-collision');
      result[nextKey] = await transform(item, `${path}/${nextKey.replaceAll('~', '~0').replaceAll('/', '~1')}`);
    }
    const reference = resourceMap.get(value.path) ?? assetMap.get(value.path);
    if (reference) {
      result.path = reference.path;
      for (const key of ['sha256', 'bytes']) if (key in value) result[key] = reference[key];
    }
    return result;
  }
  async function asset(path) {
    if (assetMap.has(path)) return assetMap.get(path);
    if (active.has(path)) throw new Error('archive.public-reference-cycle');
    const entry = inputAssets.get(path);
    if (!entry) throw new Error('archive.public-reference-missing');
    active.add(path);
    try {
      const bytes = await checkedBytes(entry);
      const value = await transform(parseJson(bytes), path);
      const emitted = await emitAsset(value);
      assetMap.set(path, emitted);
      return emitted;
    } finally { active.delete(path); }
  }
  try {
    let processed = 0;
    for (const entry of inputResources.values()) {
      if (!resourceMap.has(entry.path)) {
        const target = await inputPath(entry.path);
        const hash = createHash('sha256');
        let length = 0;
        let tail = Buffer.alloc(0);
        let sensitive = false;
        let gzip = false;
        const needles = replacements.map(item => Buffer.from(item.value));
        const overlap = Math.max(...needles.map(item => item.length)) - 1;
        for await (const chunk of createReadStream(target)) {
          if (length === 0) gzip = chunk[0] === 0x1f && chunk[1] === 0x8b;
          hash.update(chunk); length += chunk.length;
          const window = Buffer.concat([tail, chunk]);
          sensitive ||= needles.some(needle => window.includes(needle));
          tail = window.subarray(Math.max(0, window.length - overlap));
        }
        if (length !== entry.bytes || hash.digest('hex') !== entry.sha256) throw new Error('archive.public-source-changed');
        if (gzip) await inspectGzip(target, needles, overlap, gzipReviews.get(entry.sha256));
        if (sensitive) {
          if (entry.bytes > PUBLIC_ARCHIVE_LIMITS.sensitiveTextBytes) throw new Error('archive.public-sensitive-text-budget');
          const bytes = await checkedBytes(entry);
          let text;
          try { text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes); } catch { throw new Error('archive.public-sensitive-binary'); }
          if (text.includes('\0')) throw new Error('archive.public-sensitive-binary');
          const published = Buffer.from(redact(text, entry.path));
          const next = metadata(`resources/${digest(published)}`, published);
          await writeFile(join(staging, next.path), published);
          resourceMap.set(entry.path, next);
        } else {
          await copyFile(target, join(staging, entry.path));
          resourceMap.set(entry.path, { path: entry.path, sha256: entry.sha256, bytes: entry.bytes });
        }
      }
      if (++processed % 10000 === 0) onProgress({ phase: 'resources', processed, total: inputResources.size });
    }
    if (report.incompleteGzip.length !== gzipReviews.size) throw new Error('archive.gzip-review-mismatch');
    for (const path of inputAssets.keys()) await asset(path);
    const next = await transform({ ...manifest, assets: undefined, resources: undefined }, 'manifest.json');
    next.resources = manifest.resources.map(entry => ({ ...entry, ...resourceMap.get(entry.path) }));
    next.assets = [...new Map([...assetMap.values()].map(entry => [entry.path, entry])).values()];
    next.redactions = [...manifest.redactions, ...report.replacements.map(item => ({ ...item, reason: 'reviewed-sensitive-value' }))];
    next.counts.bytes = next.assets.reduce((sum, item) => sum + item.bytes, 0);
    await writeFile(join(staging, 'manifest.json'), JSON.stringify(next, null, 2));
    await writeFile(join(staging, 'catalog.json'), JSON.stringify(archiveCatalog(next)));
    if (manifest.runtimeVersion !== undefined) await prepareDemoSession(staging);

    // Runtime preparation emits new projections; retain only their reachable asset graph.
    const prepared = parseJson(await readFile(join(staging, 'manifest.json')));
    const preparedAssets = new Map(prepared.assets.map(entry => [entry.path, entry]));
    const live = new Set();
    const queue = [prepared.workflow, prepared.runView, ...prepared.sessions.map(session => session.detail)].filter(Boolean);
    function references(value) {
      if (typeof value === 'string' && preparedAssets.has(value)) queue.push(value);
      else if (Array.isArray(value)) value.forEach(references);
      else if (value && typeof value === 'object') Object.values(value).forEach(references);
    }
    while (queue.length) {
      const path = queue.pop();
      if (live.has(path)) continue;
      if (!preparedAssets.has(path)) throw new Error('archive.public-reference-missing');
      live.add(path);
      references(parseJson(await readFile(join(staging, path))));
    }
    for (const path of preparedAssets.keys()) if (!live.has(path)) await unlink(join(staging, path));
    prepared.assets = prepared.assets.filter(entry => live.has(entry.path));
    prepared.counts.bytes = prepared.assets.reduce((sum, item) => sum + item.bytes, 0);
    await writeFile(join(staging, 'manifest.json'), JSON.stringify(prepared, null, 2));
    if ([...countById.values()].some(count => count === 0)) throw new Error('archive.reviewed-value-not-found');
    onProgress({ phase: 'verification' });
    const verified = await verifyDemoSession(staging);
    if (digest(await readFile(join(source, 'manifest.json'))) !== report.sourceManifestSha256) throw new Error('archive.public-source-changed');
    const disclosure = JSON.stringify({ ...report, verification: verified }, null, 2);
    for (const text of [JSON.stringify(prepared), await readFile(join(staging, 'catalog.json'), 'utf8'), disclosure]) {
      if (replacements.some(item => text.includes(item.value))) throw new Error('archive.public-metadata-sensitive');
    }
    await writeFile(join(staging, 'disclosure-review.json'), disclosure);
    await absent(output);
    await rename(staging, output);
    return { output, ...verified, replacements: [...countById].map(([id, count]) => ({ id, count })), publication: report.publication };
  } catch (error) {
    await writeFile(join(staging, 'incomplete.json'), JSON.stringify({ code: 'archive.public-build-incomplete' }));
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [source, output, gzipReviewPath] = process.argv.slice(2);
  if (!source || !output) throw new Error('Usage: node scripts/create-public-demo-archive.mjs <source> <output> [gzip-review.json] < reviewed-values.json');
  let input = '';
  for await (const chunk of process.stdin) {
    input += chunk;
    if (input.length > 65536) throw new Error('archive.reviewed-values-budget');
  }
  const gzipReviews = gzipReviewPath ? parseJson(await readFile(gzipReviewPath)) : [];
  console.log(JSON.stringify(await createPublicDemoArchive(source, output, parseJson(input), value => console.log(JSON.stringify(value)), gzipReviews)));
}
