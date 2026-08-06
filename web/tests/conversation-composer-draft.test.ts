import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  conversationComposerDraftReducer,
  createConversationComposerDraftBoundaryHandle,
  createInitialConversationComposerDraft,
  resetConversationComposerDraft,
  type ConversationComposerDraftState,
  type ConversationComposerMulticaBinding,
} from '../src/lib/conversation-composer-draft';
import { revokeAttachmentPreviewUrls, type AttachmentItem } from '../src/lib/attachment-service';

/**
 * 回归测试：会话发起 composer 的未提交草稿（正文 + 附件）在离开
 * 会话主页再返回后必须保留。
 *
 * 真实场景里，跳转运行模式管理、设置页或其他会话窗口都会卸载
 * ConversationComposer，但其草稿已上提为 App 层 owner 状态，存活期独立于
 * 组件挂载。这里覆盖驱动该状态的纯函数 reducer 语义，以及图片预览 URL
 * 只在 owner 清理路径释放的资源语义。
 */
function makeAttachment(id: string): AttachmentItem {
  return { id, name: `${id}.txt`, size: 1, mime: 'text/plain', source: 'dialog' };
}

function makeImageAttachment(id: string): AttachmentItem {
  return {
    id,
    name: `${id}.png`,
    size: 1,
    mime: 'image/png',
    source: 'browser-file',
    previewUrl: `blob:${id}`,
  };
}

describe('ConversationComposer draft cross-page retention', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('initial draft is empty', () => {
    expect(createInitialConversationComposerDraft()).toEqual({ content: '', attachments: [], multica: null });
  });

  it('setContent stores text without losing attachments', () => {
    const state: ConversationComposerDraftState = { content: '', attachments: [makeAttachment('a1')], multica: null };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'hello' });
    expect(next.content).toBe('hello');
    expect(next.attachments).toHaveLength(1);
  });

  it('setAttachments stores attachments without losing text', () => {
    const state: ConversationComposerDraftState = { content: 'hello', attachments: [], multica: null };
    const next = conversationComposerDraftReducer(state, { type: 'setAttachments', attachments: [makeAttachment('a1'), makeAttachment('a2')] });
    expect(next.content).toBe('hello');
    expect(next.attachments.map((a) => a.id)).toEqual(['a1', 'a2']);
  });

  it('setContent with identical value is a no-op (stable reference)', () => {
    const state: ConversationComposerDraftState = { content: 'same', attachments: [], multica: null };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'same' });
    expect(next).toBe(state);
  });

  /**
   * 模拟离开会话主页再返回：owner 状态本身不随 composer 卸载而改变，
   * 因此 reducer 在两次 setContent 之间不需要任何中间清理即可保留正文。
   */
  it('retains content across a simulated unmount/remount (owner state persists)', () => {
    let state = createInitialConversationComposerDraft();
    // 用户输入正文，尚未发送
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '未发送的草稿' });
    // 模拟组件卸载（跳转配置/设置/其他会话）后再挂载：owner 状态不受影响，content 仍在
    expect(state.content).toBe('未发送的草稿');
    // 返回后继续编辑
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '未发送的草稿，继续' });
    expect(state.content).toBe('未发送的草稿，继续');
  });

  it('retains attachments across a simulated unmount/remount', () => {
    let state = createInitialConversationComposerDraft();
    state = conversationComposerDraftReducer(state, { type: 'setAttachments', attachments: [makeAttachment('img')] });
    // 模拟跳转再返回：附件仍在
    expect(state.attachments).toHaveLength(1);
    expect(state.attachments[0].id).toBe('img');
  });

  it('does not revoke image preview URLs during ordinary cross-page retention', () => {
    const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const attachment = makeImageAttachment('img');
    let state: ConversationComposerDraftState = { content: 'x', attachments: [attachment], multica: null };

    state = conversationComposerDraftReducer(state, { type: 'setContent', content: 'x after navigation' });

    expect(state.attachments[0].previewUrl).toBe('blob:img');
    expect(revokeSpy).not.toHaveBeenCalled();
  });

  it('releases preview URLs through the owner cleanup helper', () => {
    const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});

    revokeAttachmentPreviewUrls([makeAttachment('plain'), makeImageAttachment('img')]);

    expect(revokeSpy).toHaveBeenCalledTimes(1);
    expect(revokeSpy).toHaveBeenCalledWith('blob:img');
  });

  it('reset clears content and attachments (used on successful create / workspace switch)', () => {
    let state: ConversationComposerDraftState = { content: 'x', attachments: [makeAttachment('a1')], multica: null };
    state = conversationComposerDraftReducer(state, { type: 'reset' });
    expect(state).toEqual({ content: '', attachments: [], multica: null });
  });

  it('prefill writes requirement text + multica binding and drops prior attachments', () => {
    const state: ConversationComposerDraftState = { content: '本地草稿', attachments: [makeAttachment('a1')], multica: null };
    const binding: ConversationComposerMulticaBinding = { remoteTaskId: 'rt-1', workspaceId: 'ws-1', localProjectId: 'p-1' };
    const next = conversationComposerDraftReducer(state, { type: 'prefill', content: '远程任务需求正文', multica: binding });

    expect(next.content).toBe('远程任务需求正文');
    // 远程任务草稿不复用上一条本地草稿的附件。
    expect(next.attachments).toEqual([]);
    expect(next.multica).toEqual(binding);
  });

  it('editing prefilled content keeps the multica binding (setContent preserves multica)', () => {
    const binding: ConversationComposerMulticaBinding = { remoteTaskId: 'rt-1', workspaceId: 'ws-1', localProjectId: 'p-1' };
    let state = conversationComposerDraftReducer(
      { content: '需求', attachments: [], multica: null },
      { type: 'prefill', content: '需求', multica: binding },
    );
    // 用户在预填基础上微调正文。
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '需求（改）' });

    expect(state.content).toBe('需求（改）');
    // 绑定仍在 → 发送时仍走 multica 分支。
    expect(state.multica).toEqual(binding);
  });

  it('reset clears a leftover multica binding so the next local compose is not mistaken for a remote run', () => {
    const binding: ConversationComposerMulticaBinding = { remoteTaskId: 'rt-1', workspaceId: 'ws-1', localProjectId: 'p-1' };
    let state: ConversationComposerDraftState = { content: '需求', attachments: [], multica: binding };
    state = conversationComposerDraftReducer(state, { type: 'reset' });

    expect(state.multica).toBeNull();
    expect(state.content).toBe('');
  });

  it('exposes only reset to the App boundary handle', () => {
    const reset = vi.fn();
    const handle = createConversationComposerDraftBoundaryHandle({
      draft: createInitialConversationComposerDraft(),
      setContent: vi.fn(),
      setAttachments: vi.fn(),
      prefill: vi.fn(),
      reset,
    });

    expect(Object.keys(handle)).toEqual(['reset']);

    resetConversationComposerDraft(handle);
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('ignores missing boundary handles when App reset races with unmount', () => {
    expect(() => resetConversationComposerDraft(null)).not.toThrow();
    expect(() => resetConversationComposerDraft(undefined)).not.toThrow();
  });
});
