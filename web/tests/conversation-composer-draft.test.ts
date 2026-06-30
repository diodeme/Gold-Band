import { describe, expect, it } from 'vitest';
import {
  conversationComposerDraftReducer,
  createInitialConversationComposerDraft,
  type ConversationComposerDraftState,
} from '../src/lib/conversation-composer-draft';
import type { AttachmentItem } from '../src/lib/attachment-service';

/**
 * 回归测试：会话发起 composer 的未提交草稿（正文 + 附件）在跳转去配置 /
 * 修改 AUTO / 修改工作流再返回后必须保留。
 *
 * 真实场景里，conversationPage.kind 从 conversation-home 切到 run-mode-management
 * 会卸载 ConversationComposer，但其草稿已上提为 App 层 owner 状态，存活期独立于
 * 组件挂载。这里直接覆盖驱动该状态的纯函数 reducer 语义。
 */
function makeAttachment(id: string): AttachmentItem {
  return { id, name: `${id}.txt`, size: 1, mime: 'text/plain', source: 'dialog' };
}

describe('ConversationComposer draft cross-page retention', () => {
  it('initial draft is empty', () => {
    expect(createInitialConversationComposerDraft()).toEqual({ content: '', attachments: [] });
  });

  it('setContent stores text without losing attachments', () => {
    const state: ConversationComposerDraftState = { content: '', attachments: [makeAttachment('a1')] };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'hello' });
    expect(next.content).toBe('hello');
    expect(next.attachments).toHaveLength(1);
  });

  it('setAttachments stores attachments without losing text', () => {
    const state: ConversationComposerDraftState = { content: 'hello', attachments: [] };
    const next = conversationComposerDraftReducer(state, { type: 'setAttachments', attachments: [makeAttachment('a1'), makeAttachment('a2')] });
    expect(next.content).toBe('hello');
    expect(next.attachments.map((a) => a.id)).toEqual(['a1', 'a2']);
  });

  it('setContent with identical value is a no-op (stable reference)', () => {
    const state: ConversationComposerDraftState = { content: 'same', attachments: [] };
    const next = conversationComposerDraftReducer(state, { type: 'setContent', content: 'same' });
    expect(next).toBe(state);
  });

  /**
   * 模拟跳转 run-mode-management 再返回：owner 状态本身不随 composer 卸载而改变，
   * 因此 reducer 在两次 setContent 之间不需要任何中间清理即可保留正文。
   */
  it('retains content across a simulated unmount/remount (owner state persists)', () => {
    let state = createInitialConversationComposerDraft();
    // 用户输入正文，尚未发送
    state = conversationComposerDraftReducer(state, { type: 'setContent', content: '未发送的草稿' });
    // 模拟组件卸载（跳转配置页）后再挂载：owner 状态不受影响，content 仍在
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

  it('reset clears content and attachments (used on successful create / workspace switch)', () => {
    let state: ConversationComposerDraftState = { content: 'x', attachments: [makeAttachment('a1')] };
    state = conversationComposerDraftReducer(state, { type: 'reset' });
    expect(state).toEqual({ content: '', attachments: [] });
  });
});