export interface AcpLiveSessionShellPolicyInput {
  runtimeActive: boolean;
  allowEventOnlySessionShell: boolean;
  loadedEventCount: number;
}

export type AcpSessionShellState = 'available' | 'loading' | 'missing';

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
  initialSessionLoading: boolean;
}

export function shouldCreateLiveAcpSessionShell(input: AcpLiveSessionShellPolicyInput) {
  if (!input.allowEventOnlySessionShell) return false;
  return input.runtimeActive || input.loadedEventCount > 0;
}

export function resolveAcpSessionShellState(input: AcpSessionShellStateInput): AcpSessionShellState {
  if (input.hasBaseSession && (!input.initialSessionLoading || input.baseSessionReady)) return 'available';
  if (input.hasLiveSessionShell) return 'available';
  if (input.initialSessionLoading) return 'loading';
  if (input.hasBaseSession) return 'available';
  return 'missing';
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
