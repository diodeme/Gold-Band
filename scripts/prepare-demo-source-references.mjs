import { createHash } from 'node:crypto';
import { copyFile, lstat, mkdir, mkdtemp, readFile, realpath, rename, writeFile } from 'node:fs/promises';
import { dirname, join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { verifySourceReferenceBodies, verifySourceReferenceIndex, sourceLocatorFields, sourceLocatorKey } from './archive-source-references.mjs';
import { verifyDemoSession } from './verify-demo-session.mjs';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const contained = (root, target) => target === root || target.startsWith(`${root}${sep}`);
async function absent(target) {
  try { await lstat(target); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
  throw new Error('archive.source-output-exists');
}
async function localPath(root, relative) {
  if (!/^(?:assets\/[a-f0-9]{64}\.json|resources\/[a-f0-9]{64}|disclosure-review\.json)$/.test(relative)) throw new Error('archive.resource-path-invalid');
  const target = await realpath(join(root, relative));
  if (!contained(root, target)) throw new Error('archive.resource-outside-root');
  return target;
}

export async function prepareDemoSourceReferences(sourceDirectory, supplementDirectory, outputDirectory, progress = () => {}) {
  const source = await realpath(sourceDirectory);
  const supplement = await realpath(supplementDirectory);
  const parent = await realpath(dirname(resolve(outputDirectory)));
  const output = join(parent, resolve(outputDirectory).split(sep).at(-1));
  if ([source, supplement].some(root => contained(root, output) || contained(output, root))) throw new Error('archive.source-output-overlap');
  await absent(output);
  const originalBytes = await readFile(join(source, 'manifest.json'));
  const manifest = JSON.parse(originalBytes);
  const catalog = JSON.parse(await readFile(join(source, 'catalog.json'), 'utf8'));
  const supplementalBytes = await readFile(join(supplement, 'manifest.json'));
  const input = JSON.parse(supplementalBytes);
  if (manifest.version !== 1 || manifest.runtimeVersion !== 1 || manifest.sourceReferenceSupplement
    || input.version !== 1 || input.kind !== 'reviewed-source-reference-supplement'
    || input.sourceManifestSha256 !== digest(originalBytes)
    || !Array.isArray(input.references) || !Array.isArray(input.unresolved) || !Array.isArray(input.resources)) {
    throw new Error('archive.source-supplement-mismatch');
  }
  if (JSON.stringify(catalog.sessions) !== JSON.stringify(manifest.sessions)) throw new Error('archive.source-catalog-mismatch');
  const assets = new Map(manifest.assets.map(asset => [asset.path, asset]));
  const resources = new Map(manifest.resources.map(resource => [resource.path, resource]));
  const readAsset = async relative => {
    const expected = assets.get(relative);
    if (!expected) throw new Error('archive.source-evidence-unlisted');
    const bytes = await readFile(await localPath(source, relative));
    if (bytes.length !== expected.bytes || digest(bytes) !== expected.sha256) throw new Error('archive.source-evidence-corrupt');
    return JSON.parse(bytes);
  };
  const supplementalResources = new Map();
  for (const resource of input.resources) {
    if (!/^[a-f0-9]{64}$/.test(resource.sha256 ?? '') || resource.path !== `resources/${resource.sha256}`
      || !Number.isSafeInteger(resource.bytes) || resource.bytes < 0 || supplementalResources.has(resource.path)) throw new Error('archive.source-resource-invalid');
    const bytes = await readFile(await localPath(supplement, resource.path));
    if (bytes.length !== resource.bytes || digest(bytes) !== resource.sha256) throw new Error('archive.source-resource-corrupt');
    new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    supplementalResources.set(resource.path, resource);
    const prior = resources.get(resource.path);
    if (prior && (prior.bytes !== resource.bytes || prior.sha256 !== resource.sha256)) throw new Error('archive.source-resource-conflict');
    resources.set(resource.path, resource);
  }
  const sessions = new Map(manifest.sessions.map(session => [sourceLocatorKey(session), session]));
  const indexes = new Map();
  for (const [category, references] of [['references', input.references], ['unresolved', input.unresolved]]) {
    for (const reference of references) {
      const key = sourceLocatorKey(reference.locator);
      const session = sessions.get(key);
      if (!session) throw new Error('archive.source-reference-scope');
      let index = indexes.get(key);
      if (!index) {
        index = { version: 1, locator: Object.fromEntries(sourceLocatorFields.filter(field => session[field] != null).map(field => [field, session[field]])), references: [], unresolved: [] };
        indexes.set(key, index);
      }
      const { locator, ...entry } = reference;
      index[category].push(entry);
    }
  }
  const used = new Set();
  for (const [key, index] of indexes) {
    const session = sessions.get(key);
    Object.assign(index, await verifySourceReferenceIndex(index, session, await readAsset(session.detail), readAsset, resources));
    for (const reference of index.references) {
      if (!supplementalResources.has(reference.resource.path)) throw new Error('archive.source-resource-unlisted');
      used.add(reference.resource.path);
    }
    await verifySourceReferenceBodies(index.references, readAsset, async relative => readFile(await localPath(supplement, relative)));
  }
  if (used.size !== supplementalResources.size) throw new Error('archive.source-resource-unused');
  const staging = await mkdtemp(join(parent, '.source-archive-'));
  await mkdir(join(staging, 'assets'));
  await mkdir(join(staging, 'resources'));
  try {
    const originals = new Map([...manifest.assets, ...manifest.resources].map(resource => [resource.path, resource]));
    let copied = 0;
    for (const resource of originals.values()) {
      await copyFile(await localPath(source, resource.path), join(staging, resource.path));
      if (++copied % 10000 === 0) progress({ phase: 'copy', copied, total: originals.size });
    }
    for (const resource of supplementalResources.values()) {
      if (!originals.has(resource.path)) await copyFile(await localPath(supplement, resource.path), join(staging, resource.path));
      manifest.resources.push({ ...resource, source: `source-supplement/${resource.sha256}`, sourceSha256: resource.sha256, sourceBytes: resource.bytes });
    }
    const emitBytes = async bytes => {
      const sha256 = digest(bytes);
      const relative = `assets/${sha256}.json`;
      await writeFile(join(staging, relative), bytes);
      assets.set(relative, { path: relative, sha256, bytes: bytes.length });
      return relative;
    };
    const emit = value => emitBytes(Buffer.from(JSON.stringify(value)));
    for (const [key, index] of indexes) {
      const session = sessions.get(key);
      const detail = await readAsset(session.detail);
      detail.sourceReferences = await emit(index);
      session.detail = await emit(detail);
    }
    manifest.sourceReferenceSupplement = { sourceManifestSha256: digest(originalBytes), sourceSupplementSha256: digest(supplementalBytes),
      audit: await emitBytes(supplementalBytes), publication: 'pending-disclosure-review' };
    manifest.assets = [...assets.values()];
    manifest.counts.bytes = manifest.assets.reduce((total, asset) => total + asset.bytes, 0);
    catalog.sessions = manifest.sessions;
    await writeFile(join(staging, 'manifest.json'), JSON.stringify(manifest, null, 2));
    await writeFile(join(staging, 'catalog.json'), JSON.stringify(catalog));
    try { await copyFile(await localPath(source, 'disclosure-review.json'), join(staging, 'disclosure-review.json')); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    progress({ phase: 'verification' });
    const verification = await verifyDemoSession(staging);
    if (digest(await readFile(join(source, 'manifest.json'))) !== digest(originalBytes)
      || digest(await readFile(join(supplement, 'manifest.json'))) !== digest(supplementalBytes)) throw new Error('archive.source-changed');
    await absent(output);
    await rename(staging, output);
    return { output, ...verification, referenceOccurrences: input.references.length,
      uniqueReferences: [...indexes.values()].reduce((sum, index) => sum + index.references.length, 0),
      unresolvedReferences: [...indexes.values()].reduce((sum, index) => sum + index.unresolved.length, 0) };
  } catch (error) {
    await writeFile(join(staging, 'incomplete.json'), JSON.stringify({ code: 'archive.source-build-incomplete' }));
    throw error;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [source, supplement, output] = process.argv.slice(2);
  if (!source || !supplement || !output) throw new Error('Usage: node scripts/prepare-demo-source-references.mjs <archive> <supplement> <output>');
  console.log(JSON.stringify(await prepareDemoSourceReferences(source, supplement, output, value => console.log(JSON.stringify(value)))));
}
