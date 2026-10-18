import type { WorkspaceDirectoryEntryVm, WorkspaceFileLocatorVm, WorkspaceRootRef } from '@/types';
import { MAIN_WORKSPACE_TAB_ROOT, workspaceRootRef } from './workspace-root';
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

/** A reference to an open workspace file; files outside the workspace cannot be referenced. */
export function composerWorkspaceFileRefFromLocator(locator: WorkspaceFileLocatorVm): ComposerWorkspaceFileRef | null {
  if (locator.scope !== 'workspace' || !locator.relativePath) return null;
  const relativePath = locator.relativePath.replaceAll('\\', '/');
  const name = relativePath.split('/').at(-1) || relativePath;
  return {
    id: `${locator.projectId}:${relativePath}`,
    projectId: locator.projectId,
    relativePath,
    name,
    byteLength: null,
    mimeType: guessMimeFromExtension(name),
    canonicalPath: locator.canonicalPath,
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

/**
 * Open a referenced file in the files tab. `root` is the current file root
 * (`null` while the session's worktree is unavailable, in which case the tab
 * opens on its unavailable state instead of silently reading the project root).
 */
export async function openWorkspaceFileReference(
  reference: OpenableWorkspaceFileReference,
  scopeKey: string,
  openResource: (resource: FileBrowserWorkspaceResource) => unknown,
  currentRoot: WorkspaceRootRef | null,
) {
  const fileBrowser = {
    kind: 'file-browser' as const,
    key: fileBrowserWorkspaceResourceKey(reference.projectId),
    scopeKey,
    attention: false,
    projectId: reference.projectId,
    // The provider projects the tab root onto the current session.
    root: MAIN_WORKSPACE_TAB_ROOT,
    browseMain: false,
  };
  const fallbackName = reference.name ?? reference.relativePath.split('/').at(-1) ?? reference.relativePath;
  if (!currentRoot) {
    await openResource({ ...fileBrowser, title: fallbackName, description: reference.relativePath, selectedFile: null });
    return;
  }
  const root = currentRoot.projectId === reference.projectId ? currentRoot : workspaceRootRef(reference.projectId, null);
  const resolved = await resolveWorkspaceFileLink(
    root,
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
    ...fileBrowser,
    title: name,
    description: relativePath,
    selectedFile: {
      kind: 'file',
      key,
      scopeKey,
      title: name,
      description: relativePath,
      attention: false,
      projectId: reference.projectId,
      workspacePath: root.workspacePath,
      locator: resolved.locator,
      target: null,
      targetRevision: Date.now(),
    },
  });
}
