import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readFile, realpath } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ARCHIVE_VERSION } from './export-demo-session.mjs';
import { isDeepStrictEqual } from 'node:util';
import { verifySourceReferenceBodies, verifySourceReferenceIndex, sourceLocatorKey, sourceReferenceKey } from './archive-source-references.mjs';

export function createDirectoryVerifier(asset, resources) {
  const verified = new Map();
  const visiting = new Set();
  return async function verify(path) {
    if (!/^assets\/[a-f0-9]{64}\.json$/.test(path ?? '')) throw new Error('archive.directory-reference-invalid');
    if (visiting.has(path)) throw new Error('archive.directory-cycle');
    if (verified.has(path)) return verified.get(path);
    visiting.add(path);
    try {
      const listing = await asset(path);
      if (!Array.isArray(listing.entries)) throw new Error('archive.directory-listing-invalid');
      const names = new Set();
      for (const entry of listing.entries) {
        if (typeof entry.name !== 'string' || !entry.name || /[\\/:\0]/.test(entry.name) || ['.', '..'].includes(entry.name)) throw new Error('archive.directory-name-invalid');
        const name = entry.name.toLowerCase();
        if (names.has(name)) throw new Error('archive.directory-name-conflict');
        names.add(name);
        if (entry.kind === 'directory') {
          const hasChildren = await verify(entry.directory);
          if (entry.hasChildren !== hasChildren) throw new Error('archive.directory-children-invalid');
        } else if (entry.kind === 'file') {
          const resource = entry.resource;
          const listed = resources.get(resource?.path);
          if (!listed || listed.sha256 !== resource.sha256 || listed.bytes !== resource.bytes) throw new Error('archive.directory-file-reference-invalid');
        } else throw new Error('archive.directory-entry-invalid');
      }
      const hasChildren = listing.entries.length > 0;
      verified.set(path, hasChildren);
      return hasChildren;
    } finally { visiting.delete(path); }
  };
}

export function indexArchiveSourceVersions(resources) {
  const versions = new Map();
  for (const resource of resources) {
    const match = /(?:^|\/)acp\.file-blobs\/([a-f0-9]{2})\/([a-f0-9]{64})$/.exec(resource.source);
    if (!match) continue;
    if (!match[2].startsWith(match[1])) throw new Error('archive.source-version-path');
    const previous = versions.get(match[2]);
    if (previous && ['path', 'sha256', 'bytes', 'sourceSha256', 'sourceBytes'].some(key => previous[key] !== resource[key])) {
      throw new Error('archive.source-version-conflict');
    }
    versions.set(match[2], resource);
  }
  return versions;
}

export function verifyArchiveFileReferences(id, data, branchIds, resources, sourceVersions) {
  if (data.changeSet.id !== id || !branchIds.has(data.changeSet.branchId)) throw new Error('archive.change-set-identity');
  function reference(resource) {
    const listed = resource && resources.get(resource.path);
    if (!listed || listed.sha256 !== resource.sha256 || listed.bytes !== resource.bytes) throw new Error('archive.file-reference-invalid');
    return listed;
  }
  const changes = new Set();
  for (const change of data.changeSet.changes) {
    if (changes.has(change.id)) throw new Error('archive.file-change-duplicate');
    changes.add(change.id);
    for (const version of [change.beforeVersion, change.afterVersion]) {
      if (!version) continue;
      const resource = data.versions[version.contentHash];
      reference(resource);
      const source = sourceVersions.get(version.contentHash);
      if (!source || source.path !== resource.path || source.sha256 !== resource.sha256) throw new Error('archive.file-version-source');
      if (source.sourceBytes !== version.byteLength) throw new Error('archive.file-version-size');
    }
  }
  const attachments = new Set();
  for (const attachment of data.changeSet.attachments) {
    if (attachments.has(attachment.id)) throw new Error('archive.attachment-duplicate');
    attachments.add(attachment.id);
    reference(data.attachments[attachment.id]);
  }
  for (const resource of [...Object.values(data.versions), ...Object.values(data.attachments)]) reference(resource);
}

