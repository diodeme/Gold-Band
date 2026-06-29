import type { ConversationRunVm } from '@/types';

export function isConversationRunStopSettled(run: ConversationRunVm | null | undefined) {
  if (!run) return false;
  if (run.runStatus === 'running') return false;
  if (run.activeSessions.length > 0) return false;
  const selectedStatus = run.selectedSession?.status ?? null;
  return selectedStatus == null || isTerminalAcpSessionStatus(selectedStatus);
}

function isTerminalAcpSessionStatus(status: string) {
  return ['cancelled', 'canceled', 'completed', 'failed', 'failure', 'error', 'killed'].includes(status);
}
