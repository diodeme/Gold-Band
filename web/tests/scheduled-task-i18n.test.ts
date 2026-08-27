import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

import i18n from '@/i18n';

const scheduledUiSources = [
  '../src/components/conversation/ScheduledTaskDialog.tsx',
  '../src/components/conversation/ConversationComposer.tsx',
  '../src/pages/ScheduledTaskManagementPage.tsx',
  '../src/pages/ScheduledTaskDetailPage.tsx',
];

describe('scheduled task localization', () => {
  it('keeps scheduled-task customer copy out of implementation files', () => {
    for (const sourcePath of scheduledUiSources) {
      const source = readFileSync(fileURLToPath(new URL(sourcePath, import.meta.url)), 'utf8');
      expect(source, sourcePath).not.toMatch(/[\u3400-\u9fff]/u);
    }
  });

  it('defines the management, history, dialog, and settings keys in both languages', async () => {
    for (const language of ['zh-CN', 'en']) {
      await i18n.changeLanguage(language);
      for (const key of [
        'scheduled.management.title',
        'scheduled.detail.history',
        'scheduled.dialog.title',
        'scheduled.composer.create',
        'scheduled.composer.created',
        'scheduled.composer.switchTo',
        'scheduled.settings.keepAwake',
      ]) {
        expect(i18n.exists(key), `${language}:${key}`).toBe(true);
      }
    }
  });

  it('localizes the composer mode-switch prefix', async () => {
    for (const [language, expected] of [['zh-CN', '切换为'], ['en', 'Switch to']] as const) {
      await i18n.changeLanguage(language);
      expect(i18n.t('scheduled.composer.switchTo')).toBe(expected);
    }
  });
});
