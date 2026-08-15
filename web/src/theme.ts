import { getCurrentWindow } from '@tauri-apps/api/window';
import { z } from 'zod';
import { isTauriRuntime } from './api/shared';
import type { AppearancePreference, PersonalizationPreference, ResolvedColorScheme, VisualQuality } from './types';
import {
  MAX_FONT_FAMILY_CODE_POINTS,
  MAX_FONT_STACK_FAMILIES,
  type MaterialTokens,
  type SemanticThemeTokens,
  type ThemePackage,
  type ThemeScheme,
} from './theme-contract';
import { builtinThemes } from './themes/builtin-themes';

export interface ThemePreviewPalette {
  background: string; surface: string; border: string; primary: string;
  foreground: string; muted: string; success: string; danger: string;
}

export interface EffectiveAppearance {
  themeId: string;
  themeVersion: string;
  colorScheme: ResolvedColorScheme;
  visualQuality?: VisualQuality;
  scheme: ThemeScheme;
  material: MaterialTokens;
  recipes: ThemePackage['recipes'];
}

const appearancePreferenceSchema = z.object({
  schemaVersion: z.literal(2), themeId: z.string().min(1),
  colorScheme: z.enum(['system', 'light', 'dark']),
  visualQualityByTheme: z.record(z.string(), z.enum(['full', 'performance'])),
}).strict();
const themeCatalog = new Map<string, ThemePackage>(builtinThemes.map((theme) => [theme.id, theme]));
export const themePackageSummaries = builtinThemes.map((theme) => ({
  id: theme.id, version: theme.version, contractVersion: theme.contractVersion, source: theme.source,
  name: theme.name, capabilities: theme.capabilities, preview: {
    light: theme.schemes.light.preview, dark: theme.schemes.dark.preview,
  },
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
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function resolveAppearance(preference: AppearancePreference): EffectiveAppearance {
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
  return { themeId: theme.id, themeVersion: theme.version, colorScheme, visualQuality: selectedQuality, scheme, material, recipes: theme.recipes };
}

const semanticVariableNames: Record<keyof SemanticThemeTokens, string> = {
background:'--background',foreground:'--foreground',title:'--title',card:'--card',cardForeground:'--card-foreground',popover:'--popover',popoverForeground:'--popover-foreground',primary:'--primary',primaryForeground:'--primary-foreground',secondary:'--secondary',secondaryForeground:'--secondary-foreground',muted:'--muted',mutedForeground:'--muted-foreground',accent:'--accent',accentForeground:'--accent-foreground',destructive:'--destructive',border:'--border',input:'--input',ring:'--ring',selection:'--text-selection',selectionForeground:'--text-selection-foreground',messageUser:'--message-user',messageUserForeground:'--message-user-foreground',contentHeader:'--gb-content-header',contentHeaderForeground:'--gb-content-header-foreground',conversationBackground:'--gb-conversation-background',conversationForeground:'--gb-conversation-foreground',messageAssistant:'--gb-message-assistant',messageAssistantForeground:'--gb-message-assistant-foreground',composer:'--gb-composer',composerForeground:'--gb-composer-foreground',activity:'--gb-activity',activityForeground:'--gb-activity-foreground',toolCard:'--gb-tool-card',toolCardForeground:'--gb-tool-card-foreground',permissionCard:'--gb-permission-card',permissionCardForeground:'--gb-permission-card-foreground',workspaceTab:'--gb-workspace-tab',workspaceTabForeground:'--gb-workspace-tab-foreground',resourceHeader:'--gb-resource-header',resourceHeaderForeground:'--gb-resource-header-foreground',fileTree:'--gb-file-tree',fileTreeForeground:'--gb-file-tree-foreground',editor:'--gb-editor',editorForeground:'--gb-editor-foreground',diffAdded:'--gb-diff-added',diffAddedForeground:'--gb-diff-added-foreground',diffRemoved:'--gb-diff-removed',diffRemovedForeground:'--gb-diff-removed-foreground',diffModified:'--gb-diff-modified',diffModifiedForeground:'--gb-diff-modified-foreground',sidebar:'--sidebar',sidebarForeground:'--sidebar-foreground',sidebarPrimary:'--sidebar-primary',sidebarPrimaryForeground:'--sidebar-primary-foreground',sidebarAccent:'--sidebar-accent',sidebarAccentForeground:'--sidebar-accent-foreground',sidebarBorder:'--sidebar-border',sidebarRing:'--sidebar-ring',workspace:'--gold-workspace',surfaceLow:'--gold-surface-low',surfaceHigh:'--gold-surface-high',lineSoft:'--gold-line-soft',windowOutline:'--gold-window-outline',windowEdgeShadow:'--gold-window-edge-shadow',running:'--gold-running',success:'--gold-success',warning:'--gold-warning',danger:'--gold-danger',permission:'--gold-permission',titlebar:'--titlebar',titlebarForeground:'--titlebar-foreground',titlebarMuted:'--titlebar-muted',titlebarBorder:'--titlebar-border',titlebarHover:'--titlebar-hover',scrollbarTrack:'--gold-scrollbar-track',scrollbarThumb:'--gold-scrollbar-thumb',scrollbarThumbHover:'--gold-scrollbar-thumb-hover',
};

export function applyAppearance(preference: AppearancePreference): EffectiveAppearance {
  const effective = resolveAppearance(preference);
  const root = document.documentElement;
  root.dataset.theme = effective.themeId;
  root.dataset.colorScheme = effective.colorScheme;
  root.dataset.visualQuality = effective.visualQuality ?? 'full';
  root.dataset.materialModel = effective.material.model;
  root.classList.toggle('dark', effective.colorScheme === 'dark');
  root.style.colorScheme = effective.colorScheme;
  for (const [key, value] of Object.entries(effective.scheme.semantic) as [keyof SemanticThemeTokens, string][]) {
    root.style.setProperty(semanticVariableNames[key], value);
  }
  root.style.setProperty('--radius', effective.material.radius);
  root.style.setProperty('--gb-material-model', effective.material.model);
  root.style.setProperty('--gb-material-opacity', String(effective.material.surfaceOpacity));
  root.style.setProperty('--gb-material-border-highlight', effective.material.borderHighlight);
  root.style.setProperty('--gb-material-surface-overlay', effective.material.surfaceOverlay);
  root.style.setProperty('--gb-material-blur', `${effective.material.blur}px`);
  root.style.setProperty('--gb-material-saturate', `${effective.material.saturate}%`);
  root.style.setProperty('--gb-material-backdrop-brightness', `${effective.material.backdropBrightness}%`);
  root.style.setProperty('--gb-material-backdrop-contrast', `${effective.material.backdropContrast}%`);
  root.style.setProperty('--gb-material-specular-highlight', effective.material.specularHighlight);
  root.style.setProperty('--gb-material-edge-shadow', effective.material.edgeShadow);
  root.style.setProperty('--gb-material-shadow', effective.material.shadow);
  root.style.setProperty('--gb-theme-background-image', effective.material.backgroundImage);
  root.style.setProperty('--gb-theme-texture-opacity', String(effective.material.textureOpacity));
  root.style.setProperty('--gb-theme-motion-duration', effective.material.motionDuration);
  root.style.setProperty('--gb-theme-motion-easing', effective.material.motionEasing);
  root.style.setProperty('--gb-theme-ui-font-family', serializeThemeFontStack(effective.scheme.typography.ui));
  root.style.setProperty('--gb-theme-editor-font-family', serializeThemeFontStack(effective.scheme.typography.editor));
  root.style.setProperty('--gb-theme-ui-font-size', `${effective.scheme.typography.ui.size}px`);
  root.style.setProperty('--gb-theme-editor-font-size', `${effective.scheme.typography.editor.size}px`);
  void syncDesktopWindowSurface(effective);
  return effective;
}

export function syncDesktopWindowSurface(appearance: EffectiveAppearance): Promise<void> {
  if (!isTauriRuntime()) return Promise.resolve();
  return getCurrentWindow().setBackgroundColor(appearance.scheme.windowSurface).catch(() => {});
}

export function appearanceWithTheme(preference: AppearancePreference, themeId: string): AppearancePreference {
  return normalizeAppearancePreference({ ...preference, themeId });
}

export function appearanceWithQuality(preference: AppearancePreference, quality: VisualQuality): AppearancePreference {
  const theme = getThemePackage(preference.themeId);
  if (!theme.visualQualityProfiles) return preference;
  return { ...preference, visualQualityByTheme: { ...preference.visualQualityByTheme, [theme.id]: quality } };
}

export interface DesktopFontOption { id: string; labelKey: string; descriptionKey: string; preview: string }
export const desktopFontOptions = [{ id:'app-default',labelKey:'settings.fontDefault',descriptionKey:'settings.fontDefaultDescription',preview:'Gold-Band / 优化 resume 会话 / 0123' }] as const satisfies readonly DesktopFontOption[];
export const desktopEditorFontOptions = [{ id:'editor-default',labelKey:'settings.editorFontDefault',descriptionKey:'settings.editorFontDefaultDescription',preview:'const workflow = "AI";' }] as const satisfies readonly DesktopFontOption[];
export const desktopTypography = { ui:{min:12,max:18,defaultValue:14},editor:{min:10,max:18,defaultValue:12} } as const;
export function fontFamilyForStack(families: readonly string[], fallbackVariable = 'var(--gb-theme-ui-font-family)') { return serializeCustomFontStack(families, fallbackVariable); }
export function editorFontFamilyForStack(families: readonly string[]) { return serializeCustomFontStack(families, 'var(--gb-theme-editor-font-family)'); }
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
  return index >= 0
    ? normalized.filter((_, candidateIndex) => candidateIndex !== index)
    : normalizeFontFamilies([...normalized, target]);
}
export function moveFontFamily(families: readonly string[], index: number, direction: -1 | 1) {
  const normalized = normalizeFontFamilies(families);
  const target = index + direction;
  if (index < 0 || index >= normalized.length || target < 0 || target >= normalized.length) return normalized;
  [normalized[index], normalized[target]] = [normalized[target], normalized[index]];
  return normalized;
}
function serializeCustomFontStack(families: readonly string[], fallbackVariable: string) {
  return [...normalizeFontFamilies(families).map(quoteFontFamily), fallbackVariable].join(', ');
}
function serializeThemeFontStack(stack: { families: readonly string[]; fallback: string }) {
  return [...normalizeFontFamilies(stack.families).map(quoteFontFamily), stack.fallback].join(', ');
}
function quoteFontFamily(font:string) { return `"${font.replaceAll('\\','\\\\').replaceAll('"','\\"')}"`; }
