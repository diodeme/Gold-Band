import { describe, expect, it } from 'vitest';
import { displayAppError } from '@/i18n';
import i18n from '@/i18n';

describe('app error i18n', () => {
  it('uses an actionable message for ACP initialization interruption', () => {
    expect(i18n.t('acp.sessionInterrupted', { lng: 'zh-CN' })).toBe('会话发起中断，请重跑该任务');
    expect(i18n.t('acp.sessionInterrupted', { lng: 'en' })).toBe('Session launch was interrupted. Rerun the task.');
  });

  it('renders run mode management section labels', () => {
    expect(i18n.t('runMode.workflowSection', { lng: 'zh-CN' })).toBe('工作流模式');
    expect(i18n.t('runMode.autoSection', { lng: 'zh-CN' })).toBe('AUTO模式');
    expect(i18n.t('runMode.workflowSection', { lng: 'en' })).toBe('Workflow Mode');
    expect(i18n.t('runMode.autoSection', { lng: 'en' })).toBe('AUTO Mode');
  });

  it('renders active ACP prompt config-save guard as a user action', () => {
    const message = displayAppError(i18n.t.bind(i18n), {
      code: 'acp.active-prompt-blocks-config-save',
      params: { workspaceRoot: '/repo' },
    });

    expect(message).toBe('当前有会话正在运行，请先停止会话后再保存配置。');
  });

  it('renders removed conversation workspace errors', () => {
    const message = displayAppError(i18n.t.bind(i18n), {
      code: 'conversation.workspace-not-found',
      params: { projectId: 'missing' },
    });

    expect(message).toBe('找不到该工作空间。');
  });

  it('does not expose interpolation placeholders when an error has no message parameter', () => {
    const message = displayAppError(i18n.t.bind(i18n), {
      code: 'app.unexpected',
      params: {},
    });

    expect(message).toBe('操作失败，请重试。');
    expect(message).not.toContain('{{message}}');
  });

  it('localizes scheduled occurrence resume failures', () => {
    for (const [code, zh, en] of [
      ['SCHEDULED_COORDINATOR_UNAVAILABLE', '定时任务运行服务暂不可用，请重试。', 'The scheduled task service is unavailable. Try again.'],
      ['SCHEDULED_NOT_FOUND', '待恢复的定时任务执行已不存在，请刷新后重试。', 'The scheduled run to resume no longer exists. Refresh and try again.'],
      ['SCHEDULED_STORAGE_FAILED', '无法更新定时任务执行状态，请重试。', 'The scheduled run state could not be updated. Try again.'],
    ] as const) {
      expect(displayAppError(i18n.t.bind(i18n), { code, params: {} })).toBe(zh);
      expect(i18n.t(`errors.${code}`, { lng: 'en' })).toBe(en);
    }
  });
});
