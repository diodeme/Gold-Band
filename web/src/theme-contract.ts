import { z } from 'zod';

export const colorSchemeSchema = z.enum(['light', 'dark']);
export const visualQualitySchema = z.enum(['full', 'performance']);
export const themeCapabilitySchema = z.enum([
  'tokens', 'component-recipes', 'fonts', 'avatars', 'textures', 'icons',
  'wallpapers', 'visual-quality-profiles',
]);
export const themeIconSlotSchema = z.enum([
  'navigation.conversation', 'navigation.search', 'navigation.agent', 'navigation.context',
  'navigation.run-mode', 'navigation.settings', 'entity.task', 'entity.workflow',
  'entity.agent', 'entity.file', 'entity.folder', 'conversation.thought',
  'conversation.attachment', 'tool.read', 'tool.write', 'tool.command',
  'permission.request', 'status.running', 'status.success', 'status.warning',
  'status.error', 'action.send', 'action.continue', 'action.stop',
]);
export const themeWallpaperSlotSchema = z.enum(['app', 'conversation', 'workspace', 'settings']);
export type ThemeIconSlot = z.infer<typeof themeIconSlotSchema>;
export type ThemeWallpaperSlot = z.infer<typeof themeWallpaperSlotSchema>;

const localizedTextSchema = z.object({ 'zh-CN': z.string().min(1), en: z.string().min(1) }).strict();
const previewPaletteSchema = z.object({
  background: z.string(), surface: z.string(), border: z.string(), primary: z.string(),
  foreground: z.string(), muted: z.string(), success: z.string(), danger: z.string(),
}).strict();

export const semanticThemeTokensSchema = z.object({
  background: z.string(), foreground: z.string(), title: z.string(), card: z.string(), cardForeground: z.string(),
  popover: z.string(), popoverForeground: z.string(), primary: z.string(), primaryForeground: z.string(),
  secondary: z.string(), secondaryForeground: z.string(), muted: z.string(), mutedForeground: z.string(),
  accent: z.string(), accentForeground: z.string(), destructive: z.string(), border: z.string(), input: z.string(),
  ring: z.string(), selection: z.string(), selectionForeground: z.string(), messageUser: z.string(),
  messageUserForeground: z.string(), contentHeader: z.string(), contentHeaderForeground: z.string(),
  conversationBackground: z.string(), conversationForeground: z.string(), messageAssistant: z.string(),
  messageAssistantForeground: z.string(), composer: z.string(), composerForeground: z.string(), activity: z.string(),
  activityForeground: z.string(), toolCard: z.string(), toolCardForeground: z.string(), permissionCard: z.string(),
  permissionCardForeground: z.string(), workspaceTab: z.string(), workspaceTabForeground: z.string(),
  resourceHeader: z.string(), resourceHeaderForeground: z.string(), fileTree: z.string(), fileTreeForeground: z.string(),
  editor: z.string(), editorForeground: z.string(), diffAdded: z.string(), diffAddedForeground: z.string(),
  diffRemoved: z.string(), diffRemovedForeground: z.string(), diffModified: z.string(), diffModifiedForeground: z.string(),
  sidebar: z.string(), sidebarForeground: z.string(), sidebarPrimary: z.string(), sidebarPrimaryForeground: z.string(),
  sidebarAccent: z.string(), sidebarAccentForeground: z.string(), sidebarBorder: z.string(), sidebarRing: z.string(),
  workspace: z.string(), surfaceLow: z.string(), surfaceHigh: z.string(), lineSoft: z.string(),
  windowOutline: z.string(), windowEdgeShadow: z.string(), link: z.string(), running: z.string(), success: z.string(),
  warning: z.string(), danger: z.string(), permission: z.string(), titlebar: z.string(), titlebarForeground: z.string(),
  titlebarMuted: z.string(), titlebarBorder: z.string(), titlebarHover: z.string(), scrollbarTrack: z.string(),
  scrollbarThumb: z.string(), scrollbarThumbHover: z.string(),
}).strict();

