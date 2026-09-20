import type { WorkspaceDirectoryEntryVm } from '@/types';
import { guessMimeFromExtension } from './attachment-service';
import {
  addComposerWorkspaceFile,
  type ComposerWorkspaceFileRef,
} from './composer-context';

export function composerWorkspaceFileRefFromEntry(
  projectId: string,
  entry: WorkspaceDirectoryEntryVm,
): ComposerWorkspaceFileRef {
  return {
    id: `${projectId}:${entry.relativePath.replaceAll('\\', '/')}`,
    projectId,
    relativePath: entry.relativePath,
    name: entry.name,
    byteLength: entry.byteLength ?? null,
    mimeType: guessMimeFromExtension(entry.name),
    canonicalPath: entry.canonicalPath,
  };
}

export function addComposerWorkspaceFileRef(
  workspaceFiles: readonly ComposerWorkspaceFileRef[],
  attachmentCount: number,
  ref: ComposerWorkspaceFileRef,
) {
  return addComposerWorkspaceFile(workspaceFiles, attachmentCount, ref);
}
