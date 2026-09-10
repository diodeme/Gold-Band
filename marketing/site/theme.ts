import { useLayoutEffect, useState } from 'react';
import { z } from 'zod';
import type { SiteTheme } from './content';

export const THEME_KEY = 'gold-band.site.appearance.v1';
const preference = z.enum(['system', 'dark', 'light']);
export type ThemePreference = z.infer<typeof preference>;
const saved = z.object({ version: z.literal(1), theme: preference });
export function readThemePreference(storage: Pick<Storage, 'getItem'>): ThemePreference {
  try { return saved.parse(JSON.parse(storage.getItem(THEME_KEY) ?? 'null')).theme; }
  catch { return 'system'; }
}
export function effectiveTheme(selected: ThemePreference, systemDark: boolean): SiteTheme {
  return selected === 'system' ? systemDark ? 'dark' : 'light' : selected;
}
export function useSiteTheme() {
  const [selected, setSelected] = useState<ThemePreference>(() => {
    try { return readThemePreference(localStorage); } catch { return 'system'; }
  });
  const [systemDark, setSystemDark] = useState(() => matchMedia('(prefers-color-scheme: dark)').matches);
  const theme = effectiveTheme(selected, systemDark);
  useLayoutEffect(() => {
    const query = matchMedia('(prefers-color-scheme: dark)');
    const changed = () => setSystemDark(query.matches);
    query.addEventListener('change', changed);
    return () => query.removeEventListener('change', changed);
  }, []);
  useLayoutEffect(() => {
    document.documentElement.dataset.siteTheme = theme;
    document.documentElement.classList.toggle('dark', theme === 'dark');
  }, [theme]);
  const select = (value: string) => {
    const next = preference.parse(value);
    setSelected(next);
    try { localStorage.setItem(THEME_KEY, JSON.stringify({ version: 1, theme: next })); } catch { /* In-memory preference remains usable. */ }
  };
  return { theme, selected, select };
}
