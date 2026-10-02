/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const i18nState = vi.hoisted(() => ({ language: 'en' }));
const loadReleaseNotes = vi.hoisted(() => vi.fn<(locale: string) => Promise<string | null>>());
const openExternalUrl = vi.hoisted(() => vi.fn(() => Promise.resolve()));

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({
    i18n: i18nState,
    t: (key: string, options?: Record<string, unknown>) => (
      options?.version ? `${key}:${String(options.appName)}:${String(options.version)}` : key
    ),
  }),
}));
vi.mock('@/api', () => ({ openExternalUrl }));
vi.mock('@/components/update/release-notes-source', () => ({ releaseNotesVersion: '0.18.0', loadReleaseNotes }));

import { ReleaseNotesDialog } from '@/components/update/ReleaseNotesDialog';
import { TooltipProvider } from '@/components/ui/tooltip';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

beforeEach(() => {
  i18nState.language = 'en';
  loadReleaseNotes.mockReset();
  openExternalUrl.mockClear();
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

async function render(props: Partial<React.ComponentProps<typeof ReleaseNotesDialog>> = {}) {
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => {
    root?.render(
      <TooltipProvider>
        <ReleaseNotesDialog open appName="Gold Band" releaseNotesUrl="https://example.com/releases" onOpenChange={() => {}} {...props} />
      </TooltipProvider>,
    );
  });
}

function notesText() {
  return document.querySelector('[data-update-dialog-notes="true"]')?.textContent?.trim();
}

function buttonByText(text: string) {
  return Array.from(document.querySelectorAll('button')).find((button) => button.textContent?.includes(text));
}

describe('ReleaseNotesDialog', () => {
  it('loads the embedded notes for the UI language only when opened', async () => {
    loadReleaseNotes.mockResolvedValue('First line\nSecond line');
    await render({ open: false });
    expect(loadReleaseNotes).not.toHaveBeenCalled();

    await act(async () => root?.render(
      <TooltipProvider>
        <ReleaseNotesDialog open appName="Gold Band" releaseNotesUrl="" onOpenChange={() => {}} />
      </TooltipProvider>,
    ));
    expect(loadReleaseNotes).toHaveBeenCalledWith('en');
    expect(document.querySelector('[role="dialog"] h2')?.textContent).toBe('settings.updater.releaseNotes.title:Gold Band:0.18.0');
    expect(document.querySelector('[data-update-dialog-notes="true"] br')).not.toBeNull();
  });

  it('shows an empty state when the build has no notes for this version', async () => {
    loadReleaseNotes.mockResolvedValue(null);
    await render();
    expect(notesText()).toBe('settings.updater.releaseNotes.empty');
  });

  it('reports a load failure instead of the empty state', async () => {
    loadReleaseNotes.mockRejectedValue(new Error('chunk failed'));
    await render();
    expect(notesText()).toBe('settings.updater.releaseNotes.loadFailed');
  });

  it('opens the channel release notes page from More', async () => {
    loadReleaseNotes.mockResolvedValue('Notes');
    await render();
    await act(async () => buttonByText('settings.updater.releaseNotes.more')!.click());
    expect(openExternalUrl).toHaveBeenCalledWith('https://example.com/releases');
  });

  it('hides More when the channel has no release notes page', async () => {
    loadReleaseNotes.mockResolvedValue('Notes');
    await render({ releaseNotesUrl: '' });
    expect(buttonByText('settings.updater.releaseNotes.more')).toBeUndefined();
  });
});
