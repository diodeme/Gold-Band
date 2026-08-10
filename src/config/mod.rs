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
    LightGray,
    Dark,
    Black,
    System,
}

impl FromStr for DesktopThemePreference {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "light" => Ok(Self::Light),
            "light-gray" => Ok(Self::LightGray),
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ManagedAgentId(String);

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

#[derive(Debug, Clone, Copy)]
pub struct ManagedAgentPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub icon_key: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub primary_agent_dir: &'static str,
    pub compatible_agent_dirs: &'static [&'static str],
}

impl ManagedAgentPreset {
    pub fn agent_id(self) -> ManagedAgentId {
        ManagedAgentId::from_str(self.id).expect("built-in managed agent id is valid")
    }

    pub fn default_config(self) -> ManagedAgentConfig {
        ManagedAgentConfig {
            adapter: AcpAdapterConfig {
                command: self.command.to_string(),
                args: self.args.iter().map(|value| (*value).to_string()).collect(),
                display_name: self.label.to_string(),
                env: BTreeMap::new(),
            },
            primary_agent_dir: self.primary_agent_dir.to_string(),
            compatible_agent_dirs: self
                .compatible_agent_dirs
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            external_session_sync_enabled: false,
        }
    }
}

pub const MANAGED_AGENT_PRESETS: [ManagedAgentPreset; 5] = [
    ManagedAgentPreset {
        id: "claude-acp",
        label: "Claude",
        icon_key: "claude",
        command: "npx",
        args: &["-y", "@agentclientprotocol/claude-agent-acp@latest"],
        primary_agent_dir: ".claude",
        compatible_agent_dirs: &[],
    },
    ManagedAgentPreset {
        id: "codex-acp",
        label: "Codex",
        icon_key: "codex",
        command: "npx",
        args: &["-y", "@agentclientprotocol/codex-acp@latest"],
        primary_agent_dir: ".codex",
        compatible_agent_dirs: &[".agents"],
    },
    ManagedAgentPreset {
        id: "cursor",
        label: "Cursor",
        icon_key: "cursor",
        command: "cursor-agent",
        args: &["acp"],
        primary_agent_dir: ".cursor",
        compatible_agent_dirs: &[".agents"],
    },
    ManagedAgentPreset {
        id: "gemini",
        label: "Gemini",
        icon_key: "gemini",
        command: "npx",
        args: &["-y", "@google/gemini-cli@latest", "--acp"],
        primary_agent_dir: ".gemini",
        compatible_agent_dirs: &[".agents"],
    },
    ManagedAgentPreset {
        id: "opencode",
        label: "OpenCode",
        icon_key: "opencode",
        command: "opencode",
        args: &["acp"],
        primary_agent_dir: ".opencode",
        compatible_agent_dirs: &[".agents"],
    },
];

pub fn managed_agent_preset(agent_id: &ManagedAgentId) -> Option<&'static ManagedAgentPreset> {
    MANAGED_AGENT_PRESETS
        .iter()
        .find(|preset| preset.id == agent_id.as_str())
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
        MANAGED_AGENT_PRESETS[0].default_config().adapter
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
    /// Gold Band 写入、同步，同时也是 Agent 首个读取位置的主 Agent 目录。
    pub primary_agent_dir: String,
    /// Agent 额外读取但 Gold Band 不写入、不作为同步目标的兼容 Agent 目录。
    #[serde(default)]
    pub compatible_agent_dirs: Vec<String>,
    /// 是否允许 Gold Band 根据 Provider revision 重载并导入外部客户端会话历史。
    /// 仅适用于能跨客户端共享同一线性会话上下文的 Agent，默认关闭。
    #[serde(default)]
    pub external_session_sync_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkillDirectoryPolicy {
    pub write_dir_names: Vec<String>,
    pub read_dir_names: Vec<String>,
}

