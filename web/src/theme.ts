import { getCurrentWindow } from '@tauri-apps/api/window';
import { z } from 'zod';
import { isTauriRuntime } from './api/shared';
import type { AppearancePreference, PersonalizationPreference, ResolvedColorScheme, VisualQuality } from './types';
import {
  MAX_FONT_FAMILY_CODE_POINTS,
  MAX_FONT_STACK_FAMILIES,
  type MaterialTokens,
  type SemanticThemeTokens,
  type ThemeIconDescriptor,
  type ThemeIconSlot,
  type ThemePackage,
  type ThemeScheme,
  type ThemeWallpaperDescriptor,
  type ThemeWallpaperSlot,
} from './theme-contract';
import { builtinThemes } from './themes/builtin-themes';

export interface ThemePreviewPalette {
  background: string; surface: string; border: string; primary: string;
  foreground: string; muted: string; success: string; danger: string;
}
export interface ResolvedTypography {
  families: string[];
  displayName: string;
  size: number;
  lineHeight: number;
}
export interface ResolvedThemeIconDescriptor extends ThemeIconDescriptor { url: string }
export interface ResolvedThemeWallpaperDescriptor extends ThemeWallpaperDescriptor { url: string }
export interface EffectiveAppearance {
  themeId: string;
  themeVersion: string;
  colorScheme: ResolvedColorScheme;
  visualQuality?: VisualQuality;
  scheme: ThemeScheme;
  material: MaterialTokens;
  shape: ThemeScheme['shape'];
  elevation: ThemeScheme['elevation'];
  motion: ThemeScheme['motion'];
  scrollbar: ThemeScheme['scrollbar'];
  semantic: SemanticThemeTokens;
  recipes: ThemePackage['recipes'];
  typography: { ui: ResolvedTypography; editor: ResolvedTypography };
  icons: Partial<Record<ThemeIconSlot, ResolvedThemeIconDescriptor>>;
  wallpapers: Partial<Record<ThemeWallpaperSlot, ResolvedThemeWallpaperDescriptor>>;
}

const appearancePreferenceSchema = z.object({
  schemaVersion: z.literal(2), themeId: z.string().min(1), colorScheme: z.enum(['system', 'light', 'dark']),
  visualQualityByTheme: z.record(z.string(), z.enum(['full', 'performance'])),
}).strict();
const themeCatalog = new Map<string, ThemePackage>(builtinThemes.map((theme) => [theme.id, theme]));
export const themePackageSummaries = builtinThemes.map((theme) => ({
  id: theme.id, version: theme.version, contractVersion: theme.contractVersion, source: theme.source,
  name: theme.name, capabilities: theme.capabilities,
  preview: { light: theme.schemes.light.preview, dark: theme.schemes.dark.preview },
}));

export const defaultAppearancePreference: AppearancePreference = {
  schemaVersion: 2, themeId: 'builtin.gold-band', colorScheme: 'system', visualQualityByTheme: {},
};
export const defaultPersonalizationPreference: PersonalizationPreference = {
  schemaVersion: 2,
  typography: {
    ui: { fontStack: { source: 'theme' }, fontSize: { source: 'theme' } },
    editor: { fontStack: { source: 'theme' }, fontSize: { source: 'theme' } },
  },
  avatars: {
    agent: { image: { source: 'theme' }, shape: { source: 'theme' } },
    user: { image: { source: 'theme' }, shape: { source: 'theme' } },
  },
};

export function normalizeAppearancePreference(value: AppearancePreference): AppearancePreference {
  const parsed = appearancePreferenceSchema.safeParse(value);
  const candidate = parsed.success ? parsed.data : defaultAppearancePreference;
  const themeId = themeCatalog.has(candidate.themeId) ? candidate.themeId : defaultAppearancePreference.themeId;
  const visualQualityByTheme = Object.fromEntries(Object.entries(candidate.visualQualityByTheme)
    .filter(([id]) => themeCatalog.get(id)?.capabilities.includes('visual-quality-profiles')));
  return { ...candidate, themeId, visualQualityByTheme };
}

export function getThemePackage(themeId: string): ThemePackage {
  return themeCatalog.get(themeId) ?? themeCatalog.get(defaultAppearancePreference.themeId)!;
}

