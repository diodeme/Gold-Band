export const THEME_CAPABILITIES = [
  'tokens',
  'component-recipes',
  'fonts',
  'avatars',
  'textures',
  'visual-quality-profiles',
];

export const MAX_FONT_STACK_FAMILIES = 16;
export const MAX_FONT_FAMILY_CHARS = 128;

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

export const RECIPE_ROLE_NAMES = [
  'shell', 'titlebar', 'sidebar', 'panel', 'composer', 'card', 'dialog', 'sheet',
  'popover', 'input', 'button', 'editor',
];

const stringProperties = (names) => Object.fromEntries(names.map((name) => [name, { type: 'string', minLength: 1 }]));

const localizedTextSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['zh-CN', 'en'],
  properties: { 'zh-CN': { type: 'string', minLength: 1 }, en: { type: 'string', minLength: 1 } },
};

export const themeManifestSchema = {
  $schema: 'http://json-schema.org/draft-07/schema#',
  $id: 'https://gold-band.dev/schemas/theme-manifest-v2.schema.json',
  title: 'Gold Band Theme Manifest v2',
  type: 'object',
  additionalProperties: false,
  required: ['schemaVersion', 'contractVersion', 'id', 'version', 'source', 'name', 'author', 'schemes', 'capabilities'],
  properties: {
    $schema: { type: 'string' },
    schemaVersion: { const: 2 },
    contractVersion: { const: 2 },
    id: { type: 'string', pattern: '^(builtin|user)\\.[a-z0-9][a-z0-9.-]*$' },
    version: { type: 'string', pattern: '^\\d+\\.\\d+\\.\\d+$' },
    source: { enum: ['builtin', 'user'] },
    name: localizedTextSchema,
    author: { type: 'string', minLength: 1 },
    schemes: { type: 'array', uniqueItems: true, minItems: 2, maxItems: 2, items: { enum: ['light', 'dark'] }, allOf: [{ contains: { const: 'light' } }, { contains: { const: 'dark' } }] },
    capabilities: { type: 'array', uniqueItems: true, minItems: 2, items: { enum: THEME_CAPABILITIES } },
    visualQualityProfiles: {
      type: 'object',
      additionalProperties: false,
      required: ['default', 'supported', 'performance'],
      properties: {
        default: { enum: ['full', 'performance'] },
        supported: { const: ['full', 'performance'] },
        performance: { type: 'string', pattern: '^visual-quality/[a-z0-9-]+\\.json$' },
      },
    },
  },
};

const previewSchema = {
  type: 'object', additionalProperties: false,
  required: ['background', 'surface', 'border', 'primary', 'foreground', 'muted', 'success', 'danger'],
  properties: stringProperties(['background', 'surface', 'border', 'primary', 'foreground', 'muted', 'success', 'danger']),
};

const semanticSchema = {
  type: 'object', additionalProperties: false, required: SEMANTIC_TOKEN_NAMES,
  properties: stringProperties(SEMANTIC_TOKEN_NAMES),
};

const materialSchema = {
  type: 'object', additionalProperties: false,
  required: ['surfaceOpacity', 'borderHighlight', 'surfaceOverlay', 'blur', 'saturate', 'shadow', 'radius', 'backgroundImage', 'textureOpacity', 'motionDuration', 'motionEasing'],
  properties: {
    model: { enum: ['solid', 'frosted', 'liquid'] },
    surfaceOpacity: { type: 'number', minimum: 0, maximum: 1 },
    borderHighlight: { type: 'string' },
    surfaceOverlay: { type: 'string', minLength: 1 },
    blur: { type: 'number', minimum: 0, maximum: 60 },
    saturate: { type: 'number', minimum: 100, maximum: 200 },
    backdropBrightness: { type: 'number', minimum: 80, maximum: 120 },
    backdropContrast: { type: 'number', minimum: 80, maximum: 140 },
    specularHighlight: { type: 'string', minLength: 1 },
    edgeShadow: { type: 'string', minLength: 1 },
    shadow: { type: 'string' },
    radius: { type: 'string', minLength: 1 },
    backgroundImage: { type: 'string', minLength: 1 },
    textureOpacity: { type: 'number', minimum: 0, maximum: 0.04 },
    motionDuration: { type: 'string', pattern: '^\\d+(ms|s)$' },
    motionEasing: { type: 'string', minLength: 1 },
  },
};

