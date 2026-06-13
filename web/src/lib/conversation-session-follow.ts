export type ConversationSessionFollowMode = 'auto' | 'manual';

export interface ConversationSessionFollowState {
  mode: ConversationSessionFollowMode;
  selectedSessionKey: string | null;
  version: number;
}

export function resolveConversationEventSelectedSessionKey(args: {
  currentSelectedKey?: string | null;
  incomingSessionKey: string;
  followMode: ConversationSessionFollowMode;
}) {
  const { currentSelectedKey, incomingSessionKey, followMode } = args;
  if (!currentSelectedKey || followMode === 'auto') return incomingSessionKey;
  return currentSelectedKey;
}

export function resolveConversationRefreshSelectedSessionKey(args: {
  followMode: ConversationSessionFollowMode;
  pendingEventSessionKey?: string | null;
  currentSelectedKey?: string | null;
}) {
  const { followMode, pendingEventSessionKey, currentSelectedKey } = args;
  if (followMode === 'auto' && pendingEventSessionKey) return pendingEventSessionKey;
  return currentSelectedKey ?? pendingEventSessionKey ?? null;
}

export function shouldEnableConversationAutoFollow(
  isActiveSession: boolean,
  atBottom: boolean,
) {
  return isActiveSession && atBottom;
}

export function isTerminalConversationSessionStatus(status?: string | null) {
  return ['completed', 'complete', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(
    status?.trim().toLowerCase().replace(/_/g, '-') ?? '',
  );
}

export function shouldQueueConversationRunRefreshForAcpUpdate(args: {
  treeHasSession: boolean;
  alreadySelected: boolean;
  sessionStatus?: string | null;
}) {
  const { treeHasSession, alreadySelected, sessionStatus } = args;
  if (!treeHasSession || !alreadySelected) return true;
  return isTerminalConversationSessionStatus(sessionStatus);
}
