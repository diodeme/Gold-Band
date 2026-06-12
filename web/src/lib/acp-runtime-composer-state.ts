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
  | 'stopping';

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
  legacyRuntimeStatus?: string | null;
  legacyRuntimeDisplay?: {
    code?: string | null;
    tone?: string | null;
    terminal?: boolean | null;
    resumable?: boolean | null;
    reasonCode?: string | null;
  } | null;
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
  const acpActive = isSessionActiveStatus(input.acpStatus);
  const acpStopping = isSessionStopPending(input.acpStatus) || Boolean(input.lifecycle?.acp.stopping);
  const runtimeActive = input.lifecycle?.runtime.active ?? isRuntimeActiveStatus(input.legacyRuntimeStatus);
  const sessionActive = acpActive || runtimeActive;
  const composerLocked = input.waitingForPermission && !input.hasPlanIntervention;
  const stopInProgress =
    input.cancelling ||
    input.stopCommandPending ||
    acpStopping ||
    (isInterruptedInput(input) && acpActive);
  const turnSubmitting = (input.sending || input.waitingForOptimisticPrompt) && !input.turnAccepted;
  const activePromptLocked =
    input.sending ||
    input.awaitingResponse ||
    input.waitingForOptimisticPrompt ||
    sessionActive ||
    stopInProgress;
  const runtimeContinueKind = runtimeContinueKindFromInput(input);
  const runtimeErrorMessage = runtimeErrorMessageFromInput(input);
  const runtimeContinueBlockedByWorkflow = runtimeContinueKind != null && !input.workflowValid;

  const mode = composerMode({
    input,
    composerLocked,
    stopInProgress,
    turnSubmitting,
    runtimeContinueKind,
    runtimeContinueBlockedByWorkflow,
    runtimeErrorMessage,
    runtimeActive,
    activePromptLocked,
  });
  const showContinueAction = mode === 'paused-action';
  const showExternalState =
    mode === 'invalid-workflow' || mode === 'paused-action' || mode === 'runtime-error';
  const submitTarget = submitTargetForMode(input, mode, runtimeContinueKind);
  const inputDisabled = (composerLocked || activePromptLocked || showContinueAction || mode === 'invalid-workflow' || mode === 'runtime-error') && !input.hasPlanIntervention;
  const canSubmit = Boolean(input.prompt.trim()) && submitTarget !== 'none' && !inputDisabledForSubmit(inputDisabled, input.hasPlanIntervention, mode);
  const processingKind = processingKindForInput(input, stopInProgress, turnSubmitting);
  const statusActive =
    !input.waitingForPermission &&
    !composerLocked &&
    (turnSubmitting || input.awaitingResponse || sessionActive || stopInProgress);
  const externalMessage = externalMessageForMode(input, mode, runtimeErrorMessage);

  return {
    mode,
    submitTarget,
    inputDisabled,
    canSubmit,
    canStop:
      sessionActive ||
      input.awaitingResponse ||
      input.sending ||
      input.waitingForOptimisticPrompt ||
      input.cancelling,
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

export function isRuntimeActiveStatus(status?: string | null) {
  return ['pending', 'running', 'in-progress', 'in_progress', 'active'].includes(normalizeStatus(status));
}

function composerMode(input: {
  input: AcpRuntimeComposerStateInput;
  composerLocked: boolean;
  stopInProgress: boolean;
  turnSubmitting: boolean;
  runtimeContinueKind: 'input' | 'action' | null;
  runtimeContinueBlockedByWorkflow: boolean;
  runtimeErrorMessage: string | null;
  runtimeActive: boolean;
  activePromptLocked: boolean;
}): AcpComposerMode {
  if (input.composerLocked) return 'permission-blocked';
  if (input.stopInProgress) return 'stopping';
  if (input.turnSubmitting) return 'submitting';
  if (input.runtimeContinueBlockedByWorkflow) return 'invalid-workflow';
  if (input.runtimeErrorMessage) return 'runtime-error';
  if (input.runtimeContinueKind === 'input') return 'interrupted-input';
  if (input.runtimeContinueKind === 'action') return 'paused-action';
  if (input.runtimeActive || input.activePromptLocked) return 'runtime-active';
  return 'normal';
}

function submitTargetForMode(
  input: AcpRuntimeComposerStateInput,
  mode: AcpComposerMode,
  runtimeContinueKind: 'input' | 'action' | null,
): AcpComposerSubmitTarget {
  if (mode === 'permission-blocked' || input.hasPlanIntervention) return 'permission-response';
  if (mode === 'interrupted-input') return 'runtime-continue';
  if (mode === 'normal') return 'acp-prompt';
  if (runtimeContinueKind === 'action') return 'none';
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
): AcpComposerProcessingKind {
  if (stopInProgress) return 'stopping';
  if (turnSubmitting) return 'sending';
  if (input.awaitingResponse && input.turnAccepted && !input.hasResponseAfterTurn) return 'processing';
  if (!input.hasTimelineItems) return input.hasEffectiveEvents ? 'processing' : 'launching';
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
  const display = input.legacyRuntimeDisplay;
  if (display?.code !== 'paused' || !display.resumable) return null;
  if (display.reasonCode === 'process-interrupted') return 'input';
  if (display.reasonCode === 'waiting-for-user-input') return 'action';
  return null;
}

function runtimeErrorMessageFromInput(input: AcpRuntimeComposerStateInput) {
  if (input.runtimeErrorMessage) return input.runtimeErrorMessage;
  const display = input.legacyRuntimeDisplay;
  if (display?.code === 'error-blocked') return 'runtime-error';
  if (display?.tone === 'danger' && display.terminal) return 'runtime-error';
  return null;
}

function isInterruptedInput(input: AcpRuntimeComposerStateInput) {
  return runtimeContinueKindFromInput(input) === 'input';
}

function normalizeStatus(status?: string | null) {
  return status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}