impl ManagedAgentConfig {
    pub fn new(
        adapter: AcpAdapterConfig,
        primary_agent_dir: impl Into<String>,
        compatible_agent_dirs: Vec<String>,
    ) -> Self {
        Self {
            adapter,
            primary_agent_dir: primary_agent_dir.into(),
            compatible_agent_dirs,
            external_session_sync_enabled: false,
        }
    }

    pub fn skill_directory_policy(&self) -> AgentSkillDirectoryPolicy {
        let primary = self.primary_agent_dir.clone();
        let write_dir_names = vec![primary.clone()];
        let mut read_dir_names = vec![primary];
        for compatible in &self.compatible_agent_dirs {
            if !read_dir_names.iter().any(|dir_name| dir_name == compatible) {
                read_dir_names.push(compatible.clone());
            }
        }
        AgentSkillDirectoryPolicy {
            write_dir_names,
            read_dir_names,
        }
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
    pub desktop_theme: Option<DesktopThemePreference>,
    pub desktop_language: Option<DesktopLanguage>,
    pub desktop_font: Option<DesktopFontPreference>,
    pub desktop_updater_url_override: Option<String>,
    /// DEPRECATED: 仅供旧 Workbench 单 workspace 启动与最近列表兼容使用。
    /// 新会话 UI 必须使用 `conversation_workspaces` / `last_conversation_workspace`，
    /// 不得新增对该字段的依赖；待旧 Workbench 删除时一并移除。
    pub desktop_workspace: Option<String>,
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

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 3;

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
        if version < 3 {
            migrate_scheduled_runtime_settings(settings);
            migrated = true;
        }
        if migrated {
            settings.insert(
                "settingsSchemaVersion".to_string(),
                serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION),
            );
        }

