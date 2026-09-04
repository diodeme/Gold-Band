import { describe, expect, it } from 'vitest';
import {
  beginConversationSessionSelection,
  canonicalizeConversationPageIdentity,
  conversationPageForSession,
  conversationPageForIntervention,
  conversationPageMatchesRun,
  conversationPageTargetsTask,
  conversationSourceControlWorkspacePath,
  findConversationLeafForPage,
  isConversationRunNavigationLoading,
  resolveConversationHomeWorkspaceId,
  shouldCommitConversationNavigation,
  shouldSurfaceConversationNavigationError,
} from '@/lib/conversation-navigation';
import type { ConversationPage, ConversationRunVm, ConversationSessionTreeVm } from '@/types';
import fs from 'node:fs';
import path from 'node:path';

// Normalize CRLF checkouts (Windows core.autocrlf) so literal source matching
// stays portable against the LF-formatted repo content.
const readAppSource = (relativePath = 'web/src/App.tsx') =>
  fs.readFileSync(path.resolve(process.cwd(), relativePath), 'utf8').replace(/\r\n/g, '\n');

const oldRun = {
  projectId: 'project-1',
  taskId: 'task-old',
  taskUuid: 'task-uuid-old',
  runId: 'run-1',
} as ConversationRunVm;

describe('conversation navigation presentation transaction', () => {
  it('keeps the same page identity once the canonical task UUID is committed', () => {
    const page: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
    };

    expect(canonicalizeConversationPageIdentity(page, 'task-uuid-001')).toBe(page);
    expect(canonicalizeConversationPageIdentity(
      { ...page, taskUuid: undefined },
      'task-uuid-001',
    )).toEqual(page);
  });

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
      roundId: 'round-001',
      nodeId: 'direct-agent',
      attemptId: 'attempt-001',
    });
    expect(conversationPageMatchesRun(page, {
      ...oldRun,
      projectId: 'project-a',
      taskId: 'task-001',
      runId: 'run-001',
    })).toBe(false);
  });

  it('preserves canonical task and attempt identity from a notification navigation', () => {
    const page = conversationPageForIntervention({
      targetType: 'conversation',
      projectId: 'project-b',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'worker',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-002',
      dedupKey: 'permission-request-001',
    });

    expect(page).toEqual({
      kind: 'conversation-run',
      projectId: 'project-b',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'worker',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-002',
    });

    const cachedRun = {
      ...oldRun,
      projectId: page.projectId,
      taskId: page.taskId,
      taskUuid: page.taskUuid,
      runId: page.runId,
    } as ConversationRunVm;
    expect(conversationPageMatchesRun(page, cachedRun)).toBe(true);
    expect(isConversationRunNavigationLoading(page, cachedRun)).toBe(false);
    expect(shouldCommitConversationNavigation(1, 1, page, {
      ...cachedRun,
      taskUuid: 'recreated-task-uuid',
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

  it('keeps a UUID-less deep link closed until its canonical snapshot is loaded', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: oldRun.projectId,
      taskId: oldRun.taskId,
      runId: oldRun.runId,
    };

    expect(conversationPageMatchesRun(requested, oldRun)).toBe(false);
    expect(isConversationRunNavigationLoading(requested, oldRun)).toBe(true);
    expect(shouldCommitConversationNavigation(1, 1, requested, oldRun)).toBe(true);
  });

  it('commits the target page only when the full project/task/run identity matches', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-new',
      taskUuid: 'task-uuid-new',
      runId: 'run-2',
    };
    const targetRun = { ...oldRun, taskId: 'task-new', taskUuid: 'task-uuid-new', runId: 'run-2' } as ConversationRunVm;
    expect(conversationPageMatchesRun(requested, targetRun)).toBe(true);
    expect(isConversationRunNavigationLoading(requested, targetRun)).toBe(false);
    expect(conversationPageMatchesRun(requested, { ...targetRun, projectId: 'project-2' })).toBe(false);
  });

  it('rejects a stale response from a deleted task when the readable locator is reused', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'new-task-uuid',
      runId: 'run-001',
    };
    const staleRun = {
      ...oldRun,
      taskId: 'task-004',
      taskUuid: 'old-task-uuid',
      runId: 'run-001',
    } as ConversationRunVm;

    expect(conversationPageMatchesRun(requested, staleRun)).toBe(false);
    expect(shouldCommitConversationNavigation(2, 2, requested, staleRun)).toBe(false);
  });

  it('scopes deletion retirement to the canonical task entity', () => {
    const currentPage: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'current-task-uuid',
      runId: 'run-001',
    };

    expect(conversationPageTargetsTask(currentPage, {
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'current-task-uuid',
    })).toBe(true);
    expect(conversationPageTargetsTask(currentPage, {
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'recreated-task-uuid',
    })).toBe(false);
    expect(conversationPageTargetsTask({ ...currentPage, taskUuid: undefined }, {
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'current-task-uuid',
    })).toBe(true);
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
      taskUuid: 'task-uuid-new',
      runId: 'run-2',
    };
    const targetRun = { ...oldRun, taskId: 'task-new', taskUuid: 'task-uuid-new', runId: 'run-2' } as ConversationRunVm;
    expect(shouldCommitConversationNavigation(1, 2, requested, targetRun)).toBe(false);
    expect(shouldCommitConversationNavigation(2, 2, requested, targetRun)).toBe(true);
  });

  it('does not surface a retired or replaced run request as a global error', () => {
    const requested: ConversationPage = {
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'deleted-task-uuid',
      runId: 'run-001',
    };
    const recreated: ConversationPage = {
      ...requested,
      taskUuid: 'recreated-task-uuid',
    };

    expect(shouldSurfaceConversationNavigationError(3, 3, requested, requested)).toBe(true);
    expect(shouldSurfaceConversationNavigationError(3, 4, requested, requested)).toBe(false);
    expect(shouldSurfaceConversationNavigationError(3, 3, requested, recreated)).toBe(false);
    expect(shouldSurfaceConversationNavigationError(
      3,
      3,
      { ...requested, taskUuid: undefined },
      requested,
    )).toBe(true);
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

  it('binds source control to the selected dynamic worktree instead of its source branch workspace', () => {
    const mainLeaf = {
      roundId: 'round-001',
      nodeId: 'ai-dynamic',
      attemptId: 'attempt-001',
      pathLabel: 'ai-dynamic/attempt-001',
      worktreePath: null,
    };
    const dynamicLeaf = {
      roundId: 'round-001',
      nodeId: 'worker',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
      pathLabel: 'worker/attempt-001',
      worktreePath: 'D:/repo/.gold-band/worktrees/worker',
    };
    const run = {
      ...oldRun,
      sessionTree: {
        selectedSessionKey: 'round-001/ai-dynamic/attempt-001/worker/attempt-001',
        rounds: [{
          roundId: 'round-001',
          nodes: [{ attempts: [mainLeaf], outerNodes: [{ attempts: [dynamicLeaf] }] }],
        }],
      },
      selectedSession: null,
      worktree: null,
    } as ConversationRunVm;
    const dynamicPage = conversationPageForSession(run, dynamicLeaf);

    expect(conversationSourceControlWorkspacePath(dynamicPage, run))
      .toBe('D:/repo/.gold-band/worktrees/worker');
    expect(conversationSourceControlWorkspacePath({
      kind: 'conversation-run',
      projectId: run.projectId,
      taskId: run.taskId,
      taskUuid: run.taskUuid,
      runId: run.runId,
    }, run)).toBe('D:/repo/.gold-band/worktrees/worker');

    const staleMainPage = conversationPageForSession(run, mainLeaf);
    expect(conversationSourceControlWorkspacePath(staleMainPage, run))
      .toBe('D:/repo/.gold-band/worktrees/worker');

    const mainSelectedRun = {
      ...run,
      sessionTree: {
        ...run.sessionTree,
        selectedSessionKey: 'round-001/ai-dynamic/attempt-001',
      },
    } as ConversationRunVm;
    expect(conversationSourceControlWorkspacePath(staleMainPage, mainSelectedRun)).toBeNull();

    const selectionMissingRun = {
      ...run,
      sessionTree: {
        ...run.sessionTree,
        selectedSessionKey: 'round-001/missing/attempt-001',
      },
    } as ConversationRunVm;
    expect(conversationSourceControlWorkspacePath(dynamicPage, selectionMissingRun))
      .toBe('D:/repo/.gold-band/worktrees/worker');
    expect(conversationSourceControlWorkspacePath({ kind: 'conversation-home' }, run)).toBeNull();
  });
});

