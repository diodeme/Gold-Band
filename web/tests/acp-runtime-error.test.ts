import { describe, expect, it } from 'vitest';

import { acpRuntimeErrorBannerCopy, acpRuntimeErrorBannerTitle } from '@/lib/acp-runtime-error';
import type { RuntimeErrorInfoVm } from '@/types';
import i18n, { loadI18nLanguage } from '@/i18n';

function runtimeError(overrides: Partial<RuntimeErrorInfoVm> = {}): RuntimeErrorInfoVm {
  return {
    code: { domain: 'config', code: 'acp.session-config-value-unavailable' },
    domain: 'config',
    recovery: 'manual',
    retryPolicy: null,
    params: {},
    diagnostic: '',
    raw: null,
    ...overrides,
  };
}

function fakeT(prefix: string) {
  return (key: string, values?: Record<string, unknown>) => {
    if (key.startsWith('errors.')) return String(values?.defaultValue ?? key);
    if (!values) return `${prefix}:${key}`;
    const interpolated = Object.entries(values)
      .map(([name, value]) => `${name}=${String(value)}`)
      .join('|');
    return `${prefix}:${key}(${interpolated})`;
  };
}

describe('acpRuntimeErrorBannerCopy', () => {
  it('localizes storage failures even without diagnostic text', async () => {
    await loadI18nLanguage('zh-CN');
    expect(acpRuntimeErrorBannerCopy(i18n.t.bind(i18n), runtimeError({
      code: { domain: 'runtime-io', code: 'runtime.io.storage-full' },
      domain: 'runtime-io',
    }))).toContain('磁盘空间不足');
  });
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

  it('maps worktree creation failures to localized product copy', () => {
    const copy = acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      code: { domain: 'workspace', code: 'workspace.worktree-create-failed' },
      domain: 'workspace',
      params: { branch: 'gold-band/conversation/conflict' },
      diagnostic: 'git worktree add failed: branch already exists',
    }));

    expect(copy).toBe('zh:conversation.runtime.worktreeCreateFailed\ngit worktree add failed: branch already exists');
  });

  it('preserves the original diagnostic for unknown codes', () => {
    expect(acpRuntimeErrorBannerCopy(fakeT('zh'), runtimeError({
      code: { domain: 'provider', code: 'acp.initialize-failed' },
      diagnostic: '磁盘空间不足。 (os error 112)',
    }))).toBe('磁盘空间不足。 (os error 112)');
    expect(acpRuntimeErrorBannerCopy(fakeT('zh'), null)).toBeNull();
  });
});

async function zhCnT() {
  const { default: i18next } = await import('i18next');
  const { default: zhCN } = await import('@/locales/zh-CN.json');
  const i18n = i18next.createInstance();
  await i18n.init({ lng: 'zh-CN', resources: { 'zh-CN': { translation: zhCN } } });
  return i18n.t.bind(i18n) as never;
}

describe('acpRuntimeErrorBannerCopy with catalog copy', () => {
  it('localizes dynamic completion repair exhaustion by error code', async () => {
    const t = await zhCnT();

    const copy = acpRuntimeErrorBannerCopy(t, runtimeError({
      code: { domain: 'dynamic', code: 'dynamic.completion.repair-exhausted' },
      domain: 'dynamic',
      params: { repairAttempts: 3, maxRepairAttempts: 3, validationErrors: [] },
      diagnostic: 'dynamic-node-completion validation failed',
    }));

    expect(copy).toBe(
      '节点输出经 3 次修复仍不符合协议，工作流已暂停。\ndynamic-node-completion validation failed',
    );
  });

  it('localizes static workflow output repair exhaustion by error code', async () => {
    const copy = acpRuntimeErrorBannerCopy(await zhCnT(), runtimeError({
      code: { domain: 'workflow', code: 'workflow.output.repair-exhausted' },
      domain: 'workflow',
      params: { nodeId: 'accept', repairAttempts: 3, maxRepairAttempts: 3 },
    }));

    expect(copy).toBe('节点输出经 3 次修复仍不符合输出格式，工作流已暂停。');
  });
});

describe('acpRuntimeErrorBannerTitle', () => {
  it('titles workflow and dynamic pauses as workflow paused', async () => {
    const t = await zhCnT();
    for (const domain of ['workflow', 'dynamic']) {
      expect(acpRuntimeErrorBannerTitle(t, runtimeError({ code: { domain, code: 'x' }, domain })))
        .toBe('工作流已暂停');
    }
  });

  it('keeps the default ACP title for session and provider errors', async () => {
    const t = await zhCnT();
    expect(acpRuntimeErrorBannerTitle(t, runtimeError())).toBeNull();
    expect(acpRuntimeErrorBannerTitle(t, runtimeError({ code: { domain: 'provider', code: 'provider.execution-error' }, domain: 'provider' }))).toBeNull();
    expect(acpRuntimeErrorBannerTitle(t, null)).toBeNull();
  });
});
