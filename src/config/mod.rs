use std::str::FromStr;

use anyhow::{anyhow, Result};
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    pub default_provider: Option<String>,
    pub log_level: Option<RuntimeLogLevel>,
    pub log_prompts: Option<bool>,
    pub log_provider_command: Option<bool>,
    pub log_retention_days: Option<u64>,
    pub console_theme: Option<ConsoleThemeName>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub default_provider: String,
    pub log_level: RuntimeLogLevel,
    pub log_prompts: bool,
    pub log_provider_command: bool,
    pub log_retention_days: u64,
    pub console_theme: ConsoleThemeName,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            default_provider: "claude-code".to_string(),
            log_level: RuntimeLogLevel::Debug,
            log_prompts: true,
            log_provider_command: true,
            log_retention_days: 7,
            console_theme: ConsoleThemeName::GoldBand,
        }
    }
}

impl RuntimeConfig {
    pub fn apply_user_config(mut self, user_config: &UserConfig) -> Self {
        if let Some(default_provider) = &user_config.default_provider {
            self.default_provider = default_provider.clone();
        }
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
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsoleThemeName, RuntimeConfig, RuntimeLogLevel, UserConfig};
    use std::str::FromStr;

    #[test]
    fn parses_console_theme_names() {
        assert!(matches!(ConsoleThemeName::from_str("gold-band").unwrap(), ConsoleThemeName::GoldBand));
        assert!(matches!(ConsoleThemeName::from_str("nord").unwrap(), ConsoleThemeName::Nord));
        assert!(matches!(ConsoleThemeName::from_str("dracula").unwrap(), ConsoleThemeName::Dracula));
        assert!(matches!(ConsoleThemeName::from_str("cyber").unwrap(), ConsoleThemeName::Cyber));
        assert!(matches!(ConsoleThemeName::from_str("onyx").unwrap(), ConsoleThemeName::Onyx));
        assert!(matches!(ConsoleThemeName::from_str("mist").unwrap(), ConsoleThemeName::Mist));
        assert!(matches!(ConsoleThemeName::from_str("high-contrast").unwrap(), ConsoleThemeName::HighContrast));
    }

    #[test]
    fn defaults_console_theme_to_gold_band() {
        assert!(matches!(RuntimeConfig::default().console_theme, ConsoleThemeName::GoldBand));
    }

    #[test]
    fn user_config_overrides_default_values() {
        let config = RuntimeConfig::default().apply_user_config(&UserConfig {
            console_theme: Some(ConsoleThemeName::Nord),
            log_level: Some(RuntimeLogLevel::Trace),
            ..UserConfig::default()
        });
        assert_eq!(config.console_theme, ConsoleThemeName::Nord);
        assert!(matches!(config.log_level, RuntimeLogLevel::Trace));
    }

    #[test]
    fn empty_user_config_keeps_defaults() {
        let config = RuntimeConfig::default().apply_user_config(&UserConfig::default());
        assert_eq!(config.console_theme, ConsoleThemeName::GoldBand);
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
