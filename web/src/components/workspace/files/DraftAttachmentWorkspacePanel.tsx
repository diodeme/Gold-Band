import { useEffect, useState } from 'react';
import { FileText, LoaderCircle, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { isImageMime } from '@/lib/attachments';
import { readAttachmentText } from '@/lib/attachment-service';
import type { DraftAttachmentWorkspaceResource } from '../right-workspace-context';
import { ReadonlyTextWorkspaceViewer } from './ReadonlyTextWorkspaceViewer';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';

type DraftAttachmentTextState =
  | { kind: 'idle' | 'loading' | 'unavailable' }
  | { kind: 'ready'; content: string };

export function DraftAttachmentWorkspacePanel({ resource }: { resource: DraftAttachmentWorkspaceResource }) {
  const { t } = useTranslation();
  const { attachment } = resource;
  const imageSrc = isImageMime(attachment.mime) ? attachment.previewUrl : null;
  const [textState, setTextState] = useState<DraftAttachmentTextState>({ kind: 'idle' });

  useEffect(() => {
    if (isImageMime(attachment.mime)) {
      setTextState({ kind: 'idle' });
      return;
    }
    let cancelled = false;
    setTextState({ kind: 'loading' });
    void readAttachmentText(attachment)
      .then((content) => { if (!cancelled) setTextState({ kind: 'ready', content }); })
      .catch(() => { if (!cancelled) setTextState({ kind: 'unavailable' }); });
    return () => { cancelled = true; };
  }, [attachment]);

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-draft-attachment-workspace="true">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
        <FileText className="size-3.5 text-foreground" />
        <span className="min-w-0 flex-1 truncate font-mono">{attachment.name}</span>
        <span className="text-muted-foreground">{attachment.mime}</span>
      </header>
      {imageSrc ? (
        <WorkspaceImageCanvas src={imageSrc} alt={attachment.name} imageActionAsset={attachment} />
      ) : textState.kind === 'loading' || textState.kind === 'idle' ? (
        <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" />
          {t('turnFiles.loading')}
        </div>
      ) : textState.kind === 'ready' ? (
        <div className="min-h-0 flex-1 overflow-hidden">
          <ReadonlyTextWorkspaceViewer
            documentKey={resource.key}
            name={attachment.name}
            value={textState.content}
          />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-muted-foreground">
          <TriangleAlert className="size-4" />
          {t('workspace.filesPanel.draftPreviewUnavailable')}
        </div>
      )}
    </section>
  );
}
