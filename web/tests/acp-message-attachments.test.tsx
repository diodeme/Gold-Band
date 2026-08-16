/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ACPMessageList } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP message attachment layout', () => {
  it('renders images above regular files in separate rows', async () => {
    const message: AcpUiEventVm = {
      id: 'user-message-1',
      seq: 1,
      timestamp: '1Z',
      kind: 'userTextDelta',
      sessionId: 'session-1',
      content: 'Inspect both attachments',
      status: 'completed',
      raw: {
        attachments: [
          { name: 'acp.raw.jsonl', path: 'task-inputs/acp.raw.jsonl', type: 'application/json', size: 1_672_643 },
          { name: 'image.png', path: 'task-inputs/image.png', type: 'image/png', size: 81_401 },
        ],
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPMessageList
              timeline={[message]}
              sessionStatus="completed"
              sending={false}
            />
          </TooltipProvider>,
        );
      });

      const rows = Array.from(container.querySelectorAll<HTMLElement>('[data-acp-attachment-row]'));
      expect(rows.map((row) => row.dataset.acpAttachmentRow)).toEqual(['images', 'files']);
      expect(rows[0]?.querySelector('button')?.className).toContain('size-[72px]');
      expect(rows[1]?.querySelector('button')?.className).toContain('w-fit');
      expect(rows[1]?.querySelector('button')?.className).toContain('rounded-full');
      expect(rows[0]?.textContent).not.toContain('acp.raw.jsonl');
      expect(rows[1]?.textContent).toContain('acp.raw.jsonl');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
