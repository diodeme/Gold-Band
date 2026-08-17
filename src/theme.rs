use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const BUILTIN_THEME_CATALOG_JSON: &str =
    include_str!("../resources/themes/builtin-theme-catalog.json");

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {detail}")]
pub struct ThemeContractError {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeSource {
    Builtin,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeCapability {
    Tokens,
    ComponentRecipes,
    Fonts,
    Avatars,
    Textures,
    Icons,
    Wallpapers,
    VisualQualityProfiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeVisualQuality {
    Full,
    Performance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedThemeName {
    #[serde(rename = "zh-CN")]
    pub zh_cn: String,
    pub en: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemePreviewPalette {
    pub background: String,
    pub surface: String,
    pub border: String,
    pub primary: String,
    pub foreground: String,
    pub muted: String,
    pub success: String,
    pub danger: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SemanticThemeTokens {
    pub background: String,
    pub foreground: String,
    pub title: String,
    pub card: String,
    pub card_foreground: String,
    pub popover: String,
    pub popover_foreground: String,
    pub primary: String,
    pub primary_foreground: String,
    pub secondary: String,
    pub secondary_foreground: String,
    pub muted: String,
    pub muted_foreground: String,
    pub accent: String,
    pub accent_foreground: String,
    pub destructive: String,
    pub border: String,
    pub input: String,
    pub ring: String,
    pub selection: String,
    pub selection_foreground: String,
    pub message_user: String,
    pub message_user_foreground: String,
    pub content_header: String,
    pub content_header_foreground: String,
    pub conversation_background: String,
    pub conversation_foreground: String,
    pub message_assistant: String,
    pub message_assistant_foreground: String,
    pub composer: String,
    pub composer_foreground: String,
    pub activity: String,
    pub activity_foreground: String,
    pub tool_card: String,
    pub tool_card_foreground: String,
    pub permission_card: String,
    pub permission_card_foreground: String,
    pub workspace_tab: String,
    pub workspace_tab_foreground: String,
    pub resource_header: String,
    pub resource_header_foreground: String,
    pub file_tree: String,
    pub file_tree_foreground: String,
    pub editor: String,
    pub editor_foreground: String,
    pub diff_added: String,
    pub diff_added_foreground: String,
    pub diff_removed: String,
    pub diff_removed_foreground: String,
    pub diff_modified: String,
    pub diff_modified_foreground: String,
    pub sidebar: String,
    pub sidebar_foreground: String,
    pub sidebar_primary: String,
    pub sidebar_primary_foreground: String,
    pub sidebar_accent: String,
    pub sidebar_accent_foreground: String,
    pub sidebar_border: String,
    pub sidebar_ring: String,
    pub workspace: String,
    pub surface_low: String,
    pub surface_high: String,
    pub line_soft: String,
    pub window_outline: String,
    pub window_edge_shadow: String,
    pub link: String,
    pub running: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub permission: String,
    pub titlebar: String,
    pub titlebar_foreground: String,
    pub titlebar_muted: String,
    pub titlebar_border: String,
    pub titlebar_hover: String,
    pub scrollbar_track: String,
    pub scrollbar_thumb: String,
    pub scrollbar_thumb_hover: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialTokens {
    #[serde(default)]
    pub model: ThemeMaterialModel,
    pub surface_opacity: f64,
    pub border_highlight: String,
    pub surface_overlay: String,
    pub blur: f64,
    pub saturate: f64,
    #[serde(default = "default_material_percentage")]
    pub backdrop_brightness: f64,
    #[serde(default = "default_material_percentage")]
    pub backdrop_contrast: f64,
    #[serde(default = "default_specular_highlight")]
    pub specular_highlight: String,
    #[serde(default = "default_edge_shadow")]
    pub edge_shadow: String,
    pub background_image: String,
    pub texture_opacity: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMaterialModel {
    #[default]
    Solid,
    Frosted,
    Liquid,
}

fn default_material_percentage() -> f64 {
    100.0
}

fn default_specular_highlight() -> String {
    "none".to_string()
}

fn default_edge_shadow() -> String {
    "0 0 0 transparent".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTypographyWeights {
    pub read: u16,
    pub emphasize: u16,
    pub announce: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTypographyPreset {
    pub ui_stack_id: String,
    pub ui_size: f64,
    pub ui_line_height: f64,
    pub editor_stack_id: String,
    pub editor_size: f64,
    pub editor_line_height: f64,
    pub weights: ThemeTypographyWeights,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeShapeTokens {
    pub radius_control: String,
    pub radius_surface: String,
    pub radius_overlay: String,
    pub radius_avatar: String,
    pub radius_pill: String,
    pub border_hairline: String,
    pub border_default: String,
    pub border_strong: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeElevationTokens {
    pub none: String,
    pub surface: String,
    pub overlay: String,
    pub floating: String,
    pub pressed: String,
    pub press_offset: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMotionMode {
    Smooth,
    Stepped,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeMotionTokens {
    pub mode: ThemeMotionMode,
    pub duration_fast: String,
    pub duration_normal: String,
    pub duration_slow: String,
    pub easing_standard: String,
    pub easing_enter: String,
    pub easing_press: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeScrollbarButtons {
    None,
    Visible,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeScrollbarTokens {
    pub width: String,
    pub thumb_radius: String,
    pub thumb_inset: String,
    pub min_length: String,
    pub buttons: ThemeScrollbarButtons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeAvatarShape {
    Circle,
    Square,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeAvatarPreset {
    pub agent_shape: ThemeAvatarShape,
    pub user_shape: ThemeAvatarShape,
    pub agent_asset: Option<String>,
    pub user_asset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeScheme {
    pub window_surface: String,
    pub preview: ThemePreviewPalette,
    pub semantic: SemanticThemeTokens,
    pub material: MaterialTokens,
    pub shape: ThemeShapeTokens,
    pub elevation: ThemeElevationTokens,
    pub motion: ThemeMotionTokens,
    pub scrollbar: ThemeScrollbarTokens,
    pub typography: ThemeTypographyPreset,
    pub avatars: ThemeAvatarPreset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeSchemes {
    pub light: ThemeScheme,
    pub dark: ThemeScheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeBackground {
    Background,
    Card,
    Popover,
    Sidebar,
    SurfaceLow,
    SurfaceHigh,
    Accent,
    Primary,
    MessageUser,
    MessageAssistant,
    Composer,
    Activity,
    ToolCard,
    PermissionCard,
    WorkspaceTab,
    Editor,
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeForeground {
    Foreground,
    MutedForeground,
    CardForeground,
    AccentForeground,
    PrimaryForeground,
    MessageUserForeground,
    MessageAssistantForeground,
    ComposerForeground,
    ActivityForeground,
    ToolCardForeground,
    PermissionCardForeground,
    WorkspaceTabForeground,
    EditorForeground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeBorder {
    Border,
    SidebarBorder,
    Highlight,
    Ring,
    Primary,
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeMaterial {
    Flat,
    Subtle,
    Elevated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeBorderWidth {
    None,
    Hairline,
    Default,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeBorderStyle {
    Solid,
    Double,
    Dashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeRadius {
    None,
    Control,
    Surface,
    Overlay,
    Avatar,
    Pill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeElevation {
    None,
    Surface,
    Overlay,
    Floating,
    Pressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeMotion {
    None,
    Color,
    Surface,
    Press,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeStateRecipe {
    pub background: Option<RecipeBackground>,
    pub foreground: Option<RecipeForeground>,
    pub border: Option<RecipeBorder>,
    pub elevation: Option<RecipeElevation>,
    pub opacity: Option<f64>,
    pub press: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeRecipeStates {
    pub hover: Option<ThemeStateRecipe>,
    pub active: Option<ThemeStateRecipe>,
    pub selected: Option<ThemeStateRecipe>,
    pub focus: Option<ThemeStateRecipe>,
    pub disabled: Option<ThemeStateRecipe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurfaceRecipe {
    pub background: RecipeBackground,
    pub foreground: RecipeForeground,
    pub border: RecipeBorder,
    pub border_width: RecipeBorderWidth,
    pub border_style: RecipeBorderStyle,
    pub radius: RecipeRadius,
    pub elevation: RecipeElevation,
    pub material: RecipeMaterial,
    pub motion: RecipeMotion,
    pub states: Option<ThemeRecipeStates>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRecipes {
    pub shell: SurfaceRecipe,
    pub titlebar: SurfaceRecipe,
    pub sidebar: SurfaceRecipe,
    #[serde(rename = "navigation-item")]
    pub navigation_item: SurfaceRecipe,
    pub panel: SurfaceRecipe,
    pub card: SurfaceRecipe,
    pub composer: SurfaceRecipe,
    #[serde(rename = "message-user")]
    pub message_user: SurfaceRecipe,
    #[serde(rename = "message-assistant")]
    pub message_assistant: SurfaceRecipe,
    #[serde(rename = "message-disclosure")]
    pub message_disclosure: SurfaceRecipe,
    #[serde(rename = "runtime-control")]
    pub runtime_control: SurfaceRecipe,
    pub activity: SurfaceRecipe,
    #[serde(rename = "tool-card")]
    pub tool_card: SurfaceRecipe,
    #[serde(rename = "permission-card")]
    pub permission_card: SurfaceRecipe,
    pub dialog: SurfaceRecipe,
    pub sheet: SurfaceRecipe,
    pub popover: SurfaceRecipe,
    pub input: SurfaceRecipe,
    #[serde(rename = "button-primary")]
    pub button_primary: SurfaceRecipe,
    #[serde(rename = "button-secondary")]
    pub button_secondary: SurfaceRecipe,
    #[serde(rename = "button-ghost")]
    pub button_ghost: SurfaceRecipe,
    pub editor: SurfaceRecipe,
    pub diff: SurfaceRecipe,
    #[serde(rename = "workspace-tab")]
    pub workspace_tab: SurfaceRecipe,
    #[serde(rename = "workflow-node")]
    pub workflow_node: SurfaceRecipe,
    #[serde(rename = "workflow-edge")]
    pub workflow_edge: SurfaceRecipe,
    pub scrollbar: SurfaceRecipe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerformanceMaterialOverrides {
    pub blur: f64,
    pub saturate: f64,
    pub texture_opacity: f64,
    pub wallpapers: Option<WallpaperPerformanceOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperPerformanceOverride {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeVisualQualityProfiles {
    pub default: ThemeVisualQuality,
    pub supported: [ThemeVisualQuality; 2],
    pub performance: PerformanceMaterialOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeAssetKind {
    Font,
    Avatar,
    Icon,
    Texture,
    Wallpaper,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeFontMetadata {
    pub family: String,
    pub subfamily: String,
    pub postscript_name: String,
    pub weight_min: f64,
    pub weight_max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeAssetRecord {
    pub id: String,
    pub kind: ThemeAssetKind,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
    pub output_url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub font_metadata: Option<ThemeFontMetadata>,
    pub required: bool,
    pub license_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeAssetManifestSummary {
    pub schema_version: u8,
    pub count: usize,
    pub total_bytes: u64,
    pub records: Vec<ThemeAssetRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFontCoverage {
    pub scripts: Vec<String>,
    pub locales: Option<Vec<String>>,
    #[serde(rename = "unicodeRanges")]
    pub unicode_ranges: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeFontMetrics {
    pub size_adjust: Option<String>,
    pub ascent_override: Option<String>,
    pub descent_override: Option<String>,
    pub line_gap_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeFontStyle {
    Normal,
    Italic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeFontDisplay {
    Swap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeFontFace {
    pub id: String,
    pub family: String,
    pub runtime_family: String,
    pub asset_id: String,
    pub weight_min: u16,
    pub weight_max: u16,
    pub style: ThemeFontStyle,
    pub display: ThemeFontDisplay,
    pub coverage: ThemeFontCoverage,
    pub metrics: Option<ThemeFontMetrics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeFontStack {
    pub id: String,
    pub display_name: LocalizedThemeName,
    pub default_faces: Vec<String>,
    pub system_fallbacks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFontRuntime {
    pub faces: Vec<ThemeFontFace>,
    pub stacks: Vec<ThemeFontStack>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeIconRenderMode {
    Mask,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeImageRendering {
    Auto,
    Pixelated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeIconDescriptor {
    pub asset_id: String,
    pub render_mode: ThemeIconRenderMode,
    pub native_size: u8,
    pub image_rendering: ThemeImageRendering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeIconSchemeMaps {
    pub light: Option<BTreeMap<String, ThemeIconDescriptor>>,
    pub dark: Option<BTreeMap<String, ThemeIconDescriptor>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeIconMap {
    pub defaults: BTreeMap<String, ThemeIconDescriptor>,
    pub schemes: Option<ThemeIconSchemeMaps>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeWallpaperFit {
    Cover,
    Contain,
    Tile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeWallpaperPosition {
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeWallpaperRepeat {
    NoRepeat,
    Repeat,
    RepeatX,
    RepeatY,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeWallpaperDescriptor {
    pub asset_id: String,
    pub fit: ThemeWallpaperFit,
    pub position: ThemeWallpaperPosition,
    pub repeat: ThemeWallpaperRepeat,
    pub opacity: f64,
    pub overlay_color: RecipeBackground,
    pub overlay_opacity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeWallpaperMap {
    pub light: BTreeMap<String, ThemeWallpaperDescriptor>,
    pub dark: BTreeMap<String, ThemeWallpaperDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemePackage {
    pub schema_version: u8,
    pub contract_version: u8,
    pub id: String,
    pub version: String,
    pub source: ThemeSource,
    pub name: LocalizedThemeName,
    pub author: String,
    pub capabilities: Vec<ThemeCapability>,
    pub schemes: ThemeSchemes,
    pub recipes: ComponentRecipes,
    pub assets: ThemeAssetManifestSummary,
    pub fonts: Option<ThemeFontRuntime>,
    pub icons: Option<ThemeIconMap>,
    pub wallpapers: Option<ThemeWallpaperMap>,
    pub visual_quality_profiles: Option<ThemeVisualQualityProfiles>,
}

static BUILTIN_THEME_CATALOG: OnceLock<Result<Vec<ThemePackage>, ThemeContractError>> =
    OnceLock::new();

pub fn builtin_theme_catalog() -> Result<&'static [ThemePackage], ThemeContractError> {
    match BUILTIN_THEME_CATALOG.get_or_init(parse_builtin_theme_catalog) {
        Ok(catalog) => Ok(catalog),
        Err(error) => Err(error.clone()),
    }
}

pub fn builtin_theme(theme_id: &str) -> Result<Option<&'static ThemePackage>, ThemeContractError> {
    Ok(builtin_theme_catalog()?
        .iter()
        .find(|theme| theme.id == theme_id))
}

fn parse_builtin_theme_catalog() -> Result<Vec<ThemePackage>, ThemeContractError> {
    let catalog: Vec<ThemePackage> =
        serde_json::from_str(BUILTIN_THEME_CATALOG_JSON).map_err(|error| ThemeContractError {
            code: "theme.package-invalid",
            detail: error.to_string(),
        })?;
    let mut ids = BTreeSet::new();
    for theme in &catalog {
        if theme.schema_version != 2 || theme.contract_version != 2 {
            return Err(ThemeContractError {
                code: "theme.contract-version-unsupported",
                detail: format!("{} must use Theme Contract v2", theme.id),
            });
        }
        if !ids.insert(theme.id.as_str()) {
            return Err(ThemeContractError {
                code: "theme.package-invalid",
                detail: format!("duplicate builtin theme id: {}", theme.id),
            });
        }
        let declares_quality = theme
            .capabilities
            .contains(&ThemeCapability::VisualQualityProfiles);
        if declares_quality != theme.visual_quality_profiles.is_some() {
            return Err(ThemeContractError {
                code: "theme.package-invalid",
                detail: format!("quality capability mismatch for {}", theme.id),
            });
        }
        validate_capability(theme, ThemeCapability::Fonts, theme.fonts.is_some())?;
        validate_capability(theme, ThemeCapability::Icons, theme.icons.is_some())?;
        validate_capability(
            theme,
            ThemeCapability::Wallpapers,
            theme.wallpapers.is_some(),
        )?;
        if theme.assets.schema_version != 2 {
            return Err(ThemeContractError {
                code: "theme.package-invalid",
                detail: format!("asset manifest version mismatch for {}", theme.id),
            });
        }
        let mut asset_ids = BTreeSet::new();
        for asset in &theme.assets.records {
            if !asset_ids.insert(asset.id.as_str())
                || !asset.output_url.starts_with("/theme-assets/")
            {
                return Err(ThemeContractError {
                    code: "theme.package-invalid",
                    detail: format!("invalid asset record {} in {}", asset.id, theme.id),
                });
            }
        }
        if let Some(icons) = &theme.icons {
            for slot in icons.defaults.keys().chain(
                icons
                    .schemes
                    .iter()
                    .flat_map(|maps| maps.light.iter().chain(maps.dark.iter()))
                    .flat_map(|map| map.keys()),
            ) {
                if !THEME_ICON_SLOTS.contains(&slot.as_str()) {
                    return Err(ThemeContractError {
                        code: "theme.icon-slot-unknown",
                        detail: slot.clone(),
                    });
                }
            }
        }
        if let Some(wallpapers) = &theme.wallpapers {
            for slot in wallpapers.light.keys().chain(wallpapers.dark.keys()) {
                if !THEME_WALLPAPER_SLOTS.contains(&slot.as_str()) {
                    return Err(ThemeContractError {
                        code: "theme.wallpaper-slot-unknown",
                        detail: slot.clone(),
                    });
                }
            }
        }
    }
    if !ids.contains("builtin.gold-band") {
        return Err(ThemeContractError {
            code: "theme.active-package-missing",
            detail: "builtin.gold-band is required as the safe fallback".to_string(),
        });
    }
    Ok(catalog)
}

const THEME_ICON_SLOTS: &[&str] = &[
    "navigation.conversation",
    "navigation.search",
    "navigation.agent",
    "navigation.context",
    "navigation.run-mode",
    "navigation.settings",
    "entity.task",
    "entity.workflow",
    "entity.agent",
    "entity.file",
    "entity.folder",
    "conversation.thought",
    "conversation.attachment",
    "tool.read",
    "tool.write",
    "tool.command",
    "permission.request",
    "status.running",
    "status.success",
    "status.warning",
    "status.error",
    "action.send",
    "action.continue",
    "action.stop",
];
const THEME_WALLPAPER_SLOTS: &[&str] = &["app", "conversation", "workspace", "settings"];

fn validate_capability(
    theme: &ThemePackage,
    capability: ThemeCapability,
    has_payload: bool,
) -> Result<(), ThemeContractError> {
    if theme.capabilities.contains(&capability) == has_payload {
        return Ok(());
    }
    Err(ThemeContractError {
        code: "theme.capability-file-mismatch",
        detail: format!("{:?} capability mismatch for {}", capability, theme.id),
    })
}
