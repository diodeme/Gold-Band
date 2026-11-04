/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listConversationComposerDrafts: vi.fn(),
  saveConversationComposerDraft: vi.fn(),
  getConversationComposerDraft: vi.fn(),
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
      contentPreview: content,
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
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
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
    apiMocks.deleteConversationComposerDraft.mockResolvedValue(draftBox(3, ['beta']));
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

  it('shows the load error instead of an empty box and lets the user retry', async () => {
    apiMocks.listConversationComposerDrafts
      .mockRejectedValueOnce({
        code: 'conversation.composer-draft-storage-failed', params: {},
      })
      .mockRejectedValueOnce({
        code: 'conversation.composer-draft-storage-failed', params: {},
      });
    await render();
    const box = await openBox();
    // The mount prefetch and the open-time refresh both failed.
    expect(apiMocks.listConversationComposerDrafts.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(box.querySelector('[data-composer-draft-empty]')).toBeNull();
    expect(box.querySelector('[role="alert"]')?.textContent).toContain('无法访问草稿箱');
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-retry]')!.click());
    await settle();
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);
    expect(box.querySelector('[role="alert"]')).toBeNull();
  });

  it('loads the conversation drafts and shows their count on the trigger', async () => {
    await render();
    // The bounded summary (≤20 items × 280-char previews) is prefetched on
    // mount so the saved-draft count is visible without opening the box.
    expect(apiMocks.listConversationComposerDrafts).toHaveBeenCalledWith('project-1', 'task-1');
    await settle();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('2');
    const box = await openBox();
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);
    expect(box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.disabled).toBe(true);
  });

  it('takes a draft into the composer and drops it from the box', async () => {
    apiMocks.getConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) });
    const props = await render();
    const box = await openBox();
    const take = box.querySelector<HTMLButtonElement>('[data-composer-draft-id="draft-alpha"] button[aria-label="取出到输入框"]');
    await act(async () => take?.click());
    await settle();
    expect(apiMocks.getConversationComposerDraft).toHaveBeenCalledWith('project-1', 'task-1', 'draft-alpha');
    expect(props.restoreDraft).toHaveBeenCalledWith(takenDraft);
    expect(apiMocks.saveConversationComposerDraft).not.toHaveBeenCalled();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
  });

  it('blocks taking a draft while the composer holds other input', async () => {
    await render({ composerOccupied: true });
    const box = await openBox();
    const takes = box.querySelectorAll<HTMLButtonElement>('button[aria-label="取出到输入框"]');
    expect(Array.from(takes).every((button) => button.disabled)).toBe(true);
  });

  it('keeps the saved draft when the composer filled up in the meantime', async () => {
    apiMocks.getConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) });
    await render({ restoreDraft: vi.fn().mockReturnValue(false) });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')?.click());
    await settle();
    expect(apiMocks.saveConversationComposerDraft).not.toHaveBeenCalled();
    expect(apiMocks.deleteConversationComposerDraft).not.toHaveBeenCalled();
    expect(box.textContent).toContain('输入框已有内容，草稿仍保留在草稿箱。');
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('2');
  });

  it('explains that the content was restored when deleting the taken draft fails', async () => {
    apiMocks.getConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) });
    apiMocks.deleteConversationComposerDraft.mockRejectedValue({
      code: 'conversation.composer-draft-storage-failed',
      params: {},
    });
    await render();
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')?.click());
    await settle();
    // The composer holds the payload and the stored copy survives; the user
    // must be told both facts instead of a bare storage error.
    expect(box.textContent).toContain('内容已取回输入框，原草稿仍保留在草稿箱。');
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('2');
  });

  it('commits the captured composer payload only after the save succeeds', async () => {
    const commit = vi.fn();
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(3, ['gamma', 'alpha', 'beta']));
    await render({
      canSave: true,
      captureDraft: vi.fn().mockResolvedValue({
        input: { displayText: 'gamma', quotes: [] },
        attachmentPaths: [],
        commit,
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();
    expect(commit).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('3');
    expect(document.body.querySelector('[data-composer-draft-box="true"]')).toBeNull();
  });

  it('leaves the composer untouched and shows the error when the save fails', async () => {
    const commit = vi.fn();
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
      }),
    });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();
    expect(commit).not.toHaveBeenCalled();
    expect(box.textContent).toContain('草稿箱已满，最多可保留 20 条。');
  });

  it('asks for confirmation before deleting a draft', async () => {
    apiMocks.deleteConversationComposerDraft.mockResolvedValue(draftBox(3, ['beta']));
    await render();
    const box = await openBox();
    const remove = box.querySelector<HTMLButtonElement>('[data-composer-draft-id="draft-alpha"] button[aria-label="删除草稿"]');
    await act(async () => remove?.click());
    expect(apiMocks.deleteConversationComposerDraft).not.toHaveBeenCalled();
    const dialog = document.body.querySelector<HTMLElement>('[data-composer-draft-delete-confirm="true"]')!;
    expect(dialog).not.toBeNull();
    await act(async () => dialog.querySelector<HTMLButtonElement>('button[aria-label="取消"]')?.click());
    await settle();
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);

    await act(async () => host.querySelector<HTMLButtonElement>('[data-composer-draft-box-trigger="true"]')?.click());
    await settle();
    const reopenedBox = document.body.querySelector<HTMLElement>('[data-composer-draft-box="true"]')!;
    const removeAgain = reopenedBox.querySelector<HTMLButtonElement>('[data-composer-draft-id="draft-alpha"] button[aria-label="删除草稿"]');
    await act(async () => removeAgain?.click());
    const confirm = document.body.querySelector<HTMLElement>('[data-composer-draft-delete-confirm="true"]')!;
    await act(async () => confirm.querySelector<HTMLButtonElement>('button[aria-label="删除草稿"]')?.click());
    await settle();
    expect(apiMocks.deleteConversationComposerDraft).toHaveBeenCalledWith('project-1', 'task-1', 'draft-alpha');
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
  });

  function capturing(input: string) {
    return vi.fn().mockResolvedValue({
      input: { displayText: input, quotes: [] },
      attachmentPaths: [],
      commit: vi.fn(),
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
      .mockResolvedValueOnce(draftBox(2, ['alpha', 'beta']))
      .mockResolvedValueOnce(draftBox(1, ['other']))
      .mockResolvedValueOnce(draftBox(1, ['other']));
    apiMocks.saveConversationComposerDraft.mockReturnValue(save.promise);
    const captureDraft = capturing('gamma');
    await render({ taskId: 'task-1', canSave: true, captureDraft });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save="true"]')?.click());
    await settle();

    await render({ taskId: 'task-2', canSave: true, captureDraft });
    await openBox();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
    // The first conversation's save settles after the switch and must stay in its own scope.
    await act(async () => save.resolve(draftBox(9, ['gamma', 'alpha', 'beta'])));
    await settle();
    expect(host.querySelector('[data-composer-draft-count="true"]')?.textContent).toBe('1');
  });

  it('shows loading without claiming that an unread box is empty', async () => {
    const list = deferred<ConversationComposerDraftBoxVm>();
    apiMocks.listConversationComposerDrafts.mockReturnValue(list.promise);
    await render();
    // The mount prefetch is still in flight, so the box has no data yet.
    expect(apiMocks.listConversationComposerDrafts).toHaveBeenCalledTimes(1);
    const box = await openBox();
    expect(box.querySelector('[data-composer-draft-loading]')).not.toBeNull();
    expect(box.querySelector('[data-composer-draft-empty]')).toBeNull();
    await act(async () => list.resolve(draftBox(0, [])));
    await settle();
    expect(box.querySelector('[data-composer-draft-loading]')).toBeNull();
    expect(box.querySelector('[data-composer-draft-empty]')).not.toBeNull();
  });

  it('ignores a stale list failure after a successful save', async () => {
    const list = deferred<ConversationComposerDraftBoxVm>();
    apiMocks.listConversationComposerDrafts.mockReturnValue(list.promise);
    apiMocks.saveConversationComposerDraft.mockResolvedValue(draftBox(3, ['gamma', 'alpha', 'beta']));
    await render({ canSave: true, captureDraft: capturing('gamma') });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save]')!.click());
    await settle();
    await act(async () => list.reject({ code: 'conversation.composer-draft-storage-failed', params: {} }));
    await settle();
    expect(host.querySelector('[data-composer-draft-count]')?.textContent).toBe('3');
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it('does not restore or delete a previous conversation draft after switching project', async () => {
    const read = deferred<{ draft: ConversationQueuedPromptDraftVm; drafts: ConversationComposerDraftBoxVm }>();
    apiMocks.getConversationComposerDraft.mockReturnValue(read.promise);
    const oldProps = await render();
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')!.click());
    await render({ projectId: 'project-2' });
    await openBox();
    await act(async () => read.resolve({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) }));
    await settle();
    expect(oldProps.restoreDraft).not.toHaveBeenCalled();
    expect(apiMocks.deleteConversationComposerDraft).not.toHaveBeenCalled();
    expect(document.querySelector('[data-composer-draft-box]')).not.toBeNull();
  });

  it('keeps the persisted draft if restoring into the composer throws', async () => {
    apiMocks.getConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) });
    await render({ restoreDraft: vi.fn().mockImplementation(() => { throw { code: 'conversation.composer-draft-storage-failed', params: {} }; }) });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')!.click());
    await settle();
    expect(apiMocks.deleteConversationComposerDraft).not.toHaveBeenCalled();
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);
    expect(box.querySelector('[role="alert"]')).not.toBeNull();
  });

  it('keeps the saved copy and reports deletion failure after the composer accepts it', async () => {
    apiMocks.getConversationComposerDraft.mockResolvedValue({ draft: takenDraft, drafts: draftBox(2, ['alpha', 'beta']) });
    apiMocks.deleteConversationComposerDraft.mockRejectedValue({ code: 'conversation.composer-draft-storage-failed', params: {} });
    const props = await render();
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('button[aria-label="取出到输入框"]')!.click());
    await settle();
    expect(props.restoreDraft).toHaveBeenCalledExactlyOnceWith(takenDraft);
    expect(box.querySelectorAll('[data-composer-draft-id]')).toHaveLength(2);
    expect(box.querySelector('[role="alert"]')).not.toBeNull();
    expect(apiMocks.saveConversationComposerDraft).not.toHaveBeenCalled();
  });

  it('surfaces attachment materialization failure without saving or clearing the input', async () => {
    await render({ canSave: true, captureDraft: vi.fn().mockRejectedValue({ code: 'conversation.composer-draft-storage-failed', params: {} }) });
    const box = await openBox();
    await act(async () => box.querySelector<HTMLButtonElement>('[data-composer-draft-save]')!.click());
    await settle();
    expect(apiMocks.saveConversationComposerDraft).not.toHaveBeenCalled();
    expect(box.querySelector('[role="alert"]')).not.toBeNull();
  });

  it('admits one save even when clicked twice before React renders the pending state', async () => {
    const save = deferred<ConversationComposerDraftBoxVm>();
    apiMocks.saveConversationComposerDraft.mockReturnValue(save.promise);
    const captureDraft = capturing('gamma');
    await render({ canSave: true, captureDraft });
    const box = await openBox();
    const button = box.querySelector<HTMLButtonElement>('[data-composer-draft-save]')!;
    await act(async () => { button.click(); button.click(); });
    expect(captureDraft).toHaveBeenCalledTimes(1);
    expect(apiMocks.saveConversationComposerDraft).toHaveBeenCalledTimes(1);
    await act(async () => save.resolve(draftBox(3, ['gamma', 'alpha', 'beta'])));
    await settle();
  });
});
