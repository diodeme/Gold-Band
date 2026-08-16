import { FileText, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { isImageMime } from '@/lib/attachments';
import type { DraftAttachmentWorkspaceResource } from '../right-workspace-context';
import { WorkspaceImageCanvas } from './WorkspaceImageCanvas';

export function DraftAttachmentWorkspacePanel({ resource }: { resource: DraftAttachmentWorkspaceResource }) {
  const { t } = useTranslation();
  const { attachment } = resource;
  const imageSrc = isImageMime(attachment.mime) ? attachment.previewUrl : null;

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-draft-attachment-workspace="true">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
        <FileText className="size-3.5 text-foreground" />
        <span className="min-w-0 flex-1 truncate font-mono">{attachment.name}</span>
        <span className="text-muted-foreground">{attachment.mime}</span>
      </header>
      {imageSrc ? (
        <WorkspaceImageCanvas src={imageSrc} alt={attachment.name} />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-6 text-sm text-muted-foreground">
          <TriangleAlert className="size-4" />
          {t('workspace.filesPanel.draftPreviewUnavailable')}
        </div>
      )}
    </section>
  );
}
