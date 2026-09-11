import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';

describe('Multica requirements localization', () => {
  it('uses the requirements product name in both languages', () => {
    expect(i18n.t('multica.taskManagement.title', { lng: 'zh-CN' })).toBe('需求管理');
    expect(i18n.t('conversation.sidebar.multicaTaskManagement', { lng: 'zh-CN' })).toBe('需求管理');
    expect(i18n.t('conversation.sidebar.more', { lng: 'zh-CN' })).toBe('更多');

    expect(i18n.t('multica.taskManagement.title', { lng: 'en' })).toBe('Requirements');
    expect(i18n.t('conversation.sidebar.multicaTaskManagement', { lng: 'en' })).toBe('Requirements');
    expect(i18n.t('conversation.sidebar.more', { lng: 'en' })).toBe('More');
  });

  // story dev/test 拆分（§12.40）：类型徽标 / 类型过滤 / 未就绪三组 key 双语精确值。
  // 精确值断言的目的：文案是接口层验收面（用户可见文案不能悄悄漂移成 key 或英文兜底）。
  it('localizes issue kinds and the type filter in both languages', () => {
    for (const kind of ['dev', 'test', 'bug'] as const) {
      expect(i18n.t(`multica.taskManagement.issueKind.${kind}`, { lng: 'zh-CN' })).not.toBe(
        `multica.taskManagement.issueKind.${kind}`,
      );
      expect(i18n.t(`multica.taskManagement.issueKind.${kind}`, { lng: 'en' })).not.toBe(
        `multica.taskManagement.issueKind.${kind}`,
      );
    }
    expect(i18n.t('multica.taskManagement.issueKind.dev', { lng: 'zh-CN' })).toBe('开发');
    expect(i18n.t('multica.taskManagement.issueKind.dev', { lng: 'en' })).toBe('Dev');

    expect(i18n.t('multica.taskManagement.issueKindFilter.label', { lng: 'zh-CN' })).toBe('类型');
    expect(i18n.t('multica.taskManagement.issueKindFilter.label', { lng: 'en' })).toBe('Type');
    for (const filter of ['all', 'dev', 'test'] as const) {
      expect(i18n.t(`multica.taskManagement.issueKindFilter.${filter}`, { lng: 'zh-CN' })).not.toBe(
        `multica.taskManagement.issueKindFilter.${filter}`,
      );
      expect(i18n.t(`multica.taskManagement.issueKindFilter.${filter}`, { lng: 'en' })).not.toBe(
        `multica.taskManagement.issueKindFilter.${filter}`,
      );
    }
    expect(i18n.t('multica.taskManagement.issueKindFilter.all', { lng: 'zh-CN' })).toBe('全部');
    expect(i18n.t('multica.taskManagement.issueKindFilter.all', { lng: 'en' })).toBe('All');
  });

  it('localizes the not-ready marker and its advisory reason in both languages', () => {
    expect(i18n.t('multica.taskManagement.readiness.notReady', { lng: 'zh-CN' })).toBe('未就绪');
    expect(i18n.t('multica.taskManagement.readiness.notReady', { lng: 'en' })).toBe('Not ready');
    // 提醒文案不得暗示「不能执行」——未就绪仅提醒不阻断（§12.42）。
    expect(i18n.t('multica.taskManagement.readiness.notReadyHint', { lng: 'zh-CN' })).toBe(
      '对应开发任务尚未完成，建议等待其完成后再执行',
    );
    expect(i18n.t('multica.taskManagement.readiness.notReadyHint', { lng: 'en' })).toBe(
      'The matching dev task is not done yet; consider waiting for it',
    );
  });
});
