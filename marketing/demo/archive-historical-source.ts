import { z } from 'zod';
import type { WorkspaceFileLinkOrigin } from '@/api/client';

export const ARCHIVE_SOURCE_PREFIX = '/archive-source/';
const fields = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId', 'branchId'] as const;
const identifier = z.string().min(1);
const addressSchema = z.tuple([identifier, identifier, identifier, identifier, identifier, identifier,
  identifier.nullable(), identifier.nullable(), identifier, identifier, identifier]);
const missing = () => ({ code: 'demo.resource-not-found', params: {} });

export function archiveSourceName(sourcePath: string) {
  const name = sourcePath.replaceAll('\\', '/').split('/').at(-1);
  if (!name || name === '.' || name === '..' || /[:\0]/.test(name)) throw missing();
  return name;
}

export function archiveHistoricalSourcePath(origin: WorkspaceFileLinkOrigin, href: string, sourcePath: string) {
  const values = addressSchema.parse([...fields.map(field => origin.locator[field] ?? null), origin.eventId, href]);
  return `${ARCHIVE_SOURCE_PREFIX}${encodeURIComponent(JSON.stringify(values))}/${encodeURIComponent(archiveSourceName(sourcePath))}`;
}

export function parseArchiveHistoricalSourcePath(path: string) {
  try {
    if (!path.startsWith(ARCHIVE_SOURCE_PREFIX)) throw missing();
    const [address, encodedName, extra] = path.slice(ARCHIVE_SOURCE_PREFIX.length).split('/');
    if (!encodedName || extra !== undefined) throw missing();
    const values = addressSchema.parse(JSON.parse(decodeURIComponent(address)));
    const name = decodeURIComponent(encodedName);
    if (archiveSourceName(name) !== name) throw missing();
    const locator = Object.fromEntries(fields.map((field, index) => [field, values[index]])) as unknown as WorkspaceFileLinkOrigin['locator'];
    return { origin: { locator, eventId: values[9] }, href: values[10], name };
  } catch { throw missing(); }
}
