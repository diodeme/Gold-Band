import { describe, expect, it } from 'vitest';

import { acpRuntimeErrorBannerCopy } from '@/lib/acp-runtime-error';
import type { RuntimeErrorInfoVm } from '@/types';

function runtimeError(overrides: Partial<RuntimeErrorInfoVm> = {}): RuntimeErrorInfoVm {
  return {
    code: { domain: 'config', code: 'acp.session-config-value-unavailable' },
    domain: 'config',
    recovery: 'manual',
    retryPolicy: null,
    params: {},
    diagnostic: 'ACP session config value `high` is unavailable for `reasoning_effort`',
    raw: null,
    ...overrides,
  };
}

function fakeT(prefix: string) {
  return (key: string, values?: Record<string, unknown>) => {
    if (!values) return `${prefix}:${key}`;
    const interpolated = Object.entries(values)
      .map(([name, value]) => `${name}=${String(value)}`)
      .join('|');
    return `${prefix}:${key}(${interpolated})`;
  };
}

describe('acpRuntimeErrorBannerCopy', () => {
  it('maps a removed thought-level option to unsupported-model copy', () => {
    const copy = acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      params: {
        category: 'thought_level',
        configId: 'reasoning_effort',
        value: 'high',
        availableValues: [],
      },
    }));

    expect(copy).toBe('zh:conversation.runtime.sessionConfigThoughtLevelUnsupported(value=high)');
  });

  it('maps a shrunken thought-level option list to unavailable-value copy', () => {
    const copy = acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      params: {
        category: 'thought_level',
        configId: 'reasoning_effort',
        value: 'max',
        availableValues: ['low', 'high'],
      },
    }));

    expect(copy).toBe(
      'zh:conversation.runtime.sessionConfigThoughtLevelValueUnavailable(value=max|values=low, high)',
    );
  });

  it('falls back to a generic config-option copy for other categories', () => {
    const copy = acpRuntimeErrorBannerCopy(fakeT('en'), runtimeError({
      params: {
        category: 'config',
        configId: 'collaboration_mode',
        value: 'plan',
        availableValues: ['default'],
      },
    }));

    expect(copy).toBe(
      'en:conversation.runtime.sessionConfigValueUnavailable(configId=collaboration_mode|value=plan|values=default)',
    );
  });

  it('omits the available-values suffix when the agent removed the option values', () => {
    const copy = acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      params: {
        category: 'config',
        configId: 'reasoning_effort',
        value: 'high',
        availableValues: [],
      },
    }));

    expect(copy).toBe(
      'zh:conversation.runtime.sessionConfigValueUnavailableNoValues(configId=reasoning_effort|value=high)',
    );
  });

  it('returns null for unknown codes so callers keep their existing fallback', () => {
    expect(acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      code: { domain: 'provider', code: 'acp.initialize-failed' },
    }))).toBeNull();
    expect(acpRuntimeErrorBannerCopy(fakeT('zh'), null)).toBeNull();
  });
});
