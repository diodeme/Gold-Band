import type { ConversationAttemptLifecycleVm } from '@/types';

export type AcpComposerMode =
  | 'normal'
  | 'runtime-active'
  | 'stopping'
  | 'interrupted-input'
  | 'paused-action'
  | 'invalid-workflow'
  | 'runtime-error'
  | 'permission-blocked'
  | 'submitting';

export type AcpComposerSubmitTarget =
  | 'acp-prompt'
  | 'runtime-continue'
  | 'permission-response'
  | 'none';

export type AcpComposerProcessingKind =
  | 'sending'
  | 'launching'
  | 'processing'
  | 'thinking'
  | 'tool'
  | 'responding'
  | 'stopping'
  | 'launching-next-node';

export type AcpComposerPlaceholderKind =
  | 'default'
  | 'runtime-controlled'
  | 'stopping'
  | 'stopped'
  | 'plan-intervention'
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
  workflowValid: boolean;
  workflowInvalidMessage?: string | null;
  pauseMessage?: string | null;
  runtimeErrorMessage?: string | null;
  acpStatus?: string | null;
  prompt: string;
  waitingForPermission: boolean;
  hasPlanIntervention: boolean;
  sending: boolean;
  awaitingResponse: boolean;
  waitingForOptimisticPrompt: boolean;
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
  externalKind: 'invalid-workflow' | 'paused' | 'runtime-error' | null;
  externalMessage?: string | null;
  showContinueAction: boolean;
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
  const runtimeActive = Boolean(input.lifecycle?.runtime.active);
  const acpActive = Boolean(input.lifecycle?.acp.active);
  const acpTerminal = Boolean(input.lifecycle?.acp.terminal);
  const backendStopping = Boolean(input.lifecycle?.acp.stopping) || backend?.mode === 'stopping';
  const waitingForPermission = input.waitingForPermission && !input.hasPlanIntervention;
  const cancelling = !acpTerminal && input.cancelling;
  const stopCommandPending = !acpTerminal && input.stopCommandPending;
  const stopInProgress = cancelling || stopCommandPending || backendStopping;
  const turnSubmitting = (input.sending || input.waitingForOptimisticPrompt) && !input.turnAccepted;
  const awaitingResponse = !acpTerminal && input.awaitingResponse;
  const runtimeContinueKind = runtimeContinueKindFromInput(input);
  const runtimeErrorMessage = runtimeErrorMessageFromInput(input);
  const runtimeContinueBlockedByWorkflow = runtimeContinueKind != null && !input.workflowValid;
  const backendMode = normalizeComposerMode(backend?.mode);
  const mode = composerModeFromBackend({
    backendMode,
    waitingForPermission,
    stopInProgress,
    turnSubmitting,
    runtimeContinueBlockedByWorkflow,
    runtimeErrorMessage,
  });
  const submitTarget = submitTargetFromBackend(input, mode, backend?.submitTarget);
  const sessionActive = runtimeActive || acpActive || stopInProgress;
  const activePromptLocked =
    input.sending ||
    input.waitingForOptimisticPrompt ||
    awaitingResponse ||
    sessionActive ||
    stopInProgress;
  const showContinueAction = mode === 'paused-action' || Boolean(backend?.showContinueAction);
  const showExternalState =
    mode === 'invalid-workflow' || mode === 'paused-action' || mode === 'runtime-error';
  const composerLocked = waitingForPermission;
  const inputDisabled = (composerLocked || backend?.lockInput || activePromptLocked || showContinueAction || mode === 'invalid-workflow' || mode === 'runtime-error') && !input.hasPlanIntervention;
  const canSubmit = Boolean(input.prompt.trim()) && submitTarget !== 'none' && !inputDisabledForSubmit(inputDisabled, input.hasPlanIntervention, mode);
  const processingKind = processingKindForInput(
    input,
    stopInProgress,
    turnSubmitting,
    awaitingResponse,
    normalizeProcessingKind(backend?.processingKind),
  );
  const statusActive =
    !input.waitingForPermission &&
    !composerLocked &&
    (turnSubmitting || awaitingResponse || sessionActive || stopInProgress || mode === 'runtime-active');
  const externalMessage = externalMessageForMode(input, mode, runtimeErrorMessage);

  return {
    mode,
    submitTarget,
    inputDisabled,
    canSubmit,
    canStop:
      Boolean(backend?.canStop) ||
      sessionActive ||
      awaitingResponse ||
      input.sending ||
      input.waitingForOptimisticPrompt ||
      cancelling,
    stopInProgress,
    sessionActive,
    acpActive,
    runtimeActive,
    composerLocked,
    showExternalState,
    externalKind: externalKindForMode(mode),
    externalMessage,
    showContinueAction,
    processingKind,
    statusActive,
    showStatus: !input.waitingForPermission && statusActive,
    placeholderKind: placeholderKindForMode(input, mode, activePromptLocked),
    hintKind: hintKindForMode(input, mode, statusActive, turnSubmitting),
    message: externalMessage,
  };
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
    normalized === 'interrupted-input' ||
    normalized === 'paused-action' ||
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
  if (input.runtimeErrorMessage) return 'runtime-error';
  return input.backendMode;
}

