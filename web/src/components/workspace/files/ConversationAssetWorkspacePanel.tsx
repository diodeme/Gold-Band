import { useEffect, useState } from 'react';
import { FileText, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { showArtifact, showConversationAttachment, showConversationMessageAttachment } from '@/api';
import { imageSrcFromContent } from '@/lib/asset-preview';
import type { ContentVm } from '@/types';
import type { ConversationAssetWorkspaceResource } from '../right-workspace-context';
import { ReadonlyTextWorkspaceViewer } from './ReadonlyTextWorkspaceViewer';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';

export function ConversationAssetWorkspacePanel({ resource }: { resource: ConversationAssetWorkspaceResource }) {
  const { t } = useTranslation();
  const [content, setContent] = useState<ContentVm | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setContent(null);
    setFailed(false);
    const { locator } = resource;
    const request = resource.assetKind === 'artifact'
      ? showArtifact(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, resource.name, locator.outerNodeId, locator.outerAttemptId)
      : resource.assetKind === 'input-attachment'
        ? showConversationAttachment(locator.projectId, locator.taskId, resource.name)
        : showConversationMessageAttachment(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, resource.name, resource.path ?? '', locator.outerNodeId, locator.outerAttemptId);
    void request
      .then((next) => { if (!cancelled) setContent(next); })
      .catch(() => { if (!cancelled) setFailed(true); });
    return () => { cancelled = true; };
  }, [resource]);

  const imageSrc = imageSrcFromContent(content);

  if (failed) {
    return <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-destructive"><TriangleAlert className="size-4" />{t('turnFiles.assetLoadFailed')}</div>;
  }
  if (!content) {
    return <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">{t('turnFiles.loading')}</div>;
  }
  return (
    <section className="flex min-h-0 flex-1 flex-col" data-conversation-asset-workspace={resource.assetKind}>
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
        <FileText className="size-3.5 text-foreground" />
        <span className="min-w-0 flex-1 truncate font-mono">{content.title || resource.name}</span>
        <span className="text-muted-foreground">{content.kind}</span>
      </header>
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {imageSrc ? (
          <WorkspaceImageCanvas src={imageSrc} alt={resource.name} />
        ) : (
          <ReadonlyTextWorkspaceViewer
            documentKey={resource.key}
            name={resource.name}
            value={content.content}
          />
        )}
      </div>
    </section>
  );
}
