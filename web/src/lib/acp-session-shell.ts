import type { ConversationAttemptLifecycleVm } from '@/types';

export interface AcpLiveSessionShellPolicyInput {
  runtimeActive: boolean;
  allowEventOnlySessionShell: boolean;
  loadedEventCount: number;
}

export type AcpSessionShellState =
  | 'available'
  | 'initializing'
  | 'loading'
  | 'missing'
  | 'interrupted'
  | 'error';

const MISSING_ACP_SESSION_RETRY_DELAYS_MS = [
  120,
  300,
  700,
  1_200,
  2_000,
  3_000,
  5_000,
  5_000,
  5_000,
  5_000,
  5_000,
];

export interface AcpSessionShellStateInput {
  hasBaseSession: boolean;
  baseSessionReady: boolean;
  hasLiveSessionShell: boolean;
  hasEstablishedSessionShell?: boolean;
  hasSettledAttemptShell?: boolean;
  initialSessionLoading: boolean;
  initialSessionLoadFailed?: boolean;
  initializationInterrupted?: boolean;
  initializationFailed?: boolean;
  runtimeActive?: boolean;
  showInitializingShell?: boolean;
}

export type AcpTimelineSurfaceState = 'pending' | 'timeline' | 'empty';

export interface AcpTimelineSurfaceStateInput {
  hasTimelineItems: boolean;
  initialSessionLoading: boolean;
  runtimeActive: boolean;
  sending: boolean;
}

export interface AcpSessionInitializationInterruptedInput {
  orchestrated: boolean;
  runtimeStatus?: string | null;
  runtimePauseReason?: string | null;
  runtimeActive: boolean;
  sessionId?: string | null;
  sessionEstablished?: boolean;
  baseSessionReady: boolean;
  loadedEventCount: number;
}

export interface AcpSessionInitializationFailedInput {
  runtimeStatus?: string | null;
  runtimePauseReason?: string | null;
  runtimeActive: boolean;
  runtimeComposerMode?: string | null;
  runtimeErrorMessage?: string | null;
  sessionId?: string | null;
  sessionEstablished?: boolean;
  baseSessionReady: boolean;
  loadedEventCount: number;
}

export interface AcpCancelledDirectAttemptShellInput {
  isOrchestrated: boolean;
  lifecycle?: ConversationAttemptLifecycleVm | null;
}

export function shouldCreateLiveAcpSessionShell(input: AcpLiveSessionShellPolicyInput) {
  if (!input.allowEventOnlySessionShell) return false;
  return input.runtimeActive || input.loadedEventCount > 0;
}

/**
 * A Direct attempt remains a usable conversation even when its first turn is
 * stopped before the provider session is materialized. The attempt lifecycle
 * is authoritative here; this shell must not be treated as a provider session.
 */
export function shouldCreateCancelledDirectAttemptShell(
  input: AcpCancelledDirectAttemptShellInput,
) {
  const lifecycle = input.lifecycle;
  return Boolean(
    !input.isOrchestrated &&
    lifecycle?.runtime.current &&
    !lifecycle.runtime.active &&
    normalizeLifecycleCode(lifecycle.runtime.status) === 'paused' &&
    normalizeLifecycleCode(lifecycle.runtime.pauseReason) === 'process-interrupted' &&
    normalizeLifecycleCode(lifecycle.acp.sessionAvailability) === 'unavailable' &&
    normalizeLifecycleCode(lifecycle.acp.liveTurnActivity) === 'idle' &&
    normalizeLifecycleCode(lifecycle.acp.latestTurnStatus) === 'cancelled' &&
    !lifecycle.acp.stopping &&
    normalizeLifecycleCode(lifecycle.composer.mode) === 'normal' &&
    normalizeLifecycleCode(lifecycle.composer.submitTarget) === 'acp-prompt' &&
    !lifecycle.composer.lockInput
  );
}

export function resolveAcpSessionShellState(input: AcpSessionShellStateInput): AcpSessionShellState {
  if (input.initializationFailed) return 'error';
  if (input.initializationInterrupted) return 'interrupted';
  if (input.hasSettledAttemptShell) return 'available';
  if (input.initialSessionLoadFailed) return 'error';
  if (
    input.showInitializingShell &&
    !input.hasLiveSessionShell &&
    (input.initialSessionLoading || !input.baseSessionReady)
  ) return 'initializing';
  if (input.initialSessionLoading) return 'loading';
  if (input.hasEstablishedSessionShell) return 'available';
  if (input.hasBaseSession && (!input.initialSessionLoading || input.baseSessionReady)) return 'available';
  if (input.hasLiveSessionShell) return 'available';
  if (input.hasBaseSession) return 'available';
  if (input.runtimeActive) return 'loading';
  return 'missing';
}

