import { z } from 'zod';
import type { TurnFileChangeSetVm, TurnFileLocatorVm } from '@/types';

export const ARCHIVE_FILE_LIMITS = { diffBytes: 2 * 1024 * 1024, diffLines: 100_000, textBytes: 10 * 1024 * 1024,
  imageBytes: 20 * 1024 * 1024, imagePixels: 40_000_000, previewLifetimeMs: 5 * 60 * 1_000 };
const version = z.object({ id: z.string(), storageKind: z.literal('capturedBlob'), contentHash: z.string().regex(/^[a-f0-9]{64}$/), byteLength: z.number().int().nonnegative(),
  encoding: z.string().nullable().optional(), lineEnding: z.string().nullable().optional() });
export const archiveChangeSetSchema: z.ZodType<TurnFileChangeSetVm> = z.object({ schemaVersion: z.number().optional(), id: z.string(), turnId: z.string(), promptEventId: z.string(),
  branchId: z.string(), status: z.enum(['capturing', 'finalized', 'partial']), startedAt: z.string(), finishedAt: z.string().nullable().optional(),
  summary: z.object({ fileCount: z.number().int(), addedFiles: z.number().int(), modifiedFiles: z.number().int(), deletedFiles: z.number().int(), addedLines: z.number().int(), deletedLines: z.number().int() }),
  changes: z.array(z.object({ id: z.string(), changeKind: z.enum(['added', 'modified', 'deleted', 'renamed']), logicalPath: z.string(), text: z.boolean(),
    beforeVersion: version.nullable().optional(), afterVersion: version.nullable().optional() }).passthrough()),
  attachments: z.array(z.object({ id: z.string(), relativePath: z.string(), name: z.string(), byteLength: z.number().int().nonnegative() })), limitationCodes: z.array(z.string()),
});
export const archiveResourceSchema = z.object({ path: z.string().regex(/^resources\/[a-f0-9]{64}$/), bytes: z.number().int().nonnegative(), sha256: z.string().regex(/^[a-f0-9]{64}$/) })
  .refine(value => value.path === `resources/${value.sha256}`);
const attachmentSchema = z.intersection(archiveResourceSchema, z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('text'), encoding: z.literal('utf-8'), lineEnding: z.enum(['lf', 'crlf', 'mixed']) }),
  z.object({ kind: z.literal('image'), mimeType: z.string(), width: z.number().int().positive(), height: z.number().int().positive(), animated: z.boolean() }),
  z.object({ kind: z.literal('unsupported') }),
]));
export const archiveFilesSchema = z.object({ changeSet: archiveChangeSetSchema, versions: z.record(z.string(), archiveResourceSchema), attachments: z.record(z.string(), attachmentSchema) });
export type ArchiveResource = z.infer<typeof archiveResourceSchema>;
const fields = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId', 'branchId'] as const;
export const ARCHIVE_ATTACHMENT_PREFIX = '/archive-attachment/';
export function archiveAttachmentPath(locator: TurnFileLocatorVm, changeSetId: string, attachmentId: string) {
  return ARCHIVE_ATTACHMENT_PREFIX + encodeURIComponent(JSON.stringify([...fields.map(field => locator[field] ?? null), changeSetId, attachmentId]));
}
export function parseArchiveAttachmentPath(path: string) {
  if (!path.startsWith(ARCHIVE_ATTACHMENT_PREFIX)) throw { code: 'demo.resource-not-found', params: {} };
  try {
    const values = z.tuple([z.string(), z.string(), z.string(), z.string(), z.string(), z.string(), z.string().nullable(), z.string().nullable(), z.string(), z.string(), z.string()])
      .parse(JSON.parse(decodeURIComponent(path.slice(ARCHIVE_ATTACHMENT_PREFIX.length))));
    return { locator: Object.fromEntries(fields.map((field, index) => [field, values[index]])) as unknown as TurnFileLocatorVm, changeSetId: values[9], attachmentId: values[10] };
  } catch { throw { code: 'demo.archive-path-invalid', params: {} }; }
}
