import type { ConversationAttemptLifecycleVm } from '@/types';

export type AcpComposerMode =
  | 'normal'
  | 'runtime-active'
  | 'stopping'
  | 'invalid-workflow'
  | 'runtime-error'
  | 'interaction-blocked'
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
  | 'processing-workspace'
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
  waitingForUserInteraction: boolean;
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
  const backendWorkspaceTransition = backend?.mode === 'runtime-active'
    && (
      backendProcessingKind === 'preparing-workspace'
      || backendProcessingKind === 'processing-workspace'
    );
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
  const backendStopping = !acpTerminal && (Boolean(input.lifecycle?.acp.stopping) || backend?.mode === 'stopping');
  const stopRequested = input.cancelling || input.stopCommandPending || backendStopping;
  const localTurnInFlight = !acpTerminal && Boolean(input.localTurnId) && (
    input.sending || input.awaitingResponse || input.waitingForOptimisticPrompt
  );
  const sending = input.sending && !acpTerminal;
  const acpActive = !acpTerminal && lifecycleAcpRunning;
  // Stop is a control-plane fact. A stale session snapshot must not win over
  // it and keep the composer in interaction-blocked mode.
  const waitingForUserInteraction = input.waitingForUserInteraction && !stopRequested;
  const initialTimelinePending = Boolean(input.initialTimelinePending);
  const staleTerminalSnapshot = acpTerminal;
  const cancelling = !acpTerminal && input.cancelling;
  const stopCommandPending = (!acpTerminal || backendWorkspaceTransition) && input.stopCommandPending;
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
    waitingForUserInteraction,
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
  const sessionActive = runtimeActive || acpActive || stopInProgress || waitingForUserInteraction;
  const activePromptLocked =
    sending ||
    waitingForOptimisticPrompt ||
    awaitingResponse ||
    initialTimelinePending ||
    sessionActive ||
    stopInProgress;
  const showExternalState = mode === 'invalid-workflow' || mode === 'runtime-error';
  const composerLocked = waitingForUserInteraction && !directQueueFacet;
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
    acpActive,
  );
  const directTurnHandoff = directQueueFacet
    && acpTerminal
    && processingKind === 'launching-next-node'
    && !turnSubmitting
    && !awaitingResponse;
  const statusActive = !sessionSuperseded &&
    !input.waitingForUserInteraction &&
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
      (backendWorkspaceTransition && Boolean(backend?.canStop)) ||
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
    showStatus: !sessionSuperseded && !input.waitingForUserInteraction && statusActive,
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
  const localAcpRevision = local.acp.revision;
  const incomingAcpRevision = incoming.acp.revision;
  if (
    localAcpRevision != null
    && incomingAcpRevision != null
    && incomingAcpRevision > localAcpRevision
  ) {
    return false;
  }
  const localRuntimeRevision = local.runtime.revision;
  const incomingRuntimeRevision = incoming.runtime.revision;
  if (
    localRuntimeRevision != null
    && incomingRuntimeRevision != null
    && incomingRuntimeRevision > localRuntimeRevision
  ) {
    return false;
  }
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
  lifecycle: Pick<ConversationAttemptLifecycleVm, 'acp'> | null | undefined,
) {
  return Boolean(lifecycle && isTerminalAcpFacet(lifecycle.acp));
}

/**
 * Pending interaction arrays may lag behind a lifecycle-only stop/terminal
 * update. The lifecycle is authoritative for the current turn, so the UI
 * must not keep rendering a permission or elicitation card from that stale
 * session projection.
 */
export function shouldHidePendingAcpInteractions(
  lifecycle: ConversationAttemptLifecycleVm | null | undefined,
  localTurnId: string | null | undefined,
  cancelling: boolean,
  stopCommandPending: boolean,
  interactionTurnId?: string | null,
) {
  if (cancelling || stopCommandPending || Boolean(lifecycle?.acp.stopping)) {
    return true;
  }
  if (!isTerminalAcpLifecycle(lifecycle)) return false;
  const terminalTurnId = lifecycle?.acp.turnId ?? null;
  if (terminalTurnId && interactionTurnId) {
    return terminalTurnId === interactionTurnId;
  }
  return !localTurnId || terminalTurnId === localTurnId;
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
    input.input.waitingForUserInteraction ||
    input.runtimeActive ||
    input.acpActive;
}

