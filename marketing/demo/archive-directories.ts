import { z } from 'zod';
import type { TurnFileLocatorVm } from '@/types';
import { archiveResourceSchema } from './archive-files';

const name = z.string().min(1).refine(value => !/[\\/:\0]/.test(value) && value !== '.' && value !== '..');
export const archiveDirectorySchema = z.object({ entries: z.array(z.discriminatedUnion('kind', [
  z.object({ name, kind: z.literal('directory'), directory: z.string().regex(/^assets\/[a-f0-9]{64}\.json$/), hasChildren: z.boolean() }),
  z.object({ name, kind: z.literal('file'), resource: archiveResourceSchema, image: z.object({ mimeType: z.string(), width: z.number().positive(),
    height: z.number().positive(), animated: z.boolean() }).nullable() }),
])) });
export type ArchiveDirectoryLocator = Omit<TurnFileLocatorVm, 'branchId'>;
export const ARCHIVE_DIRECTORY_PREFIX = '/archive-directory/';
export function resolveArchiveRelativeReference(documentPath: string, rawReference: string) {
  let reference: string;
  try { reference = decodeURIComponent(rawReference).replaceAll('\\', '/'); }
  catch { throw { code: 'demo.archive-path-invalid', params: {} }; }
  if (!reference || /[\0:?#]/.test(reference) || reference.startsWith('/')) {
    throw { code: 'conversation-directory.path-outside-root', params: {} };
  }
  const root = 'https://archive.invalid/root/';
  const base = new URL(normalizeArchiveRelativePath(documentPath).split('/').map(encodeURIComponent).join('/'), root);
  const resolved = new URL(reference.split('/').map(encodeURIComponent).join('/'), base);
  if (!resolved.href.startsWith(root)) throw { code: 'conversation-directory.path-outside-root', params: {} };
  return normalizeArchiveRelativePath(decodeURIComponent(resolved.pathname.slice('/root/'.length)));
}
const fields = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId'] as const;
export function normalizeArchiveRelativePath(path: string) {
  const normalized = path.replaceAll('\\', '/');
  if (/[\0:]/.test(normalized) || normalized.startsWith('/') || normalized.split('/').some(part => part === '..')) throw { code: 'conversation-directory.path-outside-root', params: {} };
  return normalized.split('/').filter(part => part !== '' && part !== '.').join('/');
}
export function archiveDirectoryPath(locator: ArchiveDirectoryLocator, relativePath: string) {
  return ARCHIVE_DIRECTORY_PREFIX + encodeURIComponent(JSON.stringify(fields.map(field => locator[field] ?? null))) + '/'
    + normalizeArchiveRelativePath(relativePath).split('/').map(encodeURIComponent).join('/');
}
export function parseArchiveDirectoryPath(path: string) {
  try {
    if (!path.startsWith(ARCHIVE_DIRECTORY_PREFIX)) throw new Error();
    const rest = path.slice(ARCHIVE_DIRECTORY_PREFIX.length);
    const slash = rest.indexOf('/');
    if (slash < 0) throw new Error();
    const values = z.tuple([z.string(), z.string(), z.string(), z.string(), z.string(), z.string(), z.string().nullable(), z.string().nullable()])
      .parse(JSON.parse(decodeURIComponent(rest.slice(0, slash))));
    return { locator: Object.fromEntries(fields.map((field, index) => [field, values[index] ?? undefined])) as ArchiveDirectoryLocator,
      relativePath: normalizeArchiveRelativePath(decodeURIComponent(rest.slice(slash + 1))) };
  } catch { throw { code: 'demo.archive-path-invalid', params: {} }; }
}
