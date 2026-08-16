import { describe, expect, it } from 'vitest';
import {
  beginConversationSessionSelection,
  conversationPageForIntervention,
  conversationPageMatchesRun,
  isConversationRunNavigationLoading,
  shouldCommitConversationNavigation,
} from '@/lib/conversation-navigation';
import type { ConversationPage, ConversationRunVm } from '@/types';
import fs from 'node:fs';
import path from 'node:path';

const oldRun = {
  projectId: 'project-1',
  taskId: 'task-old',
  runId: 'run-1',
} as ConversationRunVm;

describe('conversation navigation presentation transaction', () => {
  it('keeps project identity when local task and run ids collide across workspaces', () => {
    const page = conversationPageForIntervention({
      targetType: 'conversation',
      projectId: 'project-b',
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'direct-agent',
      attemptId: 'attempt-001',
      dedupKey: 'project-b:run-001:round-001:direct-agent:attempt-001:turn-1',
    });

    expect(page).toEqual({
      kind: 'conversation-run',
      projectId: 'project-b',
      taskId: 'task-001',
      runId: 'run-001',
    });
    expect(conversationPageMatchesRun(page, {
      ...oldRun,
      projectId: 'project-a',
      taskId: 'task-001',
      runId: 'run-001',
    })).toBe(false);
  });

  it('marks a different requested conversation as loading without presenting the old run', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-new',
      runId: 'run-2',
    };
    expect(isConversationRunNavigationLoading(requested, oldRun)).toBe(true);
  });

  it('commits the target page only when the full project/task/run identity matches', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-new',
      runId: 'run-2',
    };
    const targetRun = { ...oldRun, taskId: 'task-new', runId: 'run-2' } as ConversationRunVm;
    expect(conversationPageMatchesRun(requested, targetRun)).toBe(true);
    expect(isConversationRunNavigationLoading(requested, targetRun)).toBe(false);
    expect(conversationPageMatchesRun(requested, { ...targetRun, projectId: 'project-2' })).toBe(false);
  });

  it('switches non-conversation destinations immediately', () => {
    const requested: ConversationPage = { kind: 'conversation-home' };
    expect(isConversationRunNavigationLoading(requested, oldRun)).toBe(false);
  });

  it('commits a selected session locator immediately and clears stale session content', () => {
    const selectedSession = { sessionId: 'old-session' } as ConversationRunVm['selectedSession'];
    const run = {
      ...oldRun,
      selectedSession,
      sessionTree: { rounds: [], selectedSessionKey: 'round-1/node-1/attempt-1' },
    } as ConversationRunVm;

    const next = beginConversationSessionSelection(run, 'round-2/node-2/attempt-2');

    expect(next.sessionTree.selectedSessionKey).toBe('round-2/node-2/attempt-2');
    expect(next.selectedSession).toBeNull();
    expect(run.selectedSession).toBe(selectedSession);
  });

  it('keeps loaded content when the selected session locator does not change', () => {
    const run = {
      ...oldRun,
      selectedSession: { sessionId: 'current-session' },
      sessionTree: { rounds: [], selectedSessionKey: 'round-1/node-1/attempt-1' },
    } as ConversationRunVm;

    expect(beginConversationSessionSelection(run, 'round-1/node-1/attempt-1')).toBe(run);
  });

  it('rejects a slower stale response after a newer navigation request starts', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-new',
      runId: 'run-2',
    };
    const targetRun = { ...oldRun, taskId: 'task-new', runId: 'run-2' } as ConversationRunVm;
    expect(shouldCommitConversationNavigation(1, 2, requested, targetRun)).toBe(false);
    expect(shouldCommitConversationNavigation(2, 2, requested, targetRun)).toBe(true);
  });
});

describe('conversation sidebar navigation wiring', () => {
  it('routes task and run selections through the cache-aware conversation navigation entry', () => {
    const source = fs.readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');
    const taskSelection = source.match(/onConversationSelectTask=\{[\s\S]*?onConversationSelectRun=/)?.[0] ?? '';
    const runSelection = source.match(/onConversationSelectRun=\{[\s\S]*?onConversationPauseRun=/)?.[0] ?? '';

    expect(taskSelection).toContain('onSelectConversation({');
    expect(runSelection).toContain('onSelectConversation({');
    expect(taskSelection).not.toContain('setConversationPage({ kind: \'conversation-run\'');
    expect(runSelection).not.toContain('setConversationPage({ kind: \'conversation-run\'');
  });
});
