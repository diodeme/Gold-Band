// @vitest-environment jsdom

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import '../src/i18n';
import { AppTitleBar } from '../src/components/AppTitleBar';
import { TooltipProvider } from '../src/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const host = vi.hoisted(() => ({
  isMaximized: vi.fn().mockResolvedValue(false),
  isFullscreen: vi.fn().mockResolvedValue(false),
  onResized: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => host }));
vi.mock('../src/api/shared', () => ({ isTauriRuntime: () => true }));
vi.mock('../src/components/feedback/FeedbackDialog', () => ({ FeedbackDialog: () => null }));

afterEach(() => {
  vi.clearAllMocks();
  document.body.replaceChildren();
});

it('projects maximize, fullscreen and restore into the shared titlebar and disposes its listener', async () => {
  let resized = () => {};
  const dispose = vi.fn();
  host.onResized.mockImplementation(async (callback: () => void) => {
    resized = callback;
    return dispose;
  });
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><AppTitleBar appName="Gold Band" platform="windows" sidebarCollapsed={false} onToggleSidebar={() => {}} /></TooltipProvider>));
    const header = container.querySelector('header')!;
    expect(header.dataset.windowOccludesDesktop).toBe('false');
    for (const [maximized, fullscreen] of [[true, false], [false, true], [false, false]]) {
      host.isMaximized.mockResolvedValue(maximized);
      host.isFullscreen.mockResolvedValue(fullscreen);
      await act(async () => resized());
      expect(header.dataset.windowOccludesDesktop).toBe(String(maximized || fullscreen));
    }
    let resolveStale!: (value: boolean) => void;
    host.isMaximized.mockReturnValueOnce(new Promise<boolean>((resolve) => { resolveStale = resolve; }));
    await act(async () => resized());
    await act(async () => resized());
    await act(async () => resolveStale(true));
    expect(header.dataset.windowOccludesDesktop).toBe('false');
  } finally {
    await act(async () => root.unmount());
  }
  expect(dispose).toHaveBeenCalledOnce();
});
