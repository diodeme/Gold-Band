import type { WorkspaceDirectoryEntryVm } from '@/types';
import { resolveWorkspaceFileLink } from '@/api';
import { guessMimeFromExtension } from './attachment-service';
import {
  addComposerWorkspaceFile,
  type ComposerWorkspaceFileRef,
} from './composer-context';
import {
  fileBrowserWorkspaceResourceKey,
  fileWorkspaceResourceKey,
  type FileBrowserWorkspaceResource,
} from '@/components/workspace/right-workspace-context';
import { fileContentStore } from '@/components/workspace/files/file-content-store';

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

interface OpenableWorkspaceFileReference {
  projectId: string;
  relativePath: string;
  canonicalPath?: string;
  name?: string;
}

export async function openWorkspaceFileReference(
  reference: OpenableWorkspaceFileReference,
  scopeKey: string,
  openResource: (resource: FileBrowserWorkspaceResource) => unknown,
) {
  const resolved = await resolveWorkspaceFileLink(
    reference.projectId,
    reference.canonicalPath ?? reference.relativePath,
  );
  const relativePath = resolved.locator.relativePath ?? reference.relativePath;
  const name = reference.name ?? relativePath.split('/').at(-1) ?? relativePath;
  const key = fileWorkspaceResourceKey(reference.projectId, resolved.locator.canonicalPath);
  if (resolved.externalAccessGrant) {
    fileContentStore.primeExternalGrant(
      key,
      reference.projectId,
      resolved.locator.canonicalPath,
      resolved.externalAccessGrant,
    );
  }

  await openResource({
    kind: 'file-browser',
    key: fileBrowserWorkspaceResourceKey(reference.projectId),
    scopeKey,
    title: name,
    description: relativePath,
    attention: false,
    projectId: reference.projectId,
    selectedFile: {
      kind: 'file',
      key,
      scopeKey,
      title: name,
      description: relativePath,
      attention: false,
      projectId: reference.projectId,
      locator: resolved.locator,
      target: null,
      targetRevision: Date.now(),
    },
  });
}
