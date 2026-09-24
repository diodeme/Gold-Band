import { describe, expect, it } from 'vitest';
import { DESKTOP_LANGUAGE_OPTIONS, supportedLocaleTag, themeDisplayLocale } from '../src/languages';

describe('desktop language catalog', () => {
  it('lists endonyms in the settings order', () => {
    expect(DESKTOP_LANGUAGE_OPTIONS.map((option) => [option.value, option.label])).toEqual([
      ['zh-cn', '中文'],
      ['zh-tw', '繁體中文'],
      ['en', 'English'],
      ['ja-jp', '日本語'],
      ['ko-kr', '한국어'],
      ['pt-br', 'Português (Brasil)'],
      ['es', 'Español'],
    ]);
  });

  it('maps system locale tags the same way as the desktop backend', () => {
    expect(supportedLocaleTag('zh-CN')).toBe('zh-CN');
    expect(supportedLocaleTag('zh-Hans-CN')).toBe('zh-CN');
    expect(supportedLocaleTag('zh')).toBe('zh-CN');
    expect(supportedLocaleTag('zh_TW')).toBe('zh-TW');
    expect(supportedLocaleTag('zh-Hant-HK')).toBe('zh-TW');
    expect(supportedLocaleTag('zh-MO')).toBe('zh-TW');
    expect(supportedLocaleTag('en-US')).toBe('en');
    expect(supportedLocaleTag('ja-JP')).toBe('ja-JP');
    expect(supportedLocaleTag('ko-KR')).toBe('ko-KR');
    expect(supportedLocaleTag('pt-BR')).toBe('pt-BR');
    expect(supportedLocaleTag('pt-PT')).toBe('en');
    expect(supportedLocaleTag('es-MX')).toBe('es');
    expect(supportedLocaleTag('de-DE')).toBe('en');
  });

  it('keeps theme names in Simplified Chinese or English', () => {
    expect(themeDisplayLocale('zh-cn')).toBe('zh-CN');
    expect(themeDisplayLocale('zh-TW')).toBe('en');
    expect(themeDisplayLocale('ja-jp')).toBe('en');
  });
});