export const materialTokensSchema = z.object({
  model: z.enum(['solid', 'frosted', 'liquid']), surfaceOpacity: z.number().min(0).max(1), borderHighlight: z.string(),
  surfaceOverlay: z.string(), blur: z.number().min(0).max(60), saturate: z.number().min(100).max(200),
  backdropBrightness: z.number().min(80).max(120), backdropContrast: z.number().min(80).max(140),
  specularHighlight: z.string(), edgeShadow: z.string(), backgroundImage: z.string(), textureOpacity: z.number().min(0).max(0.04),
}).strict();
export const shapeTokensSchema = z.object({
  radiusControl: z.string(), radiusSurface: z.string(), radiusOverlay: z.string(), radiusAvatar: z.string(), radiusPill: z.string(),
  borderHairline: z.string(), borderDefault: z.string(), borderStrong: z.string(),
}).strict();
export const elevationTokensSchema = z.object({
  none: z.string(), surface: z.string(), overlay: z.string(), floating: z.string(), pressed: z.string(), pressOffset: z.union([z.literal(0), z.literal(1), z.literal(2)]),
}).strict();
export const motionTokensSchema = z.object({
  mode: z.enum(['smooth', 'stepped', 'none']), durationFast: z.string(), durationNormal: z.string(), durationSlow: z.string(),
  easingStandard: z.string(), easingEnter: z.string(), easingPress: z.string(),
}).strict();
export const scrollbarTokensSchema = z.object({
  width: z.string(), thumbRadius: z.string(), thumbInset: z.string(), minLength: z.string(), buttons: z.enum(['none', 'visible']),
}).strict();

