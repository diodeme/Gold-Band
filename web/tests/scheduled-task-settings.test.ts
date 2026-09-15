import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it } from 'vitest';

import { browserApi } from '../src/api/browser';
import { ScheduledRuntimeSettings } from '@/components/scheduled-tasks/ScheduledRuntimeSettings';
import { __resetScheduledRuntimeSettingsCache, writeScheduledRuntimeSettingsCache } from '@/components/scheduled-tasks/useScheduledRuntimeSettings';
import i18n from '@/i18n';

describe('scheduled runtime settings', () => {
  beforeEach(() => {
    __resetScheduledRuntimeSettingsCache();
  });
  it('persists browser parity values and keeps effective state separate', async () => {
    const saved = await browserApi.saveScheduledRuntimeSettings({
      keepAwakeEnabled: true,
      completionNotificationsEnabled: false,
      occurrenceRetentionDays: 45,
    });

    expect(saved.keepAwakeEnabled).toBe(true);
    expect(saved.keepAwakeEffective).toBe(false);
    expect(saved.occurrenceRetentionDays).toBe(45);
    await expect(browserApi.saveScheduledRuntimeSettings({
      keepAwakeEnabled: false,
      completionNotificationsEnabled: true,
      occurrenceRetentionDays: 0,
    })).rejects.toBeTruthy();
  });

  it('renders shared controls with effective keep-awake state and retention bounds', async () => {
    await i18n.changeLanguage('en');
    // 通过缓存层预填数据：组件挂载即命中缓存，无需等待拉取（SSR 不执行 useEffect）。
    writeScheduledRuntimeSettingsCache({
      keepAwakeEnabled: true,
      keepAwakeEffective: false,
      completionNotificationsEnabled: true,
      enabledJobCount: 2,
      occurrenceRetentionDays: 30,
      powerErrorCode: 'SCHEDULED_POWER_INHIBITOR_FAILED',
    });
    const html = renderToStaticMarkup(React.createElement(ScheduledRuntimeSettings));

    expect(html).toContain('Keep the system awake');
    expect(html).toContain('Enabled, not currently active');
    expect(html).toContain('min="1"');
    expect(html).toContain('max="3650"');
  });

  it('exposes scheduled runtime settings only from the Settings page', async () => {
    const { readFileSync } = await import('node:fs');
    const { fileURLToPath } = await import('node:url');
    const management = readFileSync(fileURLToPath(new URL('../src/pages/ScheduledTaskManagementPage.tsx', import.meta.url)), 'utf8');
    const settings = readFileSync(fileURLToPath(new URL('../src/pages/SettingsPage.tsx', import.meta.url)), 'utf8');

    expect(management).not.toContain('<ScheduledRuntimeSettings');
    expect(settings).toContain('<ScheduledRuntimeSettings');
  });
});
