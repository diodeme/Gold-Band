import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import {
  COMPOSER_LEADING_ADORNMENT_CHIP_CLASS_NAME,
  USER_MESSAGE_META_CHIP_CLASS_NAME,
} from '@/lib/conversation-composer-layout';
import { cn } from '@/lib/utils';
import { slashTagDisplayName, type SlashCatalogItemKind } from '@/lib/slash-command';

interface SlashCommandInputTagProps {
  prefix: string;
  description?: string;
  content?: string;
  kind?: SlashCatalogItemKind;
  iconSrc?: string | null;
  iconClassName?: string;
  surface?: 'composer' | 'meta';
}

export function SlashCommandInputTag({
  prefix,
  description,
  content,
  kind = 'command',
  iconSrc,
  iconClassName,
  surface = 'composer',
}: SlashCommandInputTagProps) {
  const tooltipText = kind === 'role' ? (content || description) : description;
  const tag = (
    <Button
      type="button"
      variant="outline"
      size="xs"
      data-slot="slash-command-input-tag"
      data-slash-tag-kind={kind}
      className={surface === 'meta' ? USER_MESSAGE_META_CHIP_CLASS_NAME : COMPOSER_LEADING_ADORNMENT_CHIP_CLASS_NAME}
    >
      {iconSrc ? (
        <img
          src={iconSrc}
          alt=""
          className={cn('size-3.5 shrink-0 object-contain', iconClassName)}
        />
      ) : null}
      {slashTagDisplayName(prefix)}
    </Button>
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
            kind === 'role' ? 'pointer-events-auto max-w-80' : 'max-w-80 px-3 py-1.5',
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