export const MAX_FONT_STACK_FAMILIES = 16;
export const MAX_FONT_FAMILY_CODE_POINTS = 128;
const fontFaceSchema = z.object({
  id: z.string(), family: z.string(), runtimeFamily: z.string(), assetId: z.string(), weightMin: z.number().int().min(1).max(1000), weightMax: z.number().int().min(1).max(1000),
  style: z.enum(['normal', 'italic']), display: z.literal('swap'),
  coverage: z.object({ scripts: z.array(z.string()), locales: z.array(z.string()).optional(), unicodeRanges: z.array(z.string()).optional() }).strict(),
  metrics: z.object({ sizeAdjust: z.string().optional(), ascentOverride: z.string().optional(), descentOverride: z.string().optional(), lineGapOverride: z.string().optional() }).strict().optional(),
}).strict().refine((face) => face.weightMin <= face.weightMax, { message: 'font weight range must be ordered' });
const fontStackSchema = z.object({
  id: z.string(), displayName: localizedTextSchema, defaultFaces: z.array(z.string()),
  systemFallbacks: z.array(z.string()).min(1).max(MAX_FONT_STACK_FAMILIES),
}).strict();
const fontsRuntimeSchema = z.object({ faces: z.array(fontFaceSchema), stacks: z.array(fontStackSchema).min(2) }).strict();
const typographyPresetSchema = z.object({
  uiStackId: z.string(), uiSize: z.number().min(12).max(18), uiLineHeight: z.number(),
  editorStackId: z.string(), editorSize: z.number().min(10).max(18), editorLineHeight: z.number(),
  weights: z.object({ read: z.union([z.literal(400), z.literal(500)]), emphasize: z.union([z.literal(400), z.literal(500), z.literal(600)]), announce: z.union([z.literal(500), z.literal(600), z.literal(700)]) }).strict(),
}).strict();
const avatarPresetSchema = z.object({
  agentShape: z.enum(['circle', 'square']), userShape: z.enum(['circle', 'square']), agentAsset: z.string().nullable(), userAsset: z.string().nullable(),
}).strict();
const backgroundRefSchema = z.enum(['background', 'card', 'popover', 'sidebar', 'surface-low', 'surface-high', 'accent', 'primary', 'message-user', 'message-assistant', 'composer', 'activity', 'tool-card', 'permission-card', 'workspace-tab', 'editor', 'transparent']);
const foregroundRefSchema = z.enum(['foreground', 'muted-foreground', 'card-foreground', 'accent-foreground', 'primary-foreground', 'message-user-foreground', 'message-assistant-foreground', 'composer-foreground', 'activity-foreground', 'tool-card-foreground', 'permission-card-foreground', 'workspace-tab-foreground', 'editor-foreground']);
const borderRefSchema = z.enum(['border', 'sidebar-border', 'highlight', 'ring', 'primary', 'transparent']);
const stateRecipeSchema = z.object({
  background: backgroundRefSchema.optional(), foreground: foregroundRefSchema.optional(), border: borderRefSchema.optional(),
  elevation: z.enum(['none', 'surface', 'overlay', 'floating', 'pressed']).optional(), opacity: z.number().optional(), press: z.boolean().optional(),
}).strict();
const surfaceRecipeSchema = z.object({
  background: backgroundRefSchema, foreground: foregroundRefSchema, border: borderRefSchema,
  borderWidth: z.enum(['none', 'hairline', 'default', 'strong']), borderStyle: z.enum(['solid', 'double', 'dashed']),
  radius: z.enum(['none', 'control', 'surface', 'overlay', 'avatar', 'pill']), elevation: z.enum(['none', 'surface', 'overlay', 'floating']),
  material: z.enum(['flat', 'subtle', 'elevated']), motion: z.enum(['none', 'color', 'surface', 'press']),
  states: z.object({ hover: stateRecipeSchema.optional(), active: stateRecipeSchema.optional(), selected: stateRecipeSchema.optional(), focus: stateRecipeSchema.optional(), disabled: stateRecipeSchema.optional() }).strict().optional(),
}).strict();
const componentRecipesSchema = z.object({
  shell: surfaceRecipeSchema, titlebar: surfaceRecipeSchema, sidebar: surfaceRecipeSchema, 'navigation-item': surfaceRecipeSchema,
  panel: surfaceRecipeSchema, card: surfaceRecipeSchema, composer: surfaceRecipeSchema, 'message-user': surfaceRecipeSchema,
  'message-assistant': surfaceRecipeSchema, 'message-disclosure': surfaceRecipeSchema, 'runtime-control': surfaceRecipeSchema,
  activity: surfaceRecipeSchema, 'tool-card': surfaceRecipeSchema,
  'permission-card': surfaceRecipeSchema, dialog: surfaceRecipeSchema, sheet: surfaceRecipeSchema, popover: surfaceRecipeSchema,
  input: surfaceRecipeSchema, 'button-primary': surfaceRecipeSchema, 'button-secondary': surfaceRecipeSchema,
  'button-ghost': surfaceRecipeSchema, editor: surfaceRecipeSchema, diff: surfaceRecipeSchema, 'workspace-tab': surfaceRecipeSchema,
  'workflow-node': surfaceRecipeSchema, 'workflow-edge': surfaceRecipeSchema, scrollbar: surfaceRecipeSchema,
}).strict();
const assetRecordSchema = z.object({
  id: z.string(), kind: z.enum(['font', 'avatar', 'icon', 'texture', 'wallpaper', 'preview']), mediaType: z.string(), bytes: z.number(),
  sha256: z.string(), outputUrl: z.string(), width: z.number().optional(), height: z.number().optional(), required: z.boolean(), licenseId: z.string(),
  fontMetadata: z.object({ family: z.string(), subfamily: z.string(), postscriptName: z.string(), weightMin: z.number(), weightMax: z.number() }).strict().optional(),
}).strict();
const assetSummarySchema = z.object({ schemaVersion: z.literal(2), count: z.number(), totalBytes: z.number(), records: z.array(assetRecordSchema) }).strict();
const iconDescriptorSchema = z.object({ assetId: z.string(), renderMode: z.enum(['mask', 'image']), nativeSize: z.union([z.literal(16), z.literal(20), z.literal(24), z.literal(32)]), imageRendering: z.enum(['auto', 'pixelated']) }).strict();
const iconSlotMapSchema = z.partialRecord(themeIconSlotSchema, iconDescriptorSchema);
const iconMapSchema = z.object({ defaults: iconSlotMapSchema, schemes: z.object({ light: iconSlotMapSchema.optional(), dark: iconSlotMapSchema.optional() }).strict().optional() }).strict();
const wallpaperDescriptorSchema = z.object({
  assetId: z.string(), fit: z.enum(['cover', 'contain', 'tile']), position: z.enum(['center', 'top', 'bottom', 'left', 'right', 'top-left', 'top-right', 'bottom-left', 'bottom-right']),
  repeat: z.enum(['no-repeat', 'repeat', 'repeat-x', 'repeat-y']), opacity: z.number().min(0).max(1), overlayColor: backgroundRefSchema, overlayOpacity: z.number().min(0).max(1),
}).strict();
const wallpaperSlotMapSchema = z.partialRecord(themeWallpaperSlotSchema, wallpaperDescriptorSchema);
const wallpaperMapSchema = z.object({ light: wallpaperSlotMapSchema, dark: wallpaperSlotMapSchema }).strict();
const themeSchemeSchema = z.object({
  windowSurface: z.string(), preview: previewPaletteSchema, semantic: semanticThemeTokensSchema, material: materialTokensSchema,
  shape: shapeTokensSchema, elevation: elevationTokensSchema, motion: motionTokensSchema, scrollbar: scrollbarTokensSchema,
  typography: typographyPresetSchema, avatars: avatarPresetSchema,
}).strict();

