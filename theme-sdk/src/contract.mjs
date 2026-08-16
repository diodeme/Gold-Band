export const THEME_CAPABILITIES = [
  'tokens',
  'component-recipes',
  'fonts',
  'avatars',
  'textures',
  'icons',
  'wallpapers',
  'visual-quality-profiles',
];

export const THEME_ASSET_KINDS = ['font', 'avatar', 'icon', 'texture', 'wallpaper', 'preview'];
export const THEME_ICON_SLOTS = [
  'navigation.conversation', 'navigation.search', 'navigation.agent', 'navigation.context',
  'navigation.run-mode', 'navigation.settings', 'entity.task', 'entity.workflow',
  'entity.agent', 'entity.file', 'entity.folder', 'conversation.thought',
  'conversation.attachment', 'tool.read', 'tool.write', 'tool.command',
  'permission.request', 'status.running', 'status.success', 'status.warning',
  'status.error', 'action.send', 'action.continue', 'action.stop',
];
export const THEME_WALLPAPER_SLOTS = ['app', 'conversation', 'workspace', 'settings'];
export const THEME_VISUAL_STATES = ['hover', 'active', 'selected', 'focus', 'disabled'];
export const RECIPE_ROLE_NAMES = [
  'shell', 'titlebar', 'sidebar', 'navigation-item', 'panel', 'card', 'composer',
  'message-user', 'message-assistant', 'message-disclosure', 'runtime-control',
  'activity', 'tool-card', 'permission-card',
  'dialog', 'sheet', 'popover', 'input', 'button-primary', 'button-secondary',
  'button-ghost', 'editor', 'diff', 'workspace-tab', 'workflow-node', 'workflow-edge',
  'scrollbar',
];

export const MAX_FONT_STACK_FAMILIES = 16;
export const MAX_FONT_FAMILY_CHARS = 128;
export const MAX_THEME_ASSETS = 128;
export const MAX_THEME_ASSET_BYTES = 16 * 1024 * 1024;

export const SEMANTIC_TOKEN_NAMES = [
  'background', 'foreground', 'title', 'card', 'cardForeground', 'popover',
  'popoverForeground', 'primary', 'primaryForeground', 'secondary',
  'secondaryForeground', 'muted', 'mutedForeground', 'accent', 'accentForeground',
  'destructive', 'border', 'input', 'ring', 'selection', 'selectionForeground',
  'messageUser', 'messageUserForeground', 'contentHeader', 'contentHeaderForeground',
  'conversationBackground', 'conversationForeground', 'messageAssistant',
  'messageAssistantForeground', 'composer', 'composerForeground', 'activity',
  'activityForeground', 'toolCard', 'toolCardForeground', 'permissionCard',
  'permissionCardForeground', 'workspaceTab', 'workspaceTabForeground',
  'resourceHeader', 'resourceHeaderForeground', 'fileTree', 'fileTreeForeground',
  'editor', 'editorForeground', 'diffAdded', 'diffAddedForeground', 'diffRemoved',
  'diffRemovedForeground', 'diffModified', 'diffModifiedForeground', 'sidebar',
  'sidebarForeground', 'sidebarPrimary', 'sidebarPrimaryForeground', 'sidebarAccent',
  'sidebarAccentForeground', 'sidebarBorder', 'sidebarRing', 'workspace', 'surfaceLow',
  'surfaceHigh', 'lineSoft', 'windowOutline', 'windowEdgeShadow', 'link', 'running',
  'success', 'warning', 'danger', 'permission', 'titlebar', 'titlebarForeground',
  'titlebarMuted', 'titlebarBorder', 'titlebarHover', 'scrollbarTrack', 'scrollbarThumb',
  'scrollbarThumbHover',
];

const stringProperties = (names) => Object.fromEntries(names.map((name) => [name, { type: 'string', minLength: 1 }]));
const optionalSlotMap = (slots, valueSchema) => ({
  type: 'object', additionalProperties: false,
  properties: Object.fromEntries(slots.map((slot) => [slot, valueSchema])),
});
const localizedTextSchema = {
  type: 'object', additionalProperties: false, required: ['zh-CN', 'en'],
  properties: { 'zh-CN': { type: 'string', minLength: 1 }, en: { type: 'string', minLength: 1 } },
};

