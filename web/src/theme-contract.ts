import { z } from 'zod';

export const colorSchemeSchema = z.enum(['light', 'dark']);
export const visualQualitySchema = z.enum(['full', 'performance']);
export const themeCapabilitySchema = z.enum([
  'tokens',
  'component-recipes',
  'fonts',
  'avatars',
  'textures',
  'visual-quality-profiles',
]);

const localizedTextSchema = z.object({ 'zh-CN': z.string().min(1), en: z.string().min(1) }).strict();
const previewPaletteSchema = z.object({
  background: z.string(), surface: z.string(), border: z.string(), primary: z.string(),
  foreground: z.string(), muted: z.string(), success: z.string(), danger: z.string(),
}).strict();

export const semanticThemeTokensSchema = z.object({
  background: z.string(), foreground: z.string(), title: z.string(),
  card: z.string(), cardForeground: z.string(), popover: z.string(), popoverForeground: z.string(),
  primary: z.string(), primaryForeground: z.string(), secondary: z.string(), secondaryForeground: z.string(),
  muted: z.string(), mutedForeground: z.string(), accent: z.string(), accentForeground: z.string(),
  destructive: z.string(), border: z.string(), input: z.string(), ring: z.string(),
  selection: z.string(), selectionForeground: z.string(),
  messageUser: z.string(), messageUserForeground: z.string(),
  contentHeader: z.string(), contentHeaderForeground: z.string(),
  conversationBackground: z.string(), conversationForeground: z.string(),
  messageAssistant: z.string(), messageAssistantForeground: z.string(),
  composer: z.string(), composerForeground: z.string(),
  activity: z.string(), activityForeground: z.string(),
  toolCard: z.string(), toolCardForeground: z.string(),
  permissionCard: z.string(), permissionCardForeground: z.string(),
  workspaceTab: z.string(), workspaceTabForeground: z.string(),
  resourceHeader: z.string(), resourceHeaderForeground: z.string(),
  fileTree: z.string(), fileTreeForeground: z.string(),
  editor: z.string(), editorForeground: z.string(),
  diffAdded: z.string(), diffAddedForeground: z.string(),
  diffRemoved: z.string(), diffRemovedForeground: z.string(),
  diffModified: z.string(), diffModifiedForeground: z.string(),
  sidebar: z.string(), sidebarForeground: z.string(), sidebarPrimary: z.string(),
  sidebarPrimaryForeground: z.string(), sidebarAccent: z.string(), sidebarAccentForeground: z.string(),
  sidebarBorder: z.string(), sidebarRing: z.string(), workspace: z.string(),
  surfaceLow: z.string(), surfaceHigh: z.string(), lineSoft: z.string(),
  windowOutline: z.string(), windowEdgeShadow: z.string(), running: z.string(),
  success: z.string(), warning: z.string(), danger: z.string(), permission: z.string(),
  titlebar: z.string(), titlebarForeground: z.string(), titlebarMuted: z.string(),
  titlebarBorder: z.string(), titlebarHover: z.string(),
  scrollbarTrack: z.string(), scrollbarThumb: z.string(), scrollbarThumbHover: z.string(),
}).strict();

export const materialTokensSchema = z.object({
  model: z.enum(['solid', 'frosted', 'liquid']).default('solid'),
  surfaceOpacity: z.number().min(0).max(1),
  borderHighlight: z.string(),
  surfaceOverlay: z.string(),
  blur: z.number().min(0).max(60),
  saturate: z.number().min(100).max(200),
  backdropBrightness: z.number().min(80).max(120).default(100),
  backdropContrast: z.number().min(80).max(140).default(100),
  specularHighlight: z.string().default('none'),
  edgeShadow: z.string().default('0 0 0 transparent'),
  shadow: z.string(),
  radius: z.string(),
  backgroundImage: z.string(),
  textureOpacity: z.number().min(0).max(0.04),
  motionDuration: z.string(),
  motionEasing: z.string(),
}).strict();

export const MAX_FONT_STACK_FAMILIES = 16;
export const MAX_FONT_FAMILY_CODE_POINTS = 128;

