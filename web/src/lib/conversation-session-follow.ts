import type { ConversationAttemptLifecycleVm } from '@/types';

export type ConversationSessionFollowMode = 'auto' | 'manual';

export interface ConversationSessionFollowState {
  runKey: string | null;
  mode: ConversationSessionFollowMode;
  selectedSessionKey: string | null;
  version: number;
}

export interface ConversationRunReentrySelection {
  followMode: ConversationSessionFollowMode;
  selectedSessionKey: string | null;
  preserveSelectedSession: boolean;
}

export function resolveConversationRunReentrySelection(args: {
  followMode: ConversationSessionFollowMode;
  rememberedSelectedSessionKey?: string | null;
  explicitSelectedSessionKey?: string | null;
  defaultSelectedSessionKey?: string | null;
  hasSessionKey: (key: string) => boolean;
}): ConversationRunReentrySelection {
  const explicitSelectedSessionKey = args.explicitSelectedSessionKey
    && args.hasSessionKey(args.explicitSelectedSessionKey)
    ? args.explicitSelectedSessionKey
    : null;
  const rememberedSelectedSessionKey = args.rememberedSelectedSessionKey
    && args.hasSessionKey(args.rememberedSelectedSessionKey)
    ? args.rememberedSelectedSessionKey
    : null;
  const followMode = explicitSelectedSessionKey ? 'manual' : args.followMode;
  return {
    followMode,
    selectedSessionKey: explicitSelectedSessionKey
      ?? rememberedSelectedSessionKey
      ?? args.defaultSelectedSessionKey
      ?? null,
    preserveSelectedSession: followMode === 'manual',
  };
}

export function resolveConversationEventSelectedSessionKey(args: {
  currentSelectedKey?: string | null;
  incomingSessionKey: string;
  followMode: ConversationSessionFollowMode;
  currentSelectedActive?: boolean;
  currentSelectedTerminal?: boolean;
  incomingActive?: boolean;
  currentSelectedRuntimeControlled?: boolean;
  currentSelectedControlTransitionCause?: ConversationAttemptLifecycleVm['control']['transitionCause'];
  incomingRuntimeControlled?: boolean;
}) {
  const {
    currentSelectedKey,
    incomingSessionKey,
    followMode,
    currentSelectedActive = false,
    currentSelectedTerminal = false,
    incomingActive = true,
    currentSelectedRuntimeControlled = false,
    currentSelectedControlTransitionCause,
    incomingRuntimeControlled = false,
  } = args;
  if (currentSelectedKey && isNestedConversationSessionKey(currentSelectedKey, incomingSessionKey)) {
    return currentSelectedKey;
  }
  if (!currentSelectedKey) return incomingSessionKey;
  if (followMode !== 'auto') return currentSelectedKey;
  if (!incomingActive) return currentSelectedKey;
  if (!incomingRuntimeControlled) return currentSelectedKey;
  const followsRuntimeTerminal = currentSelectedTerminal
    && currentSelectedControlTransitionCause === 'runtime-terminal';
  if (!currentSelectedRuntimeControlled && !followsRuntimeTerminal) return currentSelectedKey;
  return currentSelectedActive ? currentSelectedKey : incomingSessionKey;
}

export function resolveConversationRefreshSelectedSessionKey(args: {
  followMode: ConversationSessionFollowMode;
  pendingEventSessionKey?: string | null;
  currentSelectedKey?: string | null;
  currentSelectedTerminal?: boolean;
  currentSelectedRuntimeControlled?: boolean;
  currentSelectedControlTransitionCause?: ConversationAttemptLifecycleVm['control']['transitionCause'];
  pendingEventRuntimeControlled?: boolean;
  pendingEventControlTransitionCause?: ConversationAttemptLifecycleVm['control']['transitionCause'];
}) {
  const {
    followMode,
    pendingEventSessionKey,
    currentSelectedKey,
    currentSelectedTerminal = false,
    currentSelectedRuntimeControlled = false,
    currentSelectedControlTransitionCause,
    pendingEventRuntimeControlled = false,
    pendingEventControlTransitionCause,
  } = args;
  if (
    currentSelectedKey &&
    pendingEventSessionKey &&
    isNestedConversationSessionKey(currentSelectedKey, pendingEventSessionKey)
  ) {
    return currentSelectedKey;
  }
  if (
    followMode === 'auto'
    && pendingEventSessionKey
    && (
      pendingEventRuntimeControlled
      || pendingEventControlTransitionCause === 'runtime-terminal'
    )
    && (
      currentSelectedRuntimeControlled
      || (
        currentSelectedTerminal
        && currentSelectedControlTransitionCause === 'runtime-terminal'
      )
    )
  ) return pendingEventSessionKey;
  return currentSelectedKey ?? pendingEventSessionKey ?? null;
}

