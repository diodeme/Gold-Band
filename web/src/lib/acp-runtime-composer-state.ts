import type { ConversationAttemptLifecycleVm } from '@/types';

export type AcpComposerMode =
  | 'normal'
  | 'runtime-active'
  | 'stopping'
  | 'invalid-workflow'
  | 'runtime-error'
  | 'permission-blocked'
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

export type AcpComposerHintKind =
  | 'default'
  | 'permission-pending'
  | 'stopping'
  | 'sending'
  | 'status'
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
  waitingForPermission: boolean;
  sending: boolean;
  awaitingResponse: boolean;
  waitingForOptimisticPrompt: boolean;
  localTurnInFlight?: boolean;
  cancelling: boolean;
  stopCommandPending: boolean;
  turnAccepted: boolean;
  hasResponseAfterTurn: boolean;
  hasTimelineItems: boolean;
  hasEffectiveEvents: boolean;
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
  hintKind: AcpComposerHintKind;
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
  const localTurnInFlight = Boolean(input.localTurnInFlight);
  const lifecycleAcpRunning = ['starting', 'accepted', 'running'].includes(
    input.lifecycle?.acp.liveTurnActivity ?? 'idle',
  );
  const acpTerminal = !localTurnInFlight && !lifecycleAcpRunning && (
    (input.lifecycle?.acp.latestTurnStatus ?? 'none') !== 'none'
      || (!input.lifecycle && isSessionTerminalStatus(input.acpStatus))
  );
  const acpActive = !acpTerminal && lifecycleAcpRunning;
  const backendStopping = !acpTerminal && (Boolean(input.lifecycle?.acp.stopping) || backend?.mode === 'stopping');
  const waitingForPermission = input.waitingForPermission;
  const staleTerminalSnapshot = acpTerminal && !localTurnInFlight;
  const cancelling = !acpTerminal && input.cancelling;
  const stopCommandPending = (!acpTerminal || backendWorkspacePreparing) && input.stopCommandPending;
  const stopInProgress = cancelling || stopCommandPending || backendStopping;
  const waitingForOptimisticPrompt = !staleTerminalSnapshot && input.waitingForOptimisticPrompt;
  const turnSubmitting = (input.sending || waitingForOptimisticPrompt) && !input.turnAccepted;
  const awaitingResponse = !staleTerminalSnapshot && input.awaitingResponse;
  const runtimeErrorMessage = runtimeErrorMessageFromInput(input);
  const runtimeContinueBlockedByWorkflow = false;
  const reportedBackendMode = normalizeComposerMode(backend?.mode);
  const backendMode = acpTerminal && reportedBackendMode === 'stopping'
    ? 'normal'
    : reportedBackendMode;
  const mode = composerModeFromBackend({
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
    input.sending ||
    waitingForOptimisticPrompt ||
    awaitingResponse ||
    sessionActive ||
    stopInProgress;
  const showExternalState = mode === 'invalid-workflow' || mode === 'runtime-error';
  const composerLocked = waitingForPermission && !directQueueFacet;
  const staleStoppingBackend = acpTerminal && reportedBackendMode === 'stopping';
  const backendInputLocked = !staleStoppingBackend && mode !== 'normal' && Boolean(backend?.lockInput);
  const directInputDisabled =
    stopInProgress ||
    mode === 'invalid-workflow' ||
    mode === 'runtime-error';
  const inputDisabled = (
    directQueueFacet
      ? directInputDisabled
      : composerLocked || backendInputLocked || activePromptLocked || mode === 'invalid-workflow' || mode === 'runtime-error'
  );
  const canSubmit = Boolean(input.prompt.trim())
    && submitTarget !== 'none'
    && !queueAtCapacity
    && !(input.sending && submitTarget !== 'queue-prompt')
    && !inputDisabled;
  const processingKind = processingKindForInput(
    input,
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
  const statusActive =
    !input.waitingForPermission &&
    !composerLocked &&
    !directTurnHandoff &&
    (turnSubmitting || awaitingResponse || sessionActive || stopInProgress || mode === 'runtime-active');
  const externalMessage = externalMessageForMode(input, mode, runtimeErrorMessage);

  return {
    mode,
    submitTarget,
    inputDisabled,
    canSubmit,
    canStop:
      (!acpTerminal && Boolean(backend?.canStop)) ||
      (backendWorkspacePreparing && Boolean(backend?.canStop)) ||
      sessionActive ||
      awaitingResponse ||
      input.sending ||
      waitingForOptimisticPrompt ||
      localTurnInFlight ||
      cancelling,
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
    showStatus: !input.waitingForPermission && statusActive,
    placeholderKind: directQueueFacet
      ? 'default'
      : placeholderKindForMode(input, mode, activePromptLocked),
    hintKind: hintKindForMode(input, mode, statusActive, turnSubmitting),
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
    incoming.continueKind === 'action' &&
    incoming.composer.mode === 'normal'
  );
}

export function shouldSettleRuntimeContinueSubmission(
  submitting: boolean,
  showRuntimeContinueAction: boolean,
) {
  return submitting && !showRuntimeContinueAction;
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
  const localQueue = local?.promptQueue;
  const incomingQueue = incoming.promptQueue;
  if (localQueue && (!incomingQueue || localQueue.revision > incomingQueue.revision)) {
    return { ...incoming, promptQueue: localQueue };
  }
  return incoming;
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
  if (!input.hasTimelineItems) return input.hasEffectiveEvents ? 'processing' : 'launching';
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

function hintKindForMode(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  statusActive: boolean,
  turnSubmitting: boolean,
): AcpComposerHintKind {
  if (input.waitingForPermission) return 'permission-pending';
  if (mode === 'stopping') return 'stopping';
  if (mode === 'invalid-workflow' || mode === 'runtime-error') return 'message';
  if (turnSubmitting) return 'sending';
  if (statusActive) return 'status';
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
