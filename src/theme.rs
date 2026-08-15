use std::collections::BTreeSet;
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
    pub shadow: String,
    pub radius: String,
    pub background_image: String,
    pub texture_opacity: f64,
    pub motion_duration: String,
    pub motion_easing: String,
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
pub struct ThemeFontStackPreset {
    pub families: Vec<String>,
    pub fallback: ThemeGenericFontFamily,
    pub size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeGenericFontFamily {
    #[serde(rename = "sans-serif")]
    SansSerif,
    #[serde(rename = "monospace")]
    Monospace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTypographyPreset {
    pub ui: ThemeFontStackPreset,
    pub editor: ThemeFontStackPreset,
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
    Card,
    Popover,
    Sidebar,
    SurfaceLow,
    SurfaceHigh,
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeForeground {
    Foreground,
    MutedForeground,
    CardForeground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeBorder {
    Border,
    SidebarBorder,
    Highlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeMaterial {
    Flat,
    Subtle,
    Elevated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRecipe {
    pub background: RecipeBackground,
    pub foreground: RecipeForeground,
    pub border: RecipeBorder,
    pub material: RecipeMaterial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentRecipes {
    pub shell: SurfaceRecipe,
    pub titlebar: SurfaceRecipe,
    pub sidebar: SurfaceRecipe,
    pub panel: SurfaceRecipe,
    pub composer: SurfaceRecipe,
    pub card: SurfaceRecipe,
    pub dialog: SurfaceRecipe,
    pub sheet: SurfaceRecipe,
    pub popover: SurfaceRecipe,
    pub input: SurfaceRecipe,
    pub button: SurfaceRecipe,
    pub editor: SurfaceRecipe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerformanceMaterialOverrides {
    pub blur: f64,
    pub saturate: f64,
    pub backdrop_brightness: Option<f64>,
    pub backdrop_contrast: Option<f64>,
    pub specular_highlight: Option<String>,
    pub edge_shadow: Option<String>,
    pub shadow: String,
    pub texture_opacity: f64,
    pub motion_duration: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeVisualQualityProfiles {
    pub default: ThemeVisualQuality,
    pub supported: [ThemeVisualQuality; 2],
    pub performance: PerformanceMaterialOverrides,
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
    }
    if !ids.contains("builtin.gold-band") {
        return Err(ThemeContractError {
            code: "theme.active-package-missing",
            detail: "builtin.gold-band is required as the safe fallback".to_string(),
        });
    }
    Ok(catalog)
}
