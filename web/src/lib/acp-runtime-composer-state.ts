import type { ConversationAttemptLifecycleVm } from '@/types';

export type AcpComposerMode =
  | 'normal'
  | 'runtime-active'
  | 'stopping'
  | 'invalid-workflow'
  | 'runtime-error'
  | 'permission-blocked'
  | 'session-superseded'
  | 'submitting';

export type AcpComposerSubmitTarget =
  | 'acp-prompt'
  | 'queue-prompt'
  | 'none';

export type AcpComposerProcessingKind =
  | 'sending'
  | 'launching'
  | 'processing'
  | 'thinking'
  | 'tool'
  | 'compacting'
  | 'responding'
  | 'stopping'
  | 'preparing-workspace'
  | 'launching-next-node';

export type AcpComposerPlaceholderKind =
  | 'default'
  | 'runtime-controlled'
  | 'stopping'
  | 'message';

export interface AcpRuntimeComposerStateInput {
  lifecycle?: ConversationAttemptLifecycleVm | null;
  promptQueueEnabled?: boolean;
  workflowValid: boolean;
  workflowInvalidMessage?: string | null;
  pauseMessage?: string | null;
  runtimeErrorMessage?: string | null;
  acpStatus?: string | null;
  prompt: string;
  hasAttachments?: boolean;
  waitingForPermission: boolean;
  sending: boolean;
  awaitingResponse: boolean;
  waitingForOptimisticPrompt: boolean;
  localTurnId?: string | null;
  cancelling: boolean;
  stopCommandPending: boolean;
  turnAccepted: boolean;
  hasResponseAfterTurn: boolean;
  hasTimelineItems: boolean;
  hasEffectiveEvents: boolean;
  initialTimelinePending?: boolean;
  timelineProcessingKind: AcpComposerProcessingKind;
}

export interface AcpRuntimeComposerState {
  mode: AcpComposerMode;
  submitTarget: AcpComposerSubmitTarget;
  inputDisabled: boolean;
  canSubmit: boolean;
  canStop: boolean;
  stopInProgress: boolean;
  sessionActive: boolean;
  acpActive: boolean;
  runtimeActive: boolean;
  composerLocked: boolean;
  showExternalState: boolean;
  externalKind: 'invalid-workflow' | 'runtime-error' | null;
  externalMessage?: string | null;
  processingKind: AcpComposerProcessingKind;
  statusActive: boolean;
  showStatus: boolean;
  placeholderKind: AcpComposerPlaceholderKind;
  message?: string | null;
}

