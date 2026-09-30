/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (options?.reason) return `${key}:${String(options.reason)}`;
      if (options?.percent !== undefined) return `${key}:${String(options.percent)}`;
      return key;
    },
  }),
}));
vi.mock('@/api', () => ({ openExternalUrl: vi.fn(() => Promise.resolve()) }));

import { Markdown, MarkdownImagePreviewProvider } from '@/components/prompt-kit/markdown';
import { TooltipProvider } from '@/components/ui/tooltip';
import { TitleBarUpdateButton } from '@/components/update/TitleBarUpdateButton';
import { UpdateDialog } from '@/components/update/UpdateDialog';
import { setUpdateDownloadProgress } from '@/components/update/update-progress-store';
import { describeUpdateError, updateActionState } from '@/components/update/update-state';
import type { UpdateStatusVm } from '@/types';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const update = { version: '0.18.0', currentVersion: '0.17.2', notes: '## Notes\n\nShip it.' };
const status = (overrides: Partial<UpdateStatusVm> = {}): UpdateStatusVm => ({
  status: 'available',
  checkedAt: null,
  update,
  error: null,
  background: false,
  ...overrides,
});
const downloadError = { code: 'updater.network', params: { phase: 'download', detail: 'timed out' } };

let root: Root | null = null;

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  setUpdateDownloadProgress(null);
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

async function render(node: React.ReactNode) {
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root?.render(<TooltipProvider>{node}</TooltipProvider>));
  return container;
}

function buttonByText(text: string) {
  return Array.from(document.querySelectorAll('button')).find((button) => button.textContent?.includes(text));
}

describe('update action state', () => {
  it('follows the version comparison and never hides a known newer release', () => {
    expect(updateActionState(status({ update: null }))).toBeNull();
    expect(updateActionState(status({ update: null, status: 'error', error: downloadError }))).toBeNull();
    expect(updateActionState(status())).toBe('available');
    expect(updateActionState(status({ status: 'checking' }))).toBe('available');
    expect(updateActionState(status({ status: 'downloading' }))).toBe('downloading');
    expect(updateActionState(status({ status: 'ready' }))).toBe('ready');
    expect(updateActionState(status({ error: downloadError }))).toBe('failed');
  });

  it('describes failures with the failed phase and the structured reason', () => {
    const t = ((key: string, options?: Record<string, unknown>) => options?.reason ? `${key}:${String(options.reason)}` : key) as never;
    expect(describeUpdateError(t, downloadError)).toBe('settings.updater.failure.download:errors.updater.network');
    expect(describeUpdateError(t, { code: 'updater.invalid-url', params: {} })).toBe('errors.updater.invalid-url');
  });
});

