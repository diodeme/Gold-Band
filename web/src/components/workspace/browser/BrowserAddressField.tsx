import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { reportFrontendError } from '@/api';
import { getRuntimeApi } from '@/api/client';
import { isTauriRuntime } from '@/api/shared';
import { Input } from '@/components/ui/input';
import { BrowserAddressSuggestionList } from './BrowserAddressSuggestionList';
import { browserHistoryStore } from './browser-history-store';
import {
  browserAddressSuggestions,
  type BrowserAddressSuggestion,
} from './browser-history';
import {
  browserAddressSuggestionKey,
  browserAddressSuggestionOverlayItems,
  createBrowserAddressSuggestionOverlayInput,
  nextBrowserAddressSuggestionRevision,
} from './browser-address-overlay';

export function BrowserAddressField({
  address,
  typed,
  onEditingChange,
  onAddressChange,
  onTyped,
  onSubmit,
}: {
  address: string;
  typed: boolean;
  onEditingChange?: (editing: boolean) => void;
  onAddressChange: (value: string) => void;
  onTyped: () => void;
  onSubmit: (value?: string) => void;
}) {
  const { t } = useTranslation();
  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const typedRef = useRef(typed);
  const nativeOverlay = isTauriRuntime();
  // Revision of the projection the overlay is currently displaying. Overlay clicks blur
  // the address input first; the displayed revision still identifies that click. choose
  // navigates, remove stays in the suggestion session, and hide is not part of remove.
  const lastVisibleOverlayRevisionRef = useRef(0);
  const lastOverlayProjectionRef = useRef<string | null>(null);
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
  }, []);

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
    // Deleting a visit stays inside the suggestion session. Restore editing so page
    // URL events cannot overwrite the draft, then return focus after the write so a
    // concurrent history refresh cannot resurrect the deleted row.
    setOpen(true);
    onEditingChange?.(true);
    void browserHistoryStore.remove(url).finally(() => {
      inputRef.current?.focus();
    });
  }, [onEditingChange]);

  const overlayItems = useMemo(() => browserAddressSuggestionOverlayItems(
    suggestions,
    t('workspace.browser.searchWeb'),
    t('workspace.browser.removeVisit'),
  ), [suggestions, t]);

  const reportOverlayFailure = useCallback((operation: string, error: unknown) => {
    const structured = typeof error === 'object' && error !== null
      ? error as { code?: unknown; params?: unknown }
      : null;
    const detail = typeof structured?.code === 'string'
      ? `${structured.code} ${JSON.stringify(structured.params ?? {})}`
      : String(error);
    try {
      void Promise.resolve(reportFrontendError({
        kind: 'unhandled-rejection',
        message: `browser address suggestions ${operation} failed: ${detail}`,
        pathname: window.location.pathname || null,
      })).catch(() => undefined);
    } catch {
      // Diagnostics must never break the address bar interaction.
    }
  }, []);

  const syncNativeOverlay = useCallback(() => {
    if (!nativeOverlay) return;
    const anchor = inputRef.current?.getBoundingClientRect();
    const next = showList && anchor
      ? createBrowserAddressSuggestionOverlayInput({
          revision: nextBrowserAddressSuggestionRevision(),
          anchor,
          suggestions,
          activeIndex,
          searchLabel: t('workspace.browser.searchWeb'),
          removeLabel: t('workspace.browser.removeVisit'),
        })
      : null;
    // The overlay owns its own native surface, so re-issuing the same projection only
    // races concurrent creates and hides. One request per projection change is enough.
    const signature = next
      ? JSON.stringify([next.bounds, next.activeIndex, next.items, next.theme])
      : 'hidden';
    if (lastOverlayProjectionRef.current === signature) return;
    lastOverlayProjectionRef.current = signature;
    if (!next) {
      void getRuntimeApi()
        .browserHideAddressSuggestions({ revision: nextBrowserAddressSuggestionRevision() })
        .catch((error) => {
          lastOverlayProjectionRef.current = null;
          reportOverlayFailure('hide', error);
        });
      return;
    }
    lastVisibleOverlayRevisionRef.current = next.revision;
    void getRuntimeApi().browserShowAddressSuggestions(next).catch((error) => {
      lastOverlayProjectionRef.current = null;
      reportOverlayFailure('show', error);
    });
  }, [activeIndex, nativeOverlay, reportOverlayFailure, showList, suggestions, t]);

  useLayoutEffect(() => {
    syncNativeOverlay();
  }, [syncNativeOverlay]);

  useEffect(() => {
    if (!nativeOverlay || !showList || !inputRef.current) return;
    const input = inputRef.current;
    const observer = new ResizeObserver(syncNativeOverlay);
    observer.observe(input);
    window.addEventListener('resize', syncNativeOverlay);
    window.addEventListener('gold-band-theme-icons-changed', syncNativeOverlay);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', syncNativeOverlay);
      window.removeEventListener('gold-band-theme-icons-changed', syncNativeOverlay);
    };
  }, [nativeOverlay, showList, syncNativeOverlay]);

  useEffect(() => {
    if (!nativeOverlay) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void getRuntimeApi().subscribeBrowserAddressSuggestionActions?.((event) => {
      if (disposed || event.revision !== lastVisibleOverlayRevisionRef.current) return;
      if (event.kind === 'dismiss') {
        onEditingChange?.(false);
        closeList();
        return;
      }
      const suggestion = suggestions.find((candidate) => browserAddressSuggestionKey(candidate) === event.key);
      if (!suggestion) return;
      if (event.kind === 'remove' && suggestion.kind === 'visit') {
        removeVisit(suggestion.visit.url);
        return;
      }
      if (event.kind === 'choose') choose(suggestion);
    }).then((stop) => {
      if (disposed) stop?.();
      else unlisten = stop ?? null;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [choose, closeList, nativeOverlay, onEditingChange, removeVisit, suggestions]);

  useEffect(() => {
    if (!nativeOverlay || !open) return;
    const onPointerDown = (event: Event) => {
      const input = inputRef.current;
      if (input && event.target instanceof Node && input.contains(event.target)) return;
      onEditingChange?.(false);
      closeList();
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, [closeList, nativeOverlay, onEditingChange, open]);

  useEffect(() => () => {
    if (!nativeOverlay) return;
    void getRuntimeApi().browserHideAddressSuggestions({
      revision: nextBrowserAddressSuggestionRevision(),
    });
  }, [nativeOverlay]);

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
          onEditingChange?.(true);
          refreshHistory();
          setOpen(true);
        }}
        onBlur={() => {
          // Native overlay clicks move focus to another webview. Closing here hides the
          // list and the later remove/choose then shows it again, which is the flicker.
          if (nativeOverlay && open) return;
          onEditingChange?.(false);
          closeList();
        }}
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
      {showList && !nativeOverlay ? (
        <div
          id={listId}
          role="listbox"
          data-browser-history="true"
          className="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-lg border border-border/50 bg-popover p-1 shadow-md"
        >
          <BrowserAddressSuggestionList
            items={overlayItems}
            activeIndex={activeIndex}
            onActiveIndexChange={setActiveIndex}
            onChoose={(key) => {
              const suggestion = suggestions.find((candidate) => browserAddressSuggestionKey(candidate) === key);
              if (suggestion) choose(suggestion);
            }}
            onRemove={(key) => {
              const suggestion = suggestions.find((candidate) => browserAddressSuggestionKey(candidate) === key);
              if (suggestion?.kind === 'visit') removeVisit(suggestion.visit.url);
            }}
          />
        </div>
      ) : null}
    </div>
  );
}
