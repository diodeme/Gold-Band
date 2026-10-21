/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  ConversationPromptQueue,
  moveQueueItemIds,
} from '@/components/conversation/ConversationPromptQueue';
import type { ConversationPromptQueueVm } from '@/types';
import '@/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const queue: ConversationPromptQueueVm = {
  revision: 5,
  maxItems: 10,
  dispatchMode: 'auto',
  autoDispatchSuspended: false,
  suspensionReason: null,
  items: Array.from({ length: 5 }, (_, index) => ({
    id: `item-${index + 1}`,
    content: `queued prompt ${index + 1}`,
    attachmentCount: index === 0 ? 2 : 0,
    quoteCount: index === 1 ? 2 : 0,
    workspaceFileCount: index === 2 ? 1 : 0,
    createdAt: `2026-08-07T00:00:0${index}Z`,
  })),
};

function queueWith(overrides: Partial<ConversationPromptQueueVm>): ConversationPromptQueueVm {
  return { ...queue, ...overrides };
}

function itemWith(overrides: Partial<ConversationPromptQueueVm['items'][number]>) {
  return {
    id: 'item-x',
    content: 'queued prompt',
    attachmentCount: 0,
    quoteCount: 0,
    workspaceFileCount: 0,
    createdAt: '2026-09-16T00:00:00Z',
    ...overrides,
  };
}

