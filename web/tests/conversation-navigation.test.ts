import { describe, expect, it } from 'vitest';
import {
  beginConversationSessionSelection,
  conversationPageForSession,
  conversationPageForIntervention,
  conversationPageMatchesRun,
  findConversationLeafForPage,
  isConversationRunNavigationLoading,
  resolveConversationHomeWorkspaceId,
  shouldCommitConversationNavigation,
} from '@/lib/conversation-navigation';
import type { ConversationPage, ConversationRunVm, ConversationSessionTreeVm } from '@/types';
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

  it('keeps the quick-chat draft workspace when returning from settings', () => {
    expect(resolveConversationHomeWorkspaceId(
      { kind: 'settings' },
      'project-draft',
      'project-last-session',
    )).toBe('project-draft');
  });

  it('keeps the quick-chat draft workspace when leaving a conversation', () => {
    expect(resolveConversationHomeWorkspaceId(
      { kind: 'conversation-run', projectId: 'project-run', taskId: 'task-1', runId: 'run-1' },
      'project-draft',
      'project-last-session',
    )).toBe('project-draft');
  });

  it('falls back to the current run workspace when no quick-chat draft exists', () => {
    expect(resolveConversationHomeWorkspaceId(
      { kind: 'conversation-run', projectId: 'project-run', taskId: 'task-1', runId: 'run-1' },
      null,
      'project-last-session',
    )).toBe('project-run');
  });

  it('falls back to the last session workspace when no quick-chat draft exists', () => {
    expect(resolveConversationHomeWorkspaceId(
      { kind: 'settings' },
      null,
      'project-last-session',
    )).toBe('project-last-session');
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

  it('builds and resolves a fully qualified dynamic attempt locator', () => {
    const target = {
      roundId: 'round-001',
      nodeId: 'review',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-002',
    };
    const page = conversationPageForSession(oldRun, target);
    const dynamicLeaf = { ...target, pathLabel: 'review/attempt-001' };
    const sameAttemptIdOnTopLevel = {
      roundId: 'round-001',
      nodeId: 'plan',
      attemptId: 'attempt-001',
      pathLabel: 'plan/attempt-001',
    };
    const tree = {
      selectedSessionKey: null,
      rounds: [{
        roundId: 'round-001',
        nodes: [{
          attempts: [sameAttemptIdOnTopLevel],
          outerNodes: [{ attempts: [dynamicLeaf] }],
        }],
      }],
    } as ConversationSessionTreeVm;

    expect(page).toMatchObject(target);
    expect(findConversationLeafForPage(tree, page)).toBe(dynamicLeaf);
  });
});

describe('conversation sidebar navigation wiring', () => {
  it('routes run exits plus task and run selections through the cache-aware conversation navigation entry', () => {
    const source = fs.readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');
    const quickChatSelection = source.match(/onConversationNew=\{[\s\S]*?onConversationSearch=/)?.[0] ?? '';
    const workspaceQuickChatSelection = source.match(/onConversationNewInWorkspace=\{[\s\S]*?onConversationAddWorkspace=/)?.[0] ?? '';
    const taskSelection = source.match(/onConversationSelectTask=\{[\s\S]*?onConversationSelectRun=/)?.[0] ?? '';
    const runSelection = source.match(/onConversationSelectRun=\{[\s\S]*?onConversationPauseRun=/)?.[0] ?? '';
    const searchSelection = source.match(/<ConversationSearchDialog[\s\S]*?\/>/)?.[0] ?? '';
    const interventionNavigation = source.match(/const handleInterventionNavigate[\s\S]*?useInterventionNotifications/)?.[0] ?? '';

    expect(quickChatSelection).toContain("onSelectConversation({ kind: 'conversation-home' })");
    expect(quickChatSelection).toContain('resolveConversationHomeWorkspaceId(');
    expect(workspaceQuickChatSelection).toContain("onSelectConversation({ kind: 'conversation-home' })");
    expect(taskSelection).toContain('onSelectConversation({');
    expect(runSelection).toContain('onSelectConversation({');
    expect(searchSelection).toContain('onSelectConversation(page)');
    expect(interventionNavigation).toContain('onSelectConversation(page)');
    expect(interventionNavigation).toContain('onSelectConversation(runPage)');
    expect(quickChatSelection).not.toContain('setConversationPage(');
    expect(workspaceQuickChatSelection).not.toContain('setConversationPage(');
    expect(searchSelection).not.toContain('setConversationPage(');
    expect(interventionNavigation).not.toContain('setConversationPage(');
    expect(taskSelection).not.toContain('setConversationPage({ kind: \'conversation-run\'');
    expect(runSelection).not.toContain('setConversationPage({ kind: \'conversation-run\'');
  });

  it('keeps one shell-level run-state listener and refreshes only the selected run detail', () => {
    const source = fs.readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');
    const subscriptions = source.match(/void subscribeConversationRunStateUpdates\(\(event\) =>/g) ?? [];
    const globalSubscription = source.match(/void subscribeConversationRunStateUpdates\(\(event\) => \{[\s\S]*?\}\)\.then/)?.[0] ?? '';
    const selectedRunRefresh = source.match(/const refreshConversationRun = \(\) => \{[\s\S]*?const queueConversationRunRefresh/)?.[0] ?? '';
    const selectedRunStateHandler = source.match(/const refreshSelectedRunFromStateEvent[\s\S]*?conversationRunStateRefreshRef\.current = refreshSelectedRunFromStateEvent/)?.[0] ?? '';

    expect(subscriptions).toHaveLength(1);
    expect(globalSubscription).toContain('applyConversationSidebarRunStateUpdate(current, event)');
    expect(globalSubscription).toContain('conversationRunStateRefreshRef.current?.(event)');
    expect(globalSubscription).not.toContain('getConversationSidebar(');
    expect(selectedRunRefresh).toContain('getConversationRun(');
    expect(selectedRunRefresh).not.toContain('getConversationSidebar(');
    expect(selectedRunStateHandler).toContain("event.eventKind === 'node-started'");
    expect(source).toContain('canonicalRunBoundaryInFlight');
    expect(source).toContain('pendingCanonicalRunBoundary');
  });

  it('keeps one shell-level ACP listener and clears only the matching task activity', () => {
    const source = fs.readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');
    const subscriptions = source.match(/void subscribeAcpSessionUpdates\(\(event\) =>/g) ?? [];
    const globalSubscription = source.match(/void subscribeAcpSessionUpdates\(\(event\) => \{[\s\S]*?\}\)\.then/)?.[0] ?? '';
    const selectedRunHandler = source.match(/const refreshSelectedRunFromAcpEvent[\s\S]*?conversationAcpSessionRefreshRef\.current = refreshSelectedRunFromAcpEvent/)?.[0] ?? '';

    expect(subscriptions).toHaveLength(1);
    expect(globalSubscription).toContain('conversationTaskActivityFromUpdate(event)');
    expect(globalSubscription).toContain('applyConversationTaskActivity(projectId, event.taskId, sidebarActivity)');
    expect(globalSubscription).toContain('conversationAcpSessionRefreshRef.current?.(event)');
    expect(globalSubscription).not.toContain('getConversationSidebar(');
    expect(selectedRunHandler).not.toContain('applyConversationTaskActivity(');
    expect(selectedRunHandler).not.toContain('applyConversationLifecycleSnapshotToSidebar(');
  });
});
