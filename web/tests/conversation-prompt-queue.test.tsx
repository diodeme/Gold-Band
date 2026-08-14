/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ConversationPromptQueue } from '@/components/conversation/ConversationPromptQueue';
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
      onEdit: vi.fn(),
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
    expect(host.querySelector('[data-testid="conversation-prompt-queue"]')?.className).toContain('bg-card');
    expect(host.querySelector('[data-testid="conversation-prompt-queue"]')?.className).not.toContain('bg-muted/35');
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

  it('edits a queued prompt in place and exposes accessible icon actions', async () => {
    const onEdit = vi.fn().mockResolvedValue(undefined);
    await renderQueue({ onEdit });
    const firstRow = host.querySelector('[data-queue-item-id="item-1"]') as HTMLElement;
    const editButton = firstRow.querySelector('button[aria-label="编辑"]') as HTMLButtonElement;
    const useButton = firstRow.querySelector('button[aria-label="使用"]');
    const deleteButton = firstRow.querySelector('button[aria-label="删除"]');
    expect(editButton).toBeTruthy();
    expect(useButton).toBeTruthy();
    expect(deleteButton).toBeTruthy();

    await act(async () => editButton.click());
    const textarea = firstRow.querySelector('textarea') as HTMLTextAreaElement;
    expect(textarea).toBeTruthy();
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      setter?.call(textarea, 'edited in place');
      textarea.dispatchEvent(new Event('input', { bubbles: true }));
    });
    const saveButton = firstRow.querySelector('button[aria-label="保存"]') as HTMLButtonElement;
    await act(async () => saveButton.click());

    expect(onEdit).toHaveBeenCalledWith('item-1', 'edited in place');
    expect(host.querySelector('[data-queue-item-id="item-1"]')).toBe(firstRow);
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
});
