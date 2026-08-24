import { conversationSessionKeyFromParts, findConversationLeafByKey } from '@/lib/conversation-run-snapshot';
import type {
  ConversationPage,
  ConversationRunVm,
  ConversationSidebarVm,
  ConversationSessionLeafVm,
  ConversationSessionTargetVm,
  ConversationSessionTreeVm,
  InterventionNavigateEventVm,
} from '@/types';
import { findConversationTask } from '@/lib/conversation-task-state';

type ConversationSessionLocator = Pick<
  ConversationSessionTargetVm,
  'roundId' | 'nodeId' | 'attemptId' | 'outerNodeId' | 'outerAttemptId'
>;

export function conversationPageForIntervention(
  event: Extract<InterventionNavigateEventVm, { targetType: 'conversation' }>,
): Extract<ConversationPage, { kind: 'conversation-run' }> {
  return {
    kind: 'conversation-run',
    projectId: event.projectId,
    taskId: event.taskId,
    runId: event.runId,
  };
}

export function conversationPageMatchesRun(
  page: ConversationPage,
  run: ConversationRunVm | null | undefined,
): page is Extract<ConversationPage, { kind: 'conversation-run' }> {
  return page.kind === 'conversation-run'
    && run != null
    && page.projectId === run.projectId
    && page.taskId === run.taskId
    && page.runId === run.runId;
}

export function conversationPageForRun(run: ConversationRunVm): Extract<ConversationPage, { kind: 'conversation-run' }> {
  return {
    kind: 'conversation-run',
    projectId: run.projectId,
    taskId: run.taskId,
    runId: run.runId,
  };
}

export function resolveConversationHomeWorkspaceId(
  currentPage: ConversationPage,
  draftWorkspaceId: string | null,
  lastActiveWorkspaceId: string | null,
): string | null {
  if (draftWorkspaceId !== null) return draftWorkspaceId;
  if (currentPage.kind === 'conversation-run') return currentPage.projectId;
  return lastActiveWorkspaceId;
}

export function isConversationRunNavigationLoading(
  requested: ConversationPage,
  loadedRun: ConversationRunVm | null,
) {
  return requested.kind === 'conversation-run'
    && !conversationPageMatchesRun(requested, loadedRun);
}

export function beginConversationSessionSelection(
  run: ConversationRunVm,
  selectedSessionKey: string,
): ConversationRunVm {
  if (run.sessionTree.selectedSessionKey === selectedSessionKey) return run;
  return {
    ...run,
    selectedSession: null,
    sessionTree: {
      ...run.sessionTree,
      selectedSessionKey,
    },
  };
}

export function conversationPageForSession(
  run: Pick<ConversationRunVm, 'projectId' | 'taskId' | 'runId'>,
  locator: ConversationSessionLocator,
): Extract<ConversationPage, { kind: 'conversation-run' }> {
  return {
    kind: 'conversation-run',
    projectId: run.projectId,
    taskId: run.taskId,
    runId: run.runId,
    roundId: locator.roundId,
    nodeId: locator.nodeId,
    attemptId: locator.attemptId,
    outerNodeId: locator.outerNodeId ?? undefined,
    outerAttemptId: locator.outerAttemptId ?? undefined,
  };
}

export function findConversationLeafForPage(
  tree: ConversationSessionTreeVm,
  page: Extract<ConversationPage, { kind: 'conversation-run' }>,
): ConversationSessionLeafVm | null {
  if (!page.roundId) return null;
  if (page.nodeId && page.attemptId) {
    return findConversationLeafByKey(tree, conversationSessionKeyFromParts({
      roundId: page.roundId,
      nodeId: page.nodeId,
      attemptId: page.attemptId,
      outerNodeId: page.outerNodeId,
      outerAttemptId: page.outerAttemptId,
    }));
  }
  for (const round of tree.rounds) {
    if (round.roundId !== page.roundId) continue;
    for (const node of round.nodes) {
      const leaf = node.attempts.find((attempt) => !page.attemptId || attempt.attemptId === page.attemptId);
      if (leaf) return leaf;
      for (const outerNode of node.outerNodes ?? []) {
        const nestedLeaf = outerNode.attempts.find((attempt) => !page.attemptId || attempt.attemptId === page.attemptId);
        if (nestedLeaf) return nestedLeaf;
      }
    }
  }
  return null;
}

export function shouldCommitConversationNavigation(
  requestId: number,
  currentRequestId: number,
  requested: ConversationPage,
  run: ConversationRunVm,
) {
  return requestId === currentRequestId && conversationPageMatchesRun(requested, run);
}

export function conversationTerminalResultAcknowledgementTarget(
  sidebar: ConversationSidebarVm,
  page: ConversationPage,
  loadedRun: Pick<ConversationRunVm, 'projectId' | 'taskId' | 'runId'> | null | undefined,
) {
  if (page.kind !== 'conversation-run'
    || !loadedRun
    || page.projectId !== loadedRun.projectId
    || page.taskId !== loadedRun.taskId
    || page.runId !== loadedRun.runId) {
    return null;
  }
  const unread = findConversationTask(sidebar, page.projectId, page.taskId)?.unreadTerminalResult;
  if (!unread || unread.runId !== page.runId) return null;
  return {
    projectId: page.projectId,
    taskId: page.taskId,
    runId: page.runId,
    eventId: unread.eventId,
  };
}