export function isRuntimeControlledConversationLifecycle(
  lifecycle?: Pick<ConversationAttemptLifecycleVm, 'control'> | null,
) {
  return lifecycle?.control.mode === 'runtime-controlled';
}

export function isRuntimeTerminalConversationLifecycle(
  lifecycle?: Pick<ConversationAttemptLifecycleVm, 'control'> | null,
) {
  return lifecycle?.control.transitionCause === 'runtime-terminal';
}

export function isNestedConversationSessionKey(currentSelectedKey: string, incomingSessionKey: string) {
  return currentSelectedKey.startsWith(`${incomingSessionKey}/`);
}

export function shouldEnableConversationAutoFollow(
  isActiveSession: boolean,
  atBottom: boolean,
  runtimeControlled: boolean,
) {
  return runtimeControlled && isActiveSession && atBottom;
}

export function isTerminalConversationSessionStatus(status?: string | null) {
  return ['completed', 'complete', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(
    status?.trim().toLowerCase().replace(/_/g, '-') ?? '',
  );
}

export function conversationAcpRunRefreshStatus(args: {
  dynamicSession: boolean;
  lifecycle?: Pick<ConversationAttemptLifecycleVm, 'displayStatus' | 'runtime'> | null;
  sessionStatus?: string | null;
}) {
  return args.dynamicSession
    ? args.lifecycle?.runtime.status ?? args.sessionStatus
    : args.lifecycle?.displayStatus ?? args.sessionStatus;
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
  hasRuntimeSnapshot?: boolean;
  hasSessionSnapshot?: boolean;
  hasLiveEvent: boolean;
  sessionStatus?: string | null;
  pendingPermissionCount?: number;
  followPending?: boolean;
}): ConversationAcpRunUpdatePlan {
  const {
    treeHasSession,
    alreadySelected,
    hasRuntimeSnapshot,
    hasSessionSnapshot,
    hasLiveEvent,
    sessionStatus,
    pendingPermissionCount = 0,
    followPending = false,
  } = args;
  const canPatchSnapshot = hasRuntimeSnapshot ?? Boolean(hasSessionSnapshot);
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
      patchSelectedSession: canPatchSnapshot,
      patchBackgroundSession: false,
      queueRunRefresh: terminal || interactive,
    };
  }
  if (!canPatchSnapshot) {
    return {
      patchSelectedSession: false,
      patchBackgroundSession: false,
      queueRunRefresh: hasLiveEvent && followPending,
    };
  }
  return {
    patchSelectedSession: false,
    patchBackgroundSession: !terminal && !interactive,
    queueRunRefresh: terminal || interactive || followPending,
  };
}

export function shouldQueueConversationRunRefreshForAcpUpdate(args: {
  treeHasSession: boolean;
  alreadySelected: boolean;
  hasRuntimeSnapshot?: boolean;
  hasSessionSnapshot?: boolean;
  hasLiveEvent?: boolean;
  sessionStatus?: string | null;
  pendingPermissionCount?: number;
}) {
  return planConversationAcpRunUpdate({
    treeHasSession: args.treeHasSession,
    alreadySelected: args.alreadySelected,
    hasRuntimeSnapshot: args.hasRuntimeSnapshot,
    hasSessionSnapshot: args.hasSessionSnapshot ?? Boolean(args.sessionStatus),
    hasLiveEvent: args.hasLiveEvent ?? false,
    sessionStatus: args.sessionStatus,
    pendingPermissionCount: args.pendingPermissionCount,
  }).queueRunRefresh;
}
