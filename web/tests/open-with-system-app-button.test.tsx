/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { openFileWithSystemApp } from '@/api';
import { TooltipProvider } from '@/components/ui/tooltip';
import { fileContentStore } from '@/components/workspace/files/file-content-store';
import { OpenWithSystemAppButton } from '@/components/workspace/files/OpenWithSystemAppButton';
import type { FileWorkspaceResource } from '@/components/workspace/right-workspace-context';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return {
    ...actual,
    openFileWithSystemApp: vi.fn(async () => undefined),
    releaseExternalFileAccess: vi.fn(async () => undefined),
  };
});

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function fileResource(scope: 'workspace' | 'external'): FileWorkspaceResource {
  const canonicalPath = scope === 'workspace' ? 'D:/repo/pelican-sky-odyssey.svg' : 'D:/outside/report.pdf';
  return {
    key: `file:${canonicalPath}`,
    scopeKey: 'scope-1',
    title: 'file',
    attention: false,
    kind: 'file',
    projectId: 'project-1',
    locator: { projectId: 'project-1', canonicalPath, relativePath: null, scope },
    target: null,
    targetRevision: 0,
  };
}

async function renderButton(resource: FileWorkspaceResource, variant: 'icon' | 'button' = 'icon') {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(
    <TooltipProvider>
      <OpenWithSystemAppButton resource={resource} variant={variant} />
    </TooltipProvider>,
  ));
  return { container, root };
}

afterEach(async () => {
  vi.mocked(openFileWithSystemApp).mockReset();
  vi.mocked(openFileWithSystemApp).mockResolvedValue(undefined);
  await fileContentStore.release(fileResource('external').key);
  document.body.replaceChildren();
});

describe('OpenWithSystemAppButton', () => {
  it('asks the desktop runtime to open a workspace file by project and canonical path', async () => {
    const resource = fileResource('workspace');
    const { container, root } = await renderButton(resource);
    try {
      const button = container.querySelector<HTMLButtonElement>('[data-open-with-system-app="true"]');
      expect(button?.getAttribute('aria-label')).toBe('使用系统应用打开');
      await act(async () => button?.click());
      expect(openFileWithSystemApp).toHaveBeenCalledWith({
        projectId: 'project-1',
        canonicalPath: 'D:/repo/pelican-sky-odyssey.svg',
        externalAccessToken: null,
      });
      expect(container.querySelector('[data-open-with-system-app-error]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('forwards the external access grant for files outside the workspace', async () => {
    const resource = fileResource('external');
    fileContentStore.primeExternalGrant(resource.key, resource.projectId, resource.locator.canonicalPath, {
      token: 'grant-1',
      permissions: ['read'],
      expiresAtMs: '9999999999999',
    });
    const { container, root } = await renderButton(resource, 'button');
    try {
      await act(async () => container.querySelector<HTMLButtonElement>('[data-open-with-system-app="true"]')?.click());
      expect(openFileWithSystemApp).toHaveBeenCalledWith(expect.objectContaining({ externalAccessToken: 'grant-1' }));
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows the rejection code instead of failing silently', async () => {
    vi.mocked(openFileWithSystemApp).mockRejectedValueOnce({ code: 'workspace-file.system-open-failed', params: {} });
    const { container, root } = await renderButton(fileResource('workspace'));
    try {
      await act(async () => container.querySelector<HTMLButtonElement>('[data-open-with-system-app="true"]')?.click());
      const error = container.querySelector('[data-open-with-system-app-error="workspace-file.system-open-failed"]');
      expect(error?.textContent).toBe('系统应用无法打开此文件。');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
