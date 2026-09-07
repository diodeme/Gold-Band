/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  ACPMessageList,
  MessageAttachmentPreviewButton,
  optimisticUserEvent,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const apiMocks = vi.hoisted(() => ({
  showConversationAttachment: vi.fn(),
  showConversationMessageAttachment: vi.fn(),
}));

const imageActionMocks = vi.hoisted(() => ({
  copy: vi.fn(() => Promise.resolve()),
  save: vi.fn(() => Promise.resolve(true)),
}));

vi.mock('@/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/api')>(),
  showConversationAttachment: apiMocks.showConversationAttachment,
  showConversationMessageAttachment: apiMocks.showConversationMessageAttachment,
}));

vi.mock('@/lib/image-actions', () => ({
  copyImageAsset: imageActionMocks.copy,
  IMAGE_ACTION_FEEDBACK_DURATION_MS: 1_800,
  saveImageAssetAs: imageActionMocks.save,
}));

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  apiMocks.showConversationAttachment.mockResolvedValue({
    title: 'image.png',
    kind: 'input-attachment',
    content: 'data:image/png;base64,AQIDBA==',
    metadata: { mimeType: 'image/png' },
  });
  imageActionMocks.copy.mockClear();
  imageActionMocks.save.mockClear();
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP message attachment layout', () => {
  it('offers copy and save-as from a sent image thumbnail', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <MessageAttachmentPreviewButton
              attachment={{
                name: 'image.png',
                path: 'task-inputs/image.png',
                type: 'image/png',
                size: 4,
              }}
              locator={{
                projectId: 'project-1',
                taskId: 'task-1',
                runId: 'run-1',
                roundId: 'round-1',
                nodeId: 'node-1',
                attemptId: 'attempt-1',
              }}
            />
          </TooltipProvider>,
        );
      });

      const thumbnail = container.querySelector<HTMLImageElement>('img[alt="image.png"]');
      expect(thumbnail?.src).toBe('data:image/png;base64,AQIDBA==');

      await act(async () => {
        thumbnail?.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          button: 2,
          buttons: 2,
          clientX: 12,
          clientY: 12,
        }));
      });

      const menu = document.querySelector('[data-slot="context-menu-content"]');
      expect(menu?.textContent).toContain('复制图片');
      expect(menu?.textContent).toContain('图片另存为');

      const copyItem = Array.from(document.querySelectorAll<HTMLElement>('[data-slot="context-menu-item"]'))
        .find((item) => item.textContent?.includes('复制图片'));
      await act(async () => copyItem?.click());

      expect(imageActionMocks.copy).toHaveBeenCalledWith({
        name: 'image.png',
        mime: 'image/png',
        previewUrl: 'data:image/png;base64,AQIDBA==',
      });

      await act(async () => {
        thumbnail?.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          button: 2,
          buttons: 2,
          clientX: 12,
          clientY: 12,
        }));
      });
      const saveItem = Array.from(document.querySelectorAll<HTMLElement>('[data-slot="context-menu-item"]'))
        .find((item) => item.textContent?.includes('图片另存为'));
      await act(async () => saveItem?.click());

      expect(imageActionMocks.save).toHaveBeenCalledWith({
        name: 'image.png',
        mime: 'image/png',
        previewUrl: 'data:image/png;base64,AQIDBA==',
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

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
