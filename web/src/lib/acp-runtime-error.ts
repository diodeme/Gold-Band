import type { RuntimeErrorInfoVm } from '@/types';

export const ACP_SESSION_CONFIG_VALUE_UNAVAILABLE_CODE = 'acp.session-config-value-unavailable';
export const ACP_THOUGHT_LEVEL_CATEGORY = 'thought_level';
export const WORKSPACE_WORKTREE_CREATE_FAILED_CODE = 'workspace.worktree-create-failed';

type Translate = (key: string, values?: Record<string, unknown>) => string;

function stringParam(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function stringArrayParam(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && item.trim() !== '')
    : [];
}

/** Keeps the current turn's original reason when no product mapping exists. */
export function acpRuntimeErrorBannerCopy(
  t: Translate,
  runtimeError: RuntimeErrorInfoVm | null | undefined,
): string | null {
  if (!runtimeError) return null;
  const summary = localizedRuntimeErrorSummary(t, runtimeError)
    ?? t(`errors.${runtimeError.code.code}`, { ...runtimeError.params, defaultValue: '' });
  const raw = runtimeError.raw as { data?: { details?: unknown; message?: unknown }; message?: unknown } | null;
  const extra = raw?.data?.details ?? raw?.data?.message ?? raw?.message;
  const diagnostic = runtimeError.diagnostic?.trim() ? runtimeError.diagnostic : '';
  const detail = typeof extra === 'string' && extra.trim() && !diagnostic.includes(extra) ? extra : '';
  const reason = [diagnostic, detail].filter(Boolean).join('\n');
  return [summary, reason].filter(Boolean).join('\n') || null;
}

function localizedRuntimeErrorSummary(
  t: Translate,
  runtimeError: RuntimeErrorInfoVm | null | undefined,
): string | null {
  if (!runtimeError) {
    return null;
  }
  if (runtimeError.code?.code === WORKSPACE_WORKTREE_CREATE_FAILED_CODE) {
    return t('conversation.runtime.worktreeCreateFailed');
  }
  if (runtimeError.code?.code !== ACP_SESSION_CONFIG_VALUE_UNAVAILABLE_CODE) return null;
  const params = runtimeError.params ?? {};
  const value = stringParam(params.value);
  const availableValues = stringArrayParam(params.availableValues);
  if (stringParam(params.category) === ACP_THOUGHT_LEVEL_CATEGORY) {
    return availableValues.length > 0
      ? t('conversation.runtime.sessionConfigThoughtLevelValueUnavailable', { value, values: availableValues.join(', ') })
      : t('conversation.runtime.sessionConfigThoughtLevelUnsupported', { value });
  }
  const configId = stringParam(params.configId) || runtimeError.code?.code;
  return availableValues.length > 0
    ? t('conversation.runtime.sessionConfigValueUnavailable', {
      configId,
      value,
      values: availableValues.join(', '),
    })
    : t('conversation.runtime.sessionConfigValueUnavailableNoValues', { configId, value });
}
