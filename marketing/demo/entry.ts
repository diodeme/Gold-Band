import type { PreferencesVm } from '@/types';

export function entryPreferences(preferences: PreferencesVm, search: string): PreferencesVm {
  const params = new URLSearchParams(search);
  const language = params.get('language');
  const theme = params.get('theme');
  return { ...preferences,
    language: language === 'en' ? 'en' : language === 'zh' || language === 'zh-CN' || language === 'zh-cn' ? 'zh-cn' : preferences.language,
    appearance: { ...preferences.appearance, colorScheme: theme === 'light' || theme === 'dark' || theme === 'system' ? theme : preferences.appearance.colorScheme },
  };
}
