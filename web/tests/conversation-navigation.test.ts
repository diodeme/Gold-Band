import { describe, expect, it } from 'vitest';
import {
  conversationPageMatchesRun,
  resolvePresentedConversationPage,
  shouldCommitConversationNavigation,
} from '@/lib/conversation-navigation';
import type { ConversationPage, ConversationRunVm } from '@/types';

const oldRun = {
  projectId: 'project-1',
  taskId: 'task-old',
  runId: 'run-1',
} as ConversationRunVm;

describe('conversation navigation presentation transaction', () => {
  it('keeps the complete old conversation presented while a different target is loading', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-new',
      runId: 'run-2',
    };
    expect(resolvePresentedConversationPage(requested, oldRun)).toEqual({
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-old',
      runId: 'run-1',
    });
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
    expect(resolvePresentedConversationPage(requested, targetRun)).toBe(requested);
    expect(conversationPageMatchesRun(requested, { ...targetRun, projectId: 'project-2' })).toBe(false);
  });

  it('switches non-conversation destinations immediately', () => {
    const requested: ConversationPage = { kind: 'conversation-home' };
    expect(resolvePresentedConversationPage(requested, oldRun)).toBe(requested);
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
