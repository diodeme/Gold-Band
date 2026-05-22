import type { ConcreteDesktopTheme, DesktopFontPreference, DesktopThemeMode, DesktopThemePreference } from './types';

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
    labelKey: 'settings.themeDefaultLight',
    descriptionKey: 'settings.themeDefaultLightDescription',
    preview: {
      background: '#f7faff',
      surface: '#fbfdff',
      border: '#d4deeb',
      primary: '#3157d5',
      foreground: '#111827',
      muted: '#667085',
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
      background: '#f7f2e8',
      surface: '#fffaf1',
      border: '#dccfba',
      primary: '#9a6b1f',
      foreground: '#211d16',
      muted: '#756d60',
      success: '#15803d',
      danger: '#c83e43',
    },
  },
  {
    id: 'dark',
    mode: 'dark',
    labelKey: 'settings.themeGoldDark',
    descriptionKey: 'settings.themeGoldDarkDescription',
    preview: {
      background: '#0b0d10',
      surface: '#101215',
      border: '#242832',
      primary: '#d6b36a',
      foreground: '#f4efe6',
      muted: '#9b9488',
      success: '#3ddc97',
      danger: '#ff6b76',
    },
  },
  {
    id: 'black',
    mode: 'dark',
    labelKey: 'settings.themeBlack',
    descriptionKey: 'settings.themeBlackDescription',
    preview: {
      background: '#050814',
      surface: '#080c17',
      border: '#172236',
      primary: '#6ea8fe',
      foreground: '#edf4ff',
      muted: '#98a6bd',
      success: '#2dd48f',
      danger: '#ff7185',
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
