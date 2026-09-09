/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { RawFrameViewer } from '@/components/acp/ACPChatDialog';
import { ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';
import { getAcpRawFrames } from '@/api';
import i18n from '@/i18n';
import type { AcpRawFramePageVm, ConversationRunVm } from '@/types';

vi.mock('@/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/api')>(),
  getAcpRawFrames: vi.fn(),
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const page: AcpRawFramePageVm = {
  items: Array.from({ length: 100 }, (_, index) => ({
    id: `frame-${index}`, lineNumber: index + 1, kind: 'notification',
    direction: 'inbound', content: JSON.stringify({ message: `Frame ${index}` }),
    contentTruncated: false,
  })),
  page: 0, pageSize: 100, total: 200, hasPrevious: false, hasNext: true, order: 'desc',
};

afterEach(() => { vi.clearAllMocks(); document.body.replaceChildren(); });

describe('raw frame viewport', () => {
  it('keeps query controls outside the only scrollable frame list and preserves query actions', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onQueryChange = vi.fn();
    try {
      await act(async () => root.render(
        <RawFrameViewer page={page} query={{ page: 0, pageSize: 100, order: 'desc', search: 'Frame' }}
          loading={false} onQueryChange={onQueryChange} />,
      ));
      const list = container.querySelector('[data-raw-frame-scroll-area]');
      expect(list, 'frame rows need their own scroll viewport').not.toBeNull();
      expect(list?.classList.contains('overflow-y-auto')).toBe(true);
      expect(list?.classList.contains('min-h-0')).toBe(true);
      expect(list?.querySelectorAll('details')).toHaveLength(100);
      expect(list?.querySelector('input, button, [role="combobox"]')).toBeNull();
      const viewer = list!.parentElement!;
      expect(viewer.classList.contains('flex-col')).toBe(true);
      expect(viewer.classList.contains('overflow-hidden')).toBe(true);
      const toolbar = container.querySelector('[data-raw-frame-toolbar]')!;
      expect(toolbar.parentElement).toBe(viewer);
      expect(toolbar.classList.contains('shrink-0')).toBe(true);
      const click = async (key: string) => {
        const button = Array.from(toolbar.querySelectorAll('button')).find((node) => node.textContent === i18n.t(key));
        expect(button).toBeDefined();
        await act(async () => button!.click());
      };
      await click('acp.rawOlder');
      expect(onQueryChange).toHaveBeenLastCalledWith({ page: 1, pageSize: 100, order: 'desc', search: 'Frame' });
      await click('acp.rawSearch');
      expect(onQueryChange).toHaveBeenLastCalledWith({ page: 0, pageSize: 100, order: 'desc', search: 'Frame' });
    } finally { await act(async () => root.unmount()); }
  });

  it('constrains the workspace host instead of scrolling the toolbar with the resource', async () => {
    vi.mocked(getAcpRawFrames).mockResolvedValue(page);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(
        <ConversationRunWorkspaceResourcePanel
          resource={{ kind: 'raw-frames', key: 'raw-test', scopeKey: 'test', title: 'Raw frames',
            locator: { projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt' } }}
          run={{} as ConversationRunVm} agentRegistry={null} />,
      ));
      const host = container.querySelector('[data-right-workspace-resource="raw-frames"]')!;
      expect(host.classList.contains('overflow-y-auto')).toBe(false);
      expect(host.classList.contains('overflow-hidden')).toBe(true);
      expect(host.classList.contains('flex-col')).toBe(true);
      expect(host.querySelectorAll('[data-raw-frame-scroll-area]')).toHaveLength(1);
      expect(getAcpRawFrames).toHaveBeenCalledTimes(1);
    } finally { await act(async () => root.unmount()); }
  });
});