export function deriveAcpRuntimeComposerState(
  input: AcpRuntimeComposerStateInput,
): AcpRuntimeComposerState {
  const backend = input.lifecycle?.composer;
  const backendProcessingKind = normalizeProcessingKind(backend?.processingKind);
  const backendWorkspacePreparing = backend?.mode === 'runtime-active'
    && backendProcessingKind === 'preparing-workspace';
  const runtimeActive = Boolean(input.lifecycle?.runtime.active);
  const lifecycleAcpRunning = ['starting', 'accepted', 'running'].includes(
    input.lifecycle?.acp.liveTurnActivity ?? 'idle',
  );
  const lifecycleTerminal = !lifecycleAcpRunning
    && (input.lifecycle?.acp.latestTurnStatus ?? 'none') !== 'none';
  const lifecycleMatchesLocalTurn = Boolean(
    input.localTurnId
      && input.lifecycle?.acp.turnId
      && input.lifecycle.acp.turnId === input.localTurnId,
  );
  const acpTerminal = lifecycleTerminal
    ? (!input.localTurnId || lifecycleMatchesLocalTurn)
    : (!input.lifecycle && !input.localTurnId && isSessionTerminalStatus(input.acpStatus));
  const localTurnInFlight = !acpTerminal && Boolean(input.localTurnId) && (
    input.sending || input.awaitingResponse || input.waitingForOptimisticPrompt
  );
  const sending = input.sending && !acpTerminal;
  const acpActive = !acpTerminal && lifecycleAcpRunning;
  const backendStopping = !acpTerminal && (Boolean(input.lifecycle?.acp.stopping) || backend?.mode === 'stopping');
  const waitingForPermission = input.waitingForPermission;
  const initialTimelinePending = Boolean(input.initialTimelinePending);
  const staleTerminalSnapshot = acpTerminal;
  const cancelling = !acpTerminal && input.cancelling;
  const stopCommandPending = (!acpTerminal || backendWorkspacePreparing) && input.stopCommandPending;
  const stopInProgress = cancelling || stopCommandPending || backendStopping;
  const waitingForOptimisticPrompt = !staleTerminalSnapshot && input.waitingForOptimisticPrompt;
  const turnSubmitting = (sending || waitingForOptimisticPrompt) && !input.turnAccepted;
  const awaitingResponse = !staleTerminalSnapshot && input.awaitingResponse;
  const runtimeErrorMessage = runtimeErrorMessageFromInput(input);
  const runtimeContinueBlockedByWorkflow = false;
  const reportedBackendMode = normalizeComposerMode(backend?.mode);
  const sessionSuperseded = reportedBackendMode === 'session-superseded';
  const backendMode = acpTerminal && reportedBackendMode === 'stopping'
    ? 'normal'
    : reportedBackendMode;
  const mode = sessionSuperseded ? 'session-superseded' : composerModeFromBackend({
    backendMode,
    waitingForPermission,
    stopInProgress,
    turnSubmitting,
    runtimeContinueBlockedByWorkflow,
    runtimeErrorMessage,
  });
  const directQueueFacet = Boolean(input.promptQueueEnabled) || input.lifecycle?.promptQueue != null;
  const submitTarget = directQueueFacet && shouldRouteDirectSubmissionToQueue({
    input,
    mode,
    runtimeActive,
    acpActive,
    waitingForOptimisticPrompt,
    awaitingResponse,
  })
    ? 'queue-prompt'
    : submitTargetFromBackend(input, mode, backend?.submitTarget);
  const queueAtCapacity = submitTarget === 'queue-prompt' && (
    (input.lifecycle?.promptQueue?.items.length ?? 0) >= (input.lifecycle?.promptQueue?.maxItems ?? 10)
  );
  const sessionActive = runtimeActive || acpActive || stopInProgress || waitingForPermission;
  const activePromptLocked =
    sending ||
    waitingForOptimisticPrompt ||
    awaitingResponse ||
    initialTimelinePending ||
    sessionActive ||
    stopInProgress;
  const showExternalState = mode === 'invalid-workflow' || mode === 'runtime-error';
  const composerLocked = waitingForPermission && !directQueueFacet;
  const staleStoppingBackend = acpTerminal && reportedBackendMode === 'stopping';
  const backendInputLocked = !staleStoppingBackend && mode !== 'normal' && Boolean(backend?.lockInput);
  const directInputDisabled =
    stopInProgress ||
    initialTimelinePending ||
    mode === 'invalid-workflow' ||
    mode === 'runtime-error';
  const inputDisabled = (
    directQueueFacet
      ? directInputDisabled
      : composerLocked || backendInputLocked || activePromptLocked || mode === 'invalid-workflow' || mode === 'runtime-error'
  );
  const canSubmit = (Boolean(input.prompt.trim()) || Boolean(input.hasAttachments))
    && submitTarget !== 'none'
    && !queueAtCapacity
    && !(sending && submitTarget !== 'queue-prompt')
    && !inputDisabled;
  const processingKind = processingKindForInput(
    { ...input, sending },
    stopInProgress,
    turnSubmitting,
    awaitingResponse,
    backendProcessingKind,
  );
  const directTurnHandoff = directQueueFacet
    && acpTerminal
    && processingKind === 'launching-next-node'
    && !turnSubmitting
    && !awaitingResponse;
  const statusActive = !sessionSuperseded &&
    !input.waitingForPermission &&
    !composerLocked &&
    !directTurnHandoff &&
    (turnSubmitting || awaitingResponse || initialTimelinePending || sessionActive || stopInProgress || mode === 'runtime-active');
  const externalMessage = externalMessageForMode(input, mode, runtimeErrorMessage);

  return {
    mode,
    submitTarget,
    inputDisabled,
    canSubmit,
    canStop: !sessionSuperseded && (
      (!acpTerminal && Boolean(backend?.canStop)) ||
      (backendWorkspacePreparing && Boolean(backend?.canStop)) ||
      sessionActive ||
      awaitingResponse ||
      sending ||
      waitingForOptimisticPrompt ||
      localTurnInFlight ||
      cancelling
    ),
    stopInProgress,
    sessionActive,
    acpActive,
    runtimeActive,
    composerLocked,
    showExternalState,
    externalKind: externalKindForMode(mode),
    externalMessage,
    processingKind,
    statusActive,
    showStatus: !sessionSuperseded && !input.waitingForPermission && statusActive,
    placeholderKind: initialTimelinePending
      ? 'runtime-controlled'
      : directQueueFacet
        ? 'default'
        : placeholderKindForMode(input, mode, activePromptLocked),
    message: externalMessage,
  };
}

