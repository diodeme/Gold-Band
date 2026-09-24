import type { DesktopLanguage } from './types';

export const DESKTOP_LANGUAGE_OPTIONS: ReadonlyArray<{
  value: DesktopLanguage;
  tag: 'zh-CN' | 'zh-TW' | 'en' | 'ja-JP' | 'ko-KR' | 'pt-BR' | 'es';
  label: string;
}> = [
  { value: 'zh-cn', tag: 'zh-CN', label: '中文' },
  { value: 'zh-tw', tag: 'zh-TW', label: '繁體中文' },
  { value: 'en', tag: 'en', label: 'English' },
  { value: 'ja-jp', tag: 'ja-JP', label: '日本語' },
  { value: 'ko-kr', tag: 'ko-KR', label: '한국어' },
  { value: 'pt-br', tag: 'pt-BR', label: 'Português (Brasil)' },
  { value: 'es', tag: 'es', label: 'Español' },
];

export type SupportedLocaleTag = (typeof DESKTOP_LANGUAGE_OPTIONS)[number]['tag'];

export function i18nLanguage(language: DesktopLanguage): SupportedLocaleTag {
  return DESKTOP_LANGUAGE_OPTIONS.find((option) => option.value === language)?.tag ?? 'en';
}

/** Theme display names are only required in Simplified Chinese and English. */
export function themeDisplayLocale(language: DesktopLanguage | string): 'zh-CN' | 'en' {
  return language === 'zh-cn' || language === 'zh-CN' ? 'zh-CN' : 'en';
}

/**
 * Map an OS or browser locale onto the supported catalog.
 * The same rules live in `DesktopLanguage::from_locale_tag`.
 */
export function supportedLocaleTag(language: string): SupportedLocaleTag {
  const parts = language.trim().replaceAll('_', '-').toLowerCase().split('-').filter(Boolean);
  const primary = parts[0] ?? '';
  const rest = parts.slice(1);
  if (primary === 'zh') {
    return rest.some((part) => part === 'hant' || part === 'tw' || part === 'hk' || part === 'mo') ? 'zh-TW' : 'zh-CN';
  }
  if (primary === 'ja') return 'ja-JP';
  if (primary === 'ko') return 'ko-KR';
  if (primary === 'pt') return rest.includes('br') ? 'pt-BR' : 'en';
  if (primary === 'es') return 'es';
  return 'en';
}