export function isAcpSessionLoadingSurfaceState(state: AcpSessionShellState) {
  return state === 'loading';
}

export function resolveAcpTimelineSurfaceState(
  input: AcpTimelineSurfaceStateInput,
): AcpTimelineSurfaceState {
  if (input.hasTimelineItems) return 'timeline';
  if (
    input.initialSessionLoading ||
    input.runtimeActive ||
    input.sending
  ) return 'pending';
  return 'empty';
}

export function isAcpSessionInitializationFailed(input: AcpSessionInitializationFailedInput) {
  const runtimeStatus = normalizeLifecycleCode(input.runtimeStatus);
  const runtimePauseReason = normalizeLifecycleCode(input.runtimePauseReason);
  const runtimeStoppedWithFailure =
    runtimePauseReason === 'runtime-abnormal' ||
    runtimePauseReason === 'error-blocked' ||
    ['failed', 'failure', 'error', 'killed'].includes(runtimeStatus);
  const composerStoppedWithFailure =
    normalizeLifecycleCode(input.runtimeComposerMode) === 'runtime-error';
  return (
    !input.runtimeActive &&
    (runtimeStoppedWithFailure || composerStoppedWithFailure) &&
    !input.sessionEstablished &&
    !input.sessionId?.trim() &&
    !input.baseSessionReady &&
    input.loadedEventCount === 0
  );
}

export function isAcpSessionInitializationInterrupted(
  input: AcpSessionInitializationInterruptedInput,
) {
  const runtimeStatus = normalizeLifecycleCode(input.runtimeStatus);
  const pauseReason = normalizeLifecycleCode(input.runtimePauseReason);
  return (
    input.orchestrated &&
    !input.runtimeActive &&
    runtimeStatus === 'paused' &&
    pauseReason === 'process-interrupted' &&
    !input.sessionEstablished &&
    !input.sessionId?.trim() &&
    !input.baseSessionReady &&
    input.loadedEventCount === 0
  );
}

export function missingAcpSessionRetryDelay(attempt: number) {
  return MISSING_ACP_SESSION_RETRY_DELAYS_MS[attempt] ?? null;
}

export interface AcpSessionMetadataInput {
  systemPromptAppend?: string | null;
  config?: {
    currentModelId?: string | null;
    currentModeId?: string | null;
    models?: unknown | null;
    modes?: unknown | null;
    configOptions?: unknown | null;
  } | null;
}

export function hasAcpSessionMetadata(session: AcpSessionMetadataInput | null | undefined): boolean {
  if (!session) return false;
  return Boolean(session.systemPromptAppend?.trim()) && hasAcpSessionConfigChoices(session.config);
}

function hasAcpSessionConfigChoices(config: AcpSessionMetadataInput['config']): boolean {
  if (!config) return false;
  const hasModelChoices =
    hasConfigOption(config.models, 'availableModels') ||
    hasSelectConfigOption(config.configOptions, 'model') ||
    Boolean(config.currentModelId);
  const hasModeChoices =
    hasConfigOption(config.modes, 'availableModes') ||
    hasSelectConfigOption(config.configOptions, 'mode') ||
    Boolean(config.currentModeId);
  return hasModelChoices && hasModeChoices;
}

function hasConfigOption(value: unknown, key: string): boolean {
  return Boolean(
    value &&
      typeof value === 'object' &&
      !Array.isArray(value) &&
      Array.isArray((value as Record<string, unknown>)[key]) &&
      ((value as Record<string, unknown>)[key] as unknown[]).length > 0,
  );
}

function hasSelectConfigOption(value: unknown, category: string): boolean {
  return Boolean(
    Array.isArray(value) &&
      value.some((item) => {
        if (!item || typeof item !== 'object' || Array.isArray(item)) return false;
        const option = item as Record<string, unknown>;
        const matches = option.id === category || option.category === category;
        return matches && Array.isArray(option.options) && option.options.length > 0;
      }),
  );
}

function normalizeLifecycleCode(value?: string | null) {
  return value?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}