export function shouldKeepLocalRuntimeLifecycleOverride(
  local: ConversationAttemptLifecycleVm | null | undefined,
  incoming: ConversationAttemptLifecycleVm | null | undefined,
) {
  if (!local?.runtime.active) return false;
  if (!incoming) return true;
  if (incoming.runtime.active || incoming.acp.liveTurnActivity !== 'idle' || incoming.acp.stopping) {
    return false;
  }
  if (incoming.composer.mode === 'runtime-error') return false;
  return (
    incoming.runtime.phase === 'paused' &&
    Boolean(incoming.continueKind) &&
    incoming.composer.mode === 'normal'
  );
}

export function shouldSettleRuntimeContinueSubmission(
  submitting: boolean,
  showRuntimeContinueAction: boolean,
) {
  return submitting && !showRuntimeContinueAction;
}

export function isAcceptedQueuePromptSubmitKind(kind: string) {
  return kind === 'queued' || kind === 'acp-session' || kind === 'acp-session-started';
}

export function isAcceptedAcpPromptSubmitKind(kind: string) {
  return kind === 'acp-session' || kind === 'acp-session-started';
}

export function isTerminalLifecycleForTurn(
  lifecycle: ConversationAttemptLifecycleVm | null | undefined,
  turnId: string | null | undefined,
) {
  return Boolean(
    turnId
      && lifecycle?.acp.turnId === turnId
      && isTerminalAcpFacet(lifecycle.acp),
  );
}

/** Lifecycle-only terminal patches settle transient composer state. */
export function isTerminalAcpLifecycle(
  lifecycle: ConversationAttemptLifecycleVm | null | undefined,
) {
  return Boolean(lifecycle && isTerminalAcpFacet(lifecycle.acp));
}

/**
 * Selects the session status used by timeline activity presentation.
 * A lifecycle-only terminal patch is authoritative over a stale body snapshot,
 * except while a new local prompt is still being admitted.
 */
export function activityProjectionStatus(
  lifecycle: ConversationAttemptLifecycleVm | null | undefined,
  sessionStatus: string | null | undefined,
  localPromptAdmissionPending: boolean,
  localTurnId?: string | null,
) {
  const terminalLifecycleMatchesTurn = isTerminalAcpLifecycle(lifecycle)
    && (!localTurnId || lifecycle?.acp.turnId === localTurnId);
  if (terminalLifecycleMatchesTurn) return lifecycle!.acp.latestTurnStatus;
  if (localPromptAdmissionPending) return 'running';
  if (lifecycle && lifecycle.acp.liveTurnActivity !== 'idle') {
    return lifecycle.acp.stopping ? 'cancelling' : lifecycle.acp.liveTurnActivity;
  }
  return sessionStatus;
}

/**
 * Once an ACP lifecycle facet is available, it is authoritative over a
 * session snapshot. A terminal snapshot may belong to the previous turn
 * while the lifecycle has already admitted the next one.
 */
export function shouldSettleAcpComposerTransientState(
  lifecycle: ConversationAttemptLifecycleVm | null | undefined,
  sessionStatus: string | null | undefined,
  localTurnId: string | null | undefined,
) {
  if (!lifecycle) return isSessionTerminalStatus(sessionStatus);
  return isTerminalAcpLifecycle(lifecycle)
    && (!localTurnId || lifecycle.acp.turnId === localTurnId);
}

