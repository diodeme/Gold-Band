use std::{env, fs, path::PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelConfig {
    channel: String,
    app_name: String,
    app_key: String,
    config_dir_name: String,
    home_env_var: String,
    updater_endpoint: String,
    updater_public_key: String,
    allow_http_updater: bool,
    metrics_enabled: bool,
    metrics_toggle_locked: bool,
    heartbeat_endpoint: String,
    node_metrics_endpoint: String,
    metrics_api_key: String,
}

fn main() {
    println!("cargo:rerun-if-env-changed=GOLD_BAND_RELEASE_CHANNEL");
    println!("cargo:rerun-if-changed=../configs/channels");

    let channel = env::var("GOLD_BAND_RELEASE_CHANNEL").unwrap_or_else(|_| "default".to_string());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let config_path = manifest_dir
        .parent()
        .expect("src-tauri has a parent directory")
        .join("configs")
        .join("channels")
        .join(format!("{channel}.json"));
    let config_text = fs::read_to_string(&config_path)
        .unwrap_or_else(|error| panic!("failed to read channel config {}: {error}", config_path.display()));
    let config: ChannelConfig = serde_json::from_str(&config_text)
        .unwrap_or_else(|error| panic!("failed to parse channel config {}: {error}", config_path.display()));

    if config.channel != channel {
        panic!(
            "channel config mismatch: expected {}, found {} in {}",
            channel,
            config.channel,
            config_path.display()
        );
    }

    println!("cargo:rustc-env=GOLD_BAND_RELEASE_CHANNEL={}", config.channel);
    println!("cargo:rustc-env=GOLD_BAND_APP_NAME={}", config.app_name);
    println!("cargo:rustc-env=GOLD_BAND_APP_KEY={}", config.app_key);
    println!("cargo:rustc-env=GOLD_BAND_CONFIG_DIR_NAME={}", config.config_dir_name);
    println!("cargo:rustc-env=GOLD_BAND_HOME_ENV_VAR={}", config.home_env_var);
    println!("cargo:rustc-env=GOLD_BAND_UPDATER_ENDPOINT={}", config.updater_endpoint);
    println!("cargo:rustc-env=GOLD_BAND_UPDATER_PUBLIC_KEY={}", config.updater_public_key);
    println!("cargo:rustc-env=GOLD_BAND_ALLOW_HTTP_UPDATER={}", config.allow_http_updater);
    println!("cargo:rustc-env=GOLD_BAND_METRICS_ENABLED={}", config.metrics_enabled);
    println!("cargo:rustc-env=GOLD_BAND_METRICS_TOGGLE_LOCKED={}", config.metrics_toggle_locked);
    println!("cargo:rustc-env=GOLD_BAND_HEARTBEAT_ENDPOINT={}", config.heartbeat_endpoint);
    println!("cargo:rustc-env=GOLD_BAND_NODE_METRICS_ENDPOINT={}", config.node_metrics_endpoint);
    println!("cargo:rustc-env=GOLD_BAND_METRICS_API_KEY={}", config.metrics_api_key);

    tauri_build::build()
}
