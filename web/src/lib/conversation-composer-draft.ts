import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { revokeAttachmentPreviewUrls, type AttachmentItem } from './attachment-service';

/**
 * 首页会话发起 composer 的未提交草稿。
 *
 * 设计原因：composer 的正文与附件属于同一未提交生命周期，
 * 原先作为组件本地 useState，在离开会话主页时会随 ConversationComposer
 * 卸载而丢失。此状态上提后，普通跨页面导航、打开其他会话或设置页都
 * 不会清空草稿，与 createTaskDraft 跨页面保留同一心智。
 */
export interface ConversationComposerDraftState {
  content: string;
  attachments: AttachmentItem[];
}

export function createInitialConversationComposerDraft(): ConversationComposerDraftState {
  return { content: '', attachments: [] };
}

/**
 * 纯函数草稿状态机。由 owner hook 驱动，单独导出便于单元测试覆盖跨页面保留语义，
 * 避免依赖 DOM 测试环境。
 */
export type ConversationComposerDraftAction =
  | { type: 'setContent'; content: string }
  | { type: 'setAttachments'; attachments: AttachmentItem[] }
  | { type: 'reset' };

export function conversationComposerDraftReducer(
  state: ConversationComposerDraftState,
  action: ConversationComposerDraftAction,
): ConversationComposerDraftState {
  switch (action.type) {
    case 'setContent':
      return state.content === action.content ? state : { ...state, content: action.content };
    case 'setAttachments':
      return { ...state, attachments: action.attachments };
    case 'reset':
      return createInitialConversationComposerDraft();
    default:
      return state;
  }
}

export interface ConversationComposerDraftContextValue {
  draft: ConversationComposerDraftState;
  setContent: (content: string) => void;
  setAttachments: (
    next: AttachmentItem[] | ((prev: AttachmentItem[]) => AttachmentItem[]),
  ) => void;
  reset: () => void;
}

const ConversationComposerDraftContext = createContext<ConversationComposerDraftContextValue | null>(null);

export function useConversationComposerDraft(): ConversationComposerDraftContextValue {
  const value = useContext(ConversationComposerDraftContext);
  if (!value) {
    throw new Error('useConversationComposerDraft must be used within ConversationComposerDraftProvider');
  }
  return value;
}

export const ConversationComposerDraftProvider = ConversationComposerDraftContext.Provider;

/**
 * 管理首页 composer 草稿的 owner hook。由 App 层调用一次，
 * 产生的 context value 通过 ConversationComposerDraftProvider 下发。
 * 草稿存活期独立于 ConversationComposer 的挂载/卸载，从而在离开
 * 会话主页再返回时保留正文与附件。
 */
export function useConversationComposerDraftOwner(): ConversationComposerDraftContextValue {
  const [draft, setDraft] = useState<ConversationComposerDraftState>(() => createInitialConversationComposerDraft());
  const latestAttachmentsRef = useRef<AttachmentItem[]>(draft.attachments);

  useEffect(() => {
    latestAttachmentsRef.current = draft.attachments;
  }, [draft.attachments]);

  useEffect(() => {
    return () => {
      revokeAttachmentPreviewUrls(latestAttachmentsRef.current);
    };
  }, []);

  const setContent = useCallback((content: string) => {
    setDraft((prev) => conversationComposerDraftReducer(prev, { type: 'setContent', content }));
  }, []);

  const setAttachments = useCallback(
    (next: AttachmentItem[] | ((prev: AttachmentItem[]) => AttachmentItem[])) => {
      setDraft((prev) =>
        conversationComposerDraftReducer(prev, {
          type: 'setAttachments',
          attachments: typeof next === 'function' ? (next as (p: AttachmentItem[]) => AttachmentItem[])(prev.attachments) : next,
        }),
      );
    },
    [],
  );

  const reset = useCallback(() => {
    setDraft((prev) => {
      revokeAttachmentPreviewUrls(prev.attachments);
      return conversationComposerDraftReducer(prev, { type: 'reset' });
    });
  }, []);

  return useMemo(
    () => ({ draft, setContent, setAttachments, reset }),
    [draft, setContent, setAttachments, reset],
  );
}
