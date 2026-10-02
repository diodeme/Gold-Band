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

async function renderTitleBar(props: Partial<React.ComponentProps<typeof AppTitleBar>>) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(
      <TooltipProvider>
        <AppTitleBar appName="Gold Band" platform="windows" sidebarCollapsed={false} onToggleSidebar={() => {}} {...props} />
      </TooltipProvider>,
    );
  });
  return Array.from(container.querySelectorAll('button')).find((button) => button.textContent === 'common.help')!;
}

async function openHelpMenu(help: HTMLButtonElement) {
  await act(async () => {
    help.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
    await Promise.resolve();
  });
  return Array.from(document.body.querySelectorAll<HTMLElement>('[data-slot="dropdown-menu-item"]'));
}

describe('AppTitleBar Help menu', () => {
  it('shows no tooltip when hovering Help', async () => {
    const help = await renderTitleBar({ onOpenReleaseNotes: () => {} });
    await act(async () => {
      help.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(document.querySelector('[data-slot="tooltip-content"]')).toBeNull();
  });

  it('opens release notes from Help without restoring focus to the trigger', async () => {
    const onOpenReleaseNotes = vi.fn();
    const help = await renderTitleBar({ onOpenReleaseNotes });
    const items = await openHelpMenu(help);
    expect(items.map((item) => item.textContent)).toEqual(['common.releaseNotes']);
    await act(async () => {
      items[0].click();
      await Promise.resolve();
    });
    expect(onOpenReleaseNotes).toHaveBeenCalledTimes(1);
    expect(document.activeElement).not.toBe(help);
  });

  it('lists release notes before the channel-specific Help entries', async () => {
    const onOpenPersonalAnalytics = vi.fn();
    const help = await renderTitleBar({ onOpenReleaseNotes: () => {}, onOpenPersonalAnalytics, feedbackEnabled: true });
    const items = await openHelpMenu(help);
    expect(items.map((item) => item.textContent)).toEqual(['common.releaseNotes', 'common.personalAnalytics', 'common.userFeedback']);
    await act(async () => {
      items[1].click();
      await Promise.resolve();
    });
    expect(onOpenPersonalAnalytics).toHaveBeenCalledTimes(1);
  });
});
