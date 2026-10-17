import i18n, { ensureI18nLanguage } from '@/i18n';
import type { DesktopLanguage } from '@/types';

const DEMO_STRINGS: Partial<Record<string, Record<string, unknown>>> = {
  en: { demo: { historyLoadFailed: 'Unable to load recorded history.' } },
  'zh-CN': { demo: { historyLoadFailed: '无法加载历史记录。' } },
};

// Load the full app catalog first so the demo strings never count as the language's only bundle.
export async function loadDemoLanguage(language: DesktopLanguage) {
  const tag = await ensureI18nLanguage(language);
  const strings = DEMO_STRINGS[tag];
  if (strings) i18n.addResourceBundle(tag, 'translation', strings, true, true);
  await i18n.changeLanguage(tag);
  return tag;
}
