import type { ConcreteDesktopTheme, DesktopFontPreference, DesktopThemeMode, DesktopThemePreference } from './types';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauriRuntime } from './api/shared';

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
  windowSurface: string;
  labelKey: string;
  descriptionKey: string;
  preview: ThemePreviewPalette;
}

export interface DesktopFontOption {
  id: DesktopFontPreference;
  labelKey: string;
  descriptionKey: string;
  preview: string;
  stack: string;
}

export const desktopThemeOptions = [
  {
    id: 'light',
    mode: 'light',
    windowSurface: '#f1f2f5',
    labelKey: 'settings.themeDefaultLight',
    descriptionKey: 'settings.themeDefaultLightDescription',
    preview: {
      background: '#fafafb',
      surface: '#ffffff',
      border: '#e1e3e9',
      primary: '#5b6ba8',
      foreground: '#191c24',
      muted: '#667085',
      success: '#16794b',
      danger: '#c93c48',
    },
  },
  {
    id: 'light-warm',
    mode: 'light',
    windowSurface: '#f0ede7',
    labelKey: 'settings.themeWarmLight',
    descriptionKey: 'settings.themeWarmLightDescription',
    preview: {
      background: '#faf9f6',
      surface: '#fffdfc',
      border: '#e3ded5',
      primary: '#8a6a32',
      foreground: '#29251f',
      muted: '#736b60',
      success: '#397451',
      danger: '#b84850',
    },
  },
  {
    id: 'dark',
    mode: 'dark',
    windowSurface: '#181818',
    labelKey: 'settings.themeGoldDark',
    descriptionKey: 'settings.themeGoldDarkDescription',
    preview: {
      background: '#181818',
      surface: '#242424',
      border: '#333333',
      primary: '#313131',
      foreground: '#e8e8e8',
      muted: '#9a9a9a',
      success: '#59b68b',
      danger: '#df6b6b',
    },
  },
  {
    id: 'black',
    mode: 'dark',
    windowSurface: '#111111',
    labelKey: 'settings.themeBlack',
    descriptionKey: 'settings.themeBlackDescription',
    preview: {
      background: '#111111',
      surface: '#1b1b1b',
      border: '#2b2b2b',
      primary: '#2d2d2d',
      foreground: '#e8e8e8',
      muted: '#929292',
      success: '#59b68b',
      danger: '#df6b6b',
    },
  },
] as const satisfies readonly DesktopThemeOption[];

export const desktopFontOptions = [
  {
    id: 'app-default',
    labelKey: 'settings.fontDefault',
    descriptionKey: 'settings.fontDefaultDescription',
    preview: '任务编排 / AI Workflow',
    stack: '"Gold Band MiSans", "MiSans", "Microsoft YaHei UI", "PingFang SC", "Noto Sans CJK SC", "Source Han Sans SC", system-ui, sans-serif',
  },
] as const satisfies readonly DesktopFontOption[];

export const desktopThemeGroups = {
  light: desktopThemeOptions.filter((theme) => theme.mode === 'light'),
  dark: desktopThemeOptions.filter((theme) => theme.mode === 'dark'),
};

const preferredThemeStorageKey = 'gold-band:preferred-themes';
const defaultThemeByMode = {
  light: 'light',
  dark: 'dark',
} as const satisfies Record<DesktopThemeMode, ConcreteDesktopTheme>;

type PreferredThemeByMode = Record<DesktopThemeMode, ConcreteDesktopTheme>;

export function desktopThemeMode(theme: ConcreteDesktopTheme): DesktopThemeMode {
  return desktopThemeOptions.find((option) => option.id === theme)?.mode ?? 'dark';
}

export function desktopThemeWindowSurface(theme: ConcreteDesktopTheme): string {
  return desktopThemeOptions.find((option) => option.id === theme)?.windowSurface ?? '#181818';
}

export function syncDesktopWindowSurface(theme: ConcreteDesktopTheme): Promise<void> {
  if (!isTauriRuntime()) return Promise.resolve();
  return getCurrentWindow()
    .setBackgroundColor(desktopThemeWindowSurface(theme))
    .catch(() => {});
}

export function rememberConcreteThemePreference(theme: ConcreteDesktopTheme) {
  const mode = desktopThemeMode(theme);
  const preferredThemes = preferredThemeByMode();
  preferredThemes[mode] = theme;
  window.localStorage.setItem(preferredThemeStorageKey, JSON.stringify(preferredThemes));
}

export function preferredThemeForMode(mode: DesktopThemeMode): ConcreteDesktopTheme {
  return preferredThemeByMode()[mode];
}

export function resolveThemePreference(theme: DesktopThemePreference): ConcreteDesktopTheme {
  if (theme !== 'system') return theme;
  const systemMode = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  return preferredThemeByMode()[systemMode];
}

function preferredThemeByMode(): PreferredThemeByMode {
  try {
    const saved = JSON.parse(window.localStorage.getItem(preferredThemeStorageKey) ?? '{}') as Partial<PreferredThemeByMode>;
    return {
      light: isThemeForMode(saved.light, 'light') ? saved.light : defaultThemeByMode.light,
      dark: isThemeForMode(saved.dark, 'dark') ? saved.dark : defaultThemeByMode.dark,
    };
  } catch {
    return { ...defaultThemeByMode };
  }
}

function isThemeForMode(theme: ConcreteDesktopTheme | undefined, mode: DesktopThemeMode): theme is ConcreteDesktopTheme {
  return !!theme && desktopThemeMode(theme) === mode;
}

export function applyTheme(theme: DesktopThemePreference) {
  const root = document.documentElement;
  const resolved = resolveThemePreference(theme);
  if (theme !== 'system') rememberConcreteThemePreference(theme);
  root.dataset.theme = resolved;
  root.classList.toggle('dark', desktopThemeMode(resolved) === 'dark');
  void syncDesktopWindowSurface(resolved);
  return resolved;
}

export function fontFamilyForPreference(font: DesktopFontPreference) {
  return desktopFontOptions.find((option) => option.id === font)?.stack ?? `${quoteFontFamily(font)}, "Gold Band MiSans", "MiSans", "Microsoft YaHei UI", "PingFang SC", system-ui, sans-serif`;
}

export function applyFont(font: DesktopFontPreference) {
  const root = document.documentElement;
  root.dataset.font = desktopFontOptions.some((option) => option.id === font) ? font : 'local';
  root.style.setProperty('--app-font-sans', fontFamilyForPreference(font));
}

function quoteFontFamily(font: string) {
  return `"${font.replaceAll('"', '\\"')}"`;
}
