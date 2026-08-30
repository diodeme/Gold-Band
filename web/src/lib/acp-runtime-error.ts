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

/**
 * Maps a structured runtime error to localized banner copy.
 * Returns null for unknown codes so callers can keep their existing fallback.
 */
export function acpRuntimeErrorBannerCopy(
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