function submitTargetFromBackend(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  backendSubmitTarget?: string | null,
): AcpComposerSubmitTarget {
  if (mode === 'permission-blocked' || input.hasPlanIntervention) return 'permission-response';
  if (mode === 'invalid-workflow' || mode === 'runtime-error' || mode === 'stopping') return 'none';
  const normalized = normalizeStatus(backendSubmitTarget);
  if (
    normalized === 'acp-prompt' ||
    normalized === 'runtime-continue' ||
    normalized === 'permission-response' ||
    normalized === 'none'
  ) {
    return normalized;
  }
  return 'none';
}

function inputDisabledForSubmit(inputDisabled: boolean, hasPlanIntervention: boolean, mode: AcpComposerMode) {
  if (hasPlanIntervention) return false;
  if (mode === 'interrupted-input' || mode === 'normal') return false;
  return inputDisabled;
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
  if (input.hasPlanIntervention) return 'plan-intervention';
  if (mode === 'stopping') return 'stopping';
  if (mode === 'interrupted-input') return 'stopped';
  if (mode === 'paused-action' || mode === 'invalid-workflow' || mode === 'runtime-error') return 'message';
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
  if (mode === 'paused-action' || mode === 'invalid-workflow' || mode === 'runtime-error') return 'message';
  if (turnSubmitting) return 'sending';
  if (statusActive) return 'status';
  return 'default';
}

function externalKindForMode(mode: AcpComposerMode) {
  if (mode === 'invalid-workflow') return 'invalid-workflow' as const;
  if (mode === 'paused-action') return 'paused' as const;
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
  if (mode === 'paused-action') return input.pauseMessage ?? null;
  return null;
}

function runtimeContinueKindFromInput(input: AcpRuntimeComposerStateInput): 'input' | 'action' | null {
  const lifecycleKind = input.lifecycle?.continueKind;
  if (lifecycleKind === 'input' || lifecycleKind === 'action') return lifecycleKind;
  return null;
}

function runtimeErrorMessageFromInput(input: AcpRuntimeComposerStateInput) {
  if (input.runtimeErrorMessage) return input.runtimeErrorMessage;
  if (input.lifecycle?.composer.mode === 'runtime-error') return 'runtime-error';
  return null;
}

function normalizeProcessingKind(kind?: string | null): AcpComposerProcessingKind {
  const normalized = normalizeStatus(kind);
  if (
    normalized === 'sending' ||
    normalized === 'launching' ||
    normalized === 'processing' ||
    normalized === 'thinking' ||
    normalized === 'tool' ||
    normalized === 'responding' ||
    normalized === 'stopping' ||
    normalized === 'launching-next-node'
  ) {
    return normalized;
  }
  return 'processing';
}

function normalizeStatus(status?: string | null) {
  return status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}