export const themeManifestSchema = {
  $schema: 'http://json-schema.org/draft-07/schema#',
  $id: 'https://gold-band.dev/schemas/theme-manifest-v2.schema.json',
  title: 'Gold Band Theme Manifest v2',
  type: 'object', additionalProperties: false,
  required: ['schemaVersion', 'contractVersion', 'id', 'version', 'source', 'name', 'author', 'schemes', 'capabilities'],
  properties: {
    $schema: { type: 'string' }, schemaVersion: { const: 2 }, contractVersion: { const: 2 },
    id: { type: 'string', pattern: '^(builtin|user)\\.[a-z0-9][a-z0-9.-]*$' },
    version: { type: 'string', pattern: '^\\d+\\.\\d+\\.\\d+$' },
    source: { enum: ['builtin', 'user'] }, name: localizedTextSchema,
    author: { type: 'string', minLength: 1 },
    schemes: { type: 'array', uniqueItems: true, minItems: 2, maxItems: 2, items: { enum: ['light', 'dark'] }, allOf: [{ contains: { const: 'light' } }, { contains: { const: 'dark' } }] },
    capabilities: { type: 'array', uniqueItems: true, minItems: 2, items: { enum: THEME_CAPABILITIES } },
    visualQualityProfiles: {
      type: 'object', additionalProperties: false, required: ['default', 'supported', 'performance'],
      properties: {
        default: { enum: ['full', 'performance'] }, supported: { const: ['full', 'performance'] },
        performance: { type: 'string', pattern: '^visual-quality/[a-z0-9-]+\\.json$' },
      },
    },
  },
};

export const themeAssetSourceSchema = {
  type: 'object', additionalProperties: false, required: ['id', 'kind', 'path', 'licenseId'],
  properties: {
    id: { type: 'string', pattern: '^[a-z0-9]+(?:-[a-z0-9]+)*$' },
    kind: { enum: THEME_ASSET_KINDS }, path: { type: 'string', pattern: '^assets/[a-zA-Z0-9._/-]+$' },
    licenseId: { type: 'string', pattern: '^[a-z0-9]+(?:-[a-z0-9]+)*$' }, required: { type: 'boolean', default: true },
  },
};

const previewSchema = {
  type: 'object', additionalProperties: false,
  required: ['background', 'surface', 'border', 'primary', 'foreground', 'muted', 'success', 'danger'],
  properties: stringProperties(['background', 'surface', 'border', 'primary', 'foreground', 'muted', 'success', 'danger']),
};
const semanticSchema = { type: 'object', additionalProperties: false, required: SEMANTIC_TOKEN_NAMES, properties: stringProperties(SEMANTIC_TOKEN_NAMES) };
const dimension = { type: 'string', pattern: '^(0|(?:[0-9]+(?:\\.[0-9]+)?)(?:px|rem))$' };
const duration = { type: 'string', pattern: '^(?:0|[0-9]+(?:\\.[0-9]+)?(?:ms|s))$' };
const easing = { type: 'string', pattern: '^(?:linear|cubic-bezier\\([^)]{1,80}\\)|steps\\([1-9][0-9]*(?:, ?(?:start|end))?\\))$' };
const shadow = { type: 'string', minLength: 1, maxLength: 512 };

