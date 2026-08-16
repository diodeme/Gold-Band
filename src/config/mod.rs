use std::{collections::BTreeMap, str::FromStr, sync::OnceLock};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize};
use tracing::Level;

fn embedded_project_app_config() -> &'static ProjectAppConfig {
    static CONFIG: OnceLock<ProjectAppConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        config::Config::builder()
            .add_source(config::File::from_str(
                include_str!("../../configs/app-config.toml"),
                config::FileFormat::Toml,
            ))
            .build()
            .expect("embedded app-config.toml is valid")
            .try_deserialize()
            .expect("embedded app-config.toml deserializes to ProjectAppConfig")
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl RuntimeLogLevel {
    pub const fn as_directive(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Error,
            1 => Self::Warn,
            2 => Self::Info,
            3 => Self::Debug,
            4 => Self::Trace,
            _ => Self::Info,
        }
    }

    pub const fn allows(self, level: &Level) -> bool {
        match self {
            Self::Error => matches!(*level, Level::ERROR),
            Self::Warn => matches!(*level, Level::ERROR | Level::WARN),
            Self::Info => matches!(*level, Level::ERROR | Level::WARN | Level::INFO),
            Self::Debug => {
                matches!(
                    *level,
                    Level::ERROR | Level::WARN | Level::INFO | Level::DEBUG
                )
            }
            Self::Trace => true,
        }
    }
}

