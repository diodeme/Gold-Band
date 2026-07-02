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
pub enum DesktopThemePreference {
    Light,
    LightWarm,
    Dark,
    Black,
    System,
}

impl FromStr for DesktopThemePreference {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "light" => Ok(Self::Light),
            "light-warm" => Ok(Self::LightWarm),
            "dark" => Ok(Self::Dark),
            "black" => Ok(Self::Black),
            "system" => Ok(Self::System),
            _ => Err(anyhow!("unsupported desktop theme: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopLanguage {
    ZhCn,
    En,
}

pub type DesktopFontPreference = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ManagedAgentType {
    #[serde(rename = "claude-acp")]
    ClaudeAcp,
    #[serde(rename = "codex-acp")]
    CodexAcp,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl ManagedAgentType {
    pub const ALL: [Self; 5] = [
        Self::ClaudeAcp,
        Self::CodexAcp,
        Self::Cursor,
        Self::Gemini,
        Self::OpenCode,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeAcp => "claude-acp",
            Self::CodexAcp => "codex-acp",
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
        }
    }

    pub fn is_supported(self) -> bool {
        matches!(
            self,
            Self::ClaudeAcp | Self::CodexAcp | Self::Cursor | Self::Gemini | Self::OpenCode
        )
    }

    pub fn default_adapter_config(self) -> AcpAdapterConfig {
        match self {
            Self::ClaudeAcp => AcpAdapterConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@agentclientprotocol/claude-agent-acp@latest".to_string(),
                ],
                display_name: "Claude".to_string(),
                env: BTreeMap::new(),
            },
            Self::CodexAcp => AcpAdapterConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@zed-industries/codex-acp@latest".to_string(),
                ],
                display_name: "Codex".to_string(),
                env: BTreeMap::new(),
            },
            Self::Cursor => AcpAdapterConfig {
                command: "cursor-agent".to_string(),
                args: vec!["acp".to_string()],
                display_name: "Cursor".to_string(),
                env: BTreeMap::new(),
            },
            Self::Gemini => AcpAdapterConfig {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@google/gemini-cli@latest".to_string(),
                    "--acp".to_string(),
                ],
                display_name: "Gemini".to_string(),
                env: BTreeMap::new(),
            },
            Self::OpenCode => AcpAdapterConfig {
                command: "opencode".to_string(),
                args: vec!["acp".to_string()],
                display_name: "OpenCode".to_string(),
                env: BTreeMap::new(),
            },
        }
    }
    /// 返回该 agent 的 skills 目录名（硬编码默认值）
    /// 参考 cc-switch get_app_skills_dir 混合方案：默认值 + ManagedAgentConfig::skills_dir_override 可选覆盖
    pub fn skills_dir_name(self) -> &'static str {
        match self {
            Self::ClaudeAcp => ".claude",
            Self::CodexAcp => ".codex",
            Self::Cursor => ".cursor",
            Self::Gemini => ".gemini",
            Self::OpenCode => ".opencode",
        }
    }
}

impl FromStr for ManagedAgentType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "claude-acp" => Ok(Self::ClaudeAcp),
            "codex-acp" => Ok(Self::CodexAcp),
            "cursor" => Ok(Self::Cursor),
            "gemini" => Ok(Self::Gemini),
            "opencode" => Ok(Self::OpenCode),
            _ => Err(anyhow!("unsupported agent type: {value}")),
        }
    }
}

impl<'de> Deserialize<'de> for ManagedAgentType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "claude-code" => Ok(Self::ClaudeAcp),
            "codex-cli" => Ok(Self::CodexAcp),
            "gemini-cli" => Ok(Self::Gemini),
            _ => Self::from_str(&value).map_err(serde::de::Error::custom),
        }
    }
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
        ManagedAgentType::ClaudeAcp.default_adapter_config()
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
    /// 可选覆盖 agent skills 目录名（如 ".claude"、"custom-dir"）
    /// 未设置时使用 ManagedAgentType::skills_dir_name() 硬编码默认值
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills_dir_override: Option<String>,
}

