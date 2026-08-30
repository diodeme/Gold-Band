import { useEffect, useMemo, useState } from 'react';
import { LoaderCircle, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { releaseExternalFileAccess, resolveTurnAttachmentFile } from '@/api';
import type {
  FileWorkspaceResource,
  TurnAttachmentWorkspaceResource,
} from '../right-workspace-context';
import { FileContent } from './FileWorkspacePanel';
import { fileContentStore } from './file-content-store';

type ResolutionState =
  | { key: string; kind: 'loading' }
  | { key: string; kind: 'ready'; file: FileWorkspaceResource }
  | { key: string; kind: 'error'; code: string };

export function TurnAttachmentWorkspacePanel({
  resource,
}: {
  resource: TurnAttachmentWorkspaceResource;
}) {
  const { t } = useTranslation();
  const requestKey = [
    resource.locator.projectId,
    resource.locator.taskId,
    resource.locator.runId,
    resource.locator.roundId,
    resource.locator.nodeId,
    resource.locator.attemptId,
    resource.locator.branchId,
    resource.locator.outerNodeId,
    resource.locator.outerAttemptId,
    resource.changeSetId,
    resource.attachmentId,
  ].join('\0');
  const [state, setState] = useState<ResolutionState>({ key: requestKey, kind: 'loading' });

  useEffect(() => {
    let cancelled = false;
    setState({ key: requestKey, kind: 'loading' });
    void resolveTurnAttachmentFile(resource.locator, resource.changeSetId, resource.attachmentId)
      .then((resolved) => {
        if (cancelled) {
          if (resolved.externalAccessGrant) {
            void releaseExternalFileAccess(resolved.externalAccessGrant.token).catch(() => undefined);
          }
          return;
        }
        fileContentStore.primeExternalGrant(
          resource.key,
          resource.locator.projectId,
          resolved.locator.canonicalPath,
          resolved.externalAccessGrant,
        );
        setState({
          key: requestKey,
          kind: 'ready',
          file: {
            kind: 'file',
            key: resource.key,
            scopeKey: resource.scopeKey,
            title: resource.title,
            description: resource.description,
            attention: resource.attention,
            projectId: resource.locator.projectId,
            locator: resolved.locator,
            target: resolved.target,
            targetRevision: 0,
          },
        });
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ key: requestKey, kind: 'error', code: commandErrorCode(error) });
      });
    return () => { cancelled = true; };
  // requestKey contains the complete immutable manifest locator and avoids
  // repeating authorization when the workspace provider recreates a tab object.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestKey]);

  const current = state.key === requestKey ? state : { key: requestKey, kind: 'loading' as const };
  const errorText = useMemo(() => (
    current.kind === 'error'
      ? t(`errors.${current.code}`, { defaultValue: t('turnFiles.attachmentLoadFailed') })
      : null
  ), [current, t]);
  if (current.kind === 'ready') return <FileContent resource={current.file} />;
  if (current.kind === 'error') {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-destructive">
        <TriangleAlert className="size-4" />
        {errorText}
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-muted-foreground">
      <LoaderCircle className="size-4 animate-spin" />
      {t('turnFiles.loading')}
    </div>
  );
}

function commandErrorCode(error: unknown) {
  if (error && typeof error === 'object' && 'code' in error && typeof error.code === 'string') {
    return error.code;
  }
  return 'turn-files.attachment-not-found';
}
