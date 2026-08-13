import { MessageSquareQuote } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import type { UserPromptQuote } from '@/types';

export function UserMessageQuotes({ quotes }: { quotes: readonly UserPromptQuote[] }) {
  const { t } = useTranslation();
  if (quotes.length === 0) return null;

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-7 rounded-full border-border/70 bg-background/80 px-2.5 text-xs font-normal text-muted-foreground shadow-none hover:bg-muted/50 hover:text-foreground"
          aria-label={t('acp.userQuoteCount', { count: quotes.length })}
          data-user-message-quotes-trigger="true"
        >
          <MessageSquareQuote className="size-3.5" />
          {t('acp.userQuoteCount', { count: quotes.length })}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        side="top"
        sideOffset={6}
        className="w-[min(24rem,calc(100vw-2rem))] overflow-hidden p-0"
        data-user-message-quotes-popover="true"
      >
        <div className="flex max-h-[min(24rem,calc(100vh-4rem))] min-h-0 flex-col">
          <div className="shrink-0 border-b border-border/60 px-3 py-2 text-xs font-medium">
            {t('acp.userQuoteTitle')}
          </div>
          <div
            className="min-h-0 divide-y divide-border/50 overflow-y-auto overscroll-contain"
            data-user-message-quotes-scroll="true"
          >
            {quotes.map((quote, index) => (
              <div key={quote.id} className="px-3 py-2.5">
                <div className="mb-1 text-[11px] text-muted-foreground">
                  {t('acp.userQuoteLabel', { index: index + 1 })}
                </div>
                <div className="whitespace-pre-wrap break-words text-sm leading-5 [overflow-wrap:anywhere]">
                  {quote.text}
                </div>
              </div>
            ))}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
