import type { ConversationPage, ConversationRunVm, InterventionNavigateEventVm } from '@/types';

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

export function shouldCommitConversationNavigation(
  requestId: number,
  currentRequestId: number,
  requested: ConversationPage,
  run: ConversationRunVm,
) {
  return requestId === currentRequestId && conversationPageMatchesRun(requested, run);
}