export async function verifyDemoSession(directory) {
  const root = await realpath(directory);
  const manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8'));
  if (manifest.version !== ARCHIVE_VERSION) throw new Error('archive.version-unsupported');
  const sourceVersions = indexArchiveSourceVersions(manifest.resources);
  const files = new Map();
  let bytes = 0;
  async function file(path) {
    if (!/^(?:assets\/[a-f0-9]{64}\.json|resources\/[a-f0-9]{64})$/.test(path)) throw new Error('archive.resource-path-invalid');
    const target = await realpath(join(root, path));
    if (!target.startsWith(`${root}${sep}`)) throw new Error('archive.resource-outside-root');
    return target;
  }
  for (const resource of [...manifest.assets, ...manifest.resources]) {
    const previous = files.get(resource.path);
    if (previous) {
      if (previous.sha256 !== resource.sha256 || previous.bytes !== resource.bytes) throw new Error('archive.resource-conflict');
      continue;
    }
    const digest = createHash('sha256');
    let length = 0;
    for await (const chunk of createReadStream(await file(resource.path))) { digest.update(chunk); length += chunk.length; }
    if (length !== resource.bytes || digest.digest('hex') !== resource.sha256) throw new Error(`archive.resource-corrupt:${resource.path}`);
    bytes += length;
    files.set(resource.path, resource);
  }
  async function asset(path) {
    if (!files.has(path)) throw new Error(`archive.resource-unlisted:${path}`);
    return JSON.parse(await readFile(await file(path), 'utf8'));
  }
  await asset(manifest.workflow);
  if (manifest.runView) {
    const view = await asset(manifest.runView);
    if (view.projectId !== manifest.projectId || view.taskId !== manifest.task.id || view.runId !== manifest.run.id
      || view.runStatus !== manifest.run.status || view.runOutcome !== (manifest.run.outcome ?? null)) throw new Error('archive.run-view-identity');
  }
  const locators = new Set();
  const verifyDirectory = createDirectoryVerifier(asset, files);
  let items = 0;
  let records = 0;
  let branches = 0;
  let sourceReferences = 0;
  let unresolvedSourceReferences = 0;
  const referenceIndexes = [];
  for (const session of manifest.sessions) {
    const key = JSON.stringify(['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId'].map(field => session[field] ?? null));
    if (locators.has(key)) throw new Error('archive.session-duplicate');
    locators.add(key);
    const detail = await asset(session.detail);
    if (detail.sourceReferences) {
      const references = await verifySourceReferenceIndex(await asset(detail.sourceReferences), session, detail, asset, files);
      await verifySourceReferenceBodies(references.references, asset, async path => readFile(await file(path)));
      referenceIndexes.push({ locator: session, ...references });
      sourceReferences += references.references.length;
      unresolvedSourceReferences += references.unresolved.length;
    }
    if (manifest.runtimeVersion !== undefined) await verifyDirectory(detail.directoryRoot);
    const branchIds = new Set();
    let sessionItems = 0;
    for (const branch of detail.branches) {
      if (branchIds.has(branch.id)) throw new Error('archive.branch-duplicate');
      branchIds.add(branch.id);
      branches++;
      let count = 0;
      let lastSeq = -Infinity;
      const ids = new Set();
      for (const page of branch.pages) {
        const events = await asset(page.path);
        if (events.length !== page.count || events[0]?.seq !== page.firstSeq || events.at(-1)?.seq !== page.lastSeq) throw new Error('archive.page-count-or-boundary');
        for (const event of events) {
          if (ids.has(event.id) || !Number.isSafeInteger(event.seq) || event.seq <= lastSeq) throw new Error('archive.timeline-order-or-identity');
          ids.add(event.id);
          lastSeq = event.seq;
          if (event.kind === 'toolCall') {
            const tool = await asset(branch.tools[event.id]);
            if (tool.id !== event.id || tool.seq !== event.seq) throw new Error('archive.tool-identity');
          }
        }
        count += events.length;
      }
      if (count !== branch.count) throw new Error('archive.branch-count');
      sessionItems += count;
    }
    for (const [id, path] of Object.entries(detail.changeSets ?? {})) {
      verifyArchiveFileReferences(id, await asset(path), branchIds, files, sourceVersions);
    }
    if (sessionItems !== session.itemCount) throw new Error('archive.session-count');
    items += sessionItems;
    records += session.recordCount;
  }
  if (items !== manifest.counts.items || records !== manifest.counts.records || locators.size !== manifest.counts.sessions) throw new Error('archive.manifest-count');
  const sourceItems = manifest.sourceFiles.reduce((sum, entry) => sum + entry.items, 0);
  const sourceRows = manifest.sourceFiles.reduce((sum, entry) => sum + entry.rows, 0);
  if (sourceItems !== items || sourceRows !== records) throw new Error('archive.source-count');
  if (manifest.sourceReferenceSupplement) {
    const metadata = manifest.sourceReferenceSupplement;
    const audit = await asset(metadata.audit);
    if (files.get(metadata.audit).sha256 !== metadata.sourceSupplementSha256
      || audit.sourceManifestSha256 !== metadata.sourceManifestSha256
      || audit.kind !== 'reviewed-source-reference-supplement' || audit.version !== 1) throw new Error('archive.source-supplement-mismatch');
    for (const category of ['references', 'unresolved']) {
      const canonical = references => {
        const map = new Map();
        for (const { locator, ...reference } of references) {
          const key = JSON.stringify([sourceLocatorKey(locator), sourceReferenceKey(reference)]);
          if (map.has(key) && !isDeepStrictEqual(map.get(key), reference)) throw new Error('archive.source-reference-conflict');
          map.set(key, reference);
        }
        return map;
      };
      const actual = canonical(referenceIndexes.flatMap(index => index[category].map(reference => ({ locator: index.locator, ...reference }))));
      if (!isDeepStrictEqual(actual, canonical(audit[category]))) throw new Error('archive.source-reference-audit-mismatch');
    }
  } else if (referenceIndexes.length) throw new Error('archive.source-reference-audit-missing');
  return { version: ARCHIVE_VERSION, sessions: locators.size, branches, items, records, resources: files.size, bytes,
    ...(manifest.sourceReferenceSupplement ? { sourceReferences, unresolvedSourceReferences } : {}),
    missing: manifest.missing.length, redactions: manifest.redactions.length,
    integrity: manifest.missing.length ? 'missing-resources' : 'verified', publication: 'pending-reference-and-privacy-review' };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2]) throw new Error('Usage: node scripts/verify-demo-session.mjs <archive-directory>');
  console.log(JSON.stringify(await verifyDemoSession(process.argv[2]), null, 2));
}