const fontStackSchema = (minimum, maximum, fallback) => ({
  type: 'object', additionalProperties: false,
  required: ['families', 'fallback', 'size'],
  properties: {
    families: {
      type: 'array', minItems: 1, maxItems: MAX_FONT_STACK_FAMILIES, uniqueItems: true,
      items: { type: 'string', minLength: 1, maxLength: MAX_FONT_FAMILY_CHARS, pattern: '^[^,;{}]+$' },
    },
    fallback: { const: fallback },
    size: { type: 'number', minimum, maximum },
  },
});

const typographySchema = {
  type: 'object', additionalProperties: false,
  required: ['ui', 'editor'],
  properties: {
    ui: fontStackSchema(12, 18, 'sans-serif'),
    editor: fontStackSchema(10, 18, 'monospace'),
  },
};

const avatarSchema = {
  type: 'object', additionalProperties: false,
  required: ['agentShape', 'userShape', 'agentAsset', 'userAsset'],
  properties: {
    agentShape: { enum: ['circle', 'square'] },
    userShape: { enum: ['circle', 'square'] },
    agentAsset: { type: ['string', 'null'] },
    userAsset: { type: ['string', 'null'] },
  },
};

const recipeSchema = {
  type: 'object', additionalProperties: false,
  required: ['background', 'foreground', 'border', 'material'],
  properties: {
    background: { enum: ['card', 'popover', 'sidebar', 'surface-low', 'surface-high', 'transparent'] },
    foreground: { enum: ['foreground', 'muted-foreground', 'card-foreground'] },
    border: { enum: ['border', 'sidebar-border', 'highlight'] },
    material: { enum: ['flat', 'subtle', 'elevated'] },
  },
};

const recipesSchema = {
  type: 'object', additionalProperties: false, required: RECIPE_ROLE_NAMES,
  properties: Object.fromEntries(RECIPE_ROLE_NAMES.map((role) => [role, recipeSchema])),
};

const performanceSchema = {
  type: 'object', additionalProperties: false,
  required: ['blur', 'saturate', 'shadow', 'textureOpacity', 'motionDuration'],
  properties: {
    blur: { type: 'number', minimum: 0, maximum: 24 },
    saturate: { type: 'number', minimum: 100, maximum: 160 },
    backdropBrightness: { type: 'number', minimum: 80, maximum: 120 },
    backdropContrast: { type: 'number', minimum: 80, maximum: 140 },
    specularHighlight: { type: 'string', minLength: 1 },
    edgeShadow: { type: 'string', minLength: 1 },
    shadow: { type: 'string' },
    textureOpacity: { type: 'number', minimum: 0, maximum: 0.02 },
    motionDuration: { type: 'string', pattern: '^\\d+(ms|s)$' },
  },
};

const schemeSchema = {
  type: 'object', additionalProperties: false,
  required: ['windowSurface', 'preview', 'semantic', 'material', 'typography', 'avatars'],
  properties: {
    windowSurface: { type: 'string', minLength: 1 },
    preview: previewSchema,
    semantic: semanticSchema,
    material: materialSchema,
    typography: typographySchema,
    avatars: avatarSchema,
  },
};

export const runtimeThemeSchema = {
  $schema: 'http://json-schema.org/draft-07/schema#',
  $id: 'https://gold-band.dev/schemas/theme-package-v2.schema.json',
  title: 'Gold Band Runtime Theme Package v2',
  type: 'object', additionalProperties: false,
  required: ['schemaVersion', 'contractVersion', 'id', 'version', 'source', 'name', 'author', 'capabilities', 'schemes', 'recipes'],
  properties: {
    schemaVersion: { const: 2 }, contractVersion: { const: 2 },
    id: themeManifestSchema.properties.id, version: themeManifestSchema.properties.version,
    source: themeManifestSchema.properties.source, name: localizedTextSchema,
    author: { type: 'string', minLength: 1 },
    capabilities: themeManifestSchema.properties.capabilities,
    schemes: { type: 'object', additionalProperties: false, required: ['light', 'dark'], properties: { light: schemeSchema, dark: schemeSchema } },
    recipes: recipesSchema,
    visualQualityProfiles: {
      type: 'object', additionalProperties: false,
      required: ['default', 'supported', 'performance'],
      properties: { default: { enum: ['full', 'performance'] }, supported: { const: ['full', 'performance'] }, performance: performanceSchema },
    },
  },
};
