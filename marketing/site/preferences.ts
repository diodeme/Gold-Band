import { z } from 'zod';

export const PREFERENCES_KEY = 'gold-band.site.preferences.v1';
const PreferenceSchema = z.object({ version: z.literal(1), language: z.enum(['zh', 'en']), theme: z.enum(['system', 'light', 'dark']) });
export type SitePreferences = z.infer<typeof PreferenceSchema>;
export function readPreferences(storage: Pick<Storage, 'getItem'> | undefined, browserLanguage: string): SitePreferences {
  const fallback: SitePreferences = { version: 1, language: /^en(?:-|$)/i.test(browserLanguage) ? 'en' : 'zh', theme: 'system' };
  try {
    const parsed = PreferenceSchema.safeParse(JSON.parse(storage?.getItem(PREFERENCES_KEY) ?? 'null'));
    return parsed.success ? parsed.data : fallback;
  } catch { return fallback; }
}
export function writePreferences(storage: Pick<Storage, 'setItem'> | undefined, preferences: SitePreferences) {
  try { storage?.setItem(PREFERENCES_KEY, JSON.stringify(PreferenceSchema.parse(preferences))); } catch { /* Storage can be disabled; the current session remains usable. */ }
}
export function effectiveTheme(preference: SitePreferences['theme'], systemDark: boolean) {
  return preference === 'system' ? (systemDark ? 'dark' : 'light') : preference;
}
export function browserStorage(): Storage | undefined {
  try { return window.localStorage; } catch { return undefined; }
}
