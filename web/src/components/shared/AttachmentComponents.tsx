import { X, Image as ImageIcon, FileText, Eye } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { isImageMime } from '@/lib/attachments';
import { cn } from '@/lib/utils';
import type { AttachmentItem } from '@/lib/attachment-service';
import { formatSize } from '@/lib/attachment-service';

export interface AttachmentChipProps {
  item: AttachmentItem;
  compact?: boolean;
  onRemove: () => void;
  onPreview: () => void;
}

export function AttachmentChip({ item, compact = false, onRemove, onPreview }: AttachmentChipProps) {
  const isImage = isImageMime(item.mime);
  const showPreviewUrl = isImage && item.previewUrl;

  return (
    <div
      className={cn(
        'group relative flex cursor-pointer items-center gap-1.5 rounded-lg border border-border/60 bg-background/70 shadow-sm transition-colors hover:border-border',
        compact ? 'px-1.5 py-1 text-xs' : 'px-2 py-1.5 text-xs',
      )}
      onClick={onPreview}
      title={item.name}
    >
      {showPreviewUrl ? (
        <img
          src={item.previewUrl}
          alt={item.name}
          className={cn('shrink-0 rounded object-cover', compact ? 'size-6' : 'size-8')}
        />
      ) : (
        <span
          className={cn(
            'flex shrink-0 items-center justify-center rounded bg-muted/50 text-muted-foreground',
            compact ? 'size-6' : 'size-8',
          )}
        >
          {isImage ? <ImageIcon className="size-3.5" /> : <FileText className={compact ? 'size-3' : 'size-4'} />}
        </span>
      )}
      <span className={cn('min-w-0 truncate font-medium', compact ? 'max-w-[100px]' : 'max-w-[140px]')}>
        {item.name}
      </span>
      {!compact && (
        <Badge variant="secondary" className="shrink-0 rounded-full px-1.5 py-0 text-[10px] font-normal">
          {formatSize(item.size)}
        </Badge>
      )}
      <Button
        variant="ghost"
        size="icon"
        className={cn(
          'shrink-0 rounded-full opacity-0 transition-opacity group-hover:opacity-100',
          compact ? 'size-4' : 'size-5',
        )}
        onClick={(e) => {
          e.stopPropagation();
          onRemove();
        }}
        title="Remove"
      >
        <X className={compact ? 'size-2.5' : 'size-3'} />
      </Button>
    </div>
  );
}

export interface AttachmentChipsListProps {
  attachments: AttachmentItem[];
  compact?: boolean;
  onRemove: (id: string) => void;
  onPreview: (item: AttachmentItem) => void;
  onClear: () => void;
  clearLabel?: string;
}

export function AttachmentChipsList({
  attachments,
  compact = false,
  onRemove,
  onPreview,
  onClear,
  clearLabel = 'Clear all',
}: AttachmentChipsListProps) {
  if (attachments.length === 0) return null;

  return (
    <div
      className={cn(
        'flex flex-wrap items-center rounded-xl border border-border/40 bg-card/40',
        compact ? 'gap-1.5 px-2.5 py-1.5' : 'gap-2 px-3 py-2',
      )}
    >
      {attachments.map((a) => (
        <AttachmentChip
          key={a.id}
          item={a}
          compact={compact}
          onRemove={() => onRemove(a.id)}
          onPreview={() => onPreview(a)}
        />
      ))}
      <Button
        variant="ghost"
        size="sm"
        className={cn('text-muted-foreground', compact ? 'h-6 text-[11px]' : 'h-7 text-xs')}
        onClick={onClear}
      >
        {clearLabel}
      </Button>
    </div>
  );
}

export interface AttachmentPreviewDialogsProps {
  previewImage: AttachmentItem | null;
  textPreview: { name: string; content: string } | null;
  onCloseImage: () => void;
  onCloseText: () => void;
}

export function AttachmentPreviewDialogs({
  previewImage,
  textPreview,
  onCloseImage,
  onCloseText,
}: AttachmentPreviewDialogsProps) {
  return (
    <>
      <Dialog open={!!previewImage} onOpenChange={(open) => { if (!open) onCloseImage(); }}>
        <DialogContent
          showCloseButton={false}
          overlayClassName="bg-black/70"
          className="!w-auto !max-w-[calc(100vw-4rem)] !gap-0 border-0 bg-transparent p-0 shadow-none sm:!max-w-[calc(100vw-4rem)]"
        >
          <DialogTitle className="sr-only">{previewImage?.name ?? 'Image Preview'}</DialogTitle>
          {previewImage?.previewUrl ? (
            <img
              src={previewImage.previewUrl}
              alt={previewImage.name}
              draggable={false}
              className="block max-h-[calc(100vh-4rem)] max-w-[calc(100vw-4rem)] object-contain"
            />
          ) : null}
        </DialogContent>
      </Dialog>

      <Dialog open={!!textPreview} onOpenChange={(open) => { if (!open) onCloseText(); }}>
        <DialogContent className="max-h-[86vh] max-w-4xl gap-0 overflow-hidden p-0">
          <DialogHeader className="border-b px-5 py-3">
            <DialogTitle className="truncate text-sm">{textPreview?.name}</DialogTitle>
          </DialogHeader>
          <pre className="max-h-[70vh] overflow-auto p-5 font-mono text-xs leading-relaxed text-foreground/85 whitespace-pre-wrap break-words">
            {textPreview?.content}
          </pre>
        </DialogContent>
      </Dialog>
    </>
  );
}
