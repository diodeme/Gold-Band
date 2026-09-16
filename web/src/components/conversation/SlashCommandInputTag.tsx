import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { SlashCatalogItemKind } from '@/lib/slash-command';

interface SlashCommandInputTagProps {
  prefix: string;
  description?: string;
  content?: string;
  kind?: SlashCatalogItemKind;
  iconSrc?: string | null;
  iconClassName?: string;
}

export function SlashCommandInputTag({
  prefix,
  description,
  content,
  kind = 'command',
  iconSrc,
  iconClassName,
}: SlashCommandInputTagProps) {
  const tooltipText = kind === 'role' ? (content || description) : description;
  const tag = (
    <Badge asChild variant="secondary">
      <button
        type="button"
        data-slot="slash-command-input-tag"
        data-slash-tag-kind={kind}
        className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border/70 bg-secondary/85 px-2 py-1 text-xs font-medium leading-4 text-secondary-foreground shadow-xs"
      >
        {iconSrc ? (
          <img
            src={iconSrc}
            alt=""
            className={cn('size-3 shrink-0 object-contain', iconClassName)}
          />
        ) : null}
        {prefix}
      </button>
    </Badge>
  );

  if (!tooltipText) return tag;

  return (
    <TooltipProvider delayDuration={0}>
      <Tooltip>
        <TooltipTrigger asChild>{tag}</TooltipTrigger>
        <TooltipContent
          side="top"
          sideOffset={6}
          className={cn(
            'p-0 text-pretty leading-5',
            kind === 'role' ? 'max-w-80' : 'max-w-80 px-3 py-1.5',
          )}
        >
          {kind === 'role' ? (
            <div
              data-slash-role-tooltip="true"
              className="gold-themed-scrollbar max-h-64 overflow-y-auto overscroll-contain whitespace-pre-wrap break-words px-3 py-2 [overflow-wrap:anywhere]"
            >
              {tooltipText}
            </div>
          ) : tooltipText}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
