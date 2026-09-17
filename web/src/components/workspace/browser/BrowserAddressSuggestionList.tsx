import { Search, X } from 'lucide-react';
import type { BrowserAddressSuggestionOverlayItemVm } from '@/api/client';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { BrowserFavicon } from './BrowserFavicon';

export function BrowserAddressSuggestionList({
  items,
  activeIndex,
  onActiveIndexChange,
  onChoose,
  onRemove,
  activateOnPointerDown = false,
}: {
  items: readonly BrowserAddressSuggestionOverlayItemVm[];
  activeIndex: number | null;
  onActiveIndexChange: (index: number) => void;
  onChoose: (key: string) => void;
  onRemove: (key: string) => void;
  activateOnPointerDown?: boolean;
}) {
  return items.map((item, index) => (
    <div
      key={item.key}
      role="option"
      aria-selected={index === activeIndex}
      data-browser-history-item={item.kind}
      className={cn(
        'group flex h-8 w-full cursor-pointer items-center gap-1 rounded-md px-1 text-xs',
        index === activeIndex ? 'bg-accent text-accent-foreground' : 'text-popover-foreground',
      )}
      onPointerDown={activateOnPointerDown ? (event) => {
        event.preventDefault();
        onChoose(item.key);
      } : (event) => event.preventDefault()}
      onClick={activateOnPointerDown ? undefined : () => onChoose(item.key)}
      onMouseEnter={() => onActiveIndexChange(index)}
    >
      {item.kind === 'search' ? (
        <>
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate px-1">{item.title}</span>
          <span className="shrink-0 px-1 text-muted-foreground">{item.detail}</span>
        </>
      ) : (
        <>
          <BrowserFavicon src={item.faviconDataUrl} className="size-4" />
          <span className="min-w-0 flex-1 truncate px-1">
            <span className="font-medium">{item.title}</span>
            <span className="text-muted-foreground">{' — '}{item.detail}</span>
          </span>
        </>
      )}
      {item.kind === 'visit' ? (
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          data-browser-history-remove="true"
          className={cn(
            'size-5 shrink-0 rounded-md',
            index === activeIndex ? 'opacity-70 hover:opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
          )}
          aria-label={item.removeLabel ?? undefined}
          onPointerDown={(event) => {
            event.preventDefault();
            event.stopPropagation();
            if (activateOnPointerDown) onRemove(item.key);
          }}
          onClick={activateOnPointerDown ? undefined : (event) => {
            event.preventDefault();
            event.stopPropagation();
            onRemove(item.key);
          }}
        >
          <X />
        </Button>
      ) : null}
    </div>
  ));
}
