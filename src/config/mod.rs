use std::{collections::BTreeMap, str::FromStr};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl RuntimeLogLevel {
    pub fn as_directive(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedAgentType {
    ClaudeCode,
    CodexCli,
    OpenCode,
    GeminiCli,
}

impl ManagedAgentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex-cli",
            Self::OpenCode => "opencode",
            Self::GeminiCli => "gemini-cli",
        }
    }

    pub fn is_supported(self) -> bool {
        matches!(self, Self::ClaudeCode)
    }
}

impl FromStr for ManagedAgentType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex-cli" => Ok(Self::CodexCli),
            "opencode" => Ok(Self::OpenCode),
            "gemini-cli" => Ok(Self::GeminiCli),
            _ => Err(anyhow!("unsupported agent type: {value}")),
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
        Self {
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@agentclientprotocol/claude-agent-acp@latest".to_string(),
            ],
            display_name: "Claude ACP".to_string(),
            env: BTreeMap::new(),
        }
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
}

impl ManagedAgentConfig {
    pub fn new(adapter: AcpAdapterConfig) -> Self {
        Self { adapter }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    pub log_level: Option<RuntimeLogLevel>,
    pub log_prompts: Option<bool>,
    pub log_provider_command: Option<bool>,
    pub log_retention_days: Option<u64>,
    pub console_theme: Option<ConsoleThemeName>,
    pub desktop_theme: Option<DesktopThemePreference>,
    pub desktop_language: Option<DesktopLanguage>,
    pub desktop_font: Option<DesktopFontPreference>,
    pub desktop_workspace: Option<String>,
    pub agents: Option<BTreeMap<ManagedAgentType, ManagedAgentConfig>>,
    #[serde(default)]
    pub recent_desktop_workspaces: Vec<String>,
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
    pub agents: BTreeMap<ManagedAgentType, ManagedAgentConfig>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let mut agents = BTreeMap::new();
        agents.insert(
            ManagedAgentType::ClaudeCode,
            ManagedAgentConfig::new(AcpAdapterConfig::default()),
        );
        Self {
            log_level: RuntimeLogLevel::Debug,
            log_prompts: true,
            log_provider_command: true,
            log_retention_days: 30,
            console_theme: ConsoleThemeName::GoldBand,
            desktop_theme: DesktopThemePreference::System,
            desktop_language: DesktopLanguage::ZhCn,
            desktop_font: "app-default".to_string(),
            agents,
        }
    }
}

impl RuntimeConfig {
    pub fn apply_user_config(mut self, user_config: &UserConfig) -> Self {
        if let Some(log_level) = user_config.log_level {
            self.log_level = log_level;
        }
        if let Some(log_prompts) = user_config.log_prompts {
            self.log_prompts = log_prompts;
        }
        if let Some(log_provider_command) = user_config.log_provider_command {
            self.log_provider_command = log_provider_command;
        }
        if let Some(log_retention_days) = user_config.log_retention_days {
            self.log_retention_days = log_retention_days;
        }
        if let Some(console_theme) = user_config.console_theme {
            self.console_theme = console_theme;
        }
        if let Some(desktop_theme) = user_config.desktop_theme {
            self.desktop_theme = desktop_theme;
        }
        if let Some(desktop_language) = user_config.desktop_language {
            self.desktop_language = desktop_language;
        }
        if let Some(desktop_font) = &user_config.desktop_font {
            self.desktop_font = desktop_font.clone();
        }
        if let Some(agents) = &user_config.agents {
            self.agents = agents.clone();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConsoleThemeName, DesktopLanguage, DesktopThemePreference, RuntimeConfig, RuntimeLogLevel,
        UserConfig,
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
    fn user_config_overrides_default_values() {
        let config = RuntimeConfig::default().apply_user_config(&UserConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            desktop_theme: Some(DesktopThemePreference::Dark),
            desktop_language: Some(DesktopLanguage::En),
            desktop_font: Some("Microsoft YaHei UI".to_string()),
            log_level: Some(RuntimeLogLevel::Trace),
            ..UserConfig::default()
        });
        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert_eq!(config.desktop_theme, DesktopThemePreference::Dark);
        assert_eq!(config.desktop_language, DesktopLanguage::En);
        assert_eq!(config.desktop_font, "Microsoft YaHei UI");
        assert!(matches!(config.log_level, RuntimeLogLevel::Trace));
    }

    #[test]
    fn empty_user_config_keeps_defaults() {
        let config = RuntimeConfig::default().apply_user_config(&UserConfig::default());
        assert_eq!(config.console_theme, ConsoleThemeName::GoldBand);
        assert_eq!(config.desktop_theme, DesktopThemePreference::System);
        assert_eq!(config.desktop_language, DesktopLanguage::ZhCn);
        assert_eq!(config.desktop_font, "app-default");
        assert!(matches!(config.log_level, RuntimeLogLevel::Debug));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    Project,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedProfileRef {
    pub name: String,
    pub source: ProfileSource,
    pub path: String,
}
