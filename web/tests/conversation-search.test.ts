import { describe, expect, it } from 'vitest';
import {
  conversationPageForSearchResult,
  conversationSearchHighlightSegments,
} from '@/lib/conversation-search';

describe('conversation search navigation', () => {
  it('opens the latest run returned by the search interface', () => {
    expect(conversationPageForSearchResult({
      projectId: 'project-a',
      workspacePath: 'D:/workspace-a',
      workspaceName: 'Workspace A',
      taskId: 'task-001',
      title: 'Searchable conversation',
      requirementPreview: 'find a file',
      matchPreview: 'find a file',
      latestRun: {
        runId: 'run-003',
        status: 'completed',
        startedAt: '2026-07-24T00:00:00Z',
        updatedAt: '2026-07-24T00:01:00Z',
        resumable: false,
      },
      runMode: 'direct',
    })).toEqual({
      kind: 'conversation-run',
      projectId: 'project-a',
      taskId: 'task-001',
      runId: 'run-003',
    });
  });

  it('does not create an invalid route without a run', () => {
    expect(conversationPageForSearchResult({
      projectId: 'project-a',
      workspacePath: 'D:/workspace-a',
      workspaceName: 'Workspace A',
      taskId: 'task-001',
      title: 'Searchable conversation',
      requirementPreview: 'find a file',
      matchPreview: 'find a file',
      latestRun: null,
      runMode: 'workflow',
    })).toBeNull();
  });
});

describe('conversation search match highlighting', () => {
  it('highlights the keyword inside the backend-provided match preview', () => {
    expect(conversationSearchHighlightSegments(
      '随便用askUserQuestion工具问我几个问题',
      '问题',
    )).toEqual([
      { text: '随便用askUserQuestion工具问我几个', highlighted: false },
      { text: '问题', highlighted: true },
    ]);
  });

  it('treats multiple keywords as literal case-insensitive text', () => {
    expect(conversationSearchHighlightSegments(
      'AskUserQuestion 处理 [问题]',
      'askUser [问题]',
    )).toEqual([
      { text: 'AskUser', highlighted: true },
      { text: 'Question 处理 ', highlighted: false },
      { text: '[问题]', highlighted: true },
    ]);
  });
});
