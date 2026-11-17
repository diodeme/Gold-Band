/**
 * 口令输入请求桥（连接→对话）。
 *
 * SSH keyboard-interactive 的口令等待是后端 120s 的持续状态，而 `authPrompt`
 * 是一次性事件——webview 重载、订阅未就绪或切换页面都会错过它，此后界面上
 * 任何输入框都不再出现（现场事故）。因此请求不能只活在组件状态里：这里用
 * 模块级 store 承接「会话驱动的连接 → 会话内输入框」的路由，事件只负责写入，
 * 状态（listTerminalSessions 的 AwaitingAuth 视图）负责 reconcile 兜底。
 *
 * 写入方只有 App 壳层的单一 terminal 事件订阅；读取方两处：
 * - ACPChatDialog：origin 匹配自身 run 时在 composer 上方渲染口令输入卡片；
 * - App 壳层弹窗：手动连接（无 origin）或发起对话不在屏幕上时兜底弹出。
 */

import { useSyncExternalStore } from 'react';
import type { ConnectionRunLocatorVm, TerminalSessionVm } from '@/types';

export interface TerminalOtpRequest {
  sessionId: string;
  message: string;
  /** 连接名（标题展示），来自会话视图解析，缺失时回退通用标题。 */
  label?: string | null;
  /** 发起会话的对话 run；手动从终端面板打开的连接没有它。 */
  origin?: ConnectionRunLocatorVm | null;
}

type Listener = () => void;

let requests: TerminalOtpRequest[] = [];
/** 用户主动关闭过的会话：状态 reconcile 不再弹出，新的 authPrompt 事件会清除。 */
const dismissed = new Set<string>();
const listeners = new Set<Listener>();

function emitChange(): void {
  for (const listener of listeners) listener();
}

function setRequests(next: TerminalOtpRequest[]): void {
  // 快照必须换引用：useSyncExternalStore 以 Object.is 判断快照是否变化。
  requests = next;
  emitChange();
}

export function getTerminalOtpRequests(): TerminalOtpRequest[] {
  return requests;
}

export function upsertTerminalOtpRequest(request: TerminalOtpRequest): void {
  dismissed.delete(request.sessionId);
  setRequests([
    request,
    ...requests.filter((existing) => existing.sessionId !== request.sessionId),
  ]);
}

export function resolveTerminalOtpRequest(sessionId: string): void {
  if (!requests.some((existing) => existing.sessionId === sessionId)) return;
  setRequests(requests.filter((existing) => existing.sessionId !== sessionId));
}

/** 用户主动放弃本次输入：只隐藏界面，会话仍在等待并将按超时失败。 */
export function dismissTerminalOtpRequest(sessionId: string): void {
  dismissed.add(sessionId);
  resolveTerminalOtpRequest(sessionId);
}

/**
 * 状态兜底：以后端会话视图为准收敛请求集合——只保留仍在 AwaitingAuth 的会话。
 * webview 重载 / 错过事件后调用它即可恢复输入框；事件照常 upsert（加速器）。
 */
export function reconcileTerminalOtpRequests(
  sessions: TerminalSessionVm[],
  labelFor?: (session: TerminalSessionVm) => string | null | undefined,
): void {
  const pending = sessions.filter(
    (session) => session.kind === 'remote' && session.status === 'AwaitingAuth',
  );
  const next = pending
    .filter((session) => !dismissed.has(session.sessionId))
    .map((session) => {
      const known = requests.find((existing) => existing.sessionId === session.sessionId);
      return {
        sessionId: session.sessionId,
        message: known?.message ?? '',
        label: known?.label ?? labelFor?.(session) ?? session.label ?? null,
        origin: session.origin ?? known?.origin ?? null,
      };
    });
  const changed = next.length !== requests.length
    || next.some((request, index) => {
      const known = requests[index];
      return !known
        || known.sessionId !== request.sessionId
        || known.message !== request.message
        || (known.label ?? null) !== (request.label ?? null)
        || JSON.stringify(known.origin ?? null) !== JSON.stringify(request.origin ?? null);
    });
  if (changed) setRequests(next);
}

export function onTerminalOtpRequestsChange(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 测试专用：模块级 store 的状态复位。 */
export function resetTerminalOtpRequestsForTests(): void {
  requests = [];
  dismissed.clear();
}

/** 订阅请求集合的 React hook（读侧唯一入口）。 */
export function useTerminalOtpRequests(): TerminalOtpRequest[] {
  return useSyncExternalStore(onTerminalOtpRequestsChange, getTerminalOtpRequests, getTerminalOtpRequests);
}