impl FromStr for RuntimeLogLevel {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            "trace" => Ok(Self::Trace),
            _ => Err(anyhow!("unsupported log level: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsoleThemeName {
    GoldBand,
    Nord,
    Dracula,
    Cyber,
    Onyx,
    Mist,
    HighContrast,
}

impl FromStr for ConsoleThemeName {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "gold-band" => Ok(Self::GoldBand),
            "nord" => Ok(Self::Nord),
            "dracula" => Ok(Self::Dracula),
            "cyber" => Ok(Self::Cyber),
            "onyx" => Ok(Self::Onyx),
            "mist" => Ok(Self::Mist),
            "high-contrast" => Ok(Self::HighContrast),
            _ => Err(anyhow!("unsupported console theme: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorSchemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualQuality {
    Full,
    Performance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearancePreference {
    pub schema_version: u8,
    pub theme_id: String,
    pub color_scheme: ColorSchemePreference,
    #[serde(default)]
    pub visual_quality_by_theme: BTreeMap<String, VisualQuality>,
}

impl Default for AppearancePreference {
    fn default() -> Self {
        Self {
            schema_version: 2,
            theme_id: "builtin.gold-band".to_string(),
            color_scheme: ColorSchemePreference::System,
            visual_quality_by_theme: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum FontStackPreference {
    Theme,
    Custom { families: Vec<String> },
}

pub const MAX_FONT_STACK_FAMILIES: usize = 16;
pub const MAX_FONT_FAMILY_CHARS: usize = 128;

impl FontStackPreference {
    pub fn normalized(self) -> Self {
        let Self::Custom { families } = self else {
            return Self::Theme;
        };
        let mut normalized = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for family in families {
            let family = family.trim();
            let key = family.to_lowercase();
            if family.is_empty()
                || family.chars().count() > MAX_FONT_FAMILY_CHARS
                || family.contains([',', ';', '{', '}'])
                || !seen.insert(key)
            {
                continue;
            }
            normalized.push(family.to_string());
            if normalized.len() == MAX_FONT_STACK_FAMILIES {
                break;
            }
        }
        if normalized.is_empty() {
            Self::Theme
        } else {
            Self::Custom {
                families: normalized,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum FontSizePreference {
    Theme,
    Custom { px: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum AvatarPreference {
    Theme,
    User { asset_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersonalizationAvatarShape {
    Circle,
    Square,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum AvatarShapePreference {
    Theme,
    Custom { value: PersonalizationAvatarShape },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyPreference {
    pub font_stack: FontStackPreference,
    pub font_size: FontSizePreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypographyPersonalization {
    pub ui: TypographyPreference,
    pub editor: TypographyPreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPersonalization {
    pub image: AvatarPreference,
    pub shape: AvatarShapePreference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPersonalizationSet {
    pub agent: AvatarPersonalization,
    pub user: AvatarPersonalization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalizationPreference {
    pub schema_version: u8,
    pub typography: TypographyPersonalization,
    pub avatars: AvatarPersonalizationSet,
}

impl Default for PersonalizationPreference {
    fn default() -> Self {
        let typography = || TypographyPreference {
            font_stack: FontStackPreference::Theme,
            font_size: FontSizePreference::Theme,
        };
        let avatar = || AvatarPersonalization {
            image: AvatarPreference::Theme,
            shape: AvatarShapePreference::Theme,
        };
        Self {
            schema_version: 2,
            typography: TypographyPersonalization {
                ui: typography(),
                editor: typography(),
            },
            avatars: AvatarPersonalizationSet {
                agent: avatar(),
                user: avatar(),
            },
        }
    }
}

impl PersonalizationPreference {
    pub fn normalized(mut self) -> Self {
        self.schema_version = 2;
        self.typography.ui.font_stack = self.typography.ui.font_stack.normalized();
        self.typography.editor.font_stack = self.typography.editor.font_stack.normalized();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopLanguage {
    ZhCn,
    En,
}

pub const DEFAULT_DESKTOP_UI_FONT_SIZE: u8 = 14;
pub const DEFAULT_DESKTOP_EDITOR_FONT_SIZE: u8 = 12;
pub const MIN_DESKTOP_UI_FONT_SIZE: u8 = 12;
pub const MAX_DESKTOP_UI_FONT_SIZE: u8 = 18;
pub const MIN_DESKTOP_EDITOR_FONT_SIZE: u8 = 10;
pub const MAX_DESKTOP_EDITOR_FONT_SIZE: u8 = 18;

pub fn normalize_desktop_ui_font_size(value: u8) -> u8 {
    value.clamp(MIN_DESKTOP_UI_FONT_SIZE, MAX_DESKTOP_UI_FONT_SIZE)
}

pub fn normalize_desktop_editor_font_size(value: u8) -> u8 {
    value.clamp(MIN_DESKTOP_EDITOR_FONT_SIZE, MAX_DESKTOP_EDITOR_FONT_SIZE)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ManagedAgentId(String);

pub const DEFAULT_CUSTOM_AGENT_ICON: &str = "gold-band";

impl ManagedAgentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ManagedAgentId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(anyhow!("invalid managed agent id: {value}"));
        }
        Ok(Self(value.to_string()))
    }
}

impl<'de> Deserialize<'de> for ManagedAgentId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemPromptDelivery {
    #[default]
    None,
    MetaAppend,
}

pub fn catalog_agent_default_config(agent_id: &str) -> Option<ManagedAgentConfig> {
    crate::agent_catalog::builtin_agent(agent_id).map(ManagedAgentConfig::from_catalog)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAdapterConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub display_name: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Default for AcpAdapterConfig {
    fn default() -> Self {
        catalog_agent_default_config("claude-acp")
            .expect("Claude is present in the built-in Agent catalog")
            .adapter
    }
}

impl FromStr for DesktopLanguage {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "zh-cn" => Ok(Self::ZhCn),
            "en" => Ok(Self::En),
            _ => Err(anyhow!("unsupported desktop language: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAgentConfig {
    pub adapter: AcpAdapterConfig,
    /// 用户实例自己的图标引用；新建后不再跟随 Catalog 更新。
    #[serde(default = "default_agent_icon")]
    pub icon: String,
    /// Gold Band 写入、同步，同时也是 Agent 首个读取位置的主 Agent 目录。
    #[serde(default)]
    pub primary_agent_dir: Option<String>,
    /// 项目 Skill 主目录。`None` 表示全局和项目共用 `primary_agent_dir`；
    /// `Some` 表示启用作用域拆分，项目读写使用该目录。
    #[serde(default)]
    pub project_primary_agent_dir: Option<String>,
    /// Agent 额外读取但 Gold Band 不写入、不作为同步目标的兼容 Agent 目录。
    #[serde(default)]
    pub compatible_agent_dirs: Vec<String>,
    /// system prompt 的实际传递方式。当前仅实现 ACP `_meta.systemPrompt.append`。
    #[serde(default)]
    pub system_prompt_delivery: SystemPromptDelivery,
    /// Agent 是否具备跨客户端共享同一线性 Session 的能力。
    #[serde(default)]
    pub external_session_sync_supported: bool,
    /// 是否允许 Gold Band 根据 Provider revision 重载并导入外部客户端会话历史。
    /// 仅适用于能跨客户端共享同一线性会话上下文的 Agent，默认关闭。
    #[serde(default)]
    pub external_session_sync_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkillDirectoryPolicy {
    pub global: AgentSkillDirectoryScopePolicy,
    pub project: AgentSkillDirectoryScopePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkillDirectoryScopePolicy {
    pub write_dir_names: Vec<String>,
    pub read_dir_names: Vec<String>,
}

impl AgentSkillDirectoryPolicy {
    pub fn for_source(&self, source: SkillSource) -> Option<&AgentSkillDirectoryScopePolicy> {
        match source {
            SkillSource::Global => Some(&self.global),
            SkillSource::Project => Some(&self.project),
            SkillSource::BuiltIn => None,
        }
    }
}

impl ManagedAgentConfig {
    pub fn new(
        adapter: AcpAdapterConfig,
        primary_agent_dir: impl Into<String>,
        compatible_agent_dirs: Vec<String>,
    ) -> Self {
        Self {
            adapter,
            icon: default_agent_icon(),
            primary_agent_dir: Some(primary_agent_dir.into()),
            project_primary_agent_dir: None,
            compatible_agent_dirs,
            system_prompt_delivery: SystemPromptDelivery::None,
            external_session_sync_supported: false,
            external_session_sync_enabled: false,
        }
    }

    pub fn from_catalog(entry: &crate::agent_catalog::AgentCatalogEntry) -> Self {
        Self {
            adapter: AcpAdapterConfig {
                command: entry.command.clone(),
                args: entry.args.clone(),
                display_name: entry.label.clone(),
                env: entry.env.clone(),
            },
            icon: entry.icon_key.clone(),
            primary_agent_dir: entry.primary_agent_dir.clone(),
            project_primary_agent_dir: entry.project_primary_agent_dir.clone(),
            compatible_agent_dirs: entry.compatible_agent_dirs.clone(),
            system_prompt_delivery: if entry.supports_system_prompt {
                SystemPromptDelivery::MetaAppend
            } else {
                SystemPromptDelivery::None
            },
            external_session_sync_supported: entry.supports_external_session_sync,
            external_session_sync_enabled: false,
        }
    }

    pub fn supports_system_prompt(&self) -> bool {
        self.system_prompt_delivery != SystemPromptDelivery::None
    }

    pub fn skill_directory_policy(&self) -> AgentSkillDirectoryPolicy {
        let global = skill_directory_scope_policy(
            self.primary_agent_dir.as_deref(),
            &self.compatible_agent_dirs,
        );
        let project = skill_directory_scope_policy(
            self.project_primary_agent_dir
                .as_deref()
                .or(self.primary_agent_dir.as_deref()),
            &self.compatible_agent_dirs,
        );
        AgentSkillDirectoryPolicy { global, project }
    }
}

fn skill_directory_scope_policy(
    primary_agent_dir: Option<&str>,
    compatible_agent_dirs: &[String],
) -> AgentSkillDirectoryScopePolicy {
    let mut write_dir_names = Vec::new();
    let mut read_dir_names = Vec::new();
    if let Some(primary) = primary_agent_dir
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        write_dir_names.push(primary.to_string());
        read_dir_names.push(primary.to_string());
    }
    for compatible in compatible_agent_dirs {
        let compatible = compatible.trim();
        if !compatible.is_empty() && !read_dir_names.iter().any(|dir_name| dir_name == compatible) {
            read_dir_names.push(compatible.to_string());
        }
    }
    AgentSkillDirectoryScopePolicy {
        write_dir_names,
        read_dir_names,
    }
}

fn default_agent_icon() -> String {
    "agent".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUpdateBadgeState {
    pub settings_entry_seen_version: Option<String>,
    pub settings_advanced_seen_version: Option<String>,
    pub announcement_closed_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAvailableUpdate {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

// ── MCP Server Configuration ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(flatten)]
    pub transport: McpTransportConfig,
    #[serde(default)]
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help_message: Option<String>,
}

fn default_enabled() -> bool {
    true
}

/// 对标 Zed OAuthClientSettings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientConfig {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "camelCase")]
pub enum McpTransportConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        /// 对标 Zed: OAuth 预注册客户端配置
        #[serde(skip_serializing_if = "Option::is_none")]
        oauth: Option<OAuthClientConfig>,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

// ── SKILL Constants & Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerHealthResult {
    pub status: String, // "healthy" | "unhealthy" | "auth_required" | "unknown"
    pub message: Option<String>,
    /// 对标 Zed AuthRequired — 需要 OAuth 认证时的授权 URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    /// 对标 Zed ClientSecretRequired — 需要输入 client_secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_client_secret: Option<bool>,
    /// tools/list 发现的工具列表（仅 Running 状态时填充）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolInfo>,
}

/// MCP 服务器状态机（对标 Zed ContextServerState）
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    /// 正在启动（握手进行中）
    Starting,
    /// 运行中，持有已发现的工具列表
    Running { tools: Vec<ToolInfo> },
    /// 已停止（用户禁用或手动停止）
    Stopped,
    /// 启动失败
    Error { message: String },
    /// 需要 OAuth 认证
    AuthRequired { auth_url: Option<String> },
}

/// MCP 工具信息（从 tools/list 响应解析）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

pub const GOLD_BAND_DIR_NAME: &str = ".gold-band";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const MAX_SKILL_FILE_SIZE: usize = 100 * 1024;
pub const MAX_SKILL_DESCRIPTION_LEN: usize = 1024;
pub const DEFAULT_CONVERSATION_AUTO_TITLE_MAX_CHARS: usize = 18;
pub const DEFAULT_NOTIFICATION_AUTO_DISMISS_TARGET_SECS: u64 = 20;
pub const DEFAULT_SCHEDULED_OCCURRENCE_RETENTION_DAYS: u16 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub directory_path: String,
    /// SKILL 来源目录标识：".gold-band" 为 Gold-Band 自身管理，".claude"/".codex" 等为对应 agent 目录
    pub agent_source: String,
    pub load_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synced_agent_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillSource {
    BuiltIn,
    Global,
    Project,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsConfig {
    #[serde(default)]
    pub settings_schema_version: SettingsSchemaVersion,
    pub log_level: Option<RuntimeLogLevel>,
    pub log_prompts: Option<bool>,
    pub log_provider_command: Option<bool>,
    pub log_retention_days: Option<u64>,
    pub console_theme: Option<ConsoleThemeName>,
    pub appearance: Option<AppearancePreference>,
    pub personalization: Option<PersonalizationPreference>,
    pub desktop_language: Option<DesktopLanguage>,
    pub desktop_updater_url_override: Option<String>,
    pub agents: Option<BTreeMap<ManagedAgentId, ManagedAgentConfig>>,
    pub use_local_claude: Option<bool>,
    pub desktop_metrics_enabled: Option<bool>,
    pub desktop_metrics_base_url: Option<String>,
    pub desktop_metrics_api_key: Option<String>,
    pub scheduled_keep_awake_enabled: Option<bool>,
    pub scheduled_completion_notifications_enabled: Option<bool>,
    pub scheduled_occurrence_retention_days: Option<u16>,
    #[serde(default)]
    pub context_servers: Option<Vec<McpServerConfig>>,
}

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 8;
const USE_LOCAL_CLAUDE: bool = false;

const LEGACY_CODEX_ACP_PACKAGE_PREFIX: &str = "@zed-industries/codex-acp";
const CURRENT_CODEX_ACP_PACKAGE: &str = "@agentclientprotocol/codex-acp@latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettingsSchemaVersion(pub u32);

impl Default for SettingsSchemaVersion {
    fn default() -> Self {
        Self(CURRENT_SETTINGS_SCHEMA_VERSION)
    }
}

impl SettingsConfig {
    pub fn from_json_value_with_migration(mut value: serde_json::Value) -> Result<(Self, bool)> {
        let settings = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("settings root must be a JSON object"))?;
        let version = settings
            .get("settingsSchemaVersion")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if version > u64::from(CURRENT_SETTINGS_SCHEMA_VERSION) {
            return Err(anyhow!(
                "settings schema version {version} is newer than supported version {CURRENT_SETTINGS_SCHEMA_VERSION}"
            ));
        }

        let mut migrated = false;
        if version < 1 {
            migrate_managed_agent_directories(settings)?;
            migrated = true;
        }
        if version < 2 {
            migrate_codex_acp_package(settings)?;
            migrated = true;
        }
        if version < 4 {
            migrate_scheduled_runtime_settings(settings);
            migrate_managed_agent_capabilities(settings)?;
            migrated = true;
        }
        if version < 5 {
            migrate_desktop_appearance(settings);
            migrated = true;
        }
        if version < 6 {
            settings.remove("desktopWorkspace");
            migrated = true;
        }
        if version < 7 {
            migrate_desktop_personalization(settings);
            migrated = true;
        }
        if version < 8 {
            migrate_desktop_font_stacks(settings);
            migrated = true;
        }
        if migrated {
            settings.insert(
                "settingsSchemaVersion".to_string(),
                serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION),
            );
        }

        let mut config: Self = serde_json::from_value(value)?;
        if let Some(personalization) = config.personalization.take() {
            let normalized = personalization.clone().normalized();
            if normalized != personalization {
                migrated = true;
            }
            config.personalization = Some(normalized);
        }
        Ok((config, migrated))
    }
}

fn migrate_desktop_font_stacks(settings: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(personalization) = settings
        .get_mut("personalization")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    personalization.insert("schemaVersion".to_string(), serde_json::json!(2));
    let Some(typography) = personalization
        .get_mut("typography")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for section in ["ui", "editor"] {
        let Some(preference) = typography
            .get_mut(section)
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        preference.remove("font");
        preference.insert(
            "fontStack".to_string(),
            serde_json::json!({ "source": "theme" }),
        );
    }
}

fn migrate_desktop_appearance(settings: &mut serde_json::Map<String, serde_json::Value>) {
    if settings.contains_key("appearance") {
        settings.remove("desktopTheme");
        return;
    }
    let legacy = settings
        .remove("desktopTheme")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "system".to_string());
    let (theme_id, color_scheme) = match legacy.as_str() {
        "light" => ("builtin.gold-band", "light"),
        "dark" => ("builtin.gold-band", "dark"),
        "light-gray" => ("builtin.tech-neutral", "light"),
        "black" => ("builtin.tech-neutral", "dark"),
        _ => ("builtin.gold-band", "system"),
    };
    settings.insert(
        "appearance".to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "themeId": theme_id,
            "colorScheme": color_scheme,
            "visualQualityByTheme": {}
        }),
    );
}

fn migrate_desktop_personalization(settings: &mut serde_json::Map<String, serde_json::Value>) {
    if settings.contains_key("personalization") {
        settings.remove("desktopFont");
        settings.remove("desktopEditorFont");
        settings.remove("desktopUiFontSize");
        settings.remove("desktopEditorFontSize");
        return;
    }
    let ui_font = settings
        .remove("desktopFont")
        .and_then(|value| value.as_str().map(|family| family.trim().to_string()))
        .filter(|value| !value.is_empty() && value != "app-default")
        .map_or_else(
            || serde_json::json!({ "source": "theme" }),
            |family| serde_json::json!({ "source": "local", "family": family }),
        );
    let editor_font = settings
        .remove("desktopEditorFont")
        .and_then(|value| value.as_str().map(|family| family.trim().to_string()))
        .filter(|value| !value.is_empty() && value != "editor-default")
        .map_or_else(
            || serde_json::json!({ "source": "theme" }),
            |family| serde_json::json!({ "source": "local", "family": family }),
        );
    let ui_size = settings
        .remove("desktopUiFontSize")
        .and_then(|value| value.as_u64())
        .map(|value| normalize_desktop_ui_font_size(value.min(u64::from(u8::MAX)) as u8))
        .filter(|value| *value != DEFAULT_DESKTOP_UI_FONT_SIZE)
        .map_or_else(
            || serde_json::json!({ "source": "theme" }),
            |px| serde_json::json!({ "source": "custom", "px": px }),
        );
    let editor_size = settings
        .remove("desktopEditorFontSize")
        .and_then(|value| value.as_u64())
        .map(|value| normalize_desktop_editor_font_size(value.min(u64::from(u8::MAX)) as u8))
        .filter(|value| *value != DEFAULT_DESKTOP_EDITOR_FONT_SIZE)
        .map_or_else(
            || serde_json::json!({ "source": "theme" }),
            |px| serde_json::json!({ "source": "custom", "px": px }),
        );
    settings.insert(
        "personalization".to_string(),
        serde_json::json!({
            "schemaVersion": 1,
            "typography": {
                "ui": { "font": ui_font, "fontSize": ui_size },
                "editor": { "font": editor_font, "fontSize": editor_size }
            },
            "avatars": {
                "agent": { "image": { "source": "theme" }, "shape": { "source": "theme" } },
                "user": { "image": { "source": "theme" }, "shape": { "source": "theme" } }
            }
        }),
    );
}

fn migrate_scheduled_runtime_settings(settings: &mut serde_json::Map<String, serde_json::Value>) {
    settings
        .entry("scheduledKeepAwakeEnabled".to_string())
        .or_insert_with(|| serde_json::json!(false));
    settings
        .entry("scheduledCompletionNotificationsEnabled".to_string())
        .or_insert_with(|| serde_json::json!(true));
    settings
        .entry("scheduledOccurrenceRetentionDays".to_string())
        .or_insert_with(|| serde_json::json!(DEFAULT_SCHEDULED_OCCURRENCE_RETENTION_DAYS));
}

fn migrate_codex_acp_package(
    settings: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let Some(args) = settings
        .get_mut("agents")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|agents| agents.get_mut("codex-acp"))
        .and_then(|agent| agent.get_mut("adapter"))
        .and_then(|adapter| adapter.get_mut("args"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };

    for arg in args {
        let Some(package) = arg.as_str() else {
            continue;
        };
        if package.starts_with(LEGACY_CODEX_ACP_PACKAGE_PREFIX) {
            *arg = serde_json::Value::String(CURRENT_CODEX_ACP_PACKAGE.to_string());
        }
    }
    Ok(())
}

fn migrate_managed_agent_directories(
    settings: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let Some(agents) = settings
        .get_mut("agents")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };

    let legacy_agents = std::mem::take(agents);
    for (legacy_id, mut value) in legacy_agents {
        let canonical_id = match legacy_id.as_str() {
            "claude-code" => "claude-acp",
            "codex-cli" => "codex-acp",
            "gemini-cli" => "gemini",
            _ => legacy_id.as_str(),
        };
        ManagedAgentId::from_str(canonical_id)?;
        let (default_primary, default_compatible) = legacy_agent_directories(canonical_id)
            .ok_or_else(|| anyhow!("cannot migrate unsupported managed agent `{canonical_id}`"))?;
        let config = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("managed agent `{canonical_id}` config must be an object"))?;

        let legacy_override = config
            .remove("skillsDirOverride")
            .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
            .filter(|value| !value.is_empty());
        if !config.contains_key("primaryAgentDir") {
            config.insert(
                "primaryAgentDir".to_string(),
                serde_json::Value::String(
                    legacy_override.unwrap_or_else(|| default_primary.to_string()),
                ),
            );
        }
        if !config.contains_key("compatibleAgentDirs") {
            config.insert(
                "compatibleAgentDirs".to_string(),
                serde_json::Value::Array(
                    default_compatible
                        .iter()
                        .map(|directory| serde_json::Value::String((*directory).to_string()))
                        .collect(),
                ),
            );
        }
        if agents.insert(canonical_id.to_string(), value).is_some() {
            return Err(anyhow!(
                "duplicate managed agent `{canonical_id}` after settings migration"
            ));
        }
    }
    Ok(())
}

fn legacy_agent_directories(agent_id: &str) -> Option<(&'static str, &'static [&'static str])> {
    match agent_id {
        "claude-acp" => Some((".claude", &[])),
        "codex-acp" => Some((".codex", &[".agents"])),
        "cursor" => Some((".cursor", &[".agents"])),
        "gemini" => Some((".gemini", &[".agents"])),
        "opencode" => Some((".opencode", &[".agents"])),
        _ => None,
    }
}

fn migrate_managed_agent_capabilities(
    settings: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let Some(agents) = settings
        .get_mut("agents")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    for (agent_id, value) in agents {
        let config = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("managed agent `{agent_id}` config must be an object"))?;
        let sync_enabled = config
            .get("externalSessionSyncEnabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        config.entry("icon".to_string()).or_insert_with(|| {
            serde_json::Value::String(
                match agent_id.as_str() {
                    "claude-acp" => "claude",
                    "codex-acp" => "codex",
                    "cursor" => "cursor",
                    "gemini" => "gemini",
                    "opencode" => "opencode",
                    _ => "agent",
                }
                .to_string(),
            )
        });
        config
            .entry("systemPromptDelivery".to_string())
            .or_insert_with(|| {
                serde_json::Value::String(
                    if agent_id == "claude-acp" {
                        "meta-append"
                    } else {
                        "none"
                    }
                    .to_string(),
                )
            });
        config
            .entry("externalSessionSyncSupported".to_string())
            .or_insert(serde_json::Value::Bool(sync_enabled));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateConfig {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub state_schema_version: u32,
    pub desktop_updater_last_checked_at: Option<String>,
    #[serde(default)]
    pub desktop_update_badges: DesktopUpdateBadgeState,
    pub desktop_available_update: Option<DesktopAvailableUpdate>,
    #[serde(default)]
    pub recent_desktop_workspaces: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub preferences: std::collections::HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_ui_mode: Option<DesktopUiMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversation_workspaces: Vec<ConversationWorkspaceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_conversation_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversation_pins: Vec<ConversationPin>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub conversation_run_modes: std::collections::HashMap<String, ConversationRunModeEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAppConfig {
    pub acp_session_title_refresh_enabled: Option<bool>,
    pub acp_chat_event_page_size: Option<usize>,
    pub acp_raw_max_size_bytes: Option<u64>,
    pub acp_raw_target_size_bytes: Option<u64>,
    pub acp_session_foreground_lease_ttl_secs: Option<u64>,
    pub acp_session_foreground_lease_renew_interval_secs: Option<u64>,
    pub acp_prompt_terminal_route_timeout_ms: Option<u64>,
    pub acp_session_idle_ttl_secs: Option<u64>,
    pub acp_adapter_connection_idle_ttl_secs: Option<u64>,
    pub acp_max_idle_session_runtimes: Option<usize>,
    pub acp_max_idle_adapter_connections: Option<usize>,
    pub acp_timeline_compact_max_size_bytes: Option<u64>,
    pub acp_timeline_compact_patch_ratio: Option<usize>,
    pub conversation_auto_title_max_chars: Option<usize>,
    pub notification_auto_dismiss_target_secs: Option<u64>,
    pub require_local_claude_executable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_layout: Option<WorkspaceLayoutConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_files: Option<WorkspaceFilesConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_files: Option<TurnFilesConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFilesConfig {
    pub card_preview_limit: usize,
    pub capture_max_entries: usize,
    pub capture_max_file_bytes: usize,
    pub capture_max_total_bytes: usize,
    pub diff_text_max_bytes: usize,
    pub diff_text_max_lines: usize,
    pub blob_cache_max_bytes: usize,
    pub blob_retention_policy: TurnFileBlobRetentionPolicy,
}

impl Default for TurnFilesConfig {
    fn default() -> Self {
        Self {
            card_preview_limit: 3,
            capture_max_entries: 256,
            capture_max_file_bytes: 2 * 1024 * 1024,
            capture_max_total_bytes: 16 * 1024 * 1024,
            diff_text_max_bytes: 2 * 1024 * 1024,
            diff_text_max_lines: 100_000,
            blob_cache_max_bytes: 0,
            blob_retention_policy: TurnFileBlobRetentionPolicy::Attempt,
        }
    }
}

impl TurnFilesConfig {
    fn normalized(self) -> Self {
        Self {
            card_preview_limit: self.card_preview_limit.max(1),
            capture_max_entries: self.capture_max_entries.max(1),
            capture_max_file_bytes: self.capture_max_file_bytes.max(1),
            capture_max_total_bytes: self
                .capture_max_total_bytes
                .max(self.capture_max_file_bytes),
            diff_text_max_bytes: self.diff_text_max_bytes.max(1),
            diff_text_max_lines: self.diff_text_max_lines.max(1),
            blob_cache_max_bytes: self.blob_cache_max_bytes,
            blob_retention_policy: self.blob_retention_policy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnFileBlobRetentionPolicy {
    Attempt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutProfileConfig {
    pub center_min_width: u32,
    pub center_auto_collapse_width: u32,
    pub window_min_width: u32,
}

impl WorkspaceLayoutProfileConfig {
    fn normalized(self, shell_min_width: u32) -> Self {
        let center_min_width = self.center_min_width.max(1);
        Self {
            center_min_width,
            center_auto_collapse_width: self.center_auto_collapse_width.max(center_min_width),
            window_min_width: self
                .window_min_width
                .max(center_min_width)
                .max(shell_min_width),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutConfig {
    pub shell_min_width: u32,
    pub shell_min_height: u32,
    pub right_workspace: RightWorkspaceLayoutConfig,
    pub conversation: WorkspaceLayoutProfileConfig,
    pub context_cards: WorkspaceLayoutProfileConfig,
    pub workflow_canvas: WorkspaceLayoutProfileConfig,
    pub settings: WorkspaceLayoutProfileConfig,
}

impl WorkspaceLayoutConfig {
    fn normalized(mut self) -> Self {
        self.shell_min_width = self.shell_min_width.max(1);
        self.shell_min_height = self.shell_min_height.max(1);
        self.right_workspace = self.right_workspace.normalized();
        self.conversation = self.conversation.normalized(self.shell_min_width);
        self.context_cards = self.context_cards.normalized(self.shell_min_width);
        self.workflow_canvas = self.workflow_canvas.normalized(self.shell_min_width);
        self.settings = self.settings.normalized(self.shell_min_width);
        self
    }
}

impl Default for WorkspaceLayoutConfig {
    fn default() -> Self {
        Self {
            shell_min_width: 480,
            shell_min_height: 680,
            right_workspace: RightWorkspaceLayoutConfig::default(),
            conversation: WorkspaceLayoutProfileConfig {
                center_min_width: 360,
                center_auto_collapse_width: 420,
                window_min_width: 480,
            },
            context_cards: WorkspaceLayoutProfileConfig {
                center_min_width: 520,
                center_auto_collapse_width: 520,
                window_min_width: 520,
            },
            workflow_canvas: WorkspaceLayoutProfileConfig {
                center_min_width: 640,
                center_auto_collapse_width: 640,
                window_min_width: 640,
            },
            settings: WorkspaceLayoutProfileConfig {
                center_min_width: 480,
                center_auto_collapse_width: 480,
                window_min_width: 480,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileWorkspaceLayoutConfig {
    pub split_min_width: u32,
    pub tree_default_width: u32,
    pub tree_min_width: u32,
    pub tree_max_width: u32,
}

impl FileWorkspaceLayoutConfig {
    fn normalized(mut self, right_min_width: u32, right_max_width: u32) -> Self {
        self.split_min_width = self.split_min_width.clamp(right_min_width, right_max_width);
        self.tree_min_width = self.tree_min_width.max(1).min(right_max_width);
        self.tree_max_width = self
            .tree_max_width
            .max(self.tree_min_width)
            .min(right_max_width);
        self.tree_default_width = self
            .tree_default_width
            .clamp(self.tree_min_width, self.tree_max_width);
        self
    }
}

impl Default for FileWorkspaceLayoutConfig {
    fn default() -> Self {
        Self {
            split_min_width: 500,
            tree_default_width: 280,
            tree_min_width: 220,
            tree_max_width: 420,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightWorkspaceLayoutConfig {
    pub min_width: u32,
    pub default_width: u32,
    pub max_width: u32,
    pub file: FileWorkspaceLayoutConfig,
}

impl RightWorkspaceLayoutConfig {
    fn normalized(mut self) -> Self {
        self.min_width = self.min_width.max(1);
        self.max_width = self.max_width.max(self.min_width);
        self.default_width = self.default_width.clamp(self.min_width, self.max_width);
        self.file = self.file.normalized(self.min_width, self.max_width);
        self
    }
}

impl Default for RightWorkspaceLayoutConfig {
    fn default() -> Self {
        Self {
            min_width: 288,
            default_width: 440,
            max_width: 1440,
            file: FileWorkspaceLayoutConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceFilesConfig {
    pub auto_save_delay_ms: u64,
    pub search_debounce_ms: u64,
    pub search_result_limit: usize,
    pub text_editable_max_bytes: u64,
    pub text_highlight_max_chars: usize,
    pub text_read_only_max_bytes: u64,
    pub image_preview_max_bytes: u64,
    pub image_preview_max_pixels: u64,
    pub content_cache_entries: usize,
    pub content_cache_max_bytes: u64,
    pub watch_debounce_ms: u64,
    pub preview_token_ttl_seconds: u64,
    pub external_access_grant_ttl_seconds: u64,
    pub markdown_live_preview_max_chars: usize,
    pub markdown_embedded_image_limit: usize,
    pub markdown_embedded_image_max_concurrent: usize,
}

impl WorkspaceFilesConfig {
    fn normalized(mut self) -> Self {
        self.auto_save_delay_ms = self.auto_save_delay_ms.max(1);
        self.search_debounce_ms = self.search_debounce_ms.max(1);
        self.search_result_limit = self.search_result_limit.max(1);
        self.text_editable_max_bytes = self.text_editable_max_bytes.max(1);
        self.text_highlight_max_chars = self.text_highlight_max_chars.max(1);
        self.text_read_only_max_bytes = self
            .text_read_only_max_bytes
            .max(self.text_editable_max_bytes);
        self.image_preview_max_bytes = self.image_preview_max_bytes.max(1);
        self.image_preview_max_pixels = self.image_preview_max_pixels.max(1);
        self.content_cache_entries = self.content_cache_entries.max(1);
        self.content_cache_max_bytes = self.content_cache_max_bytes.max(1);
        self.watch_debounce_ms = self.watch_debounce_ms.max(1);
        self.preview_token_ttl_seconds = self.preview_token_ttl_seconds.max(1);
        self.external_access_grant_ttl_seconds = self.external_access_grant_ttl_seconds.max(1);
        self.markdown_live_preview_max_chars = self.markdown_live_preview_max_chars.max(1);
        self.markdown_embedded_image_limit = self.markdown_embedded_image_limit.max(1);
        self.markdown_embedded_image_max_concurrent =
            self.markdown_embedded_image_max_concurrent.max(1);
        self
    }
}

impl Default for WorkspaceFilesConfig {
    fn default() -> Self {
        Self {
            auto_save_delay_ms: 300,
            search_debounce_ms: 200,
            search_result_limit: 500,
            text_editable_max_bytes: 2 * 1024 * 1024,
            text_highlight_max_chars: 120_000,
            text_read_only_max_bytes: 10 * 1024 * 1024,
            image_preview_max_bytes: 20 * 1024 * 1024,
            image_preview_max_pixels: 40_000_000,
            content_cache_entries: 12,
            content_cache_max_bytes: 16 * 1024 * 1024,
            watch_debounce_ms: 150,
            preview_token_ttl_seconds: 300,
            external_access_grant_ttl_seconds: 1_800,
            markdown_live_preview_max_chars: 200_000,
            markdown_embedded_image_limit: 100,
            markdown_embedded_image_max_concurrent: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDiagnosticSnapshot {
    pub available: bool,
    pub reason: Option<String>,
    pub checked_at: String,
    pub capabilities: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub log_level: RuntimeLogLevel,
    pub log_prompts: bool,
    pub log_provider_command: bool,
    pub log_retention_days: u64,
    pub console_theme: ConsoleThemeName,
    pub appearance: AppearancePreference,
    pub personalization: PersonalizationPreference,
    pub desktop_language: DesktopLanguage,
    pub desktop_updater_url_override: Option<String>,
    pub desktop_updater_last_checked_at: Option<String>,
    pub desktop_update_badges: DesktopUpdateBadgeState,
    pub desktop_available_update: Option<DesktopAvailableUpdate>,
    pub agents: BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    pub use_local_claude: bool,
    pub require_local_claude_executable: bool,
    pub desktop_metrics_enabled: bool,
    pub desktop_metrics_base_url: Option<String>,
    pub desktop_metrics_api_key: Option<String>,
    pub acp_session_title_refresh_enabled: bool,
    pub acp_chat_event_page_size: usize,
    pub acp_raw_max_size_bytes: u64,
    pub acp_raw_target_size_bytes: u64,
    pub acp_session_foreground_lease_ttl_secs: u64,
    pub acp_session_foreground_lease_renew_interval_secs: u64,
    pub acp_prompt_terminal_route_timeout_ms: u64,
    pub acp_session_idle_ttl_secs: u64,
    pub acp_adapter_connection_idle_ttl_secs: u64,
    pub acp_max_idle_session_runtimes: usize,
    pub acp_max_idle_adapter_connections: usize,
    pub acp_timeline_compact_max_size_bytes: u64,
    pub acp_timeline_compact_patch_ratio: usize,
    pub conversation_auto_title_max_chars: usize,
    pub notification_auto_dismiss_target_secs: u64,
    pub scheduled_keep_awake_enabled: bool,
    pub scheduled_completion_notifications_enabled: bool,
    pub scheduled_occurrence_retention_days: u16,
    pub permission_mode_mapping: BTreeMap<String, BTreeMap<String, String>>,
    pub workspace_layout: WorkspaceLayoutConfig,
    pub workspace_files: WorkspaceFilesConfig,
    pub turn_files: TurnFilesConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_diagnostics: BTreeMap<String, ProviderDiagnosticSnapshot>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        let claude_id = ManagedAgentId::from_str("claude-acp").expect("valid Claude Agent id");
        agents.insert(
            claude_id,
            catalog_agent_default_config("claude-acp")
                .expect("Claude is present in the built-in Agent catalog"),
        );
        let base = Self {
            log_level: RuntimeLogLevel::Info,
            log_prompts: true,
            log_provider_command: true,
            log_retention_days: 30,
            console_theme: ConsoleThemeName::GoldBand,
            appearance: AppearancePreference::default(),
            personalization: PersonalizationPreference::default(),
            desktop_language: DesktopLanguage::ZhCn,
            desktop_updater_url_override: None,
            desktop_updater_last_checked_at: None,
            desktop_update_badges: DesktopUpdateBadgeState::default(),
            desktop_available_update: None,
            agents,
            use_local_claude: USE_LOCAL_CLAUDE,
            require_local_claude_executable: false,
            desktop_metrics_enabled: false,
            desktop_metrics_base_url: None,
            desktop_metrics_api_key: None,
            acp_session_title_refresh_enabled: false,
            acp_chat_event_page_size: 360,
            acp_raw_max_size_bytes: 5 * 1024 * 1024,
            acp_raw_target_size_bytes: 4 * 1024 * 1024,
            acp_session_foreground_lease_ttl_secs: 90,
            acp_session_foreground_lease_renew_interval_secs: 30,
            acp_prompt_terminal_route_timeout_ms: 5_000,
            acp_session_idle_ttl_secs: 600,
            acp_adapter_connection_idle_ttl_secs: 600,
            acp_max_idle_session_runtimes: 8,
            acp_max_idle_adapter_connections: 4,
            acp_timeline_compact_max_size_bytes: 8 * 1024 * 1024,
            acp_timeline_compact_patch_ratio: 4,
            conversation_auto_title_max_chars: DEFAULT_CONVERSATION_AUTO_TITLE_MAX_CHARS,
            notification_auto_dismiss_target_secs: DEFAULT_NOTIFICATION_AUTO_DISMISS_TARGET_SECS,
            scheduled_keep_awake_enabled: false,
            scheduled_completion_notifications_enabled: true,
            scheduled_occurrence_retention_days: DEFAULT_SCHEDULED_OCCURRENCE_RETENTION_DAYS,
            permission_mode_mapping: BTreeMap::new(),
            workspace_layout: WorkspaceLayoutConfig::default(),
            workspace_files: WorkspaceFilesConfig::default(),
            turn_files: TurnFilesConfig::default(),
            provider_diagnostics: BTreeMap::new(),
        };
        base.apply_app_config(embedded_project_app_config())
    }
}

impl RuntimeConfig {
    pub fn apply_settings(mut self, settings: &SettingsConfig) -> Self {
        if let Some(log_level) = settings.log_level {
            self.log_level = log_level;
        }
        if let Some(log_prompts) = settings.log_prompts {
            self.log_prompts = log_prompts;
        }
        if let Some(log_provider_command) = settings.log_provider_command {
            self.log_provider_command = log_provider_command;
        }
        if let Some(log_retention_days) = settings.log_retention_days {
            self.log_retention_days = log_retention_days;
        }
        if let Some(console_theme) = settings.console_theme {
            self.console_theme = console_theme;
        }
        if let Some(appearance) = &settings.appearance {
            self.appearance = appearance.clone();
        }
        if let Some(personalization) = &settings.personalization {
            self.personalization = personalization.clone();
        }
        if let Some(desktop_language) = settings.desktop_language {
            self.desktop_language = desktop_language;
        }
        self.desktop_updater_url_override = settings.desktop_updater_url_override.clone();
        if let Some(agents) = &settings.agents {
            self.agents = agents.clone();
        }
        self.use_local_claude = USE_LOCAL_CLAUDE;
        if let Some(desktop_metrics_enabled) = settings.desktop_metrics_enabled {
            self.desktop_metrics_enabled = desktop_metrics_enabled;
        }
        self.desktop_metrics_base_url = settings.desktop_metrics_base_url.clone();
        self.desktop_metrics_api_key = settings.desktop_metrics_api_key.clone();
        if let Some(scheduled_keep_awake_enabled) = settings.scheduled_keep_awake_enabled {
            self.scheduled_keep_awake_enabled = scheduled_keep_awake_enabled;
        }
        if let Some(scheduled_completion_notifications_enabled) =
            settings.scheduled_completion_notifications_enabled
        {
            self.scheduled_completion_notifications_enabled =
                scheduled_completion_notifications_enabled;
        }
        if let Some(scheduled_occurrence_retention_days) =
            settings.scheduled_occurrence_retention_days
        {
            self.scheduled_occurrence_retention_days = scheduled_occurrence_retention_days;
        }
        self
    }

    pub fn apply_app_config(mut self, app_config: &ProjectAppConfig) -> Self {
        if let Some(acp_session_title_refresh_enabled) =
            app_config.acp_session_title_refresh_enabled
        {
            self.acp_session_title_refresh_enabled = acp_session_title_refresh_enabled;
        }
        if let Some(acp_chat_event_page_size) = app_config.acp_chat_event_page_size {
            self.acp_chat_event_page_size = acp_chat_event_page_size;
        }
        if let Some(acp_raw_max_size_bytes) = app_config.acp_raw_max_size_bytes {
            self.acp_raw_max_size_bytes = acp_raw_max_size_bytes;
        }
        if let Some(acp_raw_target_size_bytes) = app_config.acp_raw_target_size_bytes {
            self.acp_raw_target_size_bytes = acp_raw_target_size_bytes;
        }
        if let Some(value) = app_config
            .acp_session_foreground_lease_ttl_secs
            .filter(|value| *value > 0)
        {
            self.acp_session_foreground_lease_ttl_secs = value;
        }
        if let Some(value) = app_config
            .acp_session_foreground_lease_renew_interval_secs
            .filter(|value| *value > 0)
        {
            self.acp_session_foreground_lease_renew_interval_secs = value;
        }
        if self.acp_session_foreground_lease_renew_interval_secs
            >= self.acp_session_foreground_lease_ttl_secs
        {
            self.acp_session_foreground_lease_renew_interval_secs =
                (self.acp_session_foreground_lease_ttl_secs / 3).max(1);
        }
        if let Some(value) = app_config
            .acp_prompt_terminal_route_timeout_ms
            .filter(|value| *value > 0)
        {
            self.acp_prompt_terminal_route_timeout_ms = value;
        }
        if let Some(value) = app_config
            .acp_session_idle_ttl_secs
            .filter(|value| *value > 0)
        {
            self.acp_session_idle_ttl_secs = value;
        }
        if let Some(value) = app_config
            .acp_adapter_connection_idle_ttl_secs
            .filter(|value| *value > 0)
        {
            self.acp_adapter_connection_idle_ttl_secs = value;
        }
        if let Some(value) = app_config
            .acp_max_idle_session_runtimes
            .filter(|value| *value > 0)
        {
            self.acp_max_idle_session_runtimes = value;
        }
        if let Some(value) = app_config
            .acp_max_idle_adapter_connections
            .filter(|value| *value > 0)
        {
            self.acp_max_idle_adapter_connections = value;
        }
        if let Some(value) = app_config
            .acp_timeline_compact_max_size_bytes
            .filter(|value| *value > 0)
        {
            self.acp_timeline_compact_max_size_bytes = value;
        }
        if let Some(value) = app_config
            .acp_timeline_compact_patch_ratio
            .filter(|value| *value > 0)
        {
            self.acp_timeline_compact_patch_ratio = value;
        }
        if let Some(conversation_auto_title_max_chars) = app_config
            .conversation_auto_title_max_chars
            .filter(|value| *value > 0)
        {
            self.conversation_auto_title_max_chars = conversation_auto_title_max_chars;
        }
        if let Some(notification_auto_dismiss_target_secs) = app_config
            .notification_auto_dismiss_target_secs
            .filter(|value| *value > 0)
        {
            self.notification_auto_dismiss_target_secs = notification_auto_dismiss_target_secs;
        }
        if let Some(require_local_claude_executable) = app_config.require_local_claude_executable {
            self.require_local_claude_executable = require_local_claude_executable;
        }
        if let Some(workspace_layout) = &app_config.workspace_layout {
            self.workspace_layout = workspace_layout.clone().normalized();
        }
        if let Some(workspace_files) = &app_config.workspace_files {
            self.workspace_files = workspace_files.clone().normalized();
        }
        if let Some(turn_files) = app_config.turn_files {
            self.turn_files = turn_files.normalized();
        }
        self
    }

    pub fn apply_state(mut self, state: &StateConfig) -> Self {
        self.desktop_updater_last_checked_at = state.desktop_updater_last_checked_at.clone();
        self.desktop_update_badges = state.desktop_update_badges.clone();
        self.desktop_available_update = state.desktop_available_update.clone();
        self
    }

    pub fn with_provider_diagnostics(
        mut self,
        provider_diagnostics: BTreeMap<String, ProviderDiagnosticSnapshot>,
    ) -> Self {
        self.provider_diagnostics = provider_diagnostics;
        self
    }

    /// Resolve a normative permission mode to the provider-specific ACP mode.
    /// Providers without a configured mapping keep the normative identifier.
    pub fn resolve_permission_mode(&self, provider: &str, normative_mode: &str) -> String {
        self.permission_mode_mapping
            .get(provider)
            .and_then(|mapping| mapping.get(normative_mode))
            .cloned()
            .unwrap_or_else(|| normative_mode.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AcpAdapterConfig, AppearancePreference, ColorSchemePreference, ConsoleThemeName,
        ConversationDirectConfig, ConversationRunMode, ConversationRunModeEntry,
        DesktopAvailableUpdate, DesktopLanguage, DesktopUpdateBadgeState, FontSizePreference,
        FontStackPreference, ManagedAgentConfig, ManagedAgentId, PersonalizationPreference,
        ProjectAppConfig, RuntimeConfig, RuntimeLogLevel, SettingsConfig, StateConfig,
        SystemPromptDelivery, TurnFilesConfig, VisualQuality, WorkspaceLayoutConfig,
        catalog_agent_default_config,
    };
    use crate::agent_catalog::builtin_agent_catalog;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn custom_personalization(
        ui_family: &str,
        editor_family: &str,
        ui_size: u8,
        editor_size: u8,
    ) -> PersonalizationPreference {
        let mut personalization = PersonalizationPreference::default();
        personalization.typography.ui.font_stack = FontStackPreference::Custom {
            families: vec![ui_family.to_string()],
        };
        personalization.typography.ui.font_size = FontSizePreference::Custom { px: ui_size };
        personalization.typography.editor.font_stack = FontStackPreference::Custom {
            families: vec![editor_family.to_string()],
        };
        personalization.typography.editor.font_size =
            FontSizePreference::Custom { px: editor_size };
        personalization
    }

    #[test]
    fn parses_console_theme_names() {
        assert!(matches!(
            ConsoleThemeName::from_str("gold-band").unwrap(),
            ConsoleThemeName::GoldBand
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("nord").unwrap(),
            ConsoleThemeName::Nord
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("dracula").unwrap(),
            ConsoleThemeName::Dracula
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("cyber").unwrap(),
            ConsoleThemeName::Cyber
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("onyx").unwrap(),
            ConsoleThemeName::Onyx
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("mist").unwrap(),
            ConsoleThemeName::Mist
        ));
        assert!(matches!(
            ConsoleThemeName::from_str("high-contrast").unwrap(),
            ConsoleThemeName::HighContrast
        ));
    }

    #[test]
    fn parses_desktop_preferences() {
        assert!(matches!(
            serde_json::from_str::<ColorSchemePreference>("\"light\"").unwrap(),
            ColorSchemePreference::Light
        ));
        assert!(matches!(
            serde_json::from_str::<ColorSchemePreference>("\"dark\"").unwrap(),
            ColorSchemePreference::Dark
        ));
        assert!(matches!(
            serde_json::from_str::<ColorSchemePreference>("\"system\"").unwrap(),
            ColorSchemePreference::System
        ));
        assert!(matches!(
            serde_json::from_str::<VisualQuality>("\"performance\"").unwrap(),
            VisualQuality::Performance
        ));
        assert!(serde_json::from_str::<ColorSchemePreference>("\"light-gray\"").is_err());
        assert!(matches!(
            DesktopLanguage::from_str("zh-cn").unwrap(),
            DesktopLanguage::ZhCn
        ));
        assert!(matches!(
            DesktopLanguage::from_str("en").unwrap(),
            DesktopLanguage::En
        ));
    }

    #[test]
    fn defaults_console_theme_to_gold_band() {
        let config = RuntimeConfig::default();
        assert!(matches!(config.console_theme, ConsoleThemeName::GoldBand));
        assert_eq!(config.appearance, AppearancePreference::default());
        assert!(matches!(config.desktop_language, DesktopLanguage::ZhCn));
        assert_eq!(config.personalization, PersonalizationPreference::default());
    }

    #[test]
    fn legacy_desktop_typography_preferences_are_bounded_during_migration() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 6,
                "desktopUiFontSize": 3,
                "desktopEditorFontSize": 99
            }))
            .unwrap();

        assert!(migrated);
        let personalization = settings.personalization.unwrap();
        assert_eq!(
            personalization.typography.ui.font_size,
            FontSizePreference::Custom { px: 12 }
        );
        assert_eq!(
            personalization.typography.editor.font_size,
            FontSizePreference::Custom { px: 18 }
        );
    }

    #[test]
    fn font_stack_normalization_is_ordered_bounded_and_case_insensitive() {
        let normalized = FontStackPreference::Custom {
            families: vec![
                " Segoe UI ".to_string(),
                "segoe ui".to_string(),
                "Gold Band MiSans".to_string(),
                "bad,font".to_string(),
            ],
        }
        .normalized();
        assert_eq!(
            normalized,
            FontStackPreference::Custom {
                families: vec!["Segoe UI".to_string(), "Gold Band MiSans".to_string()],
            }
        );
        assert_eq!(
            FontStackPreference::Custom { families: vec![] }.normalized(),
            FontStackPreference::Theme
        );
    }

    #[test]
    fn settings_v8_replaces_single_font_fields_without_dual_reading() {
        let defaults = PersonalizationPreference::default();
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 7,
                "personalization": {
                    "schemaVersion": 1,
                    "typography": {
                        "ui": {
                            "font": { "source": "local", "family": "Segoe UI" },
                            "fontSize": { "source": "custom", "px": 15 }
                        },
                        "editor": {
                            "font": { "source": "local", "family": "Fira Code" },
                            "fontSize": { "source": "theme" }
                        }
                    },
                    "avatars": serde_json::to_value(defaults.avatars).unwrap()
                }
            }))
            .unwrap();

        assert!(migrated);
        let personalization = settings.personalization.unwrap();
        assert_eq!(personalization.schema_version, 2);
        assert_eq!(
            personalization.typography.ui.font_stack,
            FontStackPreference::Theme
        );
        assert_eq!(
            personalization.typography.editor.font_stack,
            FontStackPreference::Theme
        );
        assert_eq!(
            personalization.typography.ui.font_size,
            FontSizePreference::Custom { px: 15 }
        );
    }

    #[test]
    fn settings_config_roundtrips_json() {
        let settings = SettingsConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            appearance: Some(AppearancePreference {
                schema_version: 2,
                theme_id: "builtin.tech-neutral".to_string(),
                color_scheme: ColorSchemePreference::Dark,
                visual_quality_by_theme: BTreeMap::new(),
            }),
            personalization: Some(custom_personalization(
                "Microsoft YaHei UI",
                "Fira Code",
                16,
                13,
            )),
            desktop_language: Some(DesktopLanguage::En),
            desktop_updater_url_override: Some("https://updates.example/latest.json".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            ..SettingsConfig::default()
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let roundtripped: SettingsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.console_theme, Some(ConsoleThemeName::Nord));
        assert_eq!(roundtripped.appearance, settings.appearance);
        assert_eq!(roundtripped.personalization, settings.personalization);
        assert_eq!(roundtripped.desktop_language, Some(DesktopLanguage::En));
        assert!(matches!(
            roundtripped.log_level,
            Some(RuntimeLogLevel::Trace)
        ));
    }

    #[test]
    fn state_config_roundtrips_json() {
        let state = StateConfig {
            state_schema_version: 1,
            desktop_update_badges: DesktopUpdateBadgeState {
                settings_entry_seen_version: Some("1.2.3".to_string()),
                settings_advanced_seen_version: Some("1.2.3".to_string()),
                announcement_closed_version: Some("1.2.2".to_string()),
            },
            desktop_available_update: Some(DesktopAvailableUpdate {
                version: "1.2.3".to_string(),
                current_version: "1.2.2".to_string(),
                notes: Some("Patch release".to_string()),
                pub_date: Some("2026-05-27T00:00:00Z".to_string()),
            }),
            recent_desktop_workspaces: vec!["/path/a".to_string(), "/path/b".to_string()],
            ..StateConfig::default()
        };
        let json = serde_json::to_string_pretty(&state).unwrap();
        let roundtripped: StateConfig = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"stateSchemaVersion\": 1"));
        assert_eq!(roundtripped.state_schema_version, 1);
        assert_eq!(
            roundtripped
                .desktop_update_badges
                .settings_entry_seen_version
                .as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            roundtripped
                .desktop_available_update
                .as_ref()
                .map(|u| u.version.as_str()),
            Some("1.2.3")
        );
        assert_eq!(
            roundtripped.recent_desktop_workspaces,
            vec!["/path/a", "/path/b"]
        );
    }

    #[test]
    fn state_config_defaults_legacy_schema_version_and_omits_zero() {
        let legacy: StateConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.state_schema_version, 0);

        let json = serde_json::to_value(StateConfig::default()).unwrap();
        assert!(json.get("stateSchemaVersion").is_none());
    }

    #[test]
    fn apply_settings_overrides_defaults() {
        let config = RuntimeConfig::default().apply_settings(&SettingsConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            appearance: Some(AppearancePreference {
                schema_version: 2,
                theme_id: "builtin.tech-neutral".to_string(),
                color_scheme: ColorSchemePreference::Dark,
                visual_quality_by_theme: BTreeMap::new(),
            }),
            personalization: Some(custom_personalization(
                "Microsoft YaHei UI",
                "Fira Code",
                16,
                13,
            )),
            desktop_language: Some(DesktopLanguage::En),
            desktop_updater_url_override: Some("https://updates.example/latest.json".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            ..SettingsConfig::default()
        });
        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert_eq!(config.appearance.theme_id, "builtin.tech-neutral");
        assert_eq!(config.appearance.color_scheme, ColorSchemePreference::Dark);
        assert_eq!(config.desktop_language, DesktopLanguage::En);
        assert_eq!(
            config.personalization,
            custom_personalization("Microsoft YaHei UI", "Fira Code", 16, 13)
        );
        assert_eq!(
            config.desktop_updater_url_override.as_deref(),
            Some("https://updates.example/latest.json")
        );
        assert!(matches!(config.log_level, RuntimeLogLevel::Trace));
    }

    #[test]
    fn project_app_config_roundtrip_json() {
        let app_config = ProjectAppConfig {
            acp_session_title_refresh_enabled: Some(true),
            acp_chat_event_page_size: Some(240),
            conversation_auto_title_max_chars: Some(20),
            notification_auto_dismiss_target_secs: Some(20),
            require_local_claude_executable: Some(true),
            acp_session_idle_ttl_secs: Some(900),
            acp_prompt_terminal_route_timeout_ms: Some(2_500),
            acp_max_idle_session_runtimes: Some(12),
            acp_timeline_compact_patch_ratio: Some(6),
            turn_files: Some(TurnFilesConfig {
                card_preview_limit: 5,
                ..TurnFilesConfig::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&app_config).unwrap();
        let roundtripped: ProjectAppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.acp_session_title_refresh_enabled, Some(true));
        assert_eq!(roundtripped.acp_chat_event_page_size, Some(240));
        assert_eq!(roundtripped.conversation_auto_title_max_chars, Some(20));
        assert_eq!(roundtripped.notification_auto_dismiss_target_secs, Some(20));
        assert_eq!(roundtripped.require_local_claude_executable, Some(true));
        assert_eq!(roundtripped.acp_session_idle_ttl_secs, Some(900));
        assert_eq!(
            roundtripped.acp_prompt_terminal_route_timeout_ms,
            Some(2_500)
        );
        assert_eq!(roundtripped.acp_max_idle_session_runtimes, Some(12));
        assert_eq!(roundtripped.acp_timeline_compact_patch_ratio, Some(6));
        assert_eq!(roundtripped.turn_files.unwrap().card_preview_limit, 5);
    }

    #[test]
    fn apply_state_overrides_defaults() {
        let config = RuntimeConfig::default().apply_state(&StateConfig {
            desktop_updater_last_checked_at: Some("2026-05-27 10:00:00".to_string()),
            desktop_update_badges: DesktopUpdateBadgeState {
                settings_entry_seen_version: Some("1.2.3".to_string()),
                settings_advanced_seen_version: Some("1.2.3".to_string()),
                announcement_closed_version: Some("1.2.2".to_string()),
            },
            desktop_available_update: Some(DesktopAvailableUpdate {
                version: "1.2.3".to_string(),
                current_version: "1.2.2".to_string(),
                notes: Some("Patch release".to_string()),
                pub_date: Some("2026-05-27T00:00:00Z".to_string()),
            }),
            ..StateConfig::default()
        });
        assert_eq!(
            config.desktop_updater_last_checked_at.as_deref(),
            Some("2026-05-27 10:00:00")
        );
        assert_eq!(
            config
                .desktop_update_badges
                .settings_entry_seen_version
                .as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            config
                .desktop_available_update
                .as_ref()
                .map(|u| u.version.as_str()),
            Some("1.2.3")
        );
    }

    #[test]
    fn empty_settings_keeps_defaults() {
        let config = RuntimeConfig::default().apply_settings(&SettingsConfig::default());
        assert_eq!(config.console_theme, ConsoleThemeName::GoldBand);
        assert_eq!(config.appearance, AppearancePreference::default());
        assert_eq!(config.desktop_language, DesktopLanguage::ZhCn);
        assert_eq!(config.personalization, PersonalizationPreference::default());
        assert!(matches!(config.log_level, RuntimeLogLevel::Info));
    }

    #[test]
    fn apply_app_config_overrides_defaults() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            acp_session_title_refresh_enabled: Some(true),
            acp_chat_event_page_size: Some(240),
            conversation_auto_title_max_chars: Some(20),
            notification_auto_dismiss_target_secs: Some(12),
            require_local_claude_executable: Some(true),
            ..Default::default()
        });
        assert!(config.acp_session_title_refresh_enabled);
        assert_eq!(config.acp_chat_event_page_size, 240);
        assert_eq!(config.conversation_auto_title_max_chars, 20);
        assert_eq!(config.notification_auto_dismiss_target_secs, 12);
        assert!(config.require_local_claude_executable);
    }

    #[test]
    fn embedded_app_config_defines_page_window_layout_profiles() {
        let layout = RuntimeConfig::default().workspace_layout;

        assert_eq!(layout.shell_min_width, 480);
        assert_eq!(layout.shell_min_height, 680);
        assert_eq!(layout.right_workspace.min_width, 288);
        assert_eq!(layout.right_workspace.default_width, 440);
        assert_eq!(layout.right_workspace.max_width, 1440);
        assert_eq!(layout.conversation.center_min_width, 360);
        assert_eq!(layout.conversation.center_auto_collapse_width, 420);
        assert_eq!(layout.conversation.window_min_width, 480);
        assert_eq!(layout.context_cards.window_min_width, 520);
        assert_eq!(layout.workflow_canvas.window_min_width, 640);
        assert_eq!(layout.settings.window_min_width, 480);
    }

    #[test]
    fn app_config_normalizes_invalid_workspace_layout_thresholds() {
        let mut layout = WorkspaceLayoutConfig::default();
        layout.shell_min_width = 500;
        layout.shell_min_height = 0;
        layout.conversation.center_min_width = 420;
        layout.conversation.center_auto_collapse_width = 360;
        layout.conversation.window_min_width = 400;
        layout.right_workspace.min_width = 500;
        layout.right_workspace.default_width = 100;
        layout.right_workspace.max_width = 400;
        layout.right_workspace.file.tree_min_width = 300;
        layout.right_workspace.file.tree_max_width = 200;

        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            workspace_layout: Some(layout),
            ..Default::default()
        });

        assert_eq!(config.workspace_layout.shell_min_height, 1);
        assert_eq!(config.workspace_layout.conversation.center_min_width, 420);
        assert_eq!(
            config
                .workspace_layout
                .conversation
                .center_auto_collapse_width,
            420
        );
        assert_eq!(config.workspace_layout.conversation.window_min_width, 500);
        assert_eq!(config.workspace_layout.right_workspace.min_width, 500);
        assert_eq!(config.workspace_layout.right_workspace.default_width, 500);
        assert_eq!(config.workspace_layout.right_workspace.max_width, 500);
        assert_eq!(
            config.workspace_layout.right_workspace.file.tree_min_width,
            300
        );
        assert_eq!(
            config.workspace_layout.right_workspace.file.tree_max_width,
            300
        );
    }

    #[test]
    fn app_config_ignores_zero_notification_auto_dismiss_target() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            notification_auto_dismiss_target_secs: Some(0),
            ..Default::default()
        });

        assert_eq!(
            config.notification_auto_dismiss_target_secs,
            super::DEFAULT_NOTIFICATION_AUTO_DISMISS_TARGET_SECS
        );
    }

    #[test]
    fn app_config_ignores_zero_conversation_auto_title_limit() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            conversation_auto_title_max_chars: Some(0),
            ..Default::default()
        });

        assert_eq!(
            config.conversation_auto_title_max_chars,
            super::DEFAULT_CONVERSATION_AUTO_TITLE_MAX_CHARS
        );
    }

    #[test]
    fn empty_state_keeps_defaults() {
        let config = RuntimeConfig::default().apply_state(&StateConfig::default());
        assert!(config.desktop_updater_last_checked_at.is_none());
        assert!(config.desktop_available_update.is_none());
    }

    #[test]
    fn full_roundtrip_from_settings_and_state() {
        let settings = SettingsConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            appearance: Some(AppearancePreference {
                schema_version: 2,
                theme_id: "builtin.tech-neutral".to_string(),
                color_scheme: ColorSchemePreference::Dark,
                visual_quality_by_theme: BTreeMap::new(),
            }),
            personalization: Some(custom_personalization("Fira Code", "Iosevka", 15, 13)),
            desktop_language: Some(DesktopLanguage::En),
            desktop_updater_url_override: Some("https://updates.example/latest.json".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            use_local_claude: Some(true),
            ..SettingsConfig::default()
        };
        let state = StateConfig {
            desktop_updater_last_checked_at: Some("2026-05-27 10:00:00".to_string()),
            desktop_update_badges: DesktopUpdateBadgeState {
                settings_entry_seen_version: Some("1.2.3".to_string()),
                settings_advanced_seen_version: Some("1.2.3".to_string()),
                announcement_closed_version: Some("1.2.2".to_string()),
            },
            desktop_available_update: Some(DesktopAvailableUpdate {
                version: "1.2.3".to_string(),
                current_version: "1.2.2".to_string(),
                notes: Some("Patch release".to_string()),
                pub_date: Some("2026-05-27T00:00:00Z".to_string()),
            }),
            ..StateConfig::default()
        };
        let config = RuntimeConfig::default()
            .apply_settings(&settings)
            .apply_state(&state);
        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert_eq!(config.appearance.theme_id, "builtin.tech-neutral");
        assert_eq!(config.appearance.color_scheme, ColorSchemePreference::Dark);
        assert_eq!(config.desktop_language, DesktopLanguage::En);
        assert_eq!(
            config.personalization,
            custom_personalization("Fira Code", "Iosevka", 15, 13)
        );
        assert!(matches!(config.log_level, RuntimeLogLevel::Trace));
        assert!(!config.use_local_claude);
        assert_eq!(
            config.desktop_updater_url_override.as_deref(),
            Some("https://updates.example/latest.json")
        );
        assert_eq!(
            config.desktop_updater_last_checked_at.as_deref(),
            Some("2026-05-27 10:00:00")
        );
        assert_eq!(
            config
                .desktop_update_badges
                .settings_entry_seen_version
                .as_deref(),
            Some("1.2.3")
        );
        assert_eq!(
            config
                .desktop_available_update
                .as_ref()
                .map(|u| u.version.as_str()),
            Some("1.2.3")
        );
    }

    #[test]
    fn managed_agent_catalog_owns_default_agent_directories() {
        let defaults = builtin_agent_catalog()
            .agents
            .iter()
            .map(|entry| {
                (
                    entry.id.as_str(),
                    entry.primary_agent_dir.as_deref(),
                    entry.project_primary_agent_dir.as_deref(),
                    entry
                        .compatible_agent_dirs
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            defaults,
            vec![
                ("claude-acp", Some(".claude"), None, vec![]),
                ("codex-acp", Some(".codex"), None, vec![".agents"]),
                ("cursor", Some(".cursor"), None, vec![".agents"]),
                ("gemini", Some(".gemini"), None, vec![".agents"]),
                ("codebuddy-code", Some(".codebuddy"), None, vec![]),
                ("goose", Some(".goose"), None, vec![]),
                ("qwen-code", Some(".qwen"), None, vec![]),
                ("opencode", Some(".opencode"), None, vec![".agents"]),
                ("kimi", Some(".kimi-code"), None, vec![".agents"]),
                ("amp-acp", Some(".agents"), None, vec![".claude"]),
                ("pi-acp", Some(".pi/agent"), Some(".pi"), vec![".agents"]),
            ]
        );
    }

    #[test]
    fn legacy_settings_migrate_agent_directories_from_presets() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "agents": {
                    "claude-acp": {
                        "adapter": AcpAdapterConfig::default(),
                        "externalSessionSyncEnabled": false
                    },
                    "codex-acp": {
                        "adapter": catalog_agent_default_config("codex-acp").unwrap().adapter,
                        "externalSessionSyncEnabled": false
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        let agents = settings.agents.unwrap();
        let claude = &agents[&ManagedAgentId::from_str("claude-acp").unwrap()];
        assert_eq!(claude.primary_agent_dir.as_deref(), Some(".claude"));
        assert!(claude.compatible_agent_dirs.is_empty());
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(codex.primary_agent_dir.as_deref(), Some(".codex"));
        assert_eq!(codex.compatible_agent_dirs, vec![".agents"]);
    }

    #[test]
    fn settings_v1_migrates_legacy_codex_acp_package() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 1,
                "agents": {
                    "codex-acp": {
                        "adapter": {
                            "command": "npx",
                            "args": ["-y", "@zed-industries/codex-acp@0.16.0"],
                            "displayName": "Codex",
                            "env": {}
                        },
                        "primaryAgentDir": ".codex",
                        "compatibleAgentDirs": [".agents"],
                        "externalSessionSyncEnabled": false
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        let agents = settings.agents.unwrap();
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(
            codex.adapter.args,
            vec!["-y", "@agentclientprotocol/codex-acp@latest"]
        );
    }

    #[test]
    fn current_codex_catalog_entry_uses_agentclientprotocol_adapter() {
        let codex = catalog_agent_default_config("codex-acp").unwrap();

        assert_eq!(codex.adapter.command, "npx");
        assert_eq!(codex.adapter.args.first().map(String::as_str), Some("-y"));
        assert!(
            codex
                .adapter
                .args
                .iter()
                .any(|arg| arg.starts_with("@agentclientprotocol/codex-acp@"))
        );
        assert!(
            codex
                .adapter
                .args
                .iter()
                .all(|arg| !arg.starts_with("@zed-industries/codex-acp"))
        );
    }

    #[test]
    fn settings_v1_preserves_custom_codex_adapter_args() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 1,
                "agents": {
                    "codex-acp": {
                        "adapter": {
                            "command": "custom-codex-acp.exe",
                            "args": ["--stdio"],
                            "displayName": "Custom Codex",
                            "env": {}
                        },
                        "primaryAgentDir": ".codex",
                        "compatibleAgentDirs": [".agents"],
                        "externalSessionSyncEnabled": false
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        let agents = settings.agents.unwrap();
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(codex.adapter.command, "custom-codex-acp.exe");
        assert_eq!(codex.adapter.args, vec!["--stdio"]);
    }

    #[test]
    fn settings_v2_migrates_scheduled_runtime_defaults() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 2
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        assert_eq!(settings.scheduled_keep_awake_enabled, Some(false));
        assert_eq!(
            settings.scheduled_completion_notifications_enabled,
            Some(true)
        );
        assert_eq!(settings.scheduled_occurrence_retention_days, Some(30));
    }

    #[test]
    fn settings_v2_preserves_explicit_scheduled_runtime_values() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 2,
                "scheduledKeepAwakeEnabled": true,
                "scheduledCompletionNotificationsEnabled": false,
                "scheduledOccurrenceRetentionDays": 45
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(settings.scheduled_keep_awake_enabled, Some(true));
        assert_eq!(
            settings.scheduled_completion_notifications_enabled,
            Some(false)
        );
        assert_eq!(settings.scheduled_occurrence_retention_days, Some(45));
    }

    #[test]
    fn settings_v3_from_main_adds_scheduler_defaults_without_losing_agent_capabilities() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 3,
                "agents": {
                    "custom-agent": {
                        "adapter": AcpAdapterConfig::default(),
                        "icon": "custom-icon",
                        "systemPromptDelivery": "none",
                        "externalSessionSyncSupported": true,
                        "externalSessionSyncEnabled": true
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        assert_eq!(settings.scheduled_keep_awake_enabled, Some(false));
        assert_eq!(
            settings.scheduled_completion_notifications_enabled,
            Some(true)
        );
        let agents = settings.agents.unwrap();
        let custom = &agents[&ManagedAgentId::from_str("custom-agent").unwrap()];
        assert_eq!(custom.icon, "custom-icon");
        assert!(custom.external_session_sync_supported);
        assert!(custom.external_session_sync_enabled);
    }

    #[test]
    fn settings_v3_from_scheduler_adds_agent_capabilities_without_losing_schedule_values() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 3,
                "scheduledKeepAwakeEnabled": true,
                "scheduledCompletionNotificationsEnabled": false,
                "scheduledOccurrenceRetentionDays": 45,
                "agents": {
                    "claude-acp": {
                        "adapter": AcpAdapterConfig::default(),
                        "externalSessionSyncEnabled": false
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        assert_eq!(settings.scheduled_keep_awake_enabled, Some(true));
        assert_eq!(
            settings.scheduled_completion_notifications_enabled,
            Some(false)
        );
        assert_eq!(settings.scheduled_occurrence_retention_days, Some(45));
        let agents = settings.agents.unwrap();
        let claude = &agents[&ManagedAgentId::from_str("claude-acp").unwrap()];
        assert_eq!(claude.icon, "claude");
        assert_eq!(
            claude.system_prompt_delivery,
            SystemPromptDelivery::MetaAppend
        );
        assert!(!claude.external_session_sync_supported);
    }

    #[test]
    fn settings_v5_removes_legacy_desktop_workspace() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 5,
                "desktopWorkspace": "D:/Projects/legacy",
                "appearance": {
                    "schemaVersion": 2,
                    "themeId": "builtin.gold-band",
                    "colorScheme": "dark"
                }
            }))
            .unwrap();

        assert!(migrated);
        assert_eq!(
            settings.settings_schema_version.0,
            super::CURRENT_SETTINGS_SCHEMA_VERSION
        );
        let persisted = serde_json::to_value(settings).unwrap();
        assert!(persisted.get("desktopWorkspace").is_none());
        assert_eq!(
            persisted.pointer("/appearance/themeId"),
            Some(&serde_json::json!("builtin.gold-band"))
        );
    }

    #[test]
    fn runtime_config_applies_scheduled_runtime_settings() {
        let config = RuntimeConfig::default().apply_settings(&SettingsConfig {
            scheduled_keep_awake_enabled: Some(true),
            scheduled_completion_notifications_enabled: Some(false),
            scheduled_occurrence_retention_days: Some(90),
            ..SettingsConfig::default()
        });

        assert!(config.scheduled_keep_awake_enabled);
        assert!(!config.scheduled_completion_notifications_enabled);
        assert_eq!(config.scheduled_occurrence_retention_days, 90);
    }

    #[test]
    fn legacy_skill_directory_override_becomes_primary_agent_directory() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "agents": {
                    "codex-cli": {
                        "adapter": catalog_agent_default_config("codex-acp").unwrap().adapter,
                        "skillsDirOverride": "  .custom-codex  "
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        let agents = settings.agents.unwrap();
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(codex.primary_agent_dir.as_deref(), Some(".custom-codex"));
        assert_eq!(codex.compatible_agent_dirs, vec![".agents"]);
        let serialized = serde_json::to_value(codex).unwrap();
        assert!(serialized.get("skillsDirOverride").is_none());
    }

    #[test]
    fn settings_v2_migrates_instance_capabilities_without_catalog_linkage() {
        let (settings, migrated) =
            SettingsConfig::from_json_value_with_migration(serde_json::json!({
                "settingsSchemaVersion": 2,
                "agents": {
                    "claude-acp": {
                        "adapter": {
                            "command": "custom-claude-acp",
                            "args": ["--stdio"],
                            "displayName": "My Claude",
                            "env": {}
                        },
                        "primaryAgentDir": ".custom-claude",
                        "compatibleAgentDirs": [],
                        "externalSessionSyncEnabled": false
                    },
                    "private-agent": {
                        "adapter": {
                            "command": "private-agent",
                            "args": [],
                            "displayName": "Private Agent",
                            "env": {}
                        },
                        "primaryAgentDir": null,
                        "compatibleAgentDirs": [],
                        "externalSessionSyncEnabled": true
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        let agents = settings.agents.unwrap();
        let claude = &agents[&ManagedAgentId::from_str("claude-acp").unwrap()];
        assert_eq!(claude.adapter.command, "custom-claude-acp");
        assert_eq!(claude.icon, "claude");
        assert!(claude.supports_system_prompt());

        let custom = &agents[&ManagedAgentId::from_str("private-agent").unwrap()];
        assert_eq!(custom.icon, "agent");
        assert!(custom.primary_agent_dir.is_none());
        assert!(!custom.supports_system_prompt());
        assert!(custom.external_session_sync_supported);
        assert!(custom.external_session_sync_enabled);
    }

    #[test]
    fn managed_agent_external_session_sync_defaults_off_and_roundtrips() {
        let mut agents = BTreeMap::new();
        let mut agent = ManagedAgentConfig::new(
            AcpAdapterConfig::default(),
            ".custom-agent",
            vec![".agents".to_string()],
        );
        agent.external_session_sync_enabled = true;
        let agent_id = ManagedAgentId::from_str("claude-acp").unwrap();
        agents.insert(agent_id.clone(), agent);
        let settings = SettingsConfig {
            agents: Some(agents),
            ..SettingsConfig::default()
        };

        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            value["agents"]["claude-acp"]["externalSessionSyncEnabled"],
            true
        );
        let roundtripped: SettingsConfig = serde_json::from_value(value).unwrap();
        let agent = &roundtripped.agents.unwrap()[&agent_id];
        assert!(agent.external_session_sync_enabled);
        assert_eq!(agent.primary_agent_dir.as_deref(), Some(".custom-agent"));
        assert_eq!(agent.compatible_agent_dirs, vec![".agents"]);
    }

    #[test]
    fn app_config_bounds_acp_runtime_policy_values() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            acp_session_foreground_lease_ttl_secs: Some(60),
            acp_session_foreground_lease_renew_interval_secs: Some(90),
            acp_session_idle_ttl_secs: Some(0),
            acp_prompt_terminal_route_timeout_ms: Some(0),
            acp_max_idle_session_runtimes: Some(0),
            acp_timeline_compact_patch_ratio: Some(0),
            ..Default::default()
        });
        assert_eq!(config.acp_session_foreground_lease_ttl_secs, 60);
        assert_eq!(config.acp_session_foreground_lease_renew_interval_secs, 20);
        assert_eq!(config.acp_session_idle_ttl_secs, 600);
        assert_eq!(config.acp_prompt_terminal_route_timeout_ms, 5_000);
        assert_eq!(config.acp_max_idle_session_runtimes, 8);
        assert_eq!(config.acp_timeline_compact_patch_ratio, 4);
    }

    #[test]
    fn skill_directory_policy_separates_write_and_compatible_read_dirs() {
        for entry in &builtin_agent_catalog().agents {
            let config = ManagedAgentConfig::from_catalog(entry);
            let policy = config.skill_directory_policy();
            let mut expected_global_reads = Vec::new();
            if let Some(primary) = &entry.primary_agent_dir {
                assert_eq!(policy.global.write_dir_names, vec![primary.clone()]);
                expected_global_reads.push(primary.clone());
            } else {
                assert!(policy.global.write_dir_names.is_empty());
            }
            expected_global_reads.extend(entry.compatible_agent_dirs.iter().cloned());
            assert_eq!(policy.global.read_dir_names, expected_global_reads);

            let project_primary = entry
                .project_primary_agent_dir
                .as_ref()
                .or(entry.primary_agent_dir.as_ref());
            let mut expected_project_reads = Vec::new();
            if let Some(primary) = project_primary {
                assert_eq!(policy.project.write_dir_names, vec![primary.clone()]);
                expected_project_reads.push(primary.clone());
            } else {
                assert!(policy.project.write_dir_names.is_empty());
            }
            expected_project_reads.extend(entry.compatible_agent_dirs.iter().cloned());
            assert_eq!(policy.project.read_dir_names, expected_project_reads);
        }
    }

    #[test]
    fn skill_directory_policy_allows_agents_without_skill_directories() {
        let mut config = ManagedAgentConfig::new(AcpAdapterConfig::default(), "unused", Vec::new());
        config.primary_agent_dir = None;

        let policy = config.skill_directory_policy();

        assert!(policy.global.write_dir_names.is_empty());
        assert!(policy.global.read_dir_names.is_empty());
        assert!(policy.project.write_dir_names.is_empty());
        assert!(policy.project.read_dir_names.is_empty());
    }

    #[test]
    fn skill_directory_policy_deduplicates_primary_and_compatible_directories() {
        let config = ManagedAgentConfig::new(
            AcpAdapterConfig::default(),
            "custom-codex",
            vec!["custom-codex".to_string(), ".agents".to_string()],
        );
        let policy = config.skill_directory_policy();
        assert_eq!(policy.global.write_dir_names, vec!["custom-codex"]);
        assert_eq!(
            policy.global.read_dir_names,
            vec!["custom-codex", ".agents"]
        );
        assert_eq!(policy.project, policy.global);
    }

    #[test]
    fn direct_preferences_roundtrip_independently_by_workspace_and_agent() {
        let mut state = StateConfig::default();
        for (workspace, model, permission) in [
            ("workspace-a", "sonnet", "ask"),
            ("workspace-b", "opus", "bypassPermissions"),
        ] {
            let config = ConversationDirectConfig {
                agent_type: "claude-acp".to_string(),
                model_id: Some(model.to_string()),
                permission_mode: Some(permission.to_string()),
                config_options: Default::default(),
            };
            state.conversation_run_modes.insert(
                workspace.to_string(),
                ConversationRunModeEntry {
                    mode: ConversationRunMode::Direct,
                    workflow_template_id: None,
                    optional_entry_preferences: Default::default(),
                    direct_config: Some(config.clone()),
                    direct_preferences: [("claude-acp".to_string(), config)].into(),
                    auto_config: None,
                },
            );
        }

        let json = serde_json::to_string_pretty(&state).unwrap();
        let roundtripped: StateConfig = serde_json::from_str(&json).unwrap();
        let workspace_a = roundtripped
            .conversation_run_modes
            .get("workspace-a")
            .unwrap();
        let workspace_b = roundtripped
            .conversation_run_modes
            .get("workspace-b")
            .unwrap();

        assert_eq!(
            workspace_a
                .direct_preferences
                .get("claude-acp")
                .and_then(|config| config.model_id.as_deref()),
            Some("sonnet")
        );
        assert_eq!(
            workspace_b
                .direct_preferences
                .get("claude-acp")
                .and_then(|config| config.permission_mode.as_deref()),
            Some("bypassPermissions")
        );
    }
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    BuiltIn,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProfileRef {
    pub name: String,
    pub display_name: String,
    pub source: ProfileSource,
    pub path: String,
}

// ── Conversation UI state ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopUiMode {
    Conversation,
    Workbench,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorkspaceEntry {
    pub project_id: String,
    pub workspace_path: String,
    pub name: String,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPin {
    pub project_id: String,
    pub task_id: String,
    pub order: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeEntry {
    pub mode: ConversationRunMode,
    pub workflow_template_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub optional_entry_preferences: std::collections::HashMap<String, bool>,
    pub direct_config: Option<ConversationDirectConfig>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub direct_preferences: std::collections::HashMap<String, ConversationDirectConfig>,
    pub auto_config: Option<ConversationAutoConfig>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationRunMode {
    Direct,
    Workflow,
    #[default]
    Auto,
}

impl ConversationRunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Workflow => "workflow",
            Self::Auto => "auto",
        }
    }

    pub fn is_orchestrated(self) -> bool {
        matches!(self, Self::Workflow | Self::Auto)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDirectConfig {
    pub agent_type: String,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAutoConfig {
    pub agent_strategy: Option<String>,
    pub agent_type: String,
    pub bootstrap_agent_type: Option<String>,
    pub bootstrap_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bootstrap_config_options: BTreeMap<String, String>,
    pub acceptance_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub acceptance_config_options: BTreeMap<String, String>,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
    pub available_agents: Option<Vec<ConversationDynamicAgentRef>>,
    pub routing_prompt: Option<String>,
    pub allowed_workflows: Option<Vec<ConversationAllowedWorkflowRef>>,
    pub allowed_profiles: Option<Vec<String>>,
    pub global_goal: Option<String>,
    pub control: Option<ConversationDynamicControl>,
    pub active_template_id: Option<String>,
    pub active_template_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicAgentRef {
    pub provider: String,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAllowedWorkflowRef {
    pub workflow_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicControl {
    pub max_dynamic_nodes: u32,
    pub max_fanout: u32,
    pub max_depth: u32,
    pub max_parallel: u32,
    pub max_group_depth: u32,
    pub max_workflow_invocations: u32,
    pub allow_nested_dynamic: bool,
}
