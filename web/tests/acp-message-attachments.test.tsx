/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  ACPMessageList,
  optimisticUserEvent,
} from '@/components/acp/ACPChatDialog';
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
      expect(rows[0]?.querySelector('button')?.getAttribute('aria-label')).toBe('image.png');
      expect(rows[0]?.textContent).not.toContain('image.png');
      expect(rows[0]?.textContent).not.toContain('acp.raw.jsonl');
      expect(rows[1]?.textContent).toContain('acp.raw.jsonl');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it.each([
    {
      stage: 'canonical',
      message: {
        id: 'user-attachment-only',
        seq: 2,
        timestamp: '2Z',
        kind: 'userTextDelta' as const,
        sessionId: 'session-1',
        content: '',
        status: 'completed' as const,
        raw: {
          attachments: [
            { name: 'notes.txt', path: 'task-inputs/notes.txt', type: 'text/plain', size: 12 },
          ],
        },
      },
    },
    {
      stage: 'optimistic',
      message: optimisticUserEvent('', 'prompt-attachment-only', [], null, [
        { name: 'notes.txt', path: 'task-inputs/notes.txt', type: 'text/plain', size: 12 },
      ]),
    },
  ])('renders only attachments without an empty user bubble during $stage projection', async ({ message }) => {
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

      const userRow = container.querySelector<HTMLElement>('[data-acp-message-row="user"]');
      expect(userRow).not.toBeNull();
      const attachmentOnly = userRow?.querySelector<HTMLElement>('[data-acp-attachment-only="true"]');
      expect(attachmentOnly).not.toBeNull();
      expect(attachmentOnly?.className).toContain('pt-0.5');
      expect(attachmentOnly?.querySelector('[data-acp-attachment-row="files"]')).not.toBeNull();
      expect(userRow?.querySelector('.bg-message-user')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
