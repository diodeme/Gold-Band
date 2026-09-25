import { useEffect, useRef, type ReactNode } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauriRuntime } from '../api/shared';
import type { DesktopWindowFrameStyle } from '../types';

export function DesktopWindowFrame({ frameStyle, children }: {
  frameStyle: DesktopWindowFrameStyle;
  children: ReactNode;
}) {
  const frameRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || frameStyle !== 'app-outline' || !isTauriRuntime()) return;
    const appWindow = getCurrentWindow();
    const disposers: (() => void)[] = [];
    const resizeListener = new AbortController();
    let active = true;
    let revision = 0;
    let animationFrame: number | undefined;
    let physicalSize: { width: number; height: number } | undefined;

    const scheduleLayout = () => {
      if (!active || !physicalSize || animationFrame !== undefined) return;
      animationFrame = requestAnimationFrame(() => {
        animationFrame = undefined;
        if (!active || !physicalSize) return;
        frame.style.width = `${physicalSize.width / window.devicePixelRatio}px`;
        frame.style.height = `${physicalSize.height / window.devicePixelRatio}px`;
      });
    };
    const acceptSize = (size: { width: number; height: number }) => {
      if (!active) return;
      revision += 1;
      if (size.width <= 0 || size.height <= 0) return;
      physicalSize = size;
      scheduleLayout();
    };
    const register = async (subscription: Promise<() => void>) => {
      const dispose = await subscription;
      if (active) disposers.push(dispose);
      else dispose();
    };

    window.addEventListener('resize', scheduleLayout, { signal: resizeListener.signal });
    void Promise.all([
      register(appWindow.onResized(({ payload }) => acceptSize(payload))),
      register(appWindow.onScaleChanged(({ payload }) => acceptSize(payload.size))),
    ]).then(async () => {
      if (!active) return;
      const requestedRevision = revision;
      const size = await appWindow.innerSize();
      if (active && requestedRevision === revision) acceptSize(size);
    }).catch((error: unknown) => {
      if (active) console.warn('Failed to synchronize desktop window frame bounds', error);
    });

    return () => {
      active = false;
      resizeListener.abort();
      disposers.forEach((dispose) => dispose());
      if (animationFrame !== undefined) cancelAnimationFrame(animationFrame);
      frame.style.removeProperty('width');
      frame.style.removeProperty('height');
    };
  }, [frameStyle]);

  return (
    <div ref={frameRef} className="app-window-frame flex h-screen min-w-0 flex-col overflow-hidden bg-gold-workspace" data-window-frame-style={frameStyle}>
      {children}
    </div>
  );
}
