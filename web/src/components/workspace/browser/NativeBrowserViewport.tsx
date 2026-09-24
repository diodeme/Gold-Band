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
}: {
  page: BrowserPage | null;
  visible: boolean;
  loadingLabel: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<number | null>(null);
  const pageRef = useRef(page);
  pageRef.current = page;
  const liveRef = useRef(page?.live ?? false);
  const pageId = page?.pageId ?? null;
  const live = page?.live ?? false;

  const measureAndSync = useCallback(() => {
    const element = hostRef.current;
    const current = pageRef.current;
    if (!element || !current) return;
    const next = readBounds(element);
    void browserWebviewHost.ensurePage(current, next, visible).catch(() => undefined);
  }, [visible]);

  const sync = useCallback(() => {
    if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      measureAndSync();
    });
  }, [measureAndSync]);

  useLayoutEffect(() => {
    const element = hostRef.current;
    const current = pageRef.current;
    if (!element || !current) {
      return;
    }
    measureAndSync();
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    window.addEventListener('resize', sync);
    const stopVisibility = browserWebviewHost.onVisibilityChange(() => {
      if (!pageRef.current) return;
      sync();
    });
    const stopLayout = browserWebviewHost.onLayoutFrame(() => {
      if (!pageRef.current) return;
      sync();
    });
    const visualViewport = window.visualViewport;
    visualViewport?.addEventListener('resize', sync);
    return () => {
      stopVisibility();
      stopLayout();
      observer.disconnect();
      window.removeEventListener('resize', sync);
      visualViewport?.removeEventListener('resize', sync);
      if (frameRef.current != null) cancelAnimationFrame(frameRef.current);
    };
  }, [measureAndSync, pageId, sync, visible]);

  useLayoutEffect(() => {
    if (liveRef.current === live) return;
    liveRef.current = live;
    if (!pageId) return;
    measureAndSync();
  }, [live, measureAndSync, pageId]);

  return (
    <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden" data-browser-viewport="true">
      {page?.loading && !page.live ? (
        <div className="absolute inset-0 z-10">
          <BrandLoadingState label={loadingLabel} surface="background" />
        </div>
      ) : null}
      <div
        ref={hostRef}
        className="absolute inset-0"
        data-browser-native-host="true"
      />
    </div>
  );
}
