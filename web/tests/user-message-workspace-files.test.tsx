/** @vitest-environment jsdom */

import { act } from 'react';
import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { UserMessageWorkspaceFiles } from '@/components/conversation/UserMessageWorkspaceFiles';
import type { WorkspaceFileTimelineRef } from '@/lib/composer-context';
import '@/i18n';

describe('user message workspace file chips', () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  it('renders compact file chips and invokes the workspace open command', async () => {
    container = document.body.appendChild(document.createElement('div'));
    root = createRoot(container);
    const onOpen = vi.fn();
    const file: WorkspaceFileTimelineRef = {
      projectId: 'default',
      relativePath: 'src/config.json',
      canonicalPath: '/default/src/config.json',
      name: 'config.json',
    };

    await act(async () => {
      root?.render(createElement(UserMessageWorkspaceFiles, {
        files: [file],
        onOpen,
      }));
    });
    const chip = container.querySelector<HTMLButtonElement>('[data-user-workspace-files] button');
    expect(chip?.textContent).toContain('config.json');

    await act(async () => {
      chip?.click();
    });

    expect(onOpen).toHaveBeenCalledWith(file);
  });
});
