/**
 * ACP system prompt dialog display preference.
 *
 * The preference belongs to the system prompt viewer rather than an attempt,
 * so switching attempts keeps a stable viewing mode without changing runtime
 * session data.
 */
export const SYSTEM_PROMPT_VIEW_STORAGE_KEY = "gold-band-system-prompt-view-mode";

export const SYSTEM_PROMPT_VIEW_MODES = {
  rendered: "rendered",
  raw: "raw",
} as const;

export type SystemPromptViewMode =
  (typeof SYSTEM_PROMPT_VIEW_MODES)[keyof typeof SYSTEM_PROMPT_VIEW_MODES];

export const DEFAULT_SYSTEM_PROMPT_VIEW_MODE: SystemPromptViewMode =
  SYSTEM_PROMPT_VIEW_MODES.rendered;

export function loadSystemPromptViewMode(): SystemPromptViewMode {
  if (typeof localStorage === "undefined") return DEFAULT_SYSTEM_PROMPT_VIEW_MODE;

  const stored = localStorage.getItem(SYSTEM_PROMPT_VIEW_STORAGE_KEY);
  return stored === SYSTEM_PROMPT_VIEW_MODES.rendered ||
    stored === SYSTEM_PROMPT_VIEW_MODES.raw
    ? stored
    : DEFAULT_SYSTEM_PROMPT_VIEW_MODE;
}

export function saveSystemPromptViewMode(mode: SystemPromptViewMode): void {
  if (typeof localStorage === "undefined") return;

  try {
    localStorage.setItem(SYSTEM_PROMPT_VIEW_STORAGE_KEY, mode);
  } catch {
    // Storage can be unavailable in privacy mode; the in-memory choice still applies.
  }
}