export function mergeConversationAttemptLifecycle(
  local: ConversationAttemptLifecycleVm | null | undefined,
  incoming: ConversationAttemptLifecycleVm,
): ConversationAttemptLifecycleVm {
  const acp = mergeConversationAcpFacet(local?.acp, incoming.acp);

  const localRuntimeRevision = local?.runtime.revision ?? 0;
  const incomingRuntimeRevision = incoming.runtime.revision ?? 0;
  const runtime = local && localRuntimeRevision > incomingRuntimeRevision
    ? local.runtime
    : incoming.runtime;
  const localQueue = local?.promptQueue;
  const incomingQueue = incoming.promptQueue;
  const promptQueue = mergeConversationPromptQueue(localQueue, incomingQueue);
  if (acp !== incoming.acp || runtime !== incoming.runtime || promptQueue !== incomingQueue) {
    const runtimeSource = local && runtime === local.runtime ? local : incoming;
    return deriveMergedLifecycleProjection({
      ...incoming,
      acp,
      runtime,
      promptQueue,
    }, runtimeSource);
  }
  if (localQueue && (!incomingQueue || localQueue.revision > incomingQueue.revision)) {
    return { ...incoming, promptQueue: localQueue };
  }
  return incoming;
}

export function mergeConversationAcpFacet(
  local: ConversationAttemptLifecycleVm['acp'] | null | undefined,
  incoming: ConversationAttemptLifecycleVm['acp'],
): ConversationAttemptLifecycleVm['acp'] {
  if (!local) return incoming;
  const localRevision = local.revision ?? 0;
  const incomingRevision = incoming.revision ?? 0;
  if (localRevision > incomingRevision) return local;
  if (
    localRevision === incomingRevision
    && local.turnId === incoming.turnId
    && isTerminalAcpFacet(local)
    && !isTerminalAcpFacet(incoming)
  ) {
    return local;
  }
  return incoming;
}

export function mergeConversationPromptQueue(
  local: ConversationAttemptLifecycleVm['promptQueue'],
  incoming: ConversationAttemptLifecycleVm['promptQueue'],
) {
  if (!incoming) return local;
  if (local && local.revision > incoming.revision) return local;
  return incoming;
}

/**
 * Replays only independently revisioned live control facts over a canonical
 * lifecycle. A router cache must never replay its derived Runtime display or
 * composer projection across mounts.
 */
export function mergeConversationAttemptLiveControlFacets(
  canonical: ConversationAttemptLifecycleVm,
  live: {
    acp?: ConversationAttemptLifecycleVm['acp'] | null;
    promptQueue?: ConversationAttemptLifecycleVm['promptQueue'];
  },
) {
  const acp = live.acp
    ? mergeConversationAcpFacet(canonical.acp, live.acp)
    : canonical.acp;
  const promptQueue = mergeConversationPromptQueue(canonical.promptQueue, live.promptQueue);
  if (acp === canonical.acp && promptQueue === canonical.promptQueue) return canonical;
  return deriveMergedLifecycleProjection({
    ...canonical,
    acp,
    promptQueue,
  }, canonical);
}

