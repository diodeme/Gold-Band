import i18n, { type TFunction } from "i18next";
import { initReactI18next } from "react-i18next";
import type { AppErrorVm, DesktopLanguage, WorkflowErrorVm } from "./types";
import {
  DESKTOP_LANGUAGE_OPTIONS,
  i18nLanguage,
  type SupportedLocaleTag,
} from "./languages";

export { i18nLanguage } from "./languages";

type LocaleModule = { default: Record<string, unknown> };

const supportedLocaleTags = DESKTOP_LANGUAGE_OPTIONS.map(({ tag }) => tag);
const localeLoaders: Record<SupportedLocaleTag, () => Promise<LocaleModule>> = {
  "zh-CN": () => import("./locales/zh-CN.json"),
  "zh-TW": () => import("./locales/zh-TW.json"),
  en: () => import("./locales/en.json"),
  "ja-JP": () => import("./locales/ja-JP.json"),
  "ko-KR": () => import("./locales/ko-KR.json"),
  "pt-BR": () => import("./locales/pt-BR.json"),
  es: () => import("./locales/es.json"),
};

function localeTag(language: DesktopLanguage | SupportedLocaleTag): SupportedLocaleTag {
  return supportedLocaleTags.includes(language as SupportedLocaleTag)
    ? language as SupportedLocaleTag
    : i18nLanguage(language as DesktopLanguage);
}

export function displayStatus(t: TFunction, value?: string | null) {
  if (!value) return "";
  return t(`status.${value.toLowerCase()}`, { defaultValue: value });
}

export function displayPolicy(t: TFunction, value?: string | null) {
  return displayStatus(t, value);
}

export function displayNodeType(t: TFunction, value?: string | null) {
  if (!value) return t("nodeType.unknown");
  return t(`nodeType.${value.toLowerCase()}`, { defaultValue: value });
}

export function displayAppError(t: TFunction, error: unknown) {
  if (isAppError(error)) {
    if (error.code === "conversation.validation-failed" && Array.isArray(error.params.codes)) {
      return error.params.codes
        .filter((code): code is string => typeof code === "string")
        .map((code) => {
          const key = `conversation.validation.${code}`;
          return t(key, { defaultValue: key });
        })
        .join("\n");
    }
    return t(`errors.${error.code}`, {
      ...error.params,
      message: error.params.message ?? "",
      defaultValue: t("errors.app.unexpected", { message: "" }),
    });
  }
  return String(error);
}

export function displayWorkflowError(
  t: TFunction,
  error?: WorkflowErrorVm | null,
) {
  if (!error) return "";
  return displayAppError(t, error);
}

function isAppError(value: unknown): value is AppErrorVm {
  return (
    Boolean(value) &&
    typeof value === "object" &&
    typeof (value as Partial<AppErrorVm>).code === "string" &&
    Boolean((value as Partial<AppErrorVm>).params) &&
    typeof (value as Partial<AppErrorVm>).params === "object"
  );
}

if (i18n.isInitialized && typeof initReactI18next?.init === "function") {
  initReactI18next.init(i18n);
}

export const i18nInitialized: Promise<void> = i18n.isInitialized
  ? Promise.resolve()
  : i18n.use(initReactI18next).init({
    lng: "zh-CN",
    fallbackLng: false,
    supportedLngs: supportedLocaleTags,
    load: "currentOnly",
    interpolation: { escapeValue: false },
  }).then(() => undefined);

export async function ensureI18nLanguage(
  language: DesktopLanguage | SupportedLocaleTag,
): Promise<SupportedLocaleTag> {
  const tag = localeTag(language);
  await i18nInitialized;
  if (!i18n.hasResourceBundle(tag, "translation")) {
    const catalog = await localeLoaders[tag]();
    i18n.addResourceBundle(tag, "translation", catalog.default, true, true);
  }
  return tag;
}

export async function loadI18nLanguage(
  language: DesktopLanguage | SupportedLocaleTag,
): Promise<SupportedLocaleTag> {
  const tag = await ensureI18nLanguage(language);
  await i18n.changeLanguage(tag);
  return tag;
}

export default i18n;