function shouldRouteDirectSubmissionToQueue(input: {
  input: AcpRuntimeComposerStateInput;
  mode: AcpComposerMode;
  runtimeActive: boolean;
  acpActive: boolean;
  waitingForOptimisticPrompt: boolean;
  awaitingResponse: boolean;
}) {
  if (
    input.mode === 'invalid-workflow' ||
    input.mode === 'runtime-error' ||
    input.mode === 'stopping'
  ) {
    return false;
  }
  return input.input.sending ||
    input.waitingForOptimisticPrompt ||
    input.awaitingResponse ||
    input.runtimeActive ||
    input.acpActive;
}

export function mergeConversationAttemptLifecycle(
  local: ConversationAttemptLifecycleVm | null | undefined,
  incoming: ConversationAttemptLifecycleVm,
): ConversationAttemptLifecycleVm {
  const localAcpRevision = local?.acp.revision ?? 0;
  const incomingAcpRevision = incoming.acp.revision ?? 0;
  let acp = incoming.acp;
  let acpFromLocal = false;
  if (local && localAcpRevision > incomingAcpRevision) {
    acp = local.acp;
    acpFromLocal = true;
  } else if (
    local &&
    localAcpRevision === incomingAcpRevision &&
    local.acp.turnId === incoming.acp.turnId &&
    isTerminalAcpFacet(local.acp) &&
    !isTerminalAcpFacet(incoming.acp)
  ) {
    acp = local.acp;
    acpFromLocal = true;
  }

  const localRuntimeRevision = local?.runtime.revision ?? 0;
  const incomingRuntimeRevision = incoming.runtime.revision ?? 0;
  const runtime = local && localRuntimeRevision > incomingRuntimeRevision
    ? local.runtime
    : incoming.runtime;
  const localQueue = local?.promptQueue;
  const incomingQueue = incoming.promptQueue;
  const promptQueue = localQueue && (!incomingQueue || localQueue.revision > incomingQueue.revision)
    ? localQueue
    : incomingQueue;
  if (local && acpFromLocal && localRuntimeRevision >= incomingRuntimeRevision) {
    return { ...local, promptQueue };
  }
  if (acp !== incoming.acp || runtime !== incoming.runtime || promptQueue !== incomingQueue) {
    return { ...incoming, acp, runtime, promptQueue };
  }
  if (localQueue && (!incomingQueue || localQueue.revision > incomingQueue.revision)) {
    return { ...incoming, promptQueue: localQueue };
  }
  return incoming;
}

function isTerminalAcpFacet(acp: ConversationAttemptLifecycleVm['acp']) {
  return acp.liveTurnActivity === 'idle' && acp.latestTurnStatus !== 'none' && !acp.stopping;
}

export function isSessionActiveStatus(status?: string | null) {
  return ['pending', 'running', 'in-progress', 'in_progress', 'active', 'sending', 'cancelling', 'cancel-requested', 'cancel_requested'].includes(
    normalizeStatus(status),
  );
}

export function isSessionStopPending(status?: string | null) {
  return ['cancelling', 'cancel-requested', 'cancel_requested'].includes(normalizeStatus(status));
}

export function isSessionCompletedStatus(status?: string | null) {
  return ['completed', 'complete'].includes(normalizeStatus(status));
}

