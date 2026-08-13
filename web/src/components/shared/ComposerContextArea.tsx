import { FileText, Image as ImageIcon, MessageSquareQuote, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { isImageMime } from '@/lib/attachments';
import type { AttachmentItem } from '@/lib/attachment-service';
import type { ComposerQuote } from '@/lib/composer-context';
import { formatSize } from '@/lib/attachment-service';
import { cn } from '@/lib/utils';

export interface ComposerContextAreaProps {
  quotes?: readonly ComposerQuote[];
  attachments: readonly AttachmentItem[];
  error?: string | null;
  onRemoveQuote?: (id: string) => void;
  onRemoveAttachment: (id: string) => void;
  onPreviewAttachment: (item: AttachmentItem) => void;
}

export function ComposerContextArea({
  quotes = [],
  attachments,
  error,
  onRemoveQuote,
  onRemoveAttachment,
  onPreviewAttachment,
}: ComposerContextAreaProps) {
  const { t } = useTranslation();
  if (quotes.length === 0 && attachments.length === 0 && !error) return null;

  return (
    <div className="mb-1.5 px-1" data-composer-context-area="true">
      <div className="flex max-h-[4.5rem] min-w-0 flex-wrap items-center gap-1.5 overflow-y-auto py-0.5">
        {quotes.map((quote, index) => (
          <Tooltip key={quote.id}>
            <TooltipTrigger asChild>
              <span
                className="group flex h-8 min-w-0 max-w-44 items-center gap-1.5 rounded-lg bg-muted/55 px-2 text-xs text-foreground/85 transition-colors hover:bg-muted/75 focus-within:bg-muted/75"
                data-composer-quote-chip="true"
              >
                <MessageSquareQuote className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate">{t('acp.quoteLabel', { index: index + 1 })}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="-mr-1 size-5 shrink-0 rounded-full opacity-60 transition-opacity hover:bg-background/70 hover:opacity-100 focus-visible:opacity-100"
                  aria-label={t('acp.removeQuote', { index: index + 1 })}
                  onClick={() => onRemoveQuote?.(quote.id)}
                  data-prompt-input-interactive="true"
                >
                  <X className="size-3" />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent side="top" sideOffset={8} className="max-h-72 max-w-[min(36rem,calc(100vw-2rem))] overflow-auto whitespace-pre-wrap px-3 py-2 text-left leading-5">
              {quote.text}
            </TooltipContent>
          </Tooltip>
        ))}
        {attachments.map((attachment) => (
          <ComposerAttachmentItem
            key={attachment.id}
            item={attachment}
            onPreview={() => onPreviewAttachment(attachment)}
            onRemove={() => onRemoveAttachment(attachment.id)}
          />
        ))}
      </div>
      {error ? (
        <div className="px-1 pt-1 text-[11px] leading-4 text-destructive" role="alert" aria-live="polite">
          {error}
        </div>
      ) : null}
    </div>
  );
}

function ComposerAttachmentItem({ item, onPreview, onRemove }: {
  item: AttachmentItem;
  onPreview: () => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation();
  const image = isImageMime(item.mime);
  const details = `${item.name} · ${formatSize(item.size)}`;
  return (
    <Tooltip>
      <div
        className={cn(
          'group relative flex h-8 items-center rounded-lg bg-muted/55 text-xs text-foreground/85 transition-colors hover:bg-muted/75 focus-within:bg-muted/75',
          image ? 'w-8 justify-center p-0.5' : 'max-w-52 gap-1',
        )}
        data-composer-attachment-chip="true"
        data-prompt-input-interactive="true"
      >
        <TooltipTrigger asChild>
          <button
            type="button"
            className={cn(
              'flex h-full min-w-0 items-center rounded-lg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
              image ? 'w-full justify-center' : 'gap-1.5 py-1 pl-2',
            )}
            onClick={onPreview}
            aria-label={details}
          >
          {image && item.previewUrl ? (
            <img src={item.previewUrl} alt="" className="size-7 rounded-md object-cover" />
          ) : image ? (
            <ImageIcon className="size-4 text-muted-foreground" />
          ) : (
            <>
              <FileText className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="truncate">{item.name}</span>
            </>
          )}
          </button>
        </TooltipTrigger>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className={cn(
            'absolute -right-1 -top-1 size-4 rounded-full border border-border/60 bg-background/95 opacity-0 shadow-sm transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 [@media(pointer:coarse)]:opacity-100',
            !image && 'static -mr-0.5 size-5 shrink-0 border-0 bg-transparent opacity-60 shadow-none',
          )}
          aria-label={t('acp.removeAttachment', { name: item.name })}
          onClick={onRemove}
          data-prompt-input-interactive="true"
        >
          <X className="size-2.5" />
        </Button>
      </div>
      <TooltipContent side="top" sideOffset={8}>{details}</TooltipContent>
    </Tooltip>
  );
}
