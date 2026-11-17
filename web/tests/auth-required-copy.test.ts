import { describe, expect, it } from 'vitest';
import i18n, { loadI18nLanguage } from '@/i18n';
import { authRequiredCopy } from '@/lib/auth-required-copy';

const LANGUAGES = ['zh-CN', 'zh-TW', 'en', 'ja-JP', 'ko-KR', 'pt-BR', 'es'] as const;

describe('authRequiredCopy', () => {
  it.each(LANGUAGES)('shows the missing authentication environment variable in %s', async (language) => {
    await loadI18nLanguage(language);
    const copy = authRequiredCopy(i18n.getFixedT(language), {
      methods: ['qoder-personal-access-token'],
      missingEnv: ['QODER_PERSONAL_ACCESS_TOKEN'],
    });

    expect(copy).toContain('QODER_PERSONAL_ACCESS_TOKEN');
    expect(copy).not.toContain('errors.provider');
    expect(copy).not.toContain('{{env}}');
  });

  it.each(LANGUAGES)('asks for provider authentication when no env var is missing in %s', async (language) => {
    await loadI18nLanguage(language);
    const copy = authRequiredCopy(i18n.getFixedT(language), { methods: ['browser-login'], missingEnv: [] });

    expect(copy).toBeTruthy();
    expect(copy).not.toContain('errors.provider');
    expect(copy).not.toContain('{{env}}');
  });
});
