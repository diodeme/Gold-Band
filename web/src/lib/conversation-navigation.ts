import type { ConversationPage, ConversationRunVm } from '@/types';

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

export function resolvePresentedConversationPage(
  requested: ConversationPage,
  presentedRun: ConversationRunVm | null,
): ConversationPage {
  if (requested.kind !== 'conversation-run' || !presentedRun) return requested;
  return conversationPageMatchesRun(requested, presentedRun)
    ? requested
    : conversationPageForRun(presentedRun);
}

export function shouldCommitConversationNavigation(
  requestId: number,
  currentRequestId: number,
  requested: ConversationPage,
  run: ConversationRunVm,
) {
  return requestId === currentRequestId && conversationPageMatchesRun(requested, run);
}
