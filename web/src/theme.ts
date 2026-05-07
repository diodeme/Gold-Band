import type { ConcreteDesktopTheme, DesktopThemeMode, DesktopThemePreference } from './types';

export interface ThemePreviewPalette {
  background: string;
  surface: string;
  border: string;
  primary: string;
  foreground: string;
  muted: string;
  success: string;
  danger: string;
}

export interface DesktopThemeOption {
  id: ConcreteDesktopTheme;
  mode: DesktopThemeMode;
  labelKey: string;
  descriptionKey: string;
  preview: ThemePreviewPalette;
}

export const desktopThemeOptions = [
  {
    id: 'light',
    mode: 'light',
    labelKey: 'settings.themeDefaultLight',
    descriptionKey: 'settings.themeDefaultLightDescription',
    preview: {
      background: '#f8fbff',
      surface: '#ffffff',
      border: '#dbe7f5',
      primary: '#2563eb',
      foreground: '#0f172a',
      muted: '#64748b',
      success: '#15803d',
      danger: '#dc2626',
    },
  },
  {
    id: 'light-warm',
    mode: 'light',
    labelKey: 'settings.themeWarmLight',
    descriptionKey: 'settings.themeWarmLightDescription',
    preview: {
      background: '#f5efe3',
      surface: '#ffffff',
      border: '#ded2c1',
      primary: '#b87506',
      foreground: '#201d18',
      muted: '#756b5b',
      success: '#15803d',
      danger: '#dc2626',
    },
  },
  {
    id: 'dark',
    mode: 'dark',
    labelKey: 'settings.themeGoldDark',
    descriptionKey: 'settings.themeGoldDarkDescription',
    preview: {
      background: '#080808',
      surface: '#171717',
      border: '#2b2b2b',
      primary: '#f59e0b',
      foreground: '#f3f0e8',
      muted: '#8d8982',
      success: '#35d07f',
      danger: '#ff5f63',
    },
  },
  {
    id: 'black',
    mode: 'dark',
    labelKey: 'settings.themeBlack',
    descriptionKey: 'settings.themeBlackDescription',
    preview: {
      background: '#020617',
      surface: '#080d18',
      border: '#1e293b',
      primary: '#60a5fa',
      foreground: '#e5edf7',
      muted: '#94a3b8',
      success: '#22c55e',
      danger: '#fb7185',
    },
  },
] as const satisfies readonly DesktopThemeOption[];

export const desktopThemeGroups = {
  light: desktopThemeOptions.filter((theme) => theme.mode === 'light'),
  dark: desktopThemeOptions.filter((theme) => theme.mode === 'dark'),
};

export function resolveThemePreference(theme: DesktopThemePreference): ConcreteDesktopTheme {
  if (theme !== 'system') return theme;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function desktopThemeMode(theme: ConcreteDesktopTheme): DesktopThemeMode {
  return desktopThemeOptions.find((option) => option.id === theme)?.mode ?? 'dark';
}

export function applyTheme(theme: DesktopThemePreference) {
  const root = document.documentElement;
  const resolved = resolveThemePreference(theme);
  root.dataset.theme = resolved;
  root.classList.toggle('dark', desktopThemeMode(resolved) === 'dark');
}
