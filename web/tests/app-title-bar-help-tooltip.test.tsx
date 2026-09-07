/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { AppTitleBar } from '../src/components/AppTitleBar';
import { TooltipProvider } from '../src/components/ui/tooltip';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe('AppTitleBar Help tooltip', () => {
  it('closes the Help tooltip without restoring focus when Personal Analytics is selected', async () => {
    const onOpenPersonalAnalytics = vi.fn();
    const container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => {
      root?.render(
        <TooltipProvider>
          <AppTitleBar
            appName="Gold Band"
            platform="windows"
            sidebarCollapsed={false}
            onToggleSidebar={() => {}}
            onOpenPersonalAnalytics={onOpenPersonalAnalytics}
          />
        </TooltipProvider>,
      );
    });

    const help = Array.from(container.querySelectorAll('button'))
      .find((button) => button.textContent === 'common.help')!;
    await act(async () => {
      help.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(document.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe('common.help');

    await act(async () => {
      help.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
      await Promise.resolve();
    });
    const analyticsItem = Array.from(document.body.querySelectorAll<HTMLElement>('[data-slot="dropdown-menu-item"]'))
      .find((item) => item.textContent?.includes('common.personalAnalytics'))!;
    await act(async () => {
      analyticsItem.click();
      await Promise.resolve();
    });
    await act(async () => {
      help.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(onOpenPersonalAnalytics).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[data-slot="tooltip-content"]')).toBeNull();
    expect(document.activeElement).not.toBe(help);
  });
});
