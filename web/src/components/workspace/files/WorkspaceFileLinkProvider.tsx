import { useCallback, useMemo, useRef, type ReactNode } from 'react';
import { resolveWorkspaceFileLink } from '@/api';
import {
  MarkdownResourceLinkProvider,
  type MarkdownResourceLinkError,
  type MarkdownResourceLinkOpenResult,
} from '@/components/prompt-kit/markdown';
import {
  fileBrowserWorkspaceResourceKey,
  fileWorkspaceResourceKey,
  useRightWorkspaceCommands,
} from '../right-workspace-context';
import { fileContentStore } from './file-content-store';

function fileName(path: string) {
  const raw = path.replaceAll('\\', '/').split('/').at(-1) || path;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

function fileLinkError(reason: unknown): MarkdownResourceLinkError {
  if (typeof reason !== 'object' || !reason) {
    return { code: 'workspace-file.path-invalid', params: {} };
  }
  const value = reason as { code?: unknown; params?: unknown };
  return {
    code: typeof value.code === 'string' ? value.code : 'workspace-file.path-invalid',
    params: typeof value.params === 'object' && value.params
      ? value.params as Record<string, unknown>
      : {},
  };
}

export function WorkspaceFileLinkProvider({ children }: { children: ReactNode }) {
  const workspace = useRightWorkspaceCommands();
  const targetRevisionsRef = useRef(new Map<string, number>());
  const openLocalFile = useCallback(async (
    rawHref: string,
    baseCanonicalPath?: string | null,
  ): Promise<MarkdownResourceLinkOpenResult> => {
    if (!workspace.projectId || !workspace.scopeKey) {
      return {
        status: 'error',
        error: { code: 'workspace-file.project-not-found', params: {} },
      };
    }
    try {
      const resolved = await resolveWorkspaceFileLink(workspace.projectId, rawHref, baseCanonicalPath);
      const key = fileWorkspaceResourceKey(workspace.projectId, resolved.locator.canonicalPath);
      const fileBrowser = workspace.getResource(fileBrowserWorkspaceResourceKey(workspace.projectId));
      const existing = fileBrowser?.kind === 'file-browser'
        && fileBrowser.selectedFile?.key === key
        ? fileBrowser.selectedFile
        : null;
      const grantAdopted = Boolean(
        resolved.externalAccessGrant
        && await fileContentStore.reauthorize(key, resolved.externalAccessGrant),
      );
      if (!grantAdopted) {
        fileContentStore.primeExternalGrant(
          key,
          workspace.projectId,
          resolved.locator.canonicalPath,
          resolved.externalAccessGrant,
        );
      }
      const targetRevision = Math.max(
        existing?.targetRevision ?? 0,
        targetRevisionsRef.current.get(key) ?? 0,
      ) + 1;
      targetRevisionsRef.current.set(key, targetRevision);
      await workspace.openResource({
        kind: 'file',
        key,
        scopeKey: workspace.scopeKey,
        projectId: workspace.projectId,
        title: fileName(resolved.locator.canonicalPath),
        description: resolved.locator.relativePath ?? resolved.locator.canonicalPath,
        attention: false,
        locator: resolved.locator,
        target: resolved.target,
        targetRevision,
      });
      return { status: 'opened' };
    } catch (reason) {
      return { status: 'error', error: fileLinkError(reason) };
    }
  }, [workspace.getResource, workspace.openResource, workspace.projectId, workspace.scopeKey]);
  const handler = useMemo(
    () => workspace.projectId && workspace.scopeKey ? { openLocalFile } : null,
    [openLocalFile, workspace.projectId, workspace.scopeKey],
  );
  return <MarkdownResourceLinkProvider handler={handler}>{children}</MarkdownResourceLinkProvider>;
}
