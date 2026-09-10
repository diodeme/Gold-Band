import { z } from 'zod';
import type { ArchiveCatalog, ArchiveReader, ArchiveSession } from './archive';
import { archiveDirectoryPath, normalizeArchiveRelativePath } from './archive-directories';

const workerReference = z.object({ continue_ref: z.object({ snapshotFile: z.string() }) });
const WORKER_REFERENCE_MAX_BYTES = 64 * 1024;
const missing = () => ({ code: 'demo.resource-not-found', params: {} });

function absolutePath(value: string) {
  let path: string;
  try {
    if (decodeURIComponent(value).replaceAll('\\', '/').split('/').some(part => part === '..' || part === '.')) throw missing();
    if (/^file:/i.test(value)) {
      const url = new URL(value);
      if (url.host || url.search || url.hash) throw missing();
      path = decodeURIComponent(url.pathname);
    } else path = decodeURIComponent(value);
  } catch { throw missing(); }
  path = path.replaceAll('\\', '/').replace(/^\/([a-z]:\/)/i, '$1');
  if (!/^(?:[a-z]:\/|\/)/i.test(path) || /[\0?#]/.test(path) || path.startsWith('//')
    || path.split('/').some(part => part === '..' || part === '.')) throw missing();
  return path;
}

function pathKey(path: string) {
  return /^[a-z]:\//i.test(path) ? path.toLowerCase() : path;
}

function sessionSuffix(session: ArchiveSession) {
  const { projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId } = session;
  const prefix = `/projects/${projectId}/tasks/${taskId}/runs/${runId}/rounds/${roundId}/nodes/`;
  return prefix + (outerNodeId ? `${outerNodeId}/${outerAttemptId}/dynamic/nodes/` : '') + `${nodeId}/${attemptId}/`;
}

function sourceAttempt(catalog: ArchiveCatalog, projectId: string, href: string) {
  if (catalog.projectId !== projectId) throw missing();
  const path = absolutePath(href);
  const key = pathKey(path);
  const candidates = catalog.sessions.flatMap(session => {
    const suffix = sessionSuffix(session);
    const index = key.lastIndexOf(/^[a-z]:\//i.test(path) ? suffix.toLowerCase() : suffix);
    return index < 0 ? [] : [{ session, prefix: path.slice(0, index + suffix.length) }];
  }).sort((a, b) => b.prefix.length - a.prefix.length);
  // Nested attempts also live under their outer attempt. Use the most specific full locator.
  if (!candidates.length) return null;
  if (candidates[0].prefix.length === candidates[1]?.prefix.length) throw missing();
  return { ...candidates[0], path };
}

export function isArchiveRunSourceReference(catalog: ArchiveCatalog, projectId: string, href: string) {
  if (!/^(?:[a-z]:[\\/]|[\\/]|file:)/i.test(href)) return false;
  return sourceAttempt(catalog, projectId, href) !== null;
}

export async function resolveArchiveSourceReference(reader: ArchiveReader, catalog: ArchiveCatalog, projectId: string, href: string) {
  const candidate = sourceAttempt(catalog, projectId, href);
  if (!candidate) throw missing();
  const { session, prefix, path } = candidate;
  const worker = await reader.directoryFile(session, 'worker-ref.json');
  const content = await reader.resourceText(worker.resource, WORKER_REFERENCE_MAX_BYTES);
  let value: unknown;
  try { value = JSON.parse(content); } catch { throw missing(); }
  const parsed = workerReference.safeParse(value);
  if (!parsed.success || pathKey(absolutePath(parsed.data.continue_ref.snapshotFile)) !== pathKey(`${prefix}acp.snapshot.json`)) throw missing();
  const relativePath = normalizeArchiveRelativePath(path.slice(prefix.length));
  const file = await reader.directoryFile(session, relativePath);
  return { file, canonicalPath: archiveDirectoryPath(session, file.relativePath) };
}
