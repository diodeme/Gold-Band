import { isDeepStrictEqual } from 'node:util';
import { createHash } from 'node:crypto';

export const sourceLocatorFields = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId'];
export const sourceLocatorKey = locator => JSON.stringify(sourceLocatorFields.map(field => locator?.[field] ?? null));
export const sourceReferenceKey = reference => JSON.stringify([reference.branchId, reference.eventId, reference.href]);
const fail = code => { throw new Error(`archive.source-reference-${code}`); };
const nonempty = value => typeof value === 'string' && value.length > 0 && !value.includes('\0');

export async function verifySourceReferenceIndex(index, session, detail, readAsset, resources) {
  if (index?.version !== 1 || sourceLocatorKey(index.locator) !== sourceLocatorKey(session)
    || !Array.isArray(index.references) || !Array.isArray(index.unresolved)) fail('scope');
  const byBranch = new Map();
  const unique = new Map();
  for (const reference of [...index.references, ...index.unresolved]) {
    if (!nonempty(reference.branchId) || !nonempty(reference.eventId) || !nonempty(reference.href)) fail('identity');
    const key = sourceReferenceKey(reference);
    if (unique.has(key)) {
      if (!isDeepStrictEqual(unique.get(key), reference)) fail('conflict');
      continue;
    }
    unique.set(key, reference);
    const items = byBranch.get(reference.branchId) ?? [];
    items.push(reference);
    byBranch.set(reference.branchId, items);
  }
  for (const [branchId, references] of byBranch) {
    const branch = detail.branches.find(branch => branch.id === branchId);
    if (!branch) fail('branch');
    const remaining = new Map();
    for (const reference of references) {
      const entries = remaining.get(reference.eventId) ?? [];
      entries.push(reference);
      remaining.set(reference.eventId, entries);
    }
    for (const page of branch.pages) {
      for (const event of await readAsset(page.path)) {
        for (const reference of remaining.get(event.id) ?? []) {
          if (typeof event.content !== 'string' || !event.content.includes(reference.href)) fail('message');
          if (reference.provenance?.kind === 'captured-complete-read' && reference.provenance.seq > event.seq) fail('future-read');
        }
        remaining.delete(event.id);
      }
      if (!remaining.size) break;
    }
    if (remaining.size) fail('event');
  }
  for (const reference of index.references) {
    if (!nonempty(reference.sourcePath) || /[\0?#]/.test(reference.sourcePath)
      || !/^(?:[a-z]:[\\/]|\/)/i.test(reference.sourcePath)
      || reference.sourcePath.replaceAll('\\', '/').split('/').some(part => part === '.' || part === '..')) fail('path');
    const resource = resources.get(reference.resource?.path);
    if (!resource || resource.sha256 !== reference.resource.sha256 || resource.bytes !== reference.resource.bytes) fail('resource');
    if (!Array.isArray(reference.evidence) || !reference.evidence.length) fail('evidence');
    for (const path of reference.evidence) await readAsset(path);
    const provenance = reference.provenance;
    if (provenance?.kind === 'git-blob') {
      if (!/^[a-f0-9]{40}$/.test(provenance.commit ?? '') || !/^[a-f0-9]{40}$/.test(provenance.blobOid ?? '')
        || !nonempty(provenance.path) || /[\\:\0]/.test(provenance.path) || provenance.path.startsWith('/')
        || provenance.path.split('/').some(part => part === '.' || part === '..')) fail('provenance');
    } else if (provenance?.kind === 'captured-complete-read') {
      if (!reference.evidence.includes(provenance.asset) || provenance.startLine !== 1
        || !Number.isSafeInteger(provenance.seq) || !Number.isSafeInteger(provenance.totalLines) || provenance.totalLines < 1) fail('provenance');
    } else fail('provenance');
  }
  for (const reference of index.unresolved) {
    if (!nonempty(reference.reason) || reference.resource !== undefined) fail('unresolved');
  }
  return {
    references: [...new Map(index.references.map(reference => [sourceReferenceKey(reference), reference])).values()],
    unresolved: [...new Map(index.unresolved.map(reference => [sourceReferenceKey(reference), reference])).values()],
  };
}

export async function verifySourceReferenceBodies(references, readAsset, readResource) {
  for (const reference of references) {
    const bytes = await readResource(reference.resource.path);
    const provenance = reference.provenance;
    if (provenance.kind === 'git-blob') {
      const oid = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
      if (oid !== provenance.blobOid) fail('git-blob-mismatch');
    } else {
      const captured = await readAsset(provenance.asset);
      const file = captured.raw?._meta?.claudeCode?.toolResponse?.file;
      if (captured.seq !== provenance.seq || file?.startLine !== 1 || file?.numLines !== file?.totalLines
        || file?.totalLines !== provenance.totalLines || typeof file?.content !== 'string'
        || file.filePath.replaceAll('\\', '/') !== reference.sourcePath.replaceAll('\\', '/')
        || !Buffer.from(file.content).equals(bytes)) fail('complete-read-mismatch');
    }
  }
}
