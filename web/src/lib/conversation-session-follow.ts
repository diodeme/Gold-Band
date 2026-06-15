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
  if (currentSelectedKey && isNestedConversationSessionKey(currentSelectedKey, incomingSessionKey)) {
    return currentSelectedKey;
  }
  if (!currentSelectedKey || followMode === 'auto') return incomingSessionKey;
  return currentSelectedKey;
}

export function resolveConversationRefreshSelectedSessionKey(args: {
  followMode: ConversationSessionFollowMode;
  pendingEventSessionKey?: string | null;
  currentSelectedKey?: string | null;
}) {
  const { followMode, pendingEventSessionKey, currentSelectedKey } = args;
  if (
    currentSelectedKey &&
    pendingEventSessionKey &&
    isNestedConversationSessionKey(currentSelectedKey, pendingEventSessionKey)
  ) {
    return currentSelectedKey;
  }
  if (followMode === 'auto' && pendingEventSessionKey) return pendingEventSessionKey;
  return currentSelectedKey ?? pendingEventSessionKey ?? null;
}

export function isNestedConversationSessionKey(currentSelectedKey: string, incomingSessionKey: string) {
  return currentSelectedKey.startsWith(`${incomingSessionKey}/`);
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

export function needsInteractiveConversationRunRefresh(status?: string | null, pendingPermissionCount = 0) {
  const normalized = status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
  return pendingPermissionCount > 0
    || ['paused', 'waiting', 'waiting-for-user-input', 'blocked', 'error-blocked'].includes(normalized);
}

export interface ConversationAcpRunUpdatePlan {
  patchSelectedSession: boolean;
  patchBackgroundSession: boolean;
  queueRunRefresh: boolean;
}

export function planConversationAcpRunUpdate(args: {
  treeHasSession: boolean;
  alreadySelected: boolean;
  hasSessionSnapshot: boolean;
  hasLiveEvent: boolean;
  sessionStatus?: string | null;
  pendingPermissionCount?: number;
}): ConversationAcpRunUpdatePlan {
  const {
    treeHasSession,
    alreadySelected,
    hasSessionSnapshot,
    sessionStatus,
    pendingPermissionCount = 0,
  } = args;
  const terminal = isTerminalConversationSessionStatus(sessionStatus);
  const interactive = needsInteractiveConversationRunRefresh(sessionStatus, pendingPermissionCount);
  if (!treeHasSession) {
    return {
      patchSelectedSession: false,
      patchBackgroundSession: false,
      queueRunRefresh: true,
    };
  }
  if (alreadySelected) {
    return {
      patchSelectedSession: hasSessionSnapshot,
      patchBackgroundSession: false,
      queueRunRefresh: terminal || interactive,
    };
  }
  if (!hasSessionSnapshot) {
    return {
      patchSelectedSession: false,
      patchBackgroundSession: false,
      queueRunRefresh: false,
    };
  }
  return {
    patchSelectedSession: false,
    patchBackgroundSession: !terminal && !interactive,
    queueRunRefresh: terminal || interactive,
  };
}

export function shouldQueueConversationRunRefreshForAcpUpdate(args: {
  treeHasSession: boolean;
  alreadySelected: boolean;
  hasSessionSnapshot?: boolean;
  hasLiveEvent?: boolean;
  sessionStatus?: string | null;
  pendingPermissionCount?: number;
}) {
  return planConversationAcpRunUpdate({
    treeHasSession: args.treeHasSession,
    alreadySelected: args.alreadySelected,
    hasSessionSnapshot: args.hasSessionSnapshot ?? Boolean(args.sessionStatus),
    hasLiveEvent: args.hasLiveEvent ?? false,
    sessionStatus: args.sessionStatus,
    pendingPermissionCount: args.pendingPermissionCount,
  }).queueRunRefresh;
}