        let config = serde_json::from_value(value)?;
        Ok((config, migrated))
    }
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
        let agent_id = ManagedAgentId::from_str(canonical_id)?;
        let preset = managed_agent_preset(&agent_id)
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
                    legacy_override.unwrap_or_else(|| preset.primary_agent_dir.to_string()),
                ),
            );
        }
        if !config.contains_key("compatibleAgentDirs") {
            config.insert(
                "compatibleAgentDirs".to_string(),
                serde_json::Value::Array(
                    preset
                        .compatible_agent_dirs
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_diagnostics: BTreeMap<String, ProviderDiagnosticSnapshot>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        let claude_preset = MANAGED_AGENT_PRESETS[0];
        agents.insert(claude_preset.agent_id(), claude_preset.default_config());
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
        AcpAdapterConfig, ConsoleThemeName, ConversationDirectConfig, ConversationRunMode,
        ConversationRunModeEntry, DesktopAvailableUpdate, DesktopLanguage, DesktopThemePreference,
        DesktopUpdateBadgeState, MANAGED_AGENT_PRESETS, ManagedAgentConfig, ManagedAgentId,
        ProjectAppConfig, RuntimeConfig, RuntimeLogLevel, SettingsConfig, StateConfig,
        managed_agent_preset,
    };
    use std::collections::BTreeMap;
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
            DesktopThemePreference::from_str("light-gray").unwrap(),
            DesktopThemePreference::LightGray
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
        assert!(DesktopThemePreference::from_str("light-warm").is_err());
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
            conversation_auto_title_max_chars: Some(20),
            notification_auto_dismiss_target_secs: Some(20),
            require_local_claude_executable: Some(true),
            acp_session_idle_ttl_secs: Some(900),
            acp_max_idle_session_runtimes: Some(12),
            acp_timeline_compact_patch_ratio: Some(6),
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
        assert_eq!(roundtripped.acp_max_idle_session_runtimes, Some(12));
        assert_eq!(roundtripped.acp_timeline_compact_patch_ratio, Some(6));
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
    fn managed_agent_presets_own_default_agent_directories() {
        let defaults = MANAGED_AGENT_PRESETS
            .into_iter()
            .map(|preset| {
                (
                    preset.id,
                    preset.primary_agent_dir,
                    preset.compatible_agent_dirs,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            defaults,
            vec![
                ("claude-acp", ".claude", &[] as &[&str]),
                ("codex-acp", ".codex", &[".agents"]),
                ("cursor", ".cursor", &[".agents"]),
                ("gemini", ".gemini", &[".agents"]),
                ("opencode", ".opencode", &[".agents"]),
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
                        "adapter": MANAGED_AGENT_PRESETS[1].default_config().adapter,
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
        assert_eq!(claude.primary_agent_dir, ".claude");
        assert!(claude.compatible_agent_dirs.is_empty());
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(codex.primary_agent_dir, ".codex");
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
    fn current_codex_preset_uses_agentclientprotocol_adapter() {
        let codex = managed_agent_preset(&ManagedAgentId::from_str("codex-acp").unwrap())
            .unwrap()
            .default_config();

        assert_eq!(
            codex.adapter.args,
            vec!["-y", "@agentclientprotocol/codex-acp@latest"]
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
                        "adapter": MANAGED_AGENT_PRESETS[1].default_config().adapter,
                        "skillsDirOverride": "  .custom-codex  "
                    }
                }
            }))
            .unwrap();

        assert!(migrated);
        let agents = settings.agents.unwrap();
        let codex = &agents[&ManagedAgentId::from_str("codex-acp").unwrap()];
        assert_eq!(codex.primary_agent_dir, ".custom-codex");
        assert_eq!(codex.compatible_agent_dirs, vec![".agents"]);
        let serialized = serde_json::to_value(codex).unwrap();
        assert!(serialized.get("skillsDirOverride").is_none());
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
        assert_eq!(agent.primary_agent_dir, ".custom-agent");
        assert_eq!(agent.compatible_agent_dirs, vec![".agents"]);
    }

    #[test]
    fn app_config_bounds_acp_runtime_policy_values() {
        let config = RuntimeConfig::default().apply_app_config(&ProjectAppConfig {
            acp_session_foreground_lease_ttl_secs: Some(60),
            acp_session_foreground_lease_renew_interval_secs: Some(90),
            acp_session_idle_ttl_secs: Some(0),
            acp_max_idle_session_runtimes: Some(0),
            acp_timeline_compact_patch_ratio: Some(0),
            ..Default::default()
        });
        assert_eq!(config.acp_session_foreground_lease_ttl_secs, 60);
        assert_eq!(config.acp_session_foreground_lease_renew_interval_secs, 20);
        assert_eq!(config.acp_session_idle_ttl_secs, 600);
        assert_eq!(config.acp_max_idle_session_runtimes, 8);
        assert_eq!(config.acp_timeline_compact_patch_ratio, 4);
    }

    #[test]
    fn skill_directory_policy_separates_write_and_compatible_read_dirs() {
        for preset in MANAGED_AGENT_PRESETS {
            let config = preset.default_config();
            let policy = config.skill_directory_policy();
            assert_eq!(policy.write_dir_names, vec![preset.primary_agent_dir]);
            let mut expected_reads = vec![preset.primary_agent_dir];
            expected_reads.extend_from_slice(preset.compatible_agent_dirs);
            assert_eq!(policy.read_dir_names, expected_reads);
        }
    }

    #[test]
    fn skill_directory_policy_deduplicates_primary_and_compatible_directories() {
        let config = ManagedAgentConfig::new(
            AcpAdapterConfig::default(),
            "custom-codex",
            vec!["custom-codex".to_string(), ".agents".to_string()],
        );
        let policy = config.skill_directory_policy();
        assert_eq!(policy.write_dir_names, vec!["custom-codex"]);
        assert_eq!(policy.read_dir_names, vec!["custom-codex", ".agents"]);
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
                    include_interview: None,
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
    pub include_interview: Option<bool>,
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
    pub acceptance_model_id: Option<String>,
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