impl ManagedAgentConfig {
    pub fn new(adapter: AcpAdapterConfig) -> Self {
        Self {
            adapter,
            skills_dir_override: None,
        }
    }

    /// 解析 agent 的 skills 目录名：override 优先，否则使用硬编码默认值
    pub fn skills_dir_name(&self, agent_type: ManagedAgentType) -> &str {
        self.skills_dir_override
            .as_deref()
            .unwrap_or_else(|| agent_type.skills_dir_name())
    }
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

pub const AGENTS_DIR_NAME: &str = ".agents";
pub const SKILLS_DIR_NAME: &str = "skills";
pub const SKILL_FILE_NAME: &str = "SKILL.md";
pub const MAX_SKILL_FILE_SIZE: usize = 100 * 1024;
pub const MAX_SKILL_DESCRIPTION_LEN: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub directory_path: String,
    /// SKILL 来源目录标识：".agents" 为 Gold-Band 自身管理，".claude"/".codex" 等为对应 agent 目录
    pub agent_source: String,
    pub load_warnings: Vec<String>,
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
    pub log_level: Option<RuntimeLogLevel>,
    pub log_prompts: Option<bool>,
    pub log_provider_command: Option<bool>,
    pub log_retention_days: Option<u64>,
    pub console_theme: Option<ConsoleThemeName>,
    pub desktop_theme: Option<DesktopThemePreference>,
    pub desktop_language: Option<DesktopLanguage>,
    pub desktop_font: Option<DesktopFontPreference>,
    pub desktop_updater_url_override: Option<String>,
    pub desktop_workspace: Option<String>,
    pub agents: Option<BTreeMap<ManagedAgentType, ManagedAgentConfig>>,
    pub use_local_claude: Option<bool>,
    pub desktop_metrics_enabled: Option<bool>,
    pub desktop_metrics_base_url: Option<String>,
    pub desktop_metrics_api_key: Option<String>,
    #[serde(default)]
    pub context_servers: Option<Vec<McpServerConfig>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateConfig {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode_mapping: Option<BTreeMap<String, BTreeMap<String, String>>>,
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
    pub desktop_theme: DesktopThemePreference,
    pub desktop_language: DesktopLanguage,
    pub desktop_font: DesktopFontPreference,
    pub desktop_updater_url_override: Option<String>,
    pub desktop_updater_last_checked_at: Option<String>,
    pub desktop_update_badges: DesktopUpdateBadgeState,
    pub desktop_available_update: Option<DesktopAvailableUpdate>,
    pub agents: BTreeMap<ManagedAgentType, ManagedAgentConfig>,
    pub use_local_claude: bool,
    pub desktop_metrics_enabled: bool,
    pub desktop_metrics_base_url: Option<String>,
    pub desktop_metrics_api_key: Option<String>,
    pub acp_session_title_refresh_enabled: bool,
    pub acp_chat_event_page_size: usize,
    pub acp_raw_max_size_bytes: u64,
    pub acp_raw_target_size_bytes: u64,
    pub permission_mode_mapping: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_diagnostics: BTreeMap<String, ProviderDiagnosticSnapshot>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        agents.insert(
            ManagedAgentType::ClaudeAcp,
            ManagedAgentConfig::new(AcpAdapterConfig::default()),
        );
        let base = Self {
            log_level: RuntimeLogLevel::Info,
            log_prompts: true,
            log_provider_command: true,
            log_retention_days: 30,
            console_theme: ConsoleThemeName::GoldBand,
            desktop_theme: DesktopThemePreference::System,
            desktop_language: DesktopLanguage::ZhCn,
            desktop_font: "app-default".to_string(),
            desktop_updater_url_override: None,
            desktop_updater_last_checked_at: None,
            desktop_update_badges: DesktopUpdateBadgeState::default(),
            desktop_available_update: None,
            agents,
            use_local_claude: false,
            desktop_metrics_enabled: false,
            desktop_metrics_base_url: None,
            desktop_metrics_api_key: None,
            acp_session_title_refresh_enabled: false,
            acp_chat_event_page_size: 360,
            acp_raw_max_size_bytes: 5 * 1024 * 1024,
            acp_raw_target_size_bytes: 4 * 1024 * 1024,
            permission_mode_mapping: BTreeMap::new(),
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
        if let Some(desktop_theme) = settings.desktop_theme {
            self.desktop_theme = desktop_theme;
        }
        if let Some(desktop_language) = settings.desktop_language {
            self.desktop_language = desktop_language;
        }
        if let Some(desktop_font) = &settings.desktop_font {
            self.desktop_font = desktop_font.clone();
        }
        self.desktop_updater_url_override = settings.desktop_updater_url_override.clone();
        if let Some(agents) = &settings.agents {
            self.agents = agents.clone();
        }
        if let Some(use_local_claude) = settings.use_local_claude {
            self.use_local_claude = use_local_claude;
        }
        if let Some(desktop_metrics_enabled) = settings.desktop_metrics_enabled {
            self.desktop_metrics_enabled = desktop_metrics_enabled;
        }
        self.desktop_metrics_base_url = settings.desktop_metrics_base_url.clone();
        self.desktop_metrics_api_key = settings.desktop_metrics_api_key.clone();
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
        if let Some(ref mapping) = app_config.permission_mode_mapping {
            self.permission_mode_mapping = mapping.clone();
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

    /// Resolve a normative permission mode (read_only/ask/full_access) to an agent-specific mode ID.
    /// Falls back to the normative mode itself if no mapping is configured for the provider.
    pub fn resolve_permission_mode(&self, provider: &str, normative_mode: &str) -> String {
        self.permission_mode_mapping
            .get(provider)
            .and_then(|map| map.get(normative_mode))
            .cloned()
            .unwrap_or_else(|| normative_mode.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConsoleThemeName, DesktopAvailableUpdate, DesktopLanguage, DesktopThemePreference,
        DesktopUpdateBadgeState, ProjectAppConfig, RuntimeConfig, RuntimeLogLevel, SettingsConfig,
        StateConfig,
    };
    use std::str::FromStr;

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
            DesktopThemePreference::from_str("light").unwrap(),
            DesktopThemePreference::Light
        ));
        assert!(matches!(
            DesktopThemePreference::from_str("light-warm").unwrap(),
            DesktopThemePreference::LightWarm
        ));
        assert!(matches!(
            DesktopThemePreference::from_str("dark").unwrap(),
            DesktopThemePreference::Dark
        ));
        assert!(matches!(
            DesktopThemePreference::from_str("black").unwrap(),
            DesktopThemePreference::Black
        ));
        assert!(matches!(
            DesktopThemePreference::from_str("system").unwrap(),
            DesktopThemePreference::System
        ));
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
        assert!(matches!(
            config.desktop_theme,
            DesktopThemePreference::System
        ));
        assert!(matches!(config.desktop_language, DesktopLanguage::ZhCn));
        assert_eq!(config.desktop_font, "app-default");
    }

    #[test]
    fn settings_config_roundtrips_json() {
        let settings = SettingsConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            desktop_theme: Some(DesktopThemePreference::Dark),
            desktop_language: Some(DesktopLanguage::En),
            desktop_font: Some("Microsoft YaHei UI".to_string()),
            desktop_updater_url_override: Some("https://updates.example/latest.json".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            ..SettingsConfig::default()
        };
        let json = serde_json::to_string_pretty(&settings).unwrap();
        let roundtripped: SettingsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.console_theme, Some(ConsoleThemeName::Nord));
        assert_eq!(
            roundtripped.desktop_theme,
            Some(DesktopThemePreference::Dark)
        );
        assert_eq!(roundtripped.desktop_language, Some(DesktopLanguage::En));
        assert_eq!(
            roundtripped.desktop_font.as_deref(),
            Some("Microsoft YaHei UI")
        );
        assert!(matches!(
            roundtripped.log_level,
            Some(RuntimeLogLevel::Trace)
        ));
    }

