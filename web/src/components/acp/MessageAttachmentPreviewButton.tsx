import { memo, useEffect, useState } from 'react';
import { Check, CircleAlert, FileText, Image as ImageIcon, Loader2 } from 'lucide-react';
import { showConversationAttachment, showConversationMessageAttachment } from '@/api';
import { ImageActionsContextMenu } from '@/components/shared/ImageActionsContextMenu';
import { useImageActions } from '@/hooks/useImageActions';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { imageSrcFromContent, isImageMessageAttachment, isTaskInputMessageAttachment, type MessageAttachmentPreview } from '@/lib/asset-preview';

export type MessageAttachmentLocator = {
  projectId: string; taskId: string; runId: string; roundId: string; nodeId: string; attemptId: string;
  outerNodeId?: string | null; outerAttemptId?: string | null;
};

export const MessageAttachmentPreviewButton = memo(function MessageAttachmentPreviewButton({
  attachment,
  locator,
  onClick,
  imageSource,
  imageAsset,
}: {
  attachment: MessageAttachmentPreview;
  locator?: MessageAttachmentLocator;
  onClick?: (attachment: MessageAttachmentPreview) => void;
  imageSource?: string | null;
  imageAsset?: import('@/lib/image-actions').ImageActionAsset;
}) {
  const isImage = isImageMessageAttachment(attachment);
  const attachmentLabel = imageSource !== undefined ? attachment.name : `${attachment.name} (${formatAttachmentSize(attachment.size)})`;
  const [loadedPreviewSrc, setPreviewSrc] = useState<string | null>(null);
  const previewSrc = imageSource !== undefined ? imageSource : loadedPreviewSrc;

  useEffect(() => {
    if (imageSource !== undefined || !isImage || !locator) {
      setPreviewSrc(null);
      return;
    }
    let cancelled = false;
    setPreviewSrc(null);
    const contentPromise = isTaskInputMessageAttachment(attachment)
      ? showConversationAttachment(locator.projectId, locator.taskId, attachment.name)
      : showConversationMessageAttachment(
          locator.projectId,
          locator.taskId,
          locator.runId,
          locator.roundId,
          locator.nodeId,
          locator.attemptId,
          attachment.name,
          attachment.path,
          locator.outerNodeId,
          locator.outerAttemptId,
        );
    contentPromise
      .then((content) => {
        if (!cancelled) setPreviewSrc(imageSrcFromContent(content));
      })
      .catch(() => {
        if (!cancelled) setPreviewSrc(null);
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.name, attachment.path, isImage, locator, imageSource]);

  const imageActions = useImageActions(imageAsset ?? (isImage && previewSrc ? {
    name: attachment.name,
    mime: attachment.type,
    previewUrl: previewSrc,
  } : null));

  if (isImage) {
    const previewButton = (
      <button
        type="button"
        className={cn(
          "relative size-[72px] overflow-hidden rounded-lg border border-border/60 bg-card/80 text-muted-foreground shadow-sm transition-colors hover:border-primary/45 hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          imageActions.state === 'failed' && "ring-1 ring-destructive/70",
        )}
        aria-label={attachment.name}
        aria-busy={imageActions.pending || undefined}
        onClick={() => onClick?.(attachment)}
      >
        {previewSrc ? (
          <img
            src={previewSrc}
            alt={attachment.name}
            loading="lazy"
            draggable={false}
            className="size-full object-cover"
          />
        ) : (
          <span className="flex size-full items-center justify-center bg-muted/40">
            <ImageIcon className="size-5 text-blue-400" />
          </span>
        )}
        {imageActions.pending ? (
          <span className="absolute inset-0 flex items-center justify-center bg-background/65">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          </span>
        ) : imageActions.state === 'copied' || imageActions.state === 'saved' ? (
          <span className="absolute right-1 top-1 flex size-5 items-center justify-center rounded-full bg-background/85 text-emerald-600 shadow-sm">
            <Check className="size-3" aria-hidden="true" />
          </span>
        ) : imageActions.state === 'failed' ? (
          <span className="absolute right-1 top-1 flex size-5 items-center justify-center rounded-full bg-background/85 text-destructive shadow-sm">
            <CircleAlert className="size-3" aria-hidden="true" />
          </span>
        ) : null}
      </button>
    );
    return (
      <Tooltip>
        {previewSrc ? (
          <ImageActionsContextMenu actions={imageActions}>
            <TooltipTrigger asChild>{previewButton}</TooltipTrigger>
          </ImageActionsContextMenu>
        ) : (
          <TooltipTrigger asChild>{previewButton}</TooltipTrigger>
        )}
        <TooltipContent className="max-w-[360px] break-all">
          {imageActions.message ?? attachmentLabel}
        </TooltipContent>
        {imageActions.message ? (
          <span className="sr-only" aria-live="polite">{imageActions.message}</span>
        ) : null}
      </Tooltip>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex h-9 w-fit max-w-full shrink-0 items-center gap-1.5 rounded-full border border-border/60 bg-card/80 px-3 text-ui-caption text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => onClick?.(attachment)}
        >
          <FileText className="size-3 text-muted-foreground" />
          <span className="max-w-[120px] truncate">{attachment.name}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-[360px] break-all">{attachmentLabel}</TooltipContent>
    </Tooltip>
  );
});

function formatAttachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