describe('ConversationPromptQueue', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function renderQueue(overrides: Partial<React.ComponentProps<typeof ConversationPromptQueue>> = {}) {
    const props: React.ComponentProps<typeof ConversationPromptQueue> = {
      queue,
      stopInProgress: false,
      mutationPending: false,
      composerOccupied: false,
      sendingItemId: null,
      onRestore: vi.fn(),
      onReorder: vi.fn(),
      onSendNow: vi.fn(),
      onDelete: vi.fn(),
      onToggleMode: vi.fn(),
      onResumeDispatch: vi.fn(),
      onClear: vi.fn(),
      ...overrides,
    };
    await act(async () => root.render(<ConversationPromptQueue {...props} />));
    return props;
  }

  it('opens by default with a three-item preview and keeps the expanded item range across whole-panel collapse', async () => {
    await renderQueue();

    const trigger = host.querySelector<HTMLButtonElement>('[data-queue-trigger="true"]');
    expect(trigger).toBeTruthy();
    expect(trigger?.getAttribute('aria-expanded')).toBe('true');
    expect(trigger?.textContent).toContain('待发送');
    expect(trigger?.textContent).toContain('5/10');
    const queueSurface = host.querySelector('[data-testid="conversation-prompt-queue"]');
    expect(queueSurface?.classList.contains('bg-card')).toBe(true);
    expect(queueSurface?.classList.contains('border')).toBe(true);
    expect(queueSurface?.classList.contains('border-0')).toBe(false);
    expect(queueSurface?.classList.contains('bg-muted/35')).toBe(false);
    expect(host.querySelector('[data-queue-items="true"]')?.classList.contains('divide-y')).toBe(false);
    expect(host.querySelector('[data-queue-items="true"]')?.classList.contains('border-t')).toBe(false);
    expect(host.querySelector('[data-queue-show-more-row="true"]')?.classList.contains('border-t')).toBe(false);
    expect(host.querySelectorAll('[data-queue-item-id]')).toHaveLength(3);
    expect(host.textContent).toContain('queued prompt 1');
    expect(host.textContent).not.toContain('queued prompt 4');

    const showMore = host.querySelector<HTMLButtonElement>('[data-queue-show-more="true"]');
    expect(showMore?.textContent).toContain('查看更多');
    expect(showMore?.getAttribute('aria-expanded')).toBe('false');
    await act(async () => showMore?.click());
    expect(showMore?.getAttribute('aria-expanded')).toBe('true');
    expect(showMore?.textContent).toContain('仅显示前 3 条');
    expect(host.querySelectorAll('[data-queue-item-id]')).toHaveLength(5);

    await act(async () => trigger?.click());
    expect(trigger?.getAttribute('aria-expanded')).toBe('false');
    expect(host.querySelectorAll('[data-queue-item-id]')).toHaveLength(0);
    expect(host.textContent).not.toContain('queued prompt 1');

    await act(async () => trigger?.click());
    expect(trigger?.getAttribute('aria-expanded')).toBe('true');
    expect(host.querySelectorAll('[data-queue-item-id]')).toHaveLength(5);
    expect(host.textContent).toContain('queued prompt 5');
  });

  it('returns the complete queued draft to the composer and exposes accessible icon actions', async () => {
    const onRestore = vi.fn().mockResolvedValue(undefined);
    await renderQueue({ onRestore });
    const firstRow = host.querySelector('[data-queue-item-id="item-1"]') as HTMLElement;
    const editButton = firstRow.querySelector('button[aria-label="编辑"]') as HTMLButtonElement;
    const sendNowButton = firstRow.querySelector('button[aria-label="立即发送"]');
    const deleteButton = firstRow.querySelector('button[aria-label="删除"]');
    const reorderButton = firstRow.querySelector('button[aria-label="调整排队顺序"]');
    expect(editButton).toBeTruthy();
    expect(sendNowButton).toBeTruthy();
    expect(deleteButton).toBeTruthy();
    expect(reorderButton).toBeTruthy();

    await act(async () => editButton.click());
    expect(onRestore).toHaveBeenCalledWith('item-1');
    expect(host.querySelector('[data-queue-item-id="item-1"]')).toBe(firstRow);
  });

  it('protects an existing composer draft from being overwritten', async () => {
    const onRestore = vi.fn();
    await renderQueue({ composerOccupied: true, onRestore });
    const editButtons = host.querySelectorAll<HTMLButtonElement>('button[aria-label="编辑"]');
    expect(editButtons).toHaveLength(3);
    expect(Array.from(editButtons).every((button) => button.disabled)).toBe(true);
    expect(onRestore).not.toHaveBeenCalled();
  });

  it('keeps send-now usable while the session is active but not while a stop is in flight', async () => {
    await renderQueue();
    const activeButtons = host.querySelectorAll<HTMLButtonElement>('button[aria-label="立即发送"]');
    expect(activeButtons).toHaveLength(3);
    expect(Array.from(activeButtons).every((button) => !button.disabled)).toBe(true);

    await renderQueue({ stopInProgress: true });
    const stoppedButtons = host.querySelectorAll<HTMLButtonElement>('button[aria-label="立即发送"]');
    expect(stoppedButtons).toHaveLength(3);
    expect(Array.from(stoppedButtons).every((button) => button.disabled)).toBe(true);
  });

  it('marks only the in-flight row while an immediate send is pending', async () => {
    await renderQueue({ sendingItemId: 'item-2' });
    const sendingRow = host.querySelector('[data-queue-item-id="item-2"]');
    const otherRow = host.querySelector('[data-queue-item-id="item-1"]');
    expect(sendingRow?.getAttribute('data-queue-sending')).toBe('true');
    expect(otherRow?.getAttribute('data-queue-sending')).toBe('false');
  });

  it('toggles the dispatch mode from the header', async () => {
    const onToggleMode = vi.fn().mockResolvedValue(undefined);
    await renderQueue({ onToggleMode });
    const toggle = host.querySelector<HTMLButtonElement>('[data-queue-mode="auto"]');
    expect(toggle).toBeTruthy();
    expect(toggle?.textContent).toContain('自动发送');

    await act(async () => toggle?.click());
    expect(onToggleMode).toHaveBeenCalledWith('manual');
  });

  it('keeps the trigger, mode toggle and more actions on one header row', async () => {
    await renderQueue();
    const header = host.querySelector('[data-queue-header="true"]');
    expect(header).toBeTruthy();
    expect(header?.contains(host.querySelector('[data-queue-trigger="true"]'))).toBe(true);
    expect(header?.contains(host.querySelector('[data-queue-mode="auto"]'))).toBe(true);
    expect(
      header?.contains(host.querySelector('button[aria-label="更多操作"]')),
    ).toBe(true);
    // The controls live beside the trigger, not on a second row below it.
    expect(header?.parentElement?.querySelector(':scope > [data-queue-header]')).toBe(header);
  });

  it('shows the suspension reason with a resume action and clears it through resume', async () => {
    const onResumeDispatch = vi.fn().mockResolvedValue(undefined);
    await renderQueue({
      onResumeDispatch,
      queue: queueWith({ autoDispatchSuspended: true, suspensionReason: 'failed' }),
    });
    expect(host.querySelector('[data-queue-suspended-reason="failed"]')?.textContent)
      .toContain('已暂停：上一轮发送失败');

    const resumeButton = host.querySelector<HTMLButtonElement>('button[aria-label="恢复自动发送"]');
    expect(resumeButton).toBeTruthy();
    await act(async () => resumeButton?.click());
    expect(onResumeDispatch).toHaveBeenCalled();
  });

  it('clears queued prompts only after the confirmation dialog', async () => {
    const onClear = vi.fn().mockResolvedValue(undefined);
    await renderQueue({ onClear });
    expect(host.textContent).not.toContain('清空待发送队列？');

    const moreButton = host.querySelector<HTMLButtonElement>('button[aria-label="更多操作"]');
    expect(moreButton).toBeTruthy();
    // Radix portals its menus and dialogs to document.body, outside `host`.
    await act(async () => moreButton?.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
    await act(async () => moreButton?.click());
    const clearItem = await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
      return Array.from(document.body.querySelectorAll('[role="menuitem"]'))
        .find((item) => item.textContent?.includes('清空待发送'));
    });
    expect(clearItem).toBeTruthy();

    await act(async () => clearItem?.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true })));
    await act(async () => clearItem?.click());
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 50)); });
    expect(document.body.textContent).toContain('清空待发送队列？');
    expect(onClear).not.toHaveBeenCalled();

    const confirmButton = Array.from(document.body.querySelectorAll('button'))
      .find((button) => button.textContent === '清空待发送');
    expect(confirmButton).toBeTruthy();
    await act(async () => confirmButton?.click());
    expect(onClear).toHaveBeenCalled();
  });

  it('hides the whole surface in manual mode after the queue empties', async () => {
    await renderQueue({
      queue: queueWith({ items: [], dispatchMode: 'manual' }),
    });
    expect(host.querySelector('[data-testid="conversation-prompt-queue"]')).toBeNull();
  });

  it('hides the whole surface while suspended after the queue empties', async () => {
    await renderQueue({
      queue: queueWith({ items: [], autoDispatchSuspended: true, suspensionReason: 'stopped' }),
    });
    expect(host.querySelector('[data-testid="conversation-prompt-queue"]')).toBeNull();
  });

  it('resurfaces the durable manual mode and suspension once items exist again', async () => {
    await renderQueue({
      queue: queueWith({
        items: [itemWith({ id: 'queued-again', content: '再次排队' })],
        dispatchMode: 'manual',
        autoDispatchSuspended: true,
        suspensionReason: 'failed',
      }),
    });
    expect(host.querySelector('[data-queue-mode="manual"]')?.textContent).toContain('手动发送');
    expect(host.querySelector('[data-queue-suspended-reason="failed"]')).toBeTruthy();
  });

  it('points the chevron down while expanded and up while collapsed', async () => {
    await renderQueue();
    const trigger = host.querySelector<HTMLButtonElement>('[data-queue-trigger="true"]');
    const chevron = trigger?.querySelector('.transition-transform');
    expect(chevron?.classList.contains('rotate-180')).toBe(false);

    await act(async () => trigger?.click());
    expect(chevron?.classList.contains('rotate-180')).toBe(true);
  });

  it('hides the whole surface for an empty auto-mode queue without a suspension', async () => {
    await renderQueue({ queue: queueWith({ items: [] }) });
    expect(host.querySelector('[data-testid="conversation-prompt-queue"]')).toBeNull();
  });

  it('shows the structured quote count without expanding quote content', async () => {
    await renderQueue();
    const secondRow = host.querySelector('[data-queue-item-id="item-2"]') as HTMLElement;
    expect(secondRow.textContent).toContain('2 条引用');
  });

  it('shows workspace file context for a file-only queued prompt', async () => {
    await renderQueue({
      queue: queueWith({
        revision: 1,
        items: [itemWith({
          id: 'file-only',
          content: '',
          workspaceFileCount: 2,
        })],
      }),
    });
    const row = host.querySelector('[data-queue-item-id="file-only"]') as HTMLElement;

    expect(row.querySelector('[data-queue-item-content="true"]')).toBeNull();
    expect(row.querySelector('[data-queue-item-workspace-file-count]')?.textContent).toBe('2');
  });

  it('shows the queued role name and hides an empty content line', async () => {
    await renderQueue({
      queue: queueWith({
        revision: 1,
        items: [
          itemWith({
            id: 'role-only',
            content: '',
            roleName: '开发',
          }),
          itemWith({
            id: 'role-with-text',
            content: '爱仕达',
            roleName: '开发',
            createdAt: '2026-09-16T00:00:01Z',
          }),
        ],
      }),
    });
    const roleOnly = host.querySelector('[data-queue-item-id="role-only"]') as HTMLElement;
    const roleWithText = host.querySelector('[data-queue-item-id="role-with-text"]') as HTMLElement;
    expect(roleOnly.querySelector('[data-queue-item-content="true"]')).toBeNull();
    expect(roleOnly.textContent).toContain('开发');
    expect(roleWithText.querySelector('[data-queue-item-content="true"]')?.textContent).toBe('爱仕达');
    expect(roleWithText.textContent).toContain('开发');
  });

  it('calculates a complete stable-id order for pointer and keyboard sorting', () => {
    const itemIds = queue.items.map((item) => item.id);
    expect(moveQueueItemIds(itemIds, 'item-1', 'item-4')).toEqual([
      'item-2', 'item-3', 'item-4', 'item-1', 'item-5',
    ]);
    expect(moveQueueItemIds(itemIds, 'missing', 'item-2')).toEqual(itemIds);
    expect(itemIds).toEqual(['item-1', 'item-2', 'item-3', 'item-4', 'item-5']);
  });
});
