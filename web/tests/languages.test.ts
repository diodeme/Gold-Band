import { describe, expect, it } from 'vitest';
import { DESKTOP_LANGUAGE_OPTIONS, supportedLocaleTag, themeDisplayLocale } from '../src/languages';

describe('desktop language catalog', () => {
  it.each([
    ['zh-Hans', 'zh-CN'],
    ['zh-Hans-CN', 'zh-CN'],
    ['zh-Hans-SG', 'zh-CN'],
    ['zh-Hans-HK', 'zh-CN'],
    ['zh-Hans-TW', 'zh-CN'],
    ['zh-Hans-MO', 'zh-CN'],
    ['zh-Hant', 'zh-TW'],
    ['zh-Hant-CN', 'zh-TW'],
    ['zh-Hant-SG', 'zh-TW'],
    ['zh-Hant-HK', 'zh-TW'],
    [' ZH_hAnS_hK ', 'zh-CN'],
  ])('uses explicit Chinese script before region for %s', (tag, expected) => {
    expect(supportedLocaleTag(tag)).toBe(expected);
  });

  it.each([
    ['zh', 'zh-CN'],
    ['zh-CN', 'zh-CN'],
    ['zh-SG', 'zh-CN'],
    ['zh-TW', 'zh-TW'],
    ['zh-HK', 'zh-TW'],
    ['zh-MO', 'zh-TW'],
  ])('uses Chinese region when script is absent for %s', (tag, expected) => {
    expect(supportedLocaleTag(tag)).toBe(expected);
  });

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
