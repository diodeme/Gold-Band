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
  items: Array.from({ length: 5 }, (_, index) => ({
    id: `item-${index + 1}`,
    content: `queued prompt ${index + 1}`,
    attachmentCount: index === 0 ? 2 : 0,
    quoteCount: index === 1 ? 2 : 0,
    createdAt: `2026-08-07T00:00:0${index}Z`,
  })),
};

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
      sessionActive: false,
      mutationPending: false,
      composerOccupied: false,
      onRestore: vi.fn(),
      onReorder: vi.fn(),
      onUse: vi.fn(),
      onDelete: vi.fn(),
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
    expect(queueSurface?.classList.contains('border-0')).toBe(true);
    expect(queueSurface?.classList.contains('border')).toBe(false);
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
    const useButton = firstRow.querySelector('button[aria-label="使用"]');
    const deleteButton = firstRow.querySelector('button[aria-label="删除"]');
    const reorderButton = firstRow.querySelector('button[aria-label="调整排队顺序"]');
    expect(editButton).toBeTruthy();
    expect(useButton).toBeTruthy();
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

  it('disables manual use while the session is active', async () => {
    await renderQueue({ sessionActive: true });
    const useButtons = host.querySelectorAll<HTMLButtonElement>('button[aria-label="使用"]');
    expect(useButtons).toHaveLength(3);
    expect(Array.from(useButtons).every((button) => button.disabled)).toBe(true);
  });

  it('shows the structured quote count without expanding quote content', async () => {
    await renderQueue();
    const secondRow = host.querySelector('[data-queue-item-id="item-2"]') as HTMLElement;
    expect(secondRow.textContent).toContain('2 条引用');
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
