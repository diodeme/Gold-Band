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
});
