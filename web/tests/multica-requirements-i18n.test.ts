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

  it('localizes the not-ready marker, its reason and the task-not-ready error code', () => {
    expect(i18n.t('multica.taskManagement.readiness.notReady', { lng: 'zh-CN' })).toBe('未就绪');
    expect(i18n.t('multica.taskManagement.readiness.notReady', { lng: 'en' })).toBe('Not ready');
    expect(i18n.t('multica.taskManagement.readiness.notReadyHint', { lng: 'zh-CN' })).toBe(
      '等待对应开发任务完成后可执行',
    );
    expect(i18n.t('multica.taskManagement.readiness.notReadyHint', { lng: 'en' })).toBe(
      'Runs after the matching dev task is done',
    );

    // 新错误码后端只回 code，对客文案在前端映射（`errors.multica.<kebab-code>`）。
    expect(i18n.t('errors.multica.task-not-ready', { lng: 'zh-CN' })).toBe(
      '该测试任务尚未就绪：需对应的开发任务完成后才能执行。',
    );
    expect(i18n.t('errors.multica.task-not-ready', { lng: 'en' })).toBe(
      'This test task is not ready yet: it can run only after the matching dev task is done.',
    );
  });
});
