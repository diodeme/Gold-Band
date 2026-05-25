use gold_band::storage::StoragePathConfig;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopChannelConfig {
    pub channel: &'static str,
    pub app_name: &'static str,
    pub app_key: &'static str,
    pub config_dir_name: &'static str,
    pub home_env_var: &'static str,
    pub updater_endpoint: &'static str,
    pub updater_public_key: &'static str,
    pub allow_http_updater: bool,
}

pub fn current_channel_config() -> DesktopChannelConfig {
    DesktopChannelConfig {
        channel: option_env!("GOLD_BAND_RELEASE_CHANNEL").unwrap_or("default"),
        app_name: option_env!("GOLD_BAND_APP_NAME").unwrap_or("Gold Band"),
        app_key: option_env!("GOLD_BAND_APP_KEY").unwrap_or("gold-band"),
        config_dir_name: option_env!("GOLD_BAND_CONFIG_DIR_NAME").unwrap_or(".gold-band"),
        home_env_var: option_env!("GOLD_BAND_HOME_ENV_VAR").unwrap_or("GOLD_BAND_HOME"),
        updater_endpoint: option_env!("GOLD_BAND_UPDATER_ENDPOINT")
            .unwrap_or("https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json"),
        updater_public_key: option_env!("GOLD_BAND_UPDATER_PUBLIC_KEY").unwrap_or("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEYwQkQwNjYyMTA0MjdDQ0IKUldUTGZFSVFZZ2E5OEN3QnY2eHRkM0xVRnlreC9UMFNpSWdXSC9oK0ZWMlpsWXpuZ0hhbEFnWGQK"),
        allow_http_updater: option_env!("GOLD_BAND_ALLOW_HTTP_UPDATER") == Some("true"),
    }
}

pub fn storage_path_config() -> StoragePathConfig {
    let config = current_channel_config();
    StoragePathConfig {
        app_key: config.app_key,
        config_dir_name: config.config_dir_name,
        home_env_var: config.home_env_var,
    }
}
