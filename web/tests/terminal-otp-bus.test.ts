/** @vitest-environment jsdom */
// 口令输入桥（连接→对话）回归：
// 1) authPrompt 事件先于订阅（webview 重载 / 错过事件）时，状态 reconcile 必须
//    恢复输入请求——AwaitingAuth 是 120s 的持续状态，不能只靠瞬时事件；
// 2) 用户主动关闭后 reconcile 不再弹出，新的 authPrompt 事件重新给出；
// 3) 会话终结（failed/exited/状态离开 AwaitingAuth）后请求消失。
import { describe, expect, it, beforeEach } from 'vitest';

import {
  dismissTerminalOtpRequest,
  getTerminalOtpRequests,
  reconcileTerminalOtpRequests,
  resetTerminalOtpRequestsForTests,
  resolveTerminalOtpRequest,
  upsertTerminalOtpRequest,
} from '@/lib/terminal-otp-bus';
import type { TerminalSessionVm } from '@/types';

function awaitingAuthSession(sessionId: string, runId?: string): TerminalSessionVm {
  return {
    sessionId,
    kind: 'remote',
    label: `webank (${sessionId})`,
    status: 'AwaitingAuth',
    origin: runId ? { projectId: 'p1', taskId: 't1', runId } : null,
  };
}

describe('terminal otp bus', () => {
  beforeEach(() => {
    resetTerminalOtpRequestsForTests();
  });

  it('recovers a missed authPrompt from session status reconcile', () => {
    reconcileTerminalOtpRequests([awaitingAuthSession('sess-1', 'run-1')]);
    const requests = getTerminalOtpRequests();
    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({
      sessionId: 'sess-1',
      origin: { projectId: 'p1', taskId: 't1', runId: 'run-1' },
      label: 'webank (sess-1)',
    });
  });

  it('keeps event-provided message and label across reconcile', () => {
    upsertTerminalOtpRequest({ sessionId: 'sess-1', message: 'OTP:', label: 'webank', origin: null });
    reconcileTerminalOtpRequests([awaitingAuthSession('sess-1')]);
    expect(getTerminalOtpRequests()[0]).toMatchObject({ message: 'OTP:', label: 'webank' });
  });

  it('drops requests once the session leaves AwaitingAuth', () => {
    upsertTerminalOtpRequest({ sessionId: 'sess-1', message: '', origin: null });
    reconcileTerminalOtpRequests([{ ...awaitingAuthSession('sess-1'), status: 'Connected' }]);
    expect(getTerminalOtpRequests()).toHaveLength(0);
  });

  it('hides a dismissed session from reconcile but re-shows on a new authPrompt', () => {
    upsertTerminalOtpRequest({ sessionId: 'sess-1', message: '', origin: null });
    dismissTerminalOtpRequest('sess-1');
    reconcileTerminalOtpRequests([awaitingAuthSession('sess-1')]);
    expect(getTerminalOtpRequests()).toHaveLength(0);
    upsertTerminalOtpRequest({ sessionId: 'sess-1', message: 'again', origin: null });
    expect(getTerminalOtpRequests()).toHaveLength(1);
  });

  it('resolve clears the request', () => {
    upsertTerminalOtpRequest({ sessionId: 'sess-2', message: '', origin: null });
    resolveTerminalOtpRequest('sess-2');
    expect(getTerminalOtpRequests()).toHaveLength(0);
  });
});