const materialSchema = {
  type: 'object', additionalProperties: false,
  required: ['model', 'surfaceOpacity', 'borderHighlight', 'surfaceOverlay', 'blur', 'saturate', 'backdropBrightness', 'backdropContrast', 'specularHighlight', 'edgeShadow', 'backgroundImage', 'textureOpacity'],
  properties: {
    model: { enum: ['solid', 'frosted', 'liquid'] }, surfaceOpacity: { type: 'number', minimum: 0, maximum: 1 },
    borderHighlight: { type: 'string', minLength: 1 }, surfaceOverlay: { type: 'string', minLength: 1 },
    blur: { type: 'number', minimum: 0, maximum: 60 }, saturate: { type: 'number', minimum: 100, maximum: 200 },
    backdropBrightness: { type: 'number', minimum: 80, maximum: 120 }, backdropContrast: { type: 'number', minimum: 80, maximum: 140 },
    specularHighlight: { type: 'string', minLength: 1, maxLength: 512 }, edgeShadow: shadow,
    backgroundImage: { type: 'string', minLength: 1, maxLength: 512 }, textureOpacity: { type: 'number', minimum: 0, maximum: 0.04 },
  },
};
const shapeSchema = {
  type: 'object', additionalProperties: false,
  required: ['radiusControl', 'radiusSurface', 'radiusOverlay', 'radiusAvatar', 'radiusPill', 'borderHairline', 'borderDefault', 'borderStrong'],
  properties: Object.fromEntries(['radiusControl', 'radiusSurface', 'radiusOverlay', 'radiusAvatar', 'radiusPill', 'borderHairline', 'borderDefault', 'borderStrong'].map((name) => [name, dimension])),
};
const elevationSchema = {
  type: 'object', additionalProperties: false, required: ['none', 'surface', 'overlay', 'floating', 'pressed', 'pressOffset'],
  properties: { none: shadow, surface: shadow, overlay: shadow, floating: shadow, pressed: shadow, pressOffset: { enum: [0, 1, 2] } },
};
const motionSchema = {
  type: 'object', additionalProperties: false,
  required: ['mode', 'durationFast', 'durationNormal', 'durationSlow', 'easingStandard', 'easingEnter', 'easingPress'],
  properties: { mode: { enum: ['smooth', 'stepped', 'none'] }, durationFast: duration, durationNormal: duration, durationSlow: duration, easingStandard: easing, easingEnter: easing, easingPress: easing },
};
const scrollbarSchema = {
  type: 'object', additionalProperties: false, required: ['width', 'thumbRadius', 'thumbInset', 'minLength', 'buttons'],
  properties: { width: dimension, thumbRadius: dimension, thumbInset: dimension, minLength: dimension, buttons: { enum: ['none', 'visible'] } },
};
const typographyPresetSchema = {
  type: 'object', additionalProperties: false,
  required: ['uiStackId', 'uiSize', 'uiLineHeight', 'editorStackId', 'editorSize', 'editorLineHeight', 'weights'],
  properties: {
    uiStackId: { type: 'string', minLength: 1 }, uiSize: { type: 'number', minimum: 12, maximum: 18 }, uiLineHeight: { type: 'number', minimum: 1, maximum: 2 },
    editorStackId: { type: 'string', minLength: 1 }, editorSize: { type: 'number', minimum: 10, maximum: 18 }, editorLineHeight: { type: 'number', minimum: 1, maximum: 2 },
    weights: { type: 'object', additionalProperties: false, required: ['read', 'emphasize', 'announce'], properties: { read: { enum: [400, 500] }, emphasize: { enum: [400, 500, 600] }, announce: { enum: [500, 600, 700] } } },
  },
};
const avatarSchema = {
  type: 'object', additionalProperties: false, required: ['agentShape', 'userShape', 'agentAsset', 'userAsset'],
  properties: { agentShape: { enum: ['circle', 'square'] }, userShape: { enum: ['circle', 'square'] }, agentAsset: { type: ['string', 'null'] }, userAsset: { type: ['string', 'null'] } },
};
const stateRecipeSchema = {
  type: 'object', additionalProperties: false, minProperties: 1,
  properties: {
    background: { enum: ['background', 'card', 'popover', 'sidebar', 'surface-low', 'surface-high', 'accent', 'primary', 'message-user', 'message-assistant', 'composer', 'activity', 'tool-card', 'permission-card', 'workspace-tab', 'editor', 'transparent'] },
    foreground: { enum: ['foreground', 'muted-foreground', 'card-foreground', 'accent-foreground', 'primary-foreground', 'message-user-foreground', 'message-assistant-foreground', 'composer-foreground', 'activity-foreground', 'tool-card-foreground', 'permission-card-foreground', 'workspace-tab-foreground', 'editor-foreground'] },
    border: { enum: ['border', 'sidebar-border', 'highlight', 'ring', 'primary', 'transparent'] },
    elevation: { enum: ['none', 'surface', 'overlay', 'floating', 'pressed'] }, opacity: { type: 'number', minimum: 0.25, maximum: 1 }, press: { type: 'boolean' },
  },
};
const recipeSchema = {
  type: 'object', additionalProperties: false,
  required: ['background', 'foreground', 'border', 'borderWidth', 'borderStyle', 'radius', 'elevation', 'material', 'motion'],
  properties: {
    background: stateRecipeSchema.properties.background, foreground: stateRecipeSchema.properties.foreground, border: stateRecipeSchema.properties.border,
    borderWidth: { enum: ['none', 'hairline', 'default', 'strong'] }, borderStyle: { enum: ['solid', 'double', 'dashed'] },
    radius: { enum: ['none', 'control', 'surface', 'overlay', 'avatar', 'pill'] }, elevation: { enum: ['none', 'surface', 'overlay', 'floating'] },
    material: { enum: ['flat', 'subtle', 'elevated'] }, motion: { enum: ['none', 'color', 'surface', 'press'] },
    states: { type: 'object', additionalProperties: false, properties: Object.fromEntries(THEME_VISUAL_STATES.map((state) => [state, stateRecipeSchema])) },
  },
};
const recipesSchema = { type: 'object', additionalProperties: false, required: RECIPE_ROLE_NAMES, properties: Object.fromEntries(RECIPE_ROLE_NAMES.map((role) => [role, recipeSchema])) };
const fontFaceSchema = {
  type: 'object', additionalProperties: false, required: ['id', 'family', 'runtimeFamily', 'assetId', 'weightMin', 'weightMax', 'style', 'display', 'coverage'],
  properties: {
    id: { type: 'string', pattern: '^[a-z0-9]+(?:-[a-z0-9]+)*$' }, family: { type: 'string', minLength: 1, maxLength: MAX_FONT_FAMILY_CHARS }, runtimeFamily: { type: 'string', minLength: 1 }, assetId: { type: 'string', minLength: 1 },
    weightMin: { type: 'integer', minimum: 1, maximum: 1000 }, weightMax: { type: 'integer', minimum: 1, maximum: 1000 }, style: { enum: ['normal', 'italic'] }, display: { const: 'swap' },
    coverage: { type: 'object', additionalProperties: false, required: ['scripts'], properties: { scripts: { type: 'array', minItems: 1, uniqueItems: true, items: { type: 'string', pattern: '^[A-Z][a-z]{3}$' } }, locales: { type: 'array', uniqueItems: true, items: { type: 'string', minLength: 2 } }, unicodeRanges: { type: 'array', uniqueItems: true, items: { type: 'string', pattern: '^U\\+[0-9A-F?]{1,6}(?:-[0-9A-F]{1,6})?$' } } } },
    metrics: { type: 'object', additionalProperties: false, properties: stringProperties(['sizeAdjust', 'ascentOverride', 'descentOverride', 'lineGapOverride']) },
  },
};
const fontStackSchema = {
  type: 'object', additionalProperties: false, required: ['id', 'displayName', 'defaultFaces', 'systemFallbacks'],
  properties: {
    id: { type: 'string', minLength: 1 }, displayName: localizedTextSchema,
    defaultFaces: { type: 'array', maxItems: MAX_FONT_STACK_FAMILIES, uniqueItems: true, items: { type: 'string', minLength: 1 } },
    byScript: { type: 'object', patternProperties: { '^[A-Z][a-z]{3}$': { type: 'array', uniqueItems: true, items: { type: 'string', minLength: 1 } } }, additionalProperties: false },
    byLocale: { type: 'object', patternProperties: { '^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*$': { type: 'array', uniqueItems: true, items: { type: 'string', minLength: 1 } } }, additionalProperties: false },
    systemFallbacks: { type: 'array', minItems: 1, maxItems: MAX_FONT_STACK_FAMILIES, uniqueItems: true, items: { type: 'string', minLength: 1, maxLength: MAX_FONT_FAMILY_CHARS } },
  },
};
const fontsRuntimeSchema = { type: 'object', additionalProperties: false, required: ['faces', 'stacks'], properties: { faces: { type: 'array', items: fontFaceSchema }, stacks: { type: 'array', minItems: 2, items: fontStackSchema } } };
const iconDescriptorSchema = { type: 'object', additionalProperties: false, required: ['assetId', 'renderMode', 'nativeSize', 'imageRendering'], properties: { assetId: { type: 'string', minLength: 1 }, renderMode: { enum: ['mask', 'image'] }, nativeSize: { enum: [16, 20, 24, 32] }, imageRendering: { enum: ['auto', 'pixelated'] } } };
const iconMapSchema = { type: 'object', additionalProperties: false, required: ['defaults'], properties: { defaults: optionalSlotMap(THEME_ICON_SLOTS, iconDescriptorSchema), schemes: { type: 'object', additionalProperties: false, properties: { light: optionalSlotMap(THEME_ICON_SLOTS, iconDescriptorSchema), dark: optionalSlotMap(THEME_ICON_SLOTS, iconDescriptorSchema) } } } };
const wallpaperDescriptorSchema = { type: 'object', additionalProperties: false, required: ['assetId', 'fit', 'position', 'repeat', 'opacity', 'overlayColor', 'overlayOpacity'], properties: { assetId: { type: 'string', minLength: 1 }, fit: { enum: ['cover', 'contain', 'tile'] }, position: { enum: ['center', 'top', 'bottom', 'left', 'right', 'top-left', 'top-right', 'bottom-left', 'bottom-right'] }, repeat: { enum: ['no-repeat', 'repeat', 'repeat-x', 'repeat-y'] }, opacity: { type: 'number', minimum: 0, maximum: 1 }, overlayColor: stateRecipeSchema.properties.background, overlayOpacity: { type: 'number', minimum: 0, maximum: 1 } } };
const wallpaperMapSchema = { type: 'object', additionalProperties: false, required: ['light', 'dark'], properties: { light: optionalSlotMap(THEME_WALLPAPER_SLOTS, wallpaperDescriptorSchema), dark: optionalSlotMap(THEME_WALLPAPER_SLOTS, wallpaperDescriptorSchema) } };
const assetRecordSchema = {
  type: 'object', additionalProperties: false, required: ['id', 'kind', 'mediaType', 'bytes', 'sha256', 'outputUrl', 'required', 'licenseId'],
  properties: {
    id: { type: 'string' }, kind: { enum: THEME_ASSET_KINDS }, mediaType: { type: 'string' }, bytes: { type: 'integer', minimum: 1 }, sha256: { type: 'string', pattern: '^[a-f0-9]{64}$' }, outputUrl: { type: 'string', pattern: '^/theme-assets/' }, width: { type: 'integer', minimum: 1 }, height: { type: 'integer', minimum: 1 },
    fontMetadata: { type: 'object', additionalProperties: false, required: ['family', 'subfamily', 'postscriptName', 'weightMin', 'weightMax'], properties: { family: { type: 'string' }, subfamily: { type: 'string' }, postscriptName: { type: 'string' }, weightMin: { type: 'number' }, weightMax: { type: 'number' } } },
    required: { type: 'boolean' }, licenseId: { type: 'string' },
  },
};
const assetSummarySchema = { type: 'object', additionalProperties: false, required: ['schemaVersion', 'count', 'totalBytes', 'records'], properties: { schemaVersion: { const: 2 }, count: { type: 'integer', minimum: 0 }, totalBytes: { type: 'integer', minimum: 0 }, records: { type: 'array', items: assetRecordSchema } } };
const schemeSchema = {
  type: 'object', additionalProperties: false,
  required: ['windowSurface', 'preview', 'semantic', 'material', 'shape', 'elevation', 'motion', 'scrollbar', 'typography', 'avatars'],
  properties: { windowSurface: { type: 'string', minLength: 1 }, preview: previewSchema, semantic: semanticSchema, material: materialSchema, shape: shapeSchema, elevation: elevationSchema, motion: motionSchema, scrollbar: scrollbarSchema, typography: typographyPresetSchema, avatars: avatarSchema },
};
const performanceSchema = {
  type: 'object', additionalProperties: false, required: ['blur', 'saturate', 'textureOpacity'],
  properties: { blur: { type: 'number', minimum: 0, maximum: 24 }, saturate: { type: 'number', minimum: 100, maximum: 160 }, textureOpacity: { type: 'number', minimum: 0, maximum: 0.02 }, wallpapers: { type: 'object', additionalProperties: false, required: ['enabled'], properties: { enabled: { const: false } } } },
};

