import i18n from 'i18next';
import en from '@/locales/en.json';
import es from '@/locales/es.json';
import jaJP from '@/locales/ja-JP.json';
import koKR from '@/locales/ko-KR.json';
import ptBR from '@/locales/pt-BR.json';
import zhCN from '@/locales/zh-CN.json';
import zhTW from '@/locales/zh-TW.json';
import { DESKTOP_LANGUAGE_OPTIONS } from '@/languages';

await i18n.init({
  resources: {
    'zh-CN': { translation: zhCN },
    'zh-TW': { translation: zhTW },
    en: { translation: en },
    'ja-JP': { translation: jaJP },
    'ko-KR': { translation: koKR },
    'pt-BR': { translation: ptBR },
    es: { translation: es },
  },
  lng: 'zh-CN',
  fallbackLng: false,
  supportedLngs: DESKTOP_LANGUAGE_OPTIONS.map(({ tag }) => tag),
  load: 'currentOnly',
  interpolation: { escapeValue: false },
});
