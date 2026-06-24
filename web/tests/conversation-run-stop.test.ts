import { describe, expect, it } from 'vitest';
import i18n from '@/i18n';
import { isConversationRunStopSettled } from '@/lib/conversation-run-stop';
import type { ConversationRunVm } from '@/types';

function run(overrides: Partial<ConversationRunVm> = {}): ConversationRunVm {
  return {
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    title: 'Task',
    autoTitle: true,
    runMode: 'workflow',
    workflowTemplateId: null,
    runStatus: 'running',
    runOutcome: null,
    sessionTree: { selectedSessionKey: null, rounds: [] },
    selectedSession: { sessionId: 'session-1', status: 'running', events: [] } as any,
    activeSessions: [],
    artifacts: [],
    attachments: [],
    inputAttachments: [],
    workflowStatus: 'valid',
    workflowValid: true,
    workflowError: null,
    workflowJson: null,
    workflowGraph: { nodes: [], edges: [] },
    resumable: false,
    pauseReason: null,
    ...overrides,
  };
}

describe('conversation run stop state', () => {
  it('keeps the overlay while the run is still running', () => {
    expect(isConversationRunStopSettled(run({ runStatus: 'running' }))).toBe(false);
  });

  it('keeps the overlay while paused run still has active sessions', () => {
    expect(isConversationRunStopSettled(run({
      runStatus: 'paused',
      activeSessions: [{ roundId: 'round-1', nodeId: 'node-1', attemptId: 'attempt-1', pathLabel: 'node-1', status: 'running', runtimeDisplay: {} as any, manualCheckPending: false }],
    }))).toBe(false);
  });

  it('keeps the overlay until the selected ACP session is terminal', () => {
    expect(isConversationRunStopSettled(run({ runStatus: 'paused', selectedSession: { sessionId: 'session-1', status: 'running', events: [] } as any }))).toBe(false);
    expect(isConversationRunStopSettled(run({ runStatus: 'paused', selectedSession: { sessionId: 'session-1', status: 'cancelled', events: [] } as any }))).toBe(true);
  });

  it('resolves the run stop overlay copy from the conversation runtime namespace', () => {
    expect(i18n.t('conversation.runtime.stoppingRunOverlay', { lng: 'zh-CN' })).toBe('正在停止当前运行…');
    expect(i18n.t('conversation.runtime.stoppingRunOverlay', { lng: 'en' })).toBe('Stopping current run…');
  });
});