describe('conversation sidebar navigation wiring', () => {
  it('invalidates the active run request before deleting its task from disk', () => {
    const source = readAppSource();
    const deletion = source.match(/onConversationDeleteTask=\{[\s\S]*?onConversationPinTask=/)?.[0] ?? '';
    const deleteRequest = deletion.indexOf('deleteConversationTask(projectId, taskId)');
    const requestInvalidation = deletion.indexOf('conversationNavigationRequestRef.current += 1;');

    expect(deleteRequest).toBeGreaterThanOrEqual(0);
    expect(requestInvalidation).toBeGreaterThanOrEqual(0);
    expect(requestInvalidation).toBeLessThan(deleteRequest);
  });

  it('routes run exits plus sidebar task and run selections through the cache-aware conversation navigation entry', () => {
    const source = readAppSource();
    const sidebarSource = readAppSource('web/src/components/conversation/ConversationSidebar.tsx');
    const quickChatSelection = source.match(/onConversationNew=\{[\s\S]*?onConversationSearch=/)?.[0] ?? '';
    const workspaceQuickChatSelection = source.match(/onConversationNewInWorkspace=\{[\s\S]*?onConversationAddWorkspace=/)?.[0] ?? '';
    const sidebarRunSelection = sidebarSource.match(/const selectTaskRun[\s\S]*?const selectTask =/)?.[0] ?? '';
    const searchSelection = source.match(/<ConversationSearchDialog[\s\S]*?\/>/)?.[0] ?? '';
    const interventionNavigation = source.match(/const handleInterventionNavigate[\s\S]*?useInterventionNotifications/)?.[0] ?? '';

    expect(quickChatSelection).toContain("onSelectConversation({ kind: 'conversation-home' })");
    expect(quickChatSelection).toContain('resolveConversationHomeWorkspaceId(');
    expect(workspaceQuickChatSelection).toContain("onSelectConversation({ kind: 'conversation-home' })");
    expect(sidebarRunSelection).toContain('onSelect({');
    expect(sidebarRunSelection).toContain("kind: 'conversation-run'");
    expect(sidebarRunSelection).toContain('taskUuid: task.taskUuid');
    expect(searchSelection).toContain('onSelectConversation(page)');
    expect(interventionNavigation).toContain('onSelectConversation(page)');
    expect(interventionNavigation).toContain('onSelectConversation(conversationPageForIntervention(event))');
    expect(interventionNavigation).not.toContain('getConversationRun(');
    expect(quickChatSelection).not.toContain('setConversationPage(');
    expect(workspaceQuickChatSelection).not.toContain('setConversationPage(');
    expect(searchSelection).not.toContain('setConversationPage(');
    expect(interventionNavigation).not.toContain('setConversationPage(');
    expect(source).not.toContain('onConversationSelectTask=');
    expect(source).not.toContain('onConversationSelectRun=');
  });

  it('commits session selection to React state, the latest-page ref, and history in one event', () => {
    const source = readAppSource();
    const selection = source.match(/onSelectSession=\{\(leaf, followActive\) => \{[\s\S]*?onLifecycleSnapshot=/)?.[0] ?? '';

    expect(selection).toContain('const nextPage = conversationPageForSession(conversationPage, leaf);');
    expect(selection).toContain('conversationPageRef.current = nextPage;');
    expect(selection).toContain('setConversationPage(nextPage);');
    expect(selection).toMatch(/pushRoute\(\r?\n\s+'task-orchestration',\r?\n\s+taskListPage,\r?\n\s+nextPage,/u);
    expect(selection.indexOf('setConversationPage(nextPage);'))
      .toBeLessThan(selection.indexOf('pushRoute('));
    expect(selection).toContain('beginConversationSessionSelection(current, key)');
  });

  it('keeps one shell-level run-state listener and refreshes only the selected run detail', () => {
    const source = readAppSource();
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
    const source = readAppSource();
    const nativeSubscriptions = source.match(/subscribeAcpSessionUpdates\(/g) ?? [];
    const subscriptions = source.match(/subscribeConversationEvents\(\(event\) =>/g) ?? [];
    const globalSubscription = source.match(/subscribeConversationEvents\(\(event\) => \{[\s\S]*?conversationAcpSessionRefreshRef\.current\?\.\(event\);[\s\S]*?\}\);/)?.[0] ?? '';
    const selectedRunHandler = source.match(/const refreshSelectedRunFromAcpEvent[\s\S]*?conversationAcpSessionRefreshRef\.current = refreshSelectedRunFromAcpEvent/)?.[0] ?? '';

    expect(nativeSubscriptions).toHaveLength(0);
    expect(subscriptions).toHaveLength(1);
    expect(globalSubscription).toContain('conversationTaskActivityFromUpdate(event)');
    expect(globalSubscription).toContain('applyConversationTaskActivity(');
    expect(globalSubscription).toContain('event.taskActivityAt');
    expect(globalSubscription).toContain('conversationAcpSessionRefreshRef.current?.(event)');
    expect(globalSubscription).not.toContain('getConversationSidebar(');
    expect(selectedRunHandler).not.toContain('applyConversationTaskActivity(');
    expect(selectedRunHandler).not.toContain('applyConversationLifecycleSnapshotToSidebar(');

    const dialogSource = fs.readFileSync(
      path.resolve(process.cwd(), 'web/src/components/acp/ACPChatDialog.tsx'),
      'utf8',
    );
    expect(dialogSource).toContain('subscribeConversationAttemptEvents(branchLocator');
    expect(dialogSource).not.toContain('subscribeConversationEvents((event) =>');
  });
});
