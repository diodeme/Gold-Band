import { useCallback, useLayoutEffect, useRef } from 'react';
import { BrandLoadingState } from '@/components/BrandLoadingState';
import { browserWebviewHost, type BrowserBounds } from './browser-webview-host';
import type { BrowserPage } from './browser-session-store';

function readBounds(element: HTMLElement): BrowserBounds {
  const rect = element.getBoundingClientRect();
  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
  };
}

export function NativeBrowserViewport({
  page,
  visible,
  loadingLabel,
  coverTop = 0,
}: {
  page: BrowserPage | null;
  visible: boolean;
  loadingLabel: string;
  coverTop?: number;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<number | null>(null);
  const pageRef = useRef(page);
  pageRef.current = page;
  const pageId = page?.pageId ?? null;
  const live = page?.live ?? false;

  const sync = useCallback(() => {
    const element = hostRef.current;
    const current = pageRef.current;
    if (!element || !current) return;
    if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      const latest = pageRef.current;
      const host = hostRef.current;
      if (!latest || !host) return;
      const next = readBounds(host);
      void browserWebviewHost.ensurePage(latest, next, visible);
      if (visible && latest.live) {
        browserWebviewHost.scheduleBounds(latest.pageId, next);
      }
    });
  }, [pageId, visible]);

  useLayoutEffect(() => {
    const element = hostRef.current;
    const current = pageRef.current;
    if (!element || !current) {
      if (!current) void browserWebviewHost.hideAll();
      return;
    }
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    window.addEventListener('resize', sync);
    const stopVisibility = browserWebviewHost.onVisibilityChange(() => {
      if (!pageRef.current) return;
      sync();
    });
    return () => {
      stopVisibility();
      observer.disconnect();
      window.removeEventListener('resize', sync);
      if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
      void browserWebviewHost.hideAll();
    };
  }, [pageId, sync, visible]);

  useLayoutEffect(() => {
    if (!pageId) return;
    sync();
  }, [coverTop, live, pageId, sync, visible]);

  return (
    <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden" data-browser-viewport="true">
      {page?.loading && !page.live ? (
        <div className="absolute inset-0 z-10">
          <BrandLoadingState label={loadingLabel} surface="background" />
        </div>
      ) : null}
      <div
        ref={hostRef}
        className="absolute inset-x-0 bottom-0"
        style={{ top: coverTop }}
        data-browser-native-host="true"
      />
    </div>
  );
}
