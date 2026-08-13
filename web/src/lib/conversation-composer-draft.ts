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
/**
 * multica 远程任务「点击执行」的 prepare 绑定（claim-at-send）。
 *
 * 点远程任务执行按钮时**只读取**需求正文（get_multica_task_requirement，任务仍 queued），写入 draft
 * 预填 composer，同时记下这份绑定；composer 复用本地『+』页（选模型/模式 → 发送）。发送时若 draft 仍带
 * 这份绑定，则走 `start_multica_conversation_run`（claim+start + 复用本地发送链 + 叠加 multica 簿记），
 * 否则走本地 `create_conversation_run`。删 chip 只清这份绑定、不触碰服务端（任务仍 queued，可再次点执行）。
 * 草稿 reset（发送成功）即清掉绑定，无需各 reset 点单独清理——这是把 multica 绑定纳入 draft 生命周期的根本收益。
 */
export interface ConversationComposerMulticaBinding {
  /// multica remote task id（start_multica_conversation_run 寻址）。
  remoteTaskId: string;
  /// multica workspace id（start_multica_conversation_run 寻址）。
  workspaceId: string;
  /// 任务标题，仅用于 composer 绑定 chip 的展示（不参与发送寻址）。
  title: string;
}

export interface ConversationComposerDraftState {
  content: string;
  attachments: AttachmentItem[];
  /// 当前 prepare 中的 multica 远程任务绑定（null = 普通本地新建会话，走 create_conversation_run）。
  multica: ConversationComposerMulticaBinding | null;
}

export function createInitialConversationComposerDraft(): ConversationComposerDraftState {
  return { content: '', attachments: [], multica: null };
}

/**
 * 纯函数草稿状态机。由 owner hook 驱动，单独导出便于单元测试覆盖跨页面保留语义，
 * 避免依赖 DOM 测试环境。
 */
export type ConversationComposerDraftAction =
  | { type: 'setContent'; content: string }
  | { type: 'setAttachments'; attachments: AttachmentItem[] }
  | { type: 'prefill'; content: string; multica: ConversationComposerMulticaBinding }
  | { type: 'clearMultica' }
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
    case 'prefill':
      // 远程任务 prepare：正文预填 + 绑定 multica，并清空既有附件（新的远程任务草稿，不复用上一条本地草稿的附件）。
      return { content: action.content, attachments: [], multica: action.multica };
    case 'clearMultica':
      // 解除 multica 绑定但保留正文与附件：用户删掉绑定 chip 后，草稿降级为普通本地会话（发送走 create_conversation_run）。
      return state.multica === null ? state : { ...state, multica: null };
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
  /// 远程任务点击执行后预填：写正文 + 绑定 multica，清空既有附件。仅在 draft boundary 内可用。
  prefill: (content: string, multica: ConversationComposerMulticaBinding) => void;
  /// 解除 multica 绑定（保留正文与附件）。claim-at-send 下删 chip 纯属本地解绑——任务未被领取（仍 queued），无需通知服务端。
  clearMultica: () => void;
  reset: () => void;
}

export interface ConversationComposerDraftBoundaryHandle {
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

export function createConversationComposerDraftBoundaryHandle(
  owner: ConversationComposerDraftContextValue,
): ConversationComposerDraftBoundaryHandle {
  return { reset: owner.reset };
}

export function resetConversationComposerDraft(
  handle: ConversationComposerDraftBoundaryHandle | null | undefined,
) {
  handle?.reset();
}

/**
 * 管理首页 composer 草稿的 owner hook。由局部 boundary 调用一次，
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

  const prefill = useCallback(
    (content: string, multica: ConversationComposerMulticaBinding) => {
      setDraft((prev) => {
        // 覆盖式预填：释放上一份附件的预览 URL（与 reset 一致），再写入新草稿 + multica 绑定。
        revokeAttachmentPreviewUrls(prev.attachments);
        return conversationComposerDraftReducer(prev, { type: 'prefill', content, multica });
      });
    },
    [],
  );

  const reset = useCallback(() => {
    setDraft((prev) => {
      revokeAttachmentPreviewUrls(prev.attachments);
      return conversationComposerDraftReducer(prev, { type: 'reset' });
    });
  }, []);

  const clearMultica = useCallback(() => {
    setDraft((prev) => conversationComposerDraftReducer(prev, { type: 'clearMultica' }));
  }, []);

  return useMemo(
    () => ({ draft, setContent, setAttachments, prefill, clearMultica, reset }),
    [draft, setContent, setAttachments, prefill, clearMultica, reset],
  );
}
