import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { Search, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { BrowserFavicon } from './BrowserFavicon';
import { browserHistoryStore } from './browser-history-store';
import {
  browserAddressSuggestions,
  displayBrowserVisitUrl,
  type BrowserAddressSuggestion,
} from './browser-history';

export function BrowserAddressField({
  address,
  typed,
  onAddressChange,
  onTyped,
  onSubmit,
  onOverlayChange,
}: {
  address: string;
  typed: boolean;
  onAddressChange: (value: string) => void;
  onTyped: () => void;
  onSubmit: (value?: string) => void;
  onOverlayChange?: (rect: DOMRect | null) => void;
}) {
  const { t } = useTranslation();
  const listId = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const typedRef = useRef(typed);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const visits = useSyncExternalStore(
    (listener) => browserHistoryStore.subscribe(listener),
    () => browserHistoryStore.snapshot(),
    () => browserHistoryStore.snapshot(),
  );

  const refreshHistory = useCallback(() => {
    void browserHistoryStore.refresh();
  }, []);

  useEffect(() => {
    refreshHistory();
  }, [refreshHistory]);

  const suggestions = useMemo(
    () => browserAddressSuggestions(visits, address, typed),
    [address, typed, visits],
  );

  useEffect(() => {
    setActiveIndex(null);
  }, [suggestions]);

  useEffect(() => {
    if (typedRef.current && !typed) {
      setOpen(false);
    }
    typedRef.current = typed;
  }, [typed]);

  const showList = open && suggestions.length > 0;

  const closeList = useCallback(() => {
    setOpen(false);
    setActiveIndex(null);
    onOverlayChange?.(null);
  }, [onOverlayChange]);

  useLayoutEffect(() => {
    onOverlayChange?.(showList ? listRef.current?.getBoundingClientRect() ?? null : null);
  }, [onOverlayChange, showList, suggestions]);

  const choose = useCallback((suggestion: BrowserAddressSuggestion) => {
    closeList();
    inputRef.current?.blur();
    if (suggestion.kind === 'search') {
      onAddressChange(suggestion.query);
      onSubmit(suggestion.query);
      return;
    }
    onAddressChange(suggestion.visit.url);
    onSubmit(suggestion.visit.url);
  }, [closeList, onAddressChange, onSubmit]);

  const commitTyped = useCallback(() => {
    closeList();
    inputRef.current?.blur();
    onSubmit();
  }, [closeList, onSubmit]);

  const removeVisit = useCallback((url: string) => {
    void browserHistoryStore.remove(url);
  }, []);

  return (
    <div className="relative min-w-0 flex-1">
      <Input
        ref={inputRef}
        value={address}
        onChange={(event) => {
          onTyped();
          onAddressChange(event.target.value);
          setOpen(true);
        }}
        onFocus={() => {
          refreshHistory();
          setOpen(true);
        }}
        onBlur={() => closeList()}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === 'Escape') {
            closeList();
            return;
          }
          if (event.key === 'ArrowDown' && suggestions.length) {
            event.preventDefault();
            setOpen(true);
            setActiveIndex((index) => (index == null ? 0 : (index + 1) % suggestions.length));
            return;
          }
          if (event.key === 'ArrowUp' && suggestions.length) {
            event.preventDefault();
            setOpen(true);
            setActiveIndex((index) => (
              index == null ? suggestions.length - 1 : (index - 1 + suggestions.length) % suggestions.length
            ));
            return;
          }
          if (event.key === 'Enter') {
            event.preventDefault();
            if (open && activeIndex != null && suggestions[activeIndex]) {
              choose(suggestions[activeIndex]);
              return;
            }
            commitTyped();
          }
        }}
        placeholder={t('workspace.browser.addressPlaceholder')}
        variant="toolbar"
        className="h-7 text-xs"
        data-browser-address="true"
        aria-label={t('workspace.browser.addressPlaceholder')}
        aria-autocomplete="list"
        aria-expanded={showList}
        aria-controls={listId}
      />
      {showList ? (
        <div
          ref={listRef}
          id={listId}
          role="listbox"
          data-browser-history="true"
          className="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-lg border border-border/50 bg-popover p-1 shadow-md"
        >
          {suggestions.map((suggestion, index) => (
            <div
              key={suggestion.kind === 'search' ? `search:${suggestion.query}` : suggestion.visit.url}
              role="option"
              aria-selected={index === activeIndex}
              data-browser-history-item={suggestion.kind}
              className={cn(
                'group flex w-full cursor-pointer items-center gap-1 rounded-md px-1 py-0.5 text-xs',
                index === activeIndex ? 'bg-accent text-accent-foreground' : 'text-foreground',
              )}
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => choose(suggestion)}
            >
              {suggestion.kind === 'search' ? (
                <>
                  <Search className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1 truncate px-1 py-1">{suggestion.query}</span>
                  <span className="shrink-0 px-1 text-muted-foreground">{t('workspace.browser.searchWeb')}</span>
                </>
              ) : (
                <>
                  <BrowserFavicon src={suggestion.visit.faviconDataUrl} className="size-4" />
                  <span className="min-w-0 flex-1 truncate px-1 py-1">
                    <span className="font-medium">{suggestion.visit.title || displayBrowserVisitUrl(suggestion.visit.url)}</span>
                    <span className="text-muted-foreground">
                      {' — '}
                      {displayBrowserVisitUrl(suggestion.visit.url)}
                    </span>
                  </span>
                </>
              )}
              {suggestion.kind === 'visit' ? (
                <Button
                  type="button"
                  size="icon-xs"
                  variant="ghost"
                  data-browser-history-remove="true"
                  className={cn(
                    'size-5 shrink-0 rounded-md',
                    index === activeIndex ? 'opacity-70 hover:opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
                  )}
                  aria-label={t('workspace.browser.removeVisit')}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    removeVisit(suggestion.visit.url);
                  }}
                >
                  <X />
                </Button>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