function deriveMergedLifecycleProjection(
  lifecycle: ConversationAttemptLifecycleVm,
  runtimeSource: ConversationAttemptLifecycleVm,
): ConversationAttemptLifecycleVm {
  const acpStopping = lifecycle.acp.stopping || lifecycle.acp.liveTurnActivity === 'cancel-requested';
  const acpActive = ['starting', 'accepted', 'running'].includes(lifecycle.acp.liveTurnActivity);
  const runtimeActive = lifecycle.runtime.active;
  const displayStatus = acpStopping
    ? 'cancelling'
    : acpActive && !runtimeActive
      ? lifecycle.acp.liveTurnActivity === 'starting' ? 'starting' : 'running'
      : lifecycle.runtime.status || runtimeSource.displayStatus;
  const runtimeDisplay = acpStopping
    ? {
        ...runtimeSource.runtimeDisplay,
        code: 'paused',
        tone: 'warning',
        icon: 'pause',
        terminal: false,
        resumable: false,
        blockingError: false,
      }
    : acpActive && !runtimeActive
      ? {
          ...runtimeSource.runtimeDisplay,
          code: 'running',
          tone: 'running',
          icon: 'dot',
          terminal: false,
          resumable: false,
          blockingError: false,
        }
      : runtimeSource.runtimeDisplay;
  const preserveSuperseded = runtimeSource.composer.mode === 'session-superseded';
  const processingCompletedLeafWorkspace = runtimeSource.composer.processingKind === 'processing-workspace'
    && lifecycle.runtime.phase === 'preparing-workspace';
  const mode = preserveSuperseded
    ? 'session-superseded'
    : acpStopping
      ? 'stopping'
      : runtimeActive || acpActive
        ? 'runtime-active'
        : runtimeDisplay.blockingError
          ? 'runtime-error'
          : 'normal';
  const processingKind = mode === 'stopping'
    ? 'stopping'
    : mode === 'runtime-active' && lifecycle.runtime.phase === 'launching-next-node'
      ? 'launching-next-node'
      : mode === 'runtime-active' && lifecycle.runtime.phase === 'preparing-workspace'
        ? processingCompletedLeafWorkspace
          ? 'processing-workspace'
          : 'preparing-workspace'
        : mode === 'runtime-active' && !runtimeActive && lifecycle.acp.liveTurnActivity === 'starting'
          ? 'launching'
          : 'processing';
  const composer = preserveSuperseded
    ? runtimeSource.composer
    : {
        ...runtimeSource.composer,
        mode,
        submitTarget: mode === 'normal' ? 'acp-prompt' : 'none',
        processingKind,
        statusKey: mode === 'stopping'
          ? 'acp.stopping'
          : mode === 'runtime-active' && lifecycle.runtime.phase === 'launching-next-node'
            ? 'conversation.runtime.launchingNextNode'
            : mode === 'runtime-active' && lifecycle.runtime.phase === 'preparing-workspace'
              ? processingCompletedLeafWorkspace
                ? 'conversation.runtime.processingWorkspace'
                : 'conversation.runtime.preparingDevelopmentEnvironment'
              : mode === 'runtime-active'
                ? 'conversation.runtime.runtimeActive'
                : null,
        canStop: runtimeActive || acpActive || acpStopping,
        lockInput: mode !== 'normal',
      };
  return {
    ...lifecycle,
    displayStatus,
    runtimeDisplay,
    continueKind: runtimeSource.continueKind,
    composer,
  };
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
    normalized === 'interaction-blocked' ||
    normalized === 'session-superseded' ||
    normalized === 'submitting'
  ) {
    return normalized;
  }
  return 'normal';
}

function composerModeFromBackend(input: {
  backendMode: AcpComposerMode;
  waitingForUserInteraction: boolean;
  stopInProgress: boolean;
  turnSubmitting: boolean;
  runtimeContinueBlockedByWorkflow: boolean;
  runtimeErrorMessage: string | null;
}): AcpComposerMode {
  if (input.waitingForUserInteraction) return 'interaction-blocked';
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
  if (mode === 'interaction-blocked') return 'none';
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
  acpActive: boolean,
): AcpComposerProcessingKind {
  if (stopInProgress) return 'stopping';
  if (turnSubmitting) return 'sending';
  if (backendProcessingKind === 'preparing-workspace') return 'preparing-workspace';
  if (backendProcessingKind === 'processing-workspace') return 'processing-workspace';
  if (backendProcessingKind === 'launching-next-node') return 'launching-next-node';
  if (awaitingResponse && input.turnAccepted && !input.hasResponseAfterTurn) return 'processing';
  if (input.initialTimelinePending) return 'launching';
  // Timeline is a historical data surface. Once the current ACP turn is
  // terminal, its last textDelta/thought/tool item must not be reused as the
  // current composer activity. Runtime-controlled work still gets its phase
  // from the canonical runtime facet; an idle terminal session has no active
  // processing kind at all (the neutral value is kept for the API shape).
  if (!acpActive) return 'processing';
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
  if (input.waitingForUserInteraction) return 'runtime-controlled';
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
    normalized === 'processing-workspace' ||
    normalized === 'launching-next-node'
  ) {
    return normalized;
  }
  return 'processing';
}

function normalizeStatus(status?: string | null) {
  return status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}