export function isSessionTerminalStatus(status?: string | null) {
  return ['completed', 'complete', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(normalizeStatus(status));
}

export function isRuntimeActiveStatus(status?: string | null) {
  return ['pending', 'running', 'in-progress', 'in_progress', 'active'].includes(normalizeStatus(status));
}

export function isRuntimeTerminalStatus(status?: string | null) {
  return ['completed', 'complete', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(normalizeStatus(status));
}

function normalizeComposerMode(mode?: string | null): AcpComposerMode {
  const normalized = normalizeStatus(mode);
  if (
    normalized === 'normal' ||
    normalized === 'runtime-active' ||
    normalized === 'stopping' ||
    normalized === 'invalid-workflow' ||
    normalized === 'runtime-error' ||
    normalized === 'permission-blocked' ||
    normalized === 'session-superseded' ||
    normalized === 'submitting'
  ) {
    return normalized;
  }
  return 'normal';
}

function composerModeFromBackend(input: {
  backendMode: AcpComposerMode;
  waitingForPermission: boolean;
  stopInProgress: boolean;
  turnSubmitting: boolean;
  runtimeContinueBlockedByWorkflow: boolean;
  runtimeErrorMessage: string | null;
}): AcpComposerMode {
  if (input.waitingForPermission) return 'permission-blocked';
  if (input.stopInProgress) return 'stopping';
  if (input.turnSubmitting) return 'submitting';
  if (input.runtimeContinueBlockedByWorkflow) return 'invalid-workflow';
  return input.backendMode;
}

function submitTargetFromBackend(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  backendSubmitTarget?: string | null,
): AcpComposerSubmitTarget {
  if (mode === 'permission-blocked') return 'none';
  if (mode === 'invalid-workflow' || mode === 'runtime-error' || mode === 'stopping') return 'none';
  const normalized = normalizeStatus(backendSubmitTarget);
  if (
    normalized === 'acp-prompt' ||
    normalized === 'queue-prompt' ||
    normalized === 'none'
  ) {
    return normalized;
  }
  return 'none';
}

function processingKindForInput(
  input: AcpRuntimeComposerStateInput,
  stopInProgress: boolean,
  turnSubmitting: boolean,
  awaitingResponse: boolean,
  backendProcessingKind: AcpComposerProcessingKind,
): AcpComposerProcessingKind {
  if (stopInProgress) return 'stopping';
  if (turnSubmitting) return 'sending';
  if (backendProcessingKind === 'preparing-workspace') return 'preparing-workspace';
  if (backendProcessingKind === 'launching-next-node') return 'launching-next-node';
  if (awaitingResponse && input.turnAccepted && !input.hasResponseAfterTurn) return 'processing';
  if (!input.hasTimelineItems) {
    if (input.hasEffectiveEvents) return 'processing';
    const lifecycleLaunching = Boolean(
      input.initialTimelinePending
      || input.lifecycle?.runtime.active
      || (input.lifecycle?.acp.liveTurnActivity ?? 'idle') !== 'idle'
      || input.lifecycle?.acp.stopping,
    );
    return lifecycleLaunching ? 'launching' : backendProcessingKind;
  }
  if (backendProcessingKind !== 'processing') return backendProcessingKind;
  return input.timelineProcessingKind;
}

function placeholderKindForMode(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  activePromptLocked: boolean,
): AcpComposerPlaceholderKind {
  if (input.waitingForPermission) return 'runtime-controlled';
  if (mode === 'stopping') return 'stopping';
  if (mode === 'invalid-workflow' || mode === 'runtime-error') return 'message';
  if (activePromptLocked) return 'runtime-controlled';
  return 'default';
}

function externalKindForMode(mode: AcpComposerMode) {
  if (mode === 'invalid-workflow') return 'invalid-workflow' as const;
  if (mode === 'runtime-error') return 'runtime-error' as const;
  return null;
}

function externalMessageForMode(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  runtimeErrorMessage: string | null,
) {
  if (mode === 'invalid-workflow') return input.workflowInvalidMessage ?? null;
  if (mode === 'runtime-error') return runtimeErrorMessage;
  return null;
}

function runtimeErrorMessageFromInput(input: AcpRuntimeComposerStateInput) {
  if (input.lifecycle?.composer.mode !== 'runtime-error') return null;
  if (input.runtimeErrorMessage) return input.runtimeErrorMessage;
  return 'runtime-error';
}

function normalizeProcessingKind(kind?: string | null): AcpComposerProcessingKind {
  const normalized = normalizeStatus(kind);
  if (
    normalized === 'sending' ||
    normalized === 'launching' ||
    normalized === 'processing' ||
    normalized === 'thinking' ||
    normalized === 'tool' ||
    normalized === 'compacting' ||
    normalized === 'responding' ||
    normalized === 'stopping' ||
    normalized === 'preparing-workspace' ||
    normalized === 'launching-next-node'
  ) {
    return normalized;
  }
  return 'processing';
}

function normalizeStatus(status?: string | null) {
  return status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}