export function resolveColorScheme(preference: AppearancePreference['colorScheme']): ResolvedColorScheme {
  if (preference !== 'system') return preference;
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function resolveAppearance(preference: AppearancePreference, locale = currentRuntimeLocale()): EffectiveAppearance {
  const normalized = normalizeAppearancePreference(preference);
  const theme = getThemePackage(normalized.themeId);
  const colorScheme = resolveColorScheme(normalized.colorScheme);
  const selectedQuality = theme.visualQualityProfiles
    ? (normalized.visualQualityByTheme[theme.id] ?? theme.visualQualityProfiles.default)
    : undefined;
  const scheme = theme.schemes[colorScheme];
  const material = selectedQuality === 'performance' && theme.visualQualityProfiles
    ? { ...scheme.material, ...theme.visualQualityProfiles.performance }
    : scheme.material;
  const assets = new Map(theme.assets.records.map((asset) => [asset.id, asset]));
  const resolveIcon = (descriptor: ThemeIconDescriptor): ResolvedThemeIconDescriptor | undefined => {
    const asset = assets.get(descriptor.assetId);
    return asset?.kind === 'icon' ? { ...descriptor, url: asset.outputUrl } : undefined;
  };
  const iconDescriptors = { ...theme.icons?.defaults, ...theme.icons?.schemes?.[colorScheme] };
  const icons = Object.fromEntries(Object.entries(iconDescriptors).flatMap(([slot, descriptor]) => {
    const resolved = resolveIcon(descriptor);
    return resolved ? [[slot, resolved]] : [];
  })) as EffectiveAppearance['icons'];
  const wallpapersDisabled = selectedQuality === 'performance' && theme.visualQualityProfiles?.performance.wallpapers?.enabled === false;
  const wallpapers = wallpapersDisabled ? {} : Object.fromEntries(Object.entries(theme.wallpapers?.[colorScheme] ?? {}).flatMap(([slot, descriptor]) => {
    const asset = assets.get(descriptor.assetId);
    return asset?.kind === 'wallpaper' ? [[slot, { ...descriptor, url: asset.outputUrl }]] : [];
  })) as EffectiveAppearance['wallpapers'];
  return {
    themeId: theme.id, themeVersion: theme.version, colorScheme, visualQuality: selectedQuality,
    scheme, material, shape: scheme.shape, elevation: scheme.elevation, motion: scheme.motion,
    scrollbar: scheme.scrollbar, semantic: scheme.semantic, recipes: theme.recipes,
    typography: {
      ui: resolveTypography(theme, scheme.typography.uiStackId, scheme.typography.uiSize, scheme.typography.uiLineHeight, locale, 'sans-serif'),
      editor: resolveTypography(theme, scheme.typography.editorStackId, scheme.typography.editorSize, scheme.typography.editorLineHeight, locale, 'monospace'),
    },
    icons, wallpapers,
  };
}

function currentRuntimeLocale() {
  if (typeof document !== 'undefined' && document.documentElement?.lang) return document.documentElement.lang;
  if (typeof navigator !== 'undefined' && navigator.language) return navigator.language;
  return 'en';
}

function resolveTypography(theme: ThemePackage, stackId: string, size: number, lineHeight: number, locale: string, fallback: 'sans-serif' | 'monospace'): ResolvedTypography {
  const fallbackFamilies = fallback === 'monospace'
    ? ['JetBrains Mono', 'SFMono-Regular', 'Consolas']
    : ['Inter Variable', 'Gold Band MiSans', 'Microsoft YaHei UI', 'PingFang SC'];
  const stack = theme.fonts?.stacks.find((candidate) => candidate.id === stackId);
  if (!stack || !theme.fonts) return { families: fallbackFamilies, displayName: fallbackFamilies[0], size, lineHeight };
  const localeInfo = safeLocale(locale);
  const languageScript = localeInfo.language && localeInfo.script ? `${localeInfo.language}-${localeInfo.script}` : undefined;
  const faceIds = stack.byLocale?.[localeInfo.exact]
    ?? (languageScript ? stack.byLocale?.[languageScript] : undefined)
    ?? (localeInfo.script ? stack.byScript?.[localeInfo.script] : undefined)
    ?? stack.defaultFaces;
  const faces = new Map(theme.fonts.faces.map((face) => [face.id, face.runtimeFamily]));
  const families = normalizeFontFamilies([...faceIds.map((id) => faces.get(id) ?? ''), ...stack.systemFallbacks]);
  const language = localeInfo.language === 'zh' ? 'zh-CN' : 'en';
  return { families: families.length ? families : fallbackFamilies, displayName: stack.displayName[language], size, lineHeight };
}

function safeLocale(locale: string) {
  try {
    const exact = new Intl.Locale(locale);
    const maximized = exact.maximize();
    return { exact: exact.toString(), language: maximized.language, script: maximized.script };
  } catch {
    const exact = new Intl.Locale('en');
    const maximized = exact.maximize();
    return { exact: exact.toString(), language: maximized.language, script: maximized.script };
  }
}

const semanticVariableNames: Record<keyof SemanticThemeTokens, string> = {
  background:'--background',foreground:'--foreground',title:'--title',card:'--card',cardForeground:'--card-foreground',popover:'--popover',popoverForeground:'--popover-foreground',primary:'--primary',primaryForeground:'--primary-foreground',secondary:'--secondary',secondaryForeground:'--secondary-foreground',muted:'--muted',mutedForeground:'--muted-foreground',accent:'--accent',accentForeground:'--accent-foreground',destructive:'--destructive',border:'--border',input:'--input',ring:'--ring',selection:'--text-selection',selectionForeground:'--text-selection-foreground',messageUser:'--message-user',messageUserForeground:'--message-user-foreground',contentHeader:'--gb-content-header',contentHeaderForeground:'--gb-content-header-foreground',conversationBackground:'--gb-conversation-background',conversationForeground:'--gb-conversation-foreground',messageAssistant:'--gb-message-assistant',messageAssistantForeground:'--gb-message-assistant-foreground',composer:'--gb-composer',composerForeground:'--gb-composer-foreground',activity:'--gb-activity',activityForeground:'--gb-activity-foreground',toolCard:'--gb-tool-card',toolCardForeground:'--gb-tool-card-foreground',permissionCard:'--gb-permission-card',permissionCardForeground:'--gb-permission-card-foreground',workspaceTab:'--gb-workspace-tab',workspaceTabForeground:'--gb-workspace-tab-foreground',resourceHeader:'--gb-resource-header',resourceHeaderForeground:'--gb-resource-header-foreground',fileTree:'--gb-file-tree',fileTreeForeground:'--gb-file-tree-foreground',editor:'--gb-editor',editorForeground:'--gb-editor-foreground',diffAdded:'--gb-diff-added',diffAddedForeground:'--gb-diff-added-foreground',diffRemoved:'--gb-diff-removed',diffRemovedForeground:'--gb-diff-removed-foreground',diffModified:'--gb-diff-modified',diffModifiedForeground:'--gb-diff-modified-foreground',sidebar:'--sidebar',sidebarForeground:'--sidebar-foreground',sidebarPrimary:'--sidebar-primary',sidebarPrimaryForeground:'--sidebar-primary-foreground',sidebarAccent:'--sidebar-accent',sidebarAccentForeground:'--sidebar-accent-foreground',sidebarBorder:'--sidebar-border',sidebarRing:'--sidebar-ring',workspace:'--gold-workspace',surfaceLow:'--gold-surface-low',surfaceHigh:'--gold-surface-high',lineSoft:'--gold-line-soft',windowOutline:'--gold-window-outline',windowEdgeShadow:'--gold-window-edge-shadow',link:'--link',running:'--gold-running',success:'--gold-success',warning:'--gold-warning',danger:'--gold-danger',permission:'--gold-permission',titlebar:'--titlebar',titlebarForeground:'--titlebar-foreground',titlebarMuted:'--titlebar-muted',titlebarBorder:'--titlebar-border',titlebarHover:'--titlebar-hover',scrollbarTrack:'--gold-scrollbar-track',scrollbarThumb:'--gold-scrollbar-thumb',scrollbarThumbHover:'--gold-scrollbar-thumb-hover',
};

let wallpaperGeneration = 0;
const wallpaperRequests = new WeakMap<HTMLElement, HTMLImageElement>();
let currentEffectiveAppearance: EffectiveAppearance | null = null;
let currentThemeIconSnapshot: { themeId: string; icons: EffectiveAppearance['icons'] } = { themeId: 'builtin.gold-band', icons: {} };
export function getCurrentThemeIconSnapshot() { return currentThemeIconSnapshot; }
export function applyAppearance(preference: AppearancePreference, locale?: string): EffectiveAppearance {
  const effective = resolveAppearance(preference, locale);
  const root = document.documentElement;
  root.dataset.theme = effective.themeId;
  root.dataset.colorScheme = effective.colorScheme;
  root.dataset.visualQuality = effective.visualQuality ?? 'full';
  root.dataset.materialModel = effective.material.model;
  root.classList.toggle('dark', effective.colorScheme === 'dark');
  root.style.colorScheme = effective.colorScheme;
  for (const [key, value] of Object.entries(effective.semantic) as [keyof SemanticThemeTokens, string][]) root.style.setProperty(semanticVariableNames[key], value);
  applyRootVariables(root, effective);
  const generation = ++wallpaperGeneration;
  currentEffectiveAppearance = effective;
  applyVisibleWallpapers(effective, generation);
  currentThemeIconSnapshot = { themeId: effective.themeId, icons: effective.icons };
  if (typeof window !== 'undefined' && typeof window.dispatchEvent === 'function' && typeof CustomEvent === 'function') {
    window.dispatchEvent(new CustomEvent('gold-band-theme-icons-changed', { detail: currentThemeIconSnapshot }));
  }
  void syncDesktopWindowSurface(effective);
  return effective;
}

export function refreshVisibleThemeWallpapers() {
  if (currentEffectiveAppearance) applyVisibleWallpapers(currentEffectiveAppearance, wallpaperGeneration);
}

function applyRootVariables(root: HTMLElement, effective: EffectiveAppearance) {
  const { material, shape, elevation, motion, scrollbar, typography } = effective;
  const variables: Record<string, string> = {
    '--radius': shape.radiusControl, '--gb-radius-control': shape.radiusControl, '--gb-radius-surface': shape.radiusSurface,
    '--gb-radius-overlay': shape.radiusOverlay, '--gb-radius-avatar': shape.radiusAvatar, '--gb-radius-pill': shape.radiusPill,
    '--gb-border-hairline': shape.borderHairline, '--gb-border-default': shape.borderDefault, '--gb-border-strong': shape.borderStrong,
    '--gb-elevation-none': elevation.none, '--gb-elevation-surface': elevation.surface, '--gb-elevation-overlay': elevation.overlay,
    '--gb-elevation-floating': elevation.floating, '--gb-elevation-pressed': elevation.pressed, '--gb-press-offset': `${elevation.pressOffset}px`,
    '--gb-motion-fast': motion.durationFast, '--gb-motion-normal': motion.durationNormal, '--gb-motion-slow': motion.durationSlow,
    '--gb-easing-standard': motion.easingStandard, '--gb-easing-enter': motion.easingEnter, '--gb-easing-press': motion.easingPress,
    '--gb-scrollbar-width': scrollbar.width, '--gb-scrollbar-thumb-radius': scrollbar.thumbRadius,
    '--gb-scrollbar-thumb-inset': scrollbar.thumbInset, '--gb-scrollbar-min-length': scrollbar.minLength,
    '--gb-material-model': material.model, '--gb-material-opacity': String(material.surfaceOpacity),
    '--gb-material-border-highlight': material.borderHighlight, '--gb-material-surface-overlay': material.surfaceOverlay,
    '--gb-material-blur': `${material.blur}px`, '--gb-material-saturate': `${material.saturate}%`,
    '--gb-material-backdrop-brightness': `${material.backdropBrightness}%`, '--gb-material-backdrop-contrast': `${material.backdropContrast}%`,
    '--gb-material-specular-highlight': material.specularHighlight, '--gb-material-edge-shadow': material.edgeShadow,
    '--gb-theme-background-image': material.backgroundImage, '--gb-theme-texture-opacity': String(material.textureOpacity),
    '--gb-theme-ui-font-family': serializeThemeFontStack(typography.ui.families, 'sans-serif'),
    '--gb-theme-editor-font-family': serializeThemeFontStack(typography.editor.families, 'monospace'),
    '--gb-theme-ui-font-size': `${typography.ui.size}px`, '--gb-theme-editor-font-size': `${typography.editor.size}px`,
    '--gb-theme-ui-line-height': String(typography.ui.lineHeight), '--gb-theme-editor-line-height': String(typography.editor.lineHeight),
  };
  for (const [name, value] of Object.entries(variables)) root.style.setProperty(name, value);
}

function applyVisibleWallpapers(effective: EffectiveAppearance, generation: number) {
  if (typeof document === 'undefined' || typeof document.querySelectorAll !== 'function') return;
  const surfaces = document.querySelectorAll<HTMLElement>('[data-theme-wallpaper-slot]');
  for (const surface of surfaces) {
    wallpaperRequests.delete(surface);
    clearWallpaper(surface);
    const slot = surface.dataset.themeWallpaperSlot as ThemeWallpaperSlot;
    const descriptor = effective.wallpapers[slot];
    if (!descriptor || typeof Image !== 'function' || typeof surface.getClientRects !== 'function' || surface.getClientRects().length === 0) continue;
    const image = new Image();
    wallpaperRequests.set(surface, image);
    image.decoding = 'async';
    image.onload = () => {
      if (generation !== wallpaperGeneration || wallpaperRequests.get(surface) !== image || !surface.isConnected) return;
      surface.style.setProperty('--gb-wallpaper-image', `url("${descriptor.url.replaceAll('"', '\\"')}")`);
      surface.style.setProperty('--gb-wallpaper-size', descriptor.fit === 'tile' ? 'auto' : descriptor.fit);
      surface.style.setProperty('--gb-wallpaper-position', descriptor.position.replace('-', ' '));
      surface.style.setProperty('--gb-wallpaper-repeat', descriptor.repeat);
      surface.style.setProperty('--gb-wallpaper-opacity', String(descriptor.opacity));
      surface.style.setProperty('--gb-wallpaper-overlay-color', backgroundRefVariable(descriptor.overlayColor));
      surface.style.setProperty('--gb-wallpaper-overlay-opacity', String(descriptor.overlayOpacity));
    };
    image.onerror = () => {
      if (generation !== wallpaperGeneration || wallpaperRequests.get(surface) !== image || !surface.isConnected) return;
      wallpaperRequests.delete(surface);
      clearWallpaper(surface);
    };
    image.src = descriptor.url;
  }
}
function clearWallpaper(surface: HTMLElement) {
  for (const name of ['--gb-wallpaper-image', '--gb-wallpaper-size', '--gb-wallpaper-position', '--gb-wallpaper-repeat', '--gb-wallpaper-opacity', '--gb-wallpaper-overlay-color', '--gb-wallpaper-overlay-opacity']) surface.style.removeProperty(name);
}
function backgroundRefVariable(value: ThemeWallpaperDescriptor['overlayColor']) {
  if (value === 'transparent') return 'transparent';
  const names: Record<Exclude<ThemeWallpaperDescriptor['overlayColor'], 'transparent'>, string> = {
    background: '--background', card: '--card', popover: '--popover', sidebar: '--sidebar', 'surface-low': '--gold-surface-low',
    'surface-high': '--gold-surface-high', accent: '--accent', primary: '--primary', 'message-user': '--message-user',
    'message-assistant': '--gb-message-assistant', composer: '--gb-composer', activity: '--gb-activity', 'tool-card': '--gb-tool-card',
    'permission-card': '--gb-permission-card', 'workspace-tab': '--gb-workspace-tab', editor: '--gb-editor',
  };
  return `var(${names[value]})`;
}

export function syncDesktopWindowSurface(appearance: EffectiveAppearance): Promise<void> {
  if (!isTauriRuntime()) return Promise.resolve();
  return getCurrentWindow().setBackgroundColor(appearance.scheme.windowSurface).catch(() => {});
}
export function appearanceWithTheme(preference: AppearancePreference, themeId: string): AppearancePreference { return normalizeAppearancePreference({ ...preference, themeId }); }
export function appearanceWithQuality(preference: AppearancePreference, quality: VisualQuality): AppearancePreference {
  const theme = getThemePackage(preference.themeId);
  return theme.visualQualityProfiles ? { ...preference, visualQualityByTheme: { ...preference.visualQualityByTheme, [theme.id]: quality } } : preference;
}

export interface DesktopFontOption { id: string; labelKey: string; descriptionKey: string; preview: string }
export const desktopFontOptions = [{ id:'app-default',labelKey:'settings.fontDefault',descriptionKey:'settings.fontDefaultDescription',preview:'Gold-Band / 优化 resume 会话 / 0123' }] as const satisfies readonly DesktopFontOption[];
export const desktopEditorFontOptions = [{ id:'editor-default',labelKey:'settings.editorFontDefault',descriptionKey:'settings.editorFontDefaultDescription',preview:'const workflow = "AI";' }] as const satisfies readonly DesktopFontOption[];
export const desktopTypography = { ui:{min:12,max:18,defaultValue:14},editor:{min:10,max:18,defaultValue:12} } as const;
export function fontFamilyForStack(families: readonly string[], fallbackVariable = 'var(--gb-theme-ui-font-family)') { return serializeCustomFontStack(resolveRuntimeFontFamilies(families), fallbackVariable); }
export function editorFontFamilyForStack(families: readonly string[]) { return serializeCustomFontStack(resolveRuntimeFontFamilies(families), 'var(--gb-theme-editor-font-family)'); }
export function normalizeTypographySize(value:number,kind:keyof typeof desktopTypography) { const constraint=desktopTypography[kind]; if(!Number.isFinite(value))return constraint.defaultValue; return Math.min(constraint.max,Math.max(constraint.min,Math.round(value))); }
export function applyPersonalization(preference: PersonalizationPreference) {
  const root = document.documentElement;
  const uiFont = preference.typography.ui.fontStack;
  const editorFont = preference.typography.editor.fontStack;
  const uiFontSize = preference.typography.ui.fontSize;
  const editorFontSize = preference.typography.editor.fontSize;
  root.dataset.font = uiFont.source;
  root.dataset.editorFont = editorFont.source;
  root.style.setProperty('--app-font-sans', uiFont.source === 'theme' ? 'var(--gb-theme-ui-font-family)' : fontFamilyForStack(uiFont.families));
  root.style.setProperty('--app-editor-font-family', editorFont.source === 'theme' ? 'var(--gb-theme-editor-font-family)' : editorFontFamilyForStack(editorFont.families));
  root.style.setProperty('--app-ui-font-size', uiFontSize.source === 'theme' ? 'var(--gb-theme-ui-font-size)' : `${normalizeTypographySize(uiFontSize.px, 'ui')}px`);
  root.style.setProperty('--app-editor-font-size', editorFontSize.source === 'theme' ? 'var(--gb-theme-editor-font-size)' : `${normalizeTypographySize(editorFontSize.px, 'editor')}px`);
}
function resolveRuntimeFontFamilies(families: readonly string[]) {
  const theme = getThemePackage(currentEffectiveAppearance?.themeId ?? defaultAppearancePreference.themeId);
  const runtimeFamilies = new Map(theme.fonts?.faces.map((face) => [face.family.toLocaleLowerCase(), face.runtimeFamily]) ?? []);
  return normalizeFontFamilies(families).map((family) => runtimeFamilies.get(family.toLocaleLowerCase()) ?? family);
}
export function normalizeFontFamilies(families: readonly string[]) {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const family of families) {
    const trimmed = family.trim();
    const key = trimmed.toLocaleLowerCase();
    if (!trimmed || Array.from(trimmed).length > MAX_FONT_FAMILY_CODE_POINTS || /[,;{}]/u.test(trimmed) || seen.has(key)) continue;
    seen.add(key);
    normalized.push(trimmed);
    if (normalized.length === MAX_FONT_STACK_FAMILIES) break;
  }
  return normalized;
}
export function toggleFontFamily(families: readonly string[], family: string) {
  const normalized = normalizeFontFamilies(families);
  const target = family.trim();
  const index = normalized.findIndex((candidate) => candidate.toLocaleLowerCase() === target.toLocaleLowerCase());
  return index >= 0 ? normalized.filter((_, candidateIndex) => candidateIndex !== index) : normalizeFontFamilies([...normalized, target]);
}
export function moveFontFamily(families: readonly string[], index: number, direction: -1 | 1) {
  const normalized = normalizeFontFamilies(families);
  const target = index + direction;
  if (index < 0 || index >= normalized.length || target < 0 || target >= normalized.length) return normalized;
  [normalized[index], normalized[target]] = [normalized[target], normalized[index]];
  return normalized;
}
function serializeCustomFontStack(families: readonly string[], fallbackVariable: string) { return [...normalizeFontFamilies(families).map(quoteFontFamily), fallbackVariable].join(', '); }
function serializeThemeFontStack(families: readonly string[], fallback: string) { return [...normalizeFontFamilies(families).filter((family) => family !== fallback).map(quoteFontFamily), fallback].join(', '); }
function quoteFontFamily(font:string) { return `"${font.replaceAll('\\','\\\\').replaceAll('"','\\"')}"`; }
