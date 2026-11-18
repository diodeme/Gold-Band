import type { PromptWorkspaceFileRef, WorkspaceDirectoryEntryVm, WorkspaceFileLocatorVm, WorkspaceRootRef } from '@/types';
import { MAIN_WORKSPACE_TAB_ROOT, workspaceRootRef } from './workspace-root';
import { resolveRightWorkspaceFileTarget, resolveWorkspaceFileLink } from '@/api';
import { guessMimeFromExtension } from './attachment-service';
import {
  addComposerWorkspaceFile,
  type ComposerWorkspaceFileRef,
} from './composer-context';
import {
  fileBrowserWorkspaceResourceKey,
  fileWorkspaceResourceKey,
  conversationAssetWorkspaceResourceKey,
  type RightWorkspaceResource,
  type FileBrowserWorkspaceResource,
  type FileWorkspaceResource,
} from '@/components/workspace/right-workspace-context';
import { fileContentStore } from '@/components/workspace/files/file-content-store';
import { fileReferenceIdentity } from './file-reference-source';

export function composerWorkspaceFileRefFromEntry(
  projectId: string,
  entry: WorkspaceDirectoryEntryVm,
): ComposerWorkspaceFileRef {
  return {
    id: `${projectId}:${entry.relativePath.replaceAll('\\', '/')}`,
    source: { kind: 'workspace', projectId, relativePath: entry.relativePath },
    projectId,
    relativePath: entry.relativePath,
    name: entry.name,
    byteLength: entry.byteLength ?? null,
    mimeType: guessMimeFromExtension(entry.name),
    canonicalPath: entry.canonicalPath,
  };
}

/** Preserve an open file's fixed source when adding it to another conversation. */
export function composerWorkspaceFileRefFromLocator(
  locator: WorkspaceFileLocatorVm,
  source?: FileWorkspaceResource['source'],
): ComposerWorkspaceFileRef | null {
  if (!source && (locator.scope !== 'workspace' || !locator.relativePath)) return null;
  // Fixed sources carry the submission identity; external paths only supply a display name.
  const relativePath = (locator.relativePath ?? (source?.kind === 'git-file'
    ? source.relativePath
    : locator.canonicalPath.replaceAll('\\', '/').split('/').at(-1)))?.replaceAll('\\', '/');
  if (!relativePath) return null;
  const name = relativePath.split('/').at(-1) || relativePath;
  return {
    id: source ? fileReferenceIdentity(source) : `${locator.projectId}:${relativePath}`,
    source: source ?? { kind: 'workspace', projectId: locator.projectId, relativePath },
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
  source?: PromptWorkspaceFileRef;
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
  openResource: (resource: RightWorkspaceResource) => unknown,
  currentRoot: WorkspaceRootRef | null,
) {
  if (reference.source?.kind === 'artifact') {
    const { locator, name } = reference.source;
    await openResource({ kind: 'conversation-asset', assetKind: 'artifact', locator, name,
      key: conversationAssetWorkspaceResourceKey('artifact', locator, name), scopeKey,
      title: reference.name ?? name, attention: false });
    return;
  }
  const fixedSource = reference.source && reference.source.kind !== 'workspace' ? reference.source : undefined;
  let sourceRoot: WorkspaceRootRef | null = null;
  if (fixedSource) {
    const target = await resolveRightWorkspaceFileTarget(fixedSource);
    if (target.availability.kind !== 'available') throw target.availability;
    // Opening the explicitly selected file may grant access to that exact file;
    // the directory tab continues to project the receiving session's root.
    reference = { ...reference, projectId: target.projectId,
      relativePath: target.relativePath ?? target.name, canonicalPath: target.absolutePath ?? undefined };
    sourceRoot = workspaceRootRef(target.projectId, target.workspacePath);
  }
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
  if (!sourceRoot && !currentRoot) {
    await openResource({ ...fileBrowser, title: fallbackName, description: reference.relativePath, selectedFile: null });
    return;
  }
  const root = sourceRoot ?? (currentRoot?.projectId === reference.projectId ? currentRoot : workspaceRootRef(reference.projectId, null));
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
      source: fixedSource,
      locator: resolved.locator,
      target: null,
      targetRevision: Date.now(),
    },
  });
}
