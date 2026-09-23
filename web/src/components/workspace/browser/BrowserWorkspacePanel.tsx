import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ComponentProps } from 'react';
import { ArrowLeft, ArrowRight, Bookmark, ChevronDown, ExternalLink, Monitor, Plus, RefreshCw, Smartphone, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openExternalUrl } from '@/api';
import { Button } from '@/components/ui/button';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { BrowserSearchEngine } from './web-target';
import { browserSessionStore, BLANK_BROWSER_URL, isBrowserPortalUrl } from './browser-session-store';
import { browserWebviewHost } from './browser-webview-host';
import { BrowserAddressField } from './BrowserAddressField';
import { BrowserFavicon } from './BrowserFavicon';
import { browserBookmarkStore } from './browser-bookmark-store';
import { bookmarkByOrigin, isUrlBookmarked } from './browser-bookmarks';
import { BrowserPortal } from './BrowserPortal';
import { browserHistoryStore } from './browser-history-store';
import { faviconForUrl } from './browser-history';
import { NativeBrowserViewport } from './NativeBrowserViewport';
import { normalizeBrowserAddress, systemBrowserHref } from './web-target';

export function BrowserWorkspacePanel({ searchEngine = 'baidu' }: { searchEngine?: BrowserSearchEngine }) {
  const { t } = useTranslation();
  const session = useSyncExternalStore(
    (listener) => browserSessionStore.subscribe(listener),
    () => browserSessionStore.snapshot(),
    () => browserSessionStore.snapshot(),
  );
  const active = session.pages.find((page) => page.pageId === session.activePageId) ?? null;
  const [address, setAddress] = useState(active?.url === BLANK_BROWSER_URL ? '' : (active?.url ?? ''));
  const [typed, setTyped] = useState(false);
  // A typed, unsubmitted address belongs to the user. Focusing the field alone must not
  // freeze it: same-document navigations still have to replace the displayed URL.
  const editingAddressRef = useRef(false);
  const typedRef = useRef(false);
  const lastSyncedPageRef = useRef<string | null>(active?.pageId ?? null);
  const tabStripRef = useRef<HTMLDivElement>(null);
  const overflowMenuRef = useRef<HTMLButtonElement>(null);
  const [tabsOverflowing, setTabsOverflowing] = useState(false);
  const history = useSyncExternalStore(
    (listener) => browserHistoryStore.subscribe(listener),
    () => browserHistoryStore.snapshot(),
    () => browserHistoryStore.snapshot(),
  );
  const bookmarks = useSyncExternalStore(
    (listener) => browserBookmarkStore.subscribe(listener),
    () => browserBookmarkStore.snapshot(),
    () => browserBookmarkStore.snapshot(),
  );
  const externalHref = systemBrowserHref(address, active?.url ?? BLANK_BROWSER_URL, searchEngine);
  const showPortal = !active || isBrowserPortalUrl(active.url);
  const browsing = Boolean(active && !isBrowserPortalUrl(active.url));
  const bookmarked = isUrlBookmarked(bookmarks, active?.url ?? '');

  useEffect(() => {
    void browserHistoryStore.refresh();
    void browserBookmarkStore.refresh();
  }, []);

  useEffect(() => {
    const pageChanged = lastSyncedPageRef.current !== (active?.pageId ?? null);
    lastSyncedPageRef.current = active?.pageId ?? null;
    if (!pageChanged && typedRef.current) return;
    typedRef.current = false;
    setTyped(false);
    setAddress(!active || active.url === BLANK_BROWSER_URL ? '' : active.url);
  }, [active?.pageId, active?.url]);

  useLayoutEffect(() => {
    const tabStrip = tabStripRef.current;
    if (!tabStrip) return;
    const measure = () => {
      const overflowing = tabStrip.scrollWidth > tabStrip.clientWidth + (overflowMenuRef.current?.offsetWidth ?? 0) + 1;
      setTabsOverflowing((current) => current === overflowing ? current : overflowing);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(tabStrip);
    return () => observer.disconnect();
  }, [session.pages]);

  // The session store already records the structured failure for this page, so the UI
  // only needs to keep the rejection from surfacing as an unhandled promise.
  const commitNavigation = useCallback((pageId: string, url: string) => {
    void Promise.resolve(browserWebviewHost.commitNavigation(pageId, url)).catch(() => undefined);
  }, []);

  const submitAddress = useCallback((override?: string) => {
    typedRef.current = false;
    editingAddressRef.current = false;
    setTyped(false);
    const url = normalizeBrowserAddress(override ?? address, searchEngine);
    if (!active) {
      browserSessionStore.openUrl(url);
      return;
    }
    commitNavigation(active.pageId, url);
  }, [active, address, searchEngine, commitNavigation]);

  const closePages = useCallback((pageIds: string[]) => {
    if (pageIds.length === 0) return;
    void browserWebviewHost.discard(pageIds);
  }, []);

  const portal = (
    <BrowserPortal
      bookmarks={bookmarks}
      visits={history}
      onOpenBlank={() => browserSessionStore.addBlankPage()}
      onOpenBookmark={(url) => {
        if (active && isBrowserPortalUrl(active.url)) {
          commitNavigation(active.pageId, url);
          return;
        }
        browserSessionStore.openUrl(url);
      }}
      onDeleteBookmark={(bookmarkId) => {
        void browserBookmarkStore.remove(bookmarkId);
      }}
      onReorder={(orderedIds) => {
        void browserBookmarkStore.reorder(orderedIds);
      }}
    />
  );

  return (
    <section className="flex min-h-0 flex-1 flex-col overflow-visible bg-background" data-browser-workspace="true" aria-label={t('workspace.browser.title')}>
      <div className="relative z-20 flex h-10 shrink-0 items-center gap-1 overflow-visible border-b border-border/50 px-2">
        <div className="flex h-7 items-center gap-1">
          <BrowserToolbarButton label={t('workspace.browser.back')} disabled={!browsing} onClick={() => active && void browserWebviewHost.goBack(active.pageId)}><ArrowLeft /></BrowserToolbarButton>
          <BrowserToolbarButton label={t('workspace.browser.forward')} disabled={!browsing} onClick={() => active && void browserWebviewHost.goForward(active.pageId)}><ArrowRight /></BrowserToolbarButton>
          <BrowserToolbarButton
            label={t(active?.loading ? 'workspace.browser.stop' : 'workspace.browser.reload')}
            disabled={!browsing}
            onClick={() => active && void (active.loading ? browserWebviewHost.stop(active.pageId) : browserWebviewHost.reload(active.pageId))}
          >
            {active?.loading ? <X /> : <RefreshCw />}
          </BrowserToolbarButton>
        </div>
        <form
          className="min-w-0 flex-1"
          onSubmit={(event) => {
            event.preventDefault();
            submitAddress();
          }}
        >
          <BrowserAddressField
            address={address}
            typed={typed}
            onEditingChange={(editing) => {
              editingAddressRef.current = editing;
            }}
            onAddressChange={setAddress}
            onTyped={() => {
              editingAddressRef.current = true;
              typedRef.current = true;
              setTyped(true);
            }}
            onSubmit={submitAddress}
          />
        </form>
        <div className="flex h-7 items-center gap-1">
          <BrowserToolbarButton
            label={t(bookmarked ? 'workspace.browser.removeBookmark' : 'workspace.browser.addBookmark')}
            disabled={!browsing}
            className={cn(bookmarked && 'bg-accent text-accent-foreground')}
            data-browser-bookmark-toggle="true"
            data-browser-bookmarked={bookmarked ? 'true' : 'false'}
            onClick={() => {
              if (!active || !browsing) return;
              const existing = bookmarkByOrigin(bookmarks, active.url);
              if (existing) {
                void browserBookmarkStore.remove(existing.bookmarkId);
                return;
              }
              void browserBookmarkStore.add(active.url, active.title);
            }}
          >
            <Bookmark className={cn(bookmarked && 'fill-current')} />
          </BrowserToolbarButton>
          <BrowserToolbarButton
            label={t(active?.viewMode === 'mobile' ? 'workspace.browser.switchToDesktop' : 'workspace.browser.switchToMobile')}
            disabled={!browsing}
            data-browser-view-mode={active?.viewMode ?? 'desktop'}
            onClick={() => active && void browserWebviewHost.setViewMode(active.pageId, active.viewMode === 'mobile' ? 'desktop' : 'mobile')}
          >
            {active?.viewMode === 'mobile' ? <Monitor /> : <Smartphone />}
          </BrowserToolbarButton>
          <BrowserToolbarButton
            label={t('workspace.browser.openInSystemBrowser')}
            disabled={!externalHref}
            data-browser-open-external="true"
            onClick={() => externalHref && void openExternalUrl(externalHref)}
          >
            <ExternalLink />
          </BrowserToolbarButton>
        </div>
      </div>
      <div className="flex h-9 shrink-0 items-center border-b border-border/50">
        <div
          ref={tabStripRef}
          className="gold-themed-scrollbar flex min-w-0 flex-1 items-center gap-1 overflow-x-auto px-1"
          data-browser-tab-strip="true"
        >
          {session.pages.map((page) => (
            <BrowserInternalTab
              key={page.pageId}
              title={page.title || t('workspace.browser.untitled')}
              faviconSrc={faviconForUrl(history, page.url)}
              active={page.pageId === session.activePageId}
              onActivate={() => browserSessionStore.activate(page.pageId)}
              onClose={() => closePages(browserSessionStore.close(page.pageId))}
              onCloseOthers={() => closePages(browserSessionStore.closeOthers(page.pageId))}
              onCloseLeft={() => closePages(browserSessionStore.closeToTheLeft(page.pageId))}
              onCloseRight={() => closePages(browserSessionStore.closeToTheRight(page.pageId))}
              onCloseAll={() => closePages(browserSessionStore.closeAll())}
            />
          ))}
        </div>
        {tabsOverflowing ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button ref={overflowMenuRef} type="button" size="icon-xs" variant="ghost" className="shrink-0" aria-label={t('workspace.browser.allPages')}>
                <ChevronDown />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-[min(19rem,calc(100vw-2rem))]">
              {session.pages.map((page) => (
                <DropdownMenuItem key={page.pageId} onSelect={() => browserSessionStore.activate(page.pageId)}>
                  <BrowserFavicon src={faviconForUrl(history, page.url)} />
                  <span className="min-w-0 flex-1 truncate">{page.title || t('workspace.browser.untitled')}</span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="mr-1 shrink-0"
          aria-label={t('workspace.browser.newPage')}
          data-browser-new-page="true"
          onClick={() => browserSessionStore.addBlankPage()}
        >
          <Plus />
        </Button>
      </div>
      {session.noticeCode ? (
        <div className="flex shrink-0 items-center gap-2 border-b border-border/50 px-3 py-1.5 text-xs text-muted-foreground" data-browser-notice={session.noticeCode}>
          <span className="min-w-0 flex-1">{t(`errors.${session.noticeCode}`)}</span>
        </div>
      ) : null}
      {active ? (
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
          {showPortal ? (
            <div className="absolute inset-0 z-10 flex min-h-0 flex-col">
              {portal}
            </div>
          ) : null}
          <NativeBrowserViewport
            page={active}
            visible={!showPortal}
            loadingLabel={t('workspace.browser.loading')}
          />
        </div>
      ) : portal}
    </section>
  );
}

function BrowserToolbarButton({ label, children, ...props }: ComponentProps<typeof Button> & { label: string }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button type="button" size="icon-xs" variant="ghost" aria-label={label} {...props}>
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function BrowserInternalTab({
  title,
  faviconSrc,
  active,
  onActivate,
  onClose,
  onCloseOthers,
  onCloseLeft,
  onCloseRight,
  onCloseAll,
}: {
  title: string;
  faviconSrc?: string | null;
  active: boolean;
  onActivate: () => void;
  onClose: () => void;
  onCloseOthers: () => void;
  onCloseLeft: () => void;
  onCloseRight: () => void;
  onCloseAll: () => void;
}) {
  const { t } = useTranslation();
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          data-browser-page-tab="true"
          data-state={active ? 'active' : 'inactive'}
          className={cn(
            'group relative flex h-7 min-w-28 max-w-44 shrink-0 items-center gap-1 rounded-lg px-2 text-xs',
            active ? 'bg-accent/70 text-accent-foreground' : 'text-muted-foreground hover:bg-muted/35 hover:text-foreground',
          )}
        >
          <button type="button" className="flex min-w-0 flex-1 items-center gap-1 self-stretch text-left outline-none" onClick={onActivate}>
            <BrowserFavicon src={faviconSrc} />
            <span className="min-w-0 flex-1 truncate">{title}</span>
          </button>
          <Button
            type="button"
            size="icon-xs"
            variant="ghost"
            className={cn(
              'size-5 rounded-md',
              active ? 'opacity-60 hover:opacity-100' : 'opacity-0 group-hover:opacity-60 focus-visible:opacity-100',
            )}
            aria-label={t('workspace.browser.closePage')}
            onClick={onClose}
          >
            <X />
          </Button>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onClose}>{t('workspace.browser.closePage')}</ContextMenuItem>
        <ContextMenuItem onSelect={onCloseOthers}>{t('workspace.browser.closeOtherPages')}</ContextMenuItem>
        <ContextMenuItem onSelect={onCloseLeft}>{t('workspace.browser.closePagesToTheLeft')}</ContextMenuItem>
        <ContextMenuItem onSelect={onCloseRight}>{t('workspace.browser.closePagesToTheRight')}</ContextMenuItem>
        <ContextMenuItem onSelect={onCloseAll}>{t('workspace.browser.closeAllPages')}</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