describe('TitleBarUpdateButton', () => {
  it('stays hidden when no newer release is known', async () => {
    const container = await render(<TitleBarUpdateButton status={status({ update: null })} onOpen={() => {}} />);
    expect(container.querySelector('[data-titlebar-update-action]')).toBeNull();
  });

  it('opens the update dialog from every install state', async () => {
    const onOpen = vi.fn();
    const container = await render(<TitleBarUpdateButton status={status({ error: downloadError })} onOpen={onOpen} />);
    const button = container.querySelector<HTMLButtonElement>('[data-titlebar-update-action]')!;
    expect(button.dataset.titlebarUpdateAction).toBe('failed');
    expect(button.textContent).toBe('settings.updater.titleBar.failed');
    expect(button.className).toContain('bg-gold-attention');
    await act(async () => button.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('shows download progress without re-rendering its parent', async () => {
    const container = await render(<TitleBarUpdateButton status={status({ status: 'downloading' })} onOpen={() => {}} />);
    const button = container.querySelector<HTMLButtonElement>('[data-titlebar-update-action]')!;
    expect(button.textContent).toBe('settings.updater.titleBar.downloading');
    await act(async () => setUpdateDownloadProgress({ downloaded: 25, total: 100 }));
    expect(button.textContent).toBe('settings.updater.titleBar.downloadingPercent:25');
  });
});

describe('UpdateDialog', () => {
  it('caps width at 50rem while retaining responsive width and fixed actions around scrollable notes', async () => {
    const notes = Array.from({ length: 60 }, (_, index) => `Release note ${index + 1}: a longer update description.`).join('\n\n');
    await render(<UpdateDialog open status={status({ update: { ...update, notes } })} onOpenChange={() => {}} onInstall={() => {}} />);

    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.classList.contains('w-[min(92vw,50rem)]')).toBe(true);
    expect(dialog.classList.contains('sm:max-w-[min(92vw,50rem)]')).toBe(true);
    expect(dialog.className).not.toContain('60rem');
    for (const className of ['flex', 'flex-col', 'max-h-[min(88vh,56rem)]']) {
      expect(dialog.classList.contains(className)).toBe(true);
    }

    const header = dialog.querySelector<HTMLElement>('[data-slot="dialog-header"]')!;
    const notesRegion = dialog.querySelector<HTMLElement>('[data-update-dialog-notes="true"]')!;
    const footer = dialog.querySelector<HTMLElement>('[data-slot="dialog-footer"]')!;
    expect(header.parentElement).toBe(dialog);
    expect(notesRegion.previousElementSibling).toBe(header);
    expect(notesRegion.nextElementSibling).toBe(footer);
    expect(footer.parentElement).toBe(dialog);
    expect(header.classList.contains('shrink-0')).toBe(true);
    expect(footer.classList.contains('shrink-0')).toBe(true);
    for (const className of ['min-h-0', 'flex-1', 'overflow-y-auto']) {
      expect(notesRegion.classList.contains(className)).toBe(true);
    }
    expect(notesRegion.textContent).toContain('Release note 60:');
    expect(footer.contains(buttonByText('common.close')!)).toBe(true);
    expect(footer.contains(buttonByText('settings.updater.dialog.action.available')!)).toBe(true);
    expect(notesRegion.querySelector('button')).toBeNull();
  });

  it('starts the install directly from the dialog', async () => {
    const onInstall = vi.fn();
    await render(<UpdateDialog open status={status()} onOpenChange={() => {}} onInstall={onInstall} />);
    expect(document.body.textContent).toContain('Ship it.');
    const install = buttonByText('settings.updater.dialog.action.available')!;
    await act(async () => install.click());
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it('shows the failure reason and retries through the same action', async () => {
    const onInstall = vi.fn();
    await render(<UpdateDialog open status={status({ error: downloadError })} onOpenChange={() => {}} onInstall={onInstall} />);
    expect(document.querySelector('[role="alert"]')?.textContent).toBe('settings.updater.failure.download:errors.updater.network');
    await act(async () => buttonByText('settings.updater.dialog.action.failed')!.click());
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it('disables the action and shows progress while downloading', async () => {
    await render(<UpdateDialog open status={status({ status: 'downloading' })} onOpenChange={() => {}} onInstall={() => {}} />);
    expect(buttonByText('settings.updater.dialog.action.downloading')?.disabled).toBe(true);
    expect(document.querySelector('[data-update-download-progress="true"]')).not.toBeNull();
  });

  it('offers restart once the package is ready', async () => {
    await render(<UpdateDialog open status={status({ status: 'ready' })} onOpenChange={() => {}} onInstall={() => {}} />);
    expect(buttonByText('settings.updater.dialog.action.ready')?.disabled).toBe(false);
  });
});

describe('Markdown image preview', () => {
  const markdown = '![shot](https://example.com/shot.png)';

  async function loadImage() {
    const image = document.querySelector<HTMLImageElement>('img[src="https://example.com/shot.png"]')!;
    await act(async () => image.dispatchEvent(new Event('load')));
    return image;
  }

  it('opens a zoomed preview inside a preview provider', async () => {
    await render(<MarkdownImagePreviewProvider><Markdown>{markdown}</Markdown></MarkdownImagePreviewProvider>);
    const image = await loadImage();
    expect(image.className).toContain('cursor-zoom-in');
    await act(async () => image.click());
    expect(document.querySelectorAll('img[src="https://example.com/shot.png"]')).toHaveLength(2);
  });

  it('keeps chat Markdown images non-interactive without a provider', async () => {
    await render(<Markdown>{markdown}</Markdown>);
    const image = await loadImage();
    expect(image.className).not.toContain('cursor-zoom-in');
    await act(async () => image.click());
    expect(document.querySelectorAll('img[src="https://example.com/shot.png"]')).toHaveLength(1);
  });
});