export const runtimeThemeSchema = {
  $schema: 'http://json-schema.org/draft-07/schema#',
  $id: 'https://gold-band.dev/schemas/theme-package-v2.schema.json',
  title: 'Gold Band Runtime Theme Package v2', type: 'object', additionalProperties: false,
  required: ['schemaVersion', 'contractVersion', 'id', 'version', 'source', 'name', 'author', 'capabilities', 'schemes', 'recipes', 'assets'],
  properties: {
    schemaVersion: { const: 2 }, contractVersion: { const: 2 }, id: themeManifestSchema.properties.id, version: themeManifestSchema.properties.version,
    source: themeManifestSchema.properties.source, name: localizedTextSchema, author: { type: 'string', minLength: 1 }, capabilities: themeManifestSchema.properties.capabilities,
    schemes: { type: 'object', additionalProperties: false, required: ['light', 'dark'], properties: { light: schemeSchema, dark: schemeSchema } }, recipes: recipesSchema, assets: assetSummarySchema,
    fonts: fontsRuntimeSchema, icons: iconMapSchema, wallpapers: wallpaperMapSchema,
    visualQualityProfiles: { type: 'object', additionalProperties: false, required: ['default', 'supported', 'performance'], properties: { default: { enum: ['full', 'performance'] }, supported: { const: ['full', 'performance'] }, performance: performanceSchema } },
  },
};