    #[test]
    fn state_config_roundtrips_json() {
        let state = StateConfig {
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
    fn apply_settings_overrides_defaults() {
        let config = RuntimeConfig::default().apply_settings(&SettingsConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            desktop_theme: Some(DesktopThemePreference::Dark),
            desktop_language: Some(DesktopLanguage::En),
            desktop_font: Some("Microsoft YaHei UI".to_string()),
            desktop_updater_url_override: Some("https://updates.example/latest.json".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            ..SettingsConfig::default()
        });
        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert_eq!(config.desktop_theme, DesktopThemePreference::Dark);
        assert_eq!(config.desktop_language, DesktopLanguage::En);
        assert_eq!(config.desktop_font, "Microsoft YaHei UI");
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
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&app_config).unwrap();
        let roundtripped: ProjectAppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.acp_session_title_refresh_enabled, Some(true));
        assert_eq!(roundtripped.acp_chat_event_page_size, Some(240));
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
        assert_eq!(config.desktop_theme, DesktopThemePreference::System);
        assert_eq!(config.desktop_language, DesktopLanguage::ZhCn);
        assert_eq!(config.desktop_font, "app-default");
        assert!(matches!(config.log_level, RuntimeLogLevel::Info));
    }

    #[test]
    fn apply_app_config_overrides_defaults() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            acp_session_title_refresh_enabled: Some(true),
            acp_chat_event_page_size: Some(240),
            ..Default::default()
        });
        assert!(config.acp_session_title_refresh_enabled);
        assert_eq!(config.acp_chat_event_page_size, 240);
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
            desktop_theme: Some(DesktopThemePreference::Dark),
            desktop_language: Some(DesktopLanguage::En),
            desktop_font: Some("Fira Code".to_string()),
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
        assert_eq!(config.desktop_theme, DesktopThemePreference::Dark);
        assert_eq!(config.desktop_language, DesktopLanguage::En);
        assert_eq!(config.desktop_font, "Fira Code");
        assert!(matches!(config.log_level, RuntimeLogLevel::Trace));
        assert!(config.use_local_claude);
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
    fn managed_agent_type_skills_dir_name_returns_correct_dirs() {
        use super::ManagedAgentType;
        assert_eq!(ManagedAgentType::ClaudeAcp.skills_dir_name(), ".claude");
        assert_eq!(ManagedAgentType::CodexAcp.skills_dir_name(), ".codex");
        assert_eq!(ManagedAgentType::Cursor.skills_dir_name(), ".cursor");
        assert_eq!(ManagedAgentType::Gemini.skills_dir_name(), ".gemini");
        assert_eq!(ManagedAgentType::OpenCode.skills_dir_name(), ".opencode");
    }

    #[test]
    fn managed_agent_config_skills_dir_name_uses_override_when_set() {
        use super::{AcpAdapterConfig, ManagedAgentConfig, ManagedAgentType};
        let mut config = ManagedAgentConfig::new(AcpAdapterConfig::default());
        // 未设置 override，使用硬编码默认值
        assert_eq!(
            config.skills_dir_name(ManagedAgentType::ClaudeAcp),
            ".claude"
        );
        // 设置 override，优先使用 override
        config.skills_dir_override = Some("custom-claude".to_string());
        assert_eq!(
            config.skills_dir_name(ManagedAgentType::ClaudeAcp),
            "custom-claude"
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    BuiltIn,
    Project,
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
    pub mode: String,
    pub workflow_template_id: Option<String>,
    pub auto_config: Option<ConversationAutoConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAutoConfig {
    pub agent_strategy: Option<String>,
    pub agent_type: String,
    pub bootstrap_agent_type: Option<String>,
    pub bootstrap_model_id: Option<String>,
    pub acceptance_model_id: Option<String>,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
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
