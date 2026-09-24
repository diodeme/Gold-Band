import i18n, { type TFunction } from "i18next";
import { initReactI18next } from "react-i18next";
import type { AppErrorVm, WorkflowErrorVm } from "./types";
import en from "./locales/en.json";
import es from "./locales/es.json";
import jaJP from "./locales/ja-JP.json";
import koKR from "./locales/ko-KR.json";
import ptBR from "./locales/pt-BR.json";
import zhCN from "./locales/zh-CN.json";
import zhTW from "./locales/zh-TW.json";

export { i18nLanguage } from "./languages";

const resources = {
  "zh-CN": { translation: zhCN },
  "zh-TW": { translation: zhTW },
  en: { translation: en },
  "ja-JP": { translation: jaJP },
  "ko-KR": { translation: koKR },
  "pt-BR": { translation: ptBR },
  es: { translation: es },
};

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

if (!i18n.isInitialized) {
  void i18n.use(initReactI18next).init({
    resources,
    lng: "zh-CN",
    fallbackLng: "en",
    interpolation: { escapeValue: false },
  });
}

export default i18n;