const fontStackPresetSchema = (minimum: number, maximum: number, fallback: 'sans-serif' | 'monospace') => z.object({
  families: z.array(z.string().min(1).max(MAX_FONT_FAMILY_CODE_POINTS).regex(/^[^,;{}]+$/u)).min(1).max(MAX_FONT_STACK_FAMILIES),
  fallback: z.literal(fallback),
  size: z.number().min(minimum).max(maximum),
}).strict();
const typographyPresetSchema = z.object({
  ui: fontStackPresetSchema(12, 18, 'sans-serif'),
  editor: fontStackPresetSchema(10, 18, 'monospace'),
}).strict();
const avatarPresetSchema = z.object({
  agentShape: z.enum(['circle', 'square']), userShape: z.enum(['circle', 'square']),
  agentAsset: z.string().nullable(), userAsset: z.string().nullable(),
}).strict();
const surfaceRecipeSchema = z.object({
  background: z.enum(['card', 'popover', 'sidebar', 'surface-low', 'surface-high', 'transparent']),
  foreground: z.enum(['foreground', 'muted-foreground', 'card-foreground']),
  border: z.enum(['border', 'sidebar-border', 'highlight']),
  material: z.enum(['flat', 'subtle', 'elevated']),
}).strict();
const componentRecipesSchema = z.object({
  shell: surfaceRecipeSchema, titlebar: surfaceRecipeSchema, sidebar: surfaceRecipeSchema,
  panel: surfaceRecipeSchema, composer: surfaceRecipeSchema, card: surfaceRecipeSchema,
  dialog: surfaceRecipeSchema, sheet: surfaceRecipeSchema, popover: surfaceRecipeSchema,
  input: surfaceRecipeSchema, button: surfaceRecipeSchema, editor: surfaceRecipeSchema,
}).strict();

const themeSchemeSchema = z.object({
  windowSurface: z.string(), preview: previewPaletteSchema,
  semantic: semanticThemeTokensSchema, material: materialTokensSchema,
  typography: typographyPresetSchema, avatars: avatarPresetSchema,
}).strict();
const performanceOverridesSchema = z.object({
  blur: z.number().min(0).max(24), saturate: z.number().min(100).max(160),
  backdropBrightness: z.number().min(80).max(120).optional(),
  backdropContrast: z.number().min(80).max(140).optional(),
  specularHighlight: z.string().optional(), edgeShadow: z.string().optional(),
  shadow: z.string(), textureOpacity: z.number().min(0).max(0.02),
  motionDuration: z.string(),
}).strict();

export const themePackageSchema = z.object({
  schemaVersion: z.literal(2), contractVersion: z.literal(2), id: z.string().regex(/^(builtin|user)\.[a-z0-9][a-z0-9.-]*$/),
  version: z.string().regex(/^\d+\.\d+\.\d+$/), source: z.enum(['builtin', 'user']),
  name: localizedTextSchema, author: z.string().optional(),
  capabilities: z.array(themeCapabilitySchema).min(2),
  schemes: z.object({ light: themeSchemeSchema, dark: themeSchemeSchema }).strict(),
  recipes: componentRecipesSchema,
  visualQualityProfiles: z.object({
    default: visualQualitySchema,
    supported: z.tuple([z.literal('full'), z.literal('performance')]),
    performance: performanceOverridesSchema,
  }).strict().optional(),
}).strict().superRefine((theme, context) => {
  const declaresQuality = theme.capabilities.includes('visual-quality-profiles');
  if (declaresQuality !== Boolean(theme.visualQualityProfiles)) {
    context.addIssue({ code: 'custom', message: 'visual quality capability and profiles must be declared together' });
  }
});

export type ThemePackage = z.infer<typeof themePackageSchema>;
export type ThemeScheme = z.infer<typeof themeSchemeSchema>;
export type SemanticThemeTokens = z.infer<typeof semanticThemeTokensSchema>;
export type MaterialTokens = z.infer<typeof materialTokensSchema>;
