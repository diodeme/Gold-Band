/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listConversationComposerDrafts: vi.fn(),
  saveConversationComposerDraft: vi.fn(),
  takeConversationComposerDraft: vi.fn(),
  deleteConversationComposerDraft: vi.fn(),
}));

vi.mock('@/api', () => apiMocks);

import { ComposerDraftBox, type ComposerDraftBoxProps } from '@/components/conversation/ComposerDraftBox';
import type { ConversationComposerDraftBoxVm, ConversationQueuedPromptDraftVm } from '@/types';
import '@/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function draftBox(revision: number, contents: string[]): ConversationComposerDraftBoxVm {
  return {
    revision,
    maxItems: 20,
    items: contents.map((content, index) => ({
      id: `draft-${content}`,
      content,
      attachmentCount: 0,
      quoteCount: 0,
      workspaceFileCount: 0,
      createdAt: `2026-10-29T00:00:0${index}Z`,
    })),
  };
}

const takenDraft: ConversationQueuedPromptDraftVm = {
  content: 'alpha',
  quotes: [],
  attachmentPaths: [],
  workspaceFiles: [],
};

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function settle() {
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
}

describe('ComposerDraftBox', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    Object.values(apiMocks).forEach((mock) => mock.mockReset());
    apiMocks.listConversationComposerDrafts.mockResolvedValue(draftBox(2, ['alpha', 'beta']));
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function render(overrides: Partial<ComposerDraftBoxProps> = {}) {
    const props: ComposerDraftBoxProps = {
      projectId: 'project-1',
      taskId: 'task-1',
      disabled: false,
      canSave: false,
      composerOccupied: false,
      captureDraft: vi.fn().mockResolvedValue(null),
      restoreDraft: vi.fn().mockReturnValue(true),
      ...overrides,
    };
    await act(async () => root.render(<ComposerDraftBox {...props} />));
    await settle();
    return props;
  }

  async function openBox() {
    await act(async () => host.querySelector<HTMLButtonElement>('[data-composer-draft-box-trigger="true"]')?.click());
    await settle();
    return document.body.querySelector<HTMLElement>('[data-composer-draft-box="true"]')!;
  }

  it('loads the conversation drafts and shows their count on the trigger', async () => {
    await render();
    expect(apiMocks.listConversationComposerDrafts).toHaveBeenCalledWith('project-1', 'task-1');
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('2');
    const box = await openBox();
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);
    expect(box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.disabled).toBe(true);
  });

  it('takes a draft into the composer and drops it from the box', async () => {
    apiMocks.takeConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(3, ['beta']) });
    const props = await render();
    const box = await openBox();
    const take = box.querySelector<HTMLButtonElement>('[data-composer-draft-id="draft-alpha"] button[aria-label="取出到输入框"]');
    await act(async () => take?.click());
    await settle();
    expect(apiMocks.takeConversationComposerDraft).toHaveBeenCalledWith('project-1', 'task-1', 'draft-alpha');
    expect(props.restoreDraft).toHaveBeenCalledWith(takenDraft);
    expect(apiMocks.saveConversationComposerDraft).not.toHaveBeenCalled();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
  });

  it('leaves focus in the composer after taking a draft instead of returning it to the trigger', async () => {
    apiMocks.takeConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(3, ['beta']) });
    const composer = document.createElement('textarea');
    document.body.appendChild(composer);
    await render({
      restoreDraft: vi.fn(() => {
        composer.focus();
        return true;
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')?.click());
    await settle();
    expect(document.body.querySelector('[data-composer-draft-box="true"]')).toBeNull();
    expect(document.activeElement).toBe(composer);
    composer.remove();
  });

  it('leaves focus in the composer after saving a draft', async () => {
    const composer = document.createElement('textarea');
    document.body.appendChild(composer);
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(3, ['gamma', 'alpha', 'beta']));
    await render({
      canSave: true,
      captureDraft: vi.fn().mockResolvedValue({
        input: { displayText: 'gamma', quotes: [] },
        attachmentPaths: [],
        commit: () => composer.focus(),
        rollback: vi.fn(),
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();
    expect(document.body.querySelector('[data-composer-draft-box="true"]')).toBeNull();
    expect(document.activeElement).toBe(composer);
    composer.remove();
  });

  it('blocks taking a draft while the composer holds other input', async () => {
    await render({ composerOccupied: true });
    const box = await openBox();
    const takes = box.querySelectorAll<HTMLButtonElement>('button[aria-label="取出到输入框"]');
    expect(Array.from(takes).every((button) => button.disabled)).toBe(true);
  });

  it('puts a taken draft back when the composer filled up in the meantime', async () => {
    apiMocks.takeConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(3, ['beta']) });
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(4, ['alpha', 'beta']));
    await render({ restoreDraft: vi.fn().mockReturnValue(false) });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')?.click());
    await settle();
    expect(apiMocks.saveConversationComposerDraft).toHaveBeenCalledWith(
      'project-1',
      'task-1',
      { displayText: 'alpha', quotes: [] },
      undefined,
    );
    expect(box.textContent).toContain('输入框已有内容，草稿已放回草稿箱。');
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('2');
  });

  it('commits the captured composer payload only after the save succeeds', async () => {
    const commit = vi.fn();
    const rollback = vi.fn();
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(3, ['gamma', 'alpha', 'beta']));
    await render({
      canSave: true,
      captureDraft: vi.fn().mockResolvedValue({
        input: { displayText: 'gamma', quotes: [] },
        attachmentPaths: [],
        commit,
        rollback,
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();
    expect(commit).toHaveBeenCalledTimes(1);
    expect(rollback).not.toHaveBeenCalled();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('3');
    expect(document.body.querySelector('[data-composer-draft-box="true"]')).toBeNull();
  });

  it('rolls the composer payload back and shows the error when the save fails', async () => {
    const commit = vi.fn();
    const rollback = vi.fn();
    apiMocks.saveConversationComposerDraft.mockRejectedValue({
      code: 'conversation.composer-draft-full',
      params: { max: 20 },
    });
    await render({
      canSave: true,
      captureDraft: vi.fn().mockResolvedValue({
        input: { displayText: 'gamma', quotes: [] },
        attachmentPaths: [],
        commit,
        rollback,
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();
    expect(rollback).toHaveBeenCalledTimes(1);
    expect(commit).not.toHaveBeenCalled();
    expect(box.textContent).toContain('草稿箱已满，最多可保留 20 条。');
  });

  it('deletes a draft', async () => {
    apiMocks.deleteConversationComposerDraft.mockResolvedValue(draftBox(3, ['beta']));
    await render();
    const box = await openBox();
    const remove = box.querySelector<HTMLButtonElement>('[data-composer-draft-id="draft-alpha"] button[aria-label="删除草稿"]');
    await act(async () => remove?.click());
    await settle();
    expect(apiMocks.deleteConversationComposerDraft).toHaveBeenCalledWith('project-1', 'task-1', 'draft-alpha');
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(1);
  });

  function capturing(input: string) {
    return vi.fn().mockResolvedValue({
      input: { displayText: input, quotes: [] },
      attachmentPaths: [],
      commit: vi.fn(),
      rollback: vi.fn(),
    });
  }

  it('keeps a saved draft when the slower initial list lands afterwards', async () => {
    const list = deferred<ConversationComposerDraftBoxVm>();
    apiMocks.listConversationComposerDrafts.mockReturnValue(list.promise);
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(3, ['gamma', 'alpha', 'beta']));
    await render({ canSave: true, captureDraft: capturing('gamma') });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();

    await act(async () => list.resolve(draftBox(2, ['alpha', 'beta'])));
    await settle();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('3');
  });

  it('never shows a previous conversation box after switching conversations', async () => {
    const save = deferred<ConversationComposerDraftBoxVm>();
    apiMocks.listConversationComposerDrafts
      .mockResolvedValueOnce(draftBox(2, ['alpha', 'beta']))
      .mockResolvedValueOnce(draftBox(1, ['other']));
    apiMocks.saveConversationComposerDraft.mockReturnValue(save.promise);
    const captureDraft = capturing('gamma');
    await render({ taskId: 'task-1', canSave: true, captureDraft });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();

    await render({ taskId: 'task-2', canSave: true, captureDraft });
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
    // The first conversation's save settles after the switch and must stay in its own scope.
    await act(async () => save.resolve(draftBox(9, ['gamma', 'alpha', 'beta'])));
    await settle();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
  });
});
