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
    pub metrics_enabled: bool,
    pub metrics_toggle_locked: bool,
    pub metrics_base_url: &'static str,
    pub metrics_api_key: &'static str,
    pub silent_update_enabled: bool,
    pub builtin_mcp_servers_json: &'static str,
}

pub fn current_channel_config() -> DesktopChannelConfig {
    let config = DesktopChannelConfig {
        channel: option_env!("GOLD_BAND_RELEASE_CHANNEL").unwrap_or("default"),
        app_name: option_env!("GOLD_BAND_APP_NAME").unwrap_or("Gold Band"),
        app_key: option_env!("GOLD_BAND_APP_KEY").unwrap_or("gold-band"),
        config_dir_name: option_env!("GOLD_BAND_CONFIG_DIR_NAME").unwrap_or(".gold-band"),
        home_env_var: option_env!("GOLD_BAND_HOME_ENV_VAR").unwrap_or("GOLD_BAND_HOME"),
        updater_endpoint: option_env!("GOLD_BAND_UPDATER_ENDPOINT")
            .unwrap_or("https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json"),
        updater_public_key: option_env!("GOLD_BAND_UPDATER_PUBLIC_KEY").unwrap_or("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEYwQkQwNjYyMTA0MjdDQ0IKUldUTGZFSVFZZ2E5OEN3QnY2eHRkM0xVRnlreC9UMFNpSWdXSC9oK0ZWMlpsWXpuZ0hhbEFnWGQK"),
        allow_http_updater: option_env!("GOLD_BAND_ALLOW_HTTP_UPDATER") == Some("true"),
        metrics_enabled: option_env!("GOLD_BAND_METRICS_ENABLED") == Some("true"),
        metrics_toggle_locked: option_env!("GOLD_BAND_METRICS_TOGGLE_LOCKED") == Some("true"),
        metrics_base_url: option_env!("GOLD_BAND_METRICS_BASE_URL").unwrap_or(""),
        metrics_api_key: option_env!("GOLD_BAND_METRICS_API_KEY").unwrap_or(""),
        silent_update_enabled: option_env!("GOLD_BAND_SILENT_UPDATE_ENABLED") == Some("true"),
        builtin_mcp_servers_json: option_env!("GOLD_BAND_BUILTIN_MCP_SERVERS").unwrap_or("[]"),
    };
    eprintln!(
        "[metrics] compile-time channel={} metrics_enabled={} metrics_locked={} base_url={} apikey_set={}",
        config.channel,
        config.metrics_enabled,
        config.metrics_toggle_locked,
        config.metrics_base_url,
        !config.metrics_api_key.is_empty(),
    );
    config
}

pub fn storage_path_config() -> StoragePathConfig {
    let config = current_channel_config();
    StoragePathConfig {
        app_key: config.app_key,
        config_dir_name: config.config_dir_name,
        home_env_var: config.home_env_var,
    }
}