export const themePackageSchema = z.object({
  schemaVersion: z.literal(2), contractVersion: z.literal(2), id: z.string().regex(/^(builtin|user)\.[a-z0-9][a-z0-9.-]*$/u),
  version: z.string().regex(/^\d+\.\d+\.\d+$/u), source: z.enum(['builtin', 'user']), name: localizedTextSchema, author: z.string(),
  capabilities: z.array(themeCapabilitySchema).min(2), schemes: z.object({ light: themeSchemeSchema, dark: themeSchemeSchema }).strict(),
  recipes: componentRecipesSchema, assets: assetSummarySchema, fonts: fontsRuntimeSchema.optional(), icons: iconMapSchema.optional(), wallpapers: wallpaperMapSchema.optional(),
  visualQualityProfiles: z.object({
    default: visualQualitySchema, supported: z.tuple([z.literal('full'), z.literal('performance')]),
    performance: z.object({ blur: z.number(), saturate: z.number(), textureOpacity: z.number(), wallpapers: z.object({ enabled: z.literal(false) }).strict().optional() }).strict(),
  }).strict().optional(),
}).strict().superRefine((theme, context) => {
  for (const capability of ['fonts', 'icons', 'wallpapers'] as const) {
    if (theme.capabilities.includes(capability) !== Boolean(theme[capability])) context.addIssue({ code: 'custom', message: `${capability} capability mismatch` });
  }
  if (theme.capabilities.includes('visual-quality-profiles') !== Boolean(theme.visualQualityProfiles)) context.addIssue({ code: 'custom', message: 'visual quality capability mismatch' });
});

export type ThemePackage = z.infer<typeof themePackageSchema>;
export type ThemeScheme = z.infer<typeof themeSchemeSchema>;
export type SemanticThemeTokens = z.infer<typeof semanticThemeTokensSchema>;
export type MaterialTokens = z.infer<typeof materialTokensSchema>;
export type ThemeIconDescriptor = z.infer<typeof iconDescriptorSchema>;
export type ThemeWallpaperDescriptor = z.infer<typeof wallpaperDescriptorSchema>;
