import { describe, expect, it } from 'vitest';

import { conversationSidebarTerminalResultDotClass } from '@/components/conversation/ConversationSidebar';
import { conversationTerminalResultAcknowledgementTarget } from '@/lib/conversation-navigation';
import {
  applyConversationSidebarTerminalResultAcknowledgement,
  applyConversationSidebarTerminalResultUpdate,
} from '@/lib/conversation-sidebar-activity';
import type {
  ConversationPage,
  ConversationSidebarVm,
  ConversationTaskRowVm,
  ConversationTerminalResultVm,
} from '@/types';

function terminalResult(eventId: string, kind: ConversationTerminalResultVm['kind'] = 'completed') {
  return {
    eventId,
    runId: 'run-001',
    kind,
    occurredAt: '2026-08-18T10:00:00Z',
  } satisfies ConversationTerminalResultVm;
}

function task(unreadTerminalResult: ConversationTerminalResultVm | null): ConversationTaskRowVm {
  return {
    projectId: 'project-001',
    taskId: 'task-001',
    title: 'Direct conversation',
    autoTitle: false,
    runMode: 'direct',
    agentIdentity: { agentType: 'codex', displayName: 'Codex', iconKey: 'codex' },
    unreadTerminalResult,
    latestRun: {
      runId: 'run-001',
      status: 'completed',
      outcome: 'success',
      startedAt: '2026-08-18T09:00:00Z',
      updatedAt: '2026-08-18T10:00:00Z',
      resumable: false,
    },
    runs: [],
    pinned: true,
    pinnedOrder: 0,
  };
}

function sidebar(unreadTerminalResult: ConversationTerminalResultVm | null): ConversationSidebarVm {
  const row = task(unreadTerminalResult);
  return {
    workspaces: [{ projectId: 'project-001', workspacePath: 'D:/project', name: 'Project' }],
    pinnedTasks: [row],
    tasksByWorkspace: { 'project-001': [{ ...row }] },
  };
}

describe('Direct unread terminal result', () => {
  it('projects a terminal update into pinned and workspace task rows', () => {
    const result = terminalResult('event-001', 'failed');
    const next = applyConversationSidebarTerminalResultUpdate(sidebar(null), {
      projectId: 'project-001',
      taskId: 'task-001',
      unreadTerminalResult: result,
    });

    expect(next.pinnedTasks[0]?.unreadTerminalResult).toEqual(result);
    expect(next.tasksByWorkspace['project-001']?.[0]?.unreadTerminalResult).toEqual(result);
  });

  it('clears only the matching event and preserves a newer result against a stale acknowledgement', () => {
    const current = sidebar(terminalResult('event-002', 'failed'));
    expect(applyConversationSidebarTerminalResultAcknowledgement(
      current,
      'project-001',
      'task-001',
      'event-001',
      null,
    )).toBe(current);

    const cleared = applyConversationSidebarTerminalResultAcknowledgement(
      current,
      'project-001',
      'task-001',
      'event-002',
      null,
    );
    expect(cleared.pinnedTasks[0]?.unreadTerminalResult).toBeNull();
    expect(cleared.tasksByWorkspace['project-001']?.[0]?.unreadTerminalResult).toBeNull();
  });

  it('acknowledges only after the exact run is the presented navigation target', () => {
    const current = sidebar(terminalResult('event-001'));
    const page: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-001',
      taskId: 'task-001',
      runId: 'run-001',
    };
    expect(conversationTerminalResultAcknowledgementTarget(current, page, {
      projectId: 'project-001',
      taskId: 'task-001',
      runId: 'run-001',
    })).toEqual({
      projectId: 'project-001',
      taskId: 'task-001',
      runId: 'run-001',
      eventId: 'event-001',
    });
    expect(conversationTerminalResultAcknowledgementTarget(current, page, {
      projectId: 'project-001',
      taskId: 'task-001',
      runId: 'run-older',
    })).toBeNull();
  });

  it('uses semantic theme tokens for completed, stopped, and failed dots', () => {
    expect(conversationSidebarTerminalResultDotClass('completed')).toBe('bg-gold-success');
    expect(conversationSidebarTerminalResultDotClass('stopped')).toBe('bg-gold-warning');
    expect(conversationSidebarTerminalResultDotClass('failed')).toBe('bg-gold-danger');
  });
});
