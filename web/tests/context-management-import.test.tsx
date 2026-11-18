/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', () => ({
  getProfiles: vi.fn(),
  importProfilesFromFiles: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@/components/prompt-kit/markdown', () => ({
  Markdown: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { open } from '@tauri-apps/plugin-dialog';
import { getProfiles, importProfilesFromFiles } from '@/api';
import i18n from '@/i18n';
import { ContextManagementPage } from '@/pages/ContextManagementPage';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  vi.mocked(getProfiles).mockResolvedValue({ profiles: [] });
  vi.mocked(open).mockResolvedValue(null);
  await i18n.changeLanguage('zh-CN');
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(
    <TooltipProvider>
      <ContextManagementPage agentRegistry={null} onAgentRegistryChange={() => {}} />
    </TooltipProvider>,
  ));
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});

async function pickImportFiles() {
  const importButton = Array.from(host.querySelectorAll('button')).find(
    (button) => button.textContent === i18n.t('contextManagement.importProfile'),
  );
  expect(importButton).toBeDefined();
  await act(async () => importButton!.click());

  const trigger = document.querySelector<HTMLButtonElement>(
    '[data-slot="profile-import-settings-sheet"] button[aria-haspopup="menu"]',
  );
  expect(trigger).not.toBeNull();
  await act(async () => trigger!.dispatchEvent(
    new MouseEvent('pointerdown', { bubbles: true, button: 0 }),
  ));

  const menuItem = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
    (item) => item.textContent === i18n.t('contextManagement.importPickFiles'),
  );
  expect(menuItem).toBeDefined();
  await act(async () => menuItem!.click());
}

describe('profile file import', () => {
  it('offers only .md files and keeps settings open when selection is cancelled', async () => {
    await pickImportFiles();

    expect(open).toHaveBeenCalledExactlyOnceWith({
      multiple: true,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
      title: i18n.t('contextManagement.importPickFilesTitle'),
    });
    expect(importProfilesFromFiles).not.toHaveBeenCalled();
    expect(document.querySelector('[data-slot="profile-import-settings-sheet"]')).not.toBeNull();
  });

  it.each([
    { label: 'a single file', selected: 'C:/roles/developer.md', filePaths: ['C:/roles/developer.md'] },
    {
      label: 'multiple files',
      selected: ['C:/roles/developer.md', 'C:/roles/reviewer.MD'],
      filePaths: ['C:/roles/developer.md', 'C:/roles/reviewer.MD'],
    },
  ])('passes $label to the import API and shows the result', async ({ selected, filePaths }) => {
    vi.mocked(open).mockResolvedValue(selected);
    vi.mocked(importProfilesFromFiles).mockResolvedValue({
      totalScanned: filePaths.length,
      imported: filePaths.map((sourcePath, index) => ({
        sourcePath,
        status: 'imported',
        name: `Imported role ${index + 1}`,
        fallbacks: [],
        importedId: `imported-${index + 1}`,
        error: null,
      })),
      failed: [],
      truncated: false,
    });

    await pickImportFiles();

    expect(importProfilesFromFiles).toHaveBeenCalledExactlyOnceWith(filePaths, false);
    const result = document.querySelector('[data-slot="profile-import-result-sheet"]');
    expect(result).not.toBeNull();
    for (const sourcePath of filePaths) expect(result!.textContent).toContain(sourcePath);
  });
});
