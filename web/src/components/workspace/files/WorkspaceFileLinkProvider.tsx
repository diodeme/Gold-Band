import { useCallback, useMemo, useRef, type ReactNode } from 'react';
import { resolveWorkspaceFileLink } from '@/api';
import { MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';
import {
  fileWorkspaceResourceKey,
  useRightWorkspace,
  type FileWorkspaceResource,
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

export function WorkspaceFileLinkProvider({ children }: { children: ReactNode }) {
  const workspace = useRightWorkspace();
  const targetRevisionsRef = useRef(new Map<string, number>());
  const openLocalFile = useCallback(async (rawHref: string) => {
    if (!workspace.projectId || !workspace.scopeKey) return;
    try {
      const resolved = await resolveWorkspaceFileLink(workspace.projectId, rawHref);
      const key = fileWorkspaceResourceKey(workspace.projectId, resolved.locator.canonicalPath);
      fileContentStore.primeExternalGrant(
        key,
        workspace.projectId,
        resolved.locator.canonicalPath,
        resolved.externalAccessGrant,
      );
      const existing = workspace.tabs.find((tab): tab is FileWorkspaceResource => tab.kind === 'file' && tab.key === key);
      if (existing && resolved.externalAccessGrant) {
        await fileContentStore.reauthorize(key, resolved.externalAccessGrant);
      }
      const targetRevision = Math.max(
        existing?.targetRevision ?? 0,
        targetRevisionsRef.current.get(key) ?? 0,
      ) + 1;
      targetRevisionsRef.current.set(key, targetRevision);
      workspace.openResource({
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
    } catch {
      const key = fileWorkspaceResourceKey(workspace.projectId, rawHref);
      workspace.openResource({
        kind: 'file',
        key,
        scopeKey: workspace.scopeKey,
        projectId: workspace.projectId,
        title: fileName(rawHref),
        description: rawHref,
        attention: true,
        locator: {
          projectId: workspace.projectId,
          canonicalPath: rawHref,
          relativePath: null,
          scope: 'external',
        },
        target: null,
        targetRevision: 1,
      });
    }
  }, [workspace.openResource, workspace.projectId, workspace.scopeKey, workspace.tabs]);
  const handler = useMemo(
    () => workspace.projectId && workspace.scopeKey ? { openLocalFile } : null,
    [openLocalFile, workspace.projectId, workspace.scopeKey],
  );
  return <MarkdownResourceLinkProvider handler={handler}>{children}</MarkdownResourceLinkProvider>;
}
