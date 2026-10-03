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

  it('keeps initial focus on the dialog instead of controls inside the notes', async () => {
    const notes = '```sh\necho ok\n```';
    await render(<UpdateDialog open status={status({ update: { ...update, notes } })} onOpenChange={() => {}} onInstall={() => {}} />);
    expect(document.activeElement).toBe(document.querySelector('[role="dialog"]'));
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
    await render(<MarkdownImagePreviewProvider><Markdown flavor="github-release">{markdown}</Markdown></MarkdownImagePreviewProvider>);
    const image = await loadImage();
    expect(image.className).toContain('cursor-zoom-in');
    await act(async () => image.click());
    expect(document.querySelectorAll('img[src="https://example.com/shot.png"]')).toHaveLength(2);
  });
});

describe('GitHub release Markdown flavor', () => {
  const alerts = ['note', 'tip', 'important', 'warning', 'caution'];
  // Each alert type uses its own theme token so the five callouts stay distinguishable in every theme.
  const alertColors: Record<string, string> = { note: 'attention', tip: 'success', important: 'emphasis', warning: 'warning', caution: 'danger' };
  const notes = [
    'First line\nSecond line',
    ...alerts.map((type) => `> [!${type.toUpperCase()}]\n> ${type} body`),
    '> [!attention] Not a GitHub alert',
  ].join('\n\n');

  it('renders single newlines and the five GitHub alerts in the UI language', async () => {
    const container = await render(<Markdown flavor="github-release">{notes}</Markdown>);
    expect(container.querySelector('p br')).not.toBeNull();
    for (const type of alerts) {
      const alert = container.querySelector(`[data-gb-markdown-alert="${type}"]`)!;
      expect(alert.getAttribute('role')).toBe('note');
      expect(alert.querySelector('[data-slot="alert-title"]')?.textContent).toBe(`common.markdownAlert.${type}`);
      expect(alert.querySelector('[data-slot="alert-description"]')?.textContent?.trim()).toBe(`${type} body`);
      expect(alert.querySelector('svg.octicon')).toBeNull();
      expect(alert.className).toContain(`text-gold-${alertColors[type]}`);
    }
    const plainQuotes = Array.from(container.querySelectorAll('blockquote')).filter((quote) => !quote.hasAttribute('data-gb-markdown-alert'));
    expect(plainQuotes.map((quote) => quote.textContent?.trim())).toEqual(['[!attention] Not a GitHub alert']);
  });

  it('renders GitHub alerts in default Markdown but keeps CommonMark line breaks', async () => {
    const container = await render(<Markdown>{notes}</Markdown>);
    expect(container.querySelector('br')).toBeNull();
    expect([...container.querySelectorAll('[data-gb-markdown-alert]')].map((alert) => alert.getAttribute('data-gb-markdown-alert'))).toEqual(alerts);
    const plainQuotes = Array.from(container.querySelectorAll('blockquote')).filter((quote) => !quote.hasAttribute('data-gb-markdown-alert'));
    expect(plainQuotes.map((quote) => quote.textContent?.trim())).toEqual(['[!attention] Not a GitHub alert']);
  });

  it('gives document flavors a heading hierarchy while chat headings stay near body size', async () => {
    const headings = '## Section\n\nText\n\n### 1.Feature\n\nText';
    for (const flavor of ['document', 'github-release'] as const) {
      const doc = await render(<Markdown flavor={flavor}>{headings}</Markdown>);
      expect(doc.querySelector('h2')?.className).toContain('text-lg');
      expect(doc.querySelector('h2')?.className).toContain('border-b');
      expect(doc.querySelector('h3')?.className).toContain('text-base');
    }
    const chat = await render(<Markdown>{'# Title\n\n' + headings}</Markdown>);
    // Each level is one weight step above the next so chat sections stay distinguishable at body size.
    expect(chat.querySelector('h1')?.className).toContain('font-bold');
    expect(chat.querySelector('h2')?.className).toMatch(/text-\[15px\].*\bfont-bold\b/);
    expect(chat.querySelector('h3')?.className).toMatch(/\btext-sm\b.*\bfont-semibold\b/);
  });

  it('keeps chat heading spacing while streaming and only drops it for the first streamed block', async () => {
    const container = await render(<Markdown streaming>{'## First\n\nText\n\n## Second\n\nText'}</Markdown>);
    const blocks = [...container.querySelectorAll('[data-gb-stream-block]')];
    const headings = [...container.querySelectorAll('h2')];
    expect(headings).toHaveLength(2);
    expect(blocks[0]?.firstElementChild).toBe(headings[0]);
    for (const heading of headings) {
      expect(heading.className).toContain('mt-3.5');
      expect(heading.className).toContain('[[data-gb-stream-block]:first-child>&]:mt-0');
      expect(heading.className).not.toContain('first:mt-0');
    }
  });

  it('only lets the alert marker classes through the default sanitizer', async () => {
    const container = await render(<Markdown>{'<blockquote class="markdown-alert-warning evil">raw</blockquote>\n\n<p class="evil">text</p>'}</Markdown>);
    expect(container.innerHTML).not.toContain('evil');
  });

  it('only lets the alert marker classes through the release sanitizer', async () => {
    const container = await render(<Markdown flavor="github-release">{'<blockquote class="markdown-alert-warning evil">raw</blockquote>\n\n<p class="evil">text</p>'}</Markdown>);
    expect(container.querySelector('.evil')).toBeNull();
    expect(container.innerHTML).not.toContain('evil');
  });
});
