use gold_band::{
    app::App,
    remote_image_trust::{RemoteImageHostError, RemoteImageTrust, normalize_remote_image_host},
};
use tauri::State;

use crate::{
    commands::{CommandErrorVm, CommandResult, command_error},
    state::DesktopState,
};

fn host_error(error: RemoteImageHostError) -> CommandErrorVm {
    CommandErrorVm::new(error.code, serde_json::json!({ "input": error.input }))
}

fn normalize_hosts(hosts: &[String]) -> CommandResult<Vec<String>> {
    hosts
        .iter()
        .map(|host| normalize_remote_image_host(host).map_err(host_error))
        .collect()
}

pub fn load_remote_image_trust(app: &App) -> RemoteImageTrust {
    app.load_state()
        .map(|state| state.remote_image_trust)
        .unwrap_or_default()
}

/// Trusts every host; all-or-nothing so one invalid host never leaves a partial write.
pub fn trust_hosts(app: &App, hosts: &[String]) -> CommandResult<RemoteImageTrust> {
    let hosts = normalize_hosts(hosts)?;
    app.with_state(|state| {
        let changed = state.remote_image_trust.trust(hosts);
        (changed, state.remote_image_trust.clone())
    })
    .map_err(command_error)
}

pub fn revoke_host(app: &App, host: &str) -> CommandResult<RemoteImageTrust> {
    let host = normalize_remote_image_host(host).map_err(host_error)?;
    app.with_state(|state| {
        let changed = state.remote_image_trust.revoke(&host);
        (changed, state.remote_image_trust.clone())
    })
    .map_err(command_error)
}

#[tauri::command]
pub fn trust_remote_image_hosts(
    state: State<'_, DesktopState>,
    hosts: Vec<String>,
) -> CommandResult<RemoteImageTrust> {
    let app = state.app().map_err(command_error)?;
    trust_hosts(&app, &hosts)
}

#[tauri::command]
pub fn revoke_remote_image_host(
    state: State<'_, DesktopState>,
    host: String,
) -> CommandResult<RemoteImageTrust> {
    let app = state.app().map_err(command_error)?;
    revoke_host(&app, &host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use gold_band::{config::RuntimeConfig, storage::StoragePathConfig};
    use tempfile::tempdir;

    const PATH_CONFIG: StoragePathConfig = StoragePathConfig {
        app_key: "gold-band-remote-image-trust-test",
        config_dir_name: ".gold-band-remote-image-trust-test",
        home_env_var: "GOLD_BAND_REMOTE_IMAGE_TRUST_TEST_HOME",
    };

    /// Tests share the home env var, so they must not run concurrently.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn test_app(directory: &tempfile::TempDir) -> App {
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("workspace")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        unsafe { std::env::set_var(PATH_CONFIG.home_env_var, directory.path().join("home")) };
        App::with_config_and_path_config(workspace, RuntimeConfig::default(), PATH_CONFIG)
    }

    fn error_code(error: CommandErrorVm) -> String {
        error.code
    }

    #[test]
    fn trust_revoke_persist_and_reject_invalid_hosts_atomically() {
        let _guard = env_guard();
        let directory = tempdir().unwrap();
        let app = test_app(&directory);

        let trusted = trust_hosts(&app, &["Static.Dion.Blue".into(), "b.example".into()]).unwrap();
        assert_eq!(trusted.trusted_hosts, ["b.example", "static.dion.blue"]);
        assert_eq!(
            load_remote_image_trust(&app),
            trusted,
            "reload from state.json"
        );

        let rejected = trust_hosts(&app, &["c.example".into(), "https://bad/".into()]).unwrap_err();
        assert_eq!(error_code(rejected), "remote-image.host-invalid");
        assert_eq!(load_remote_image_trust(&app), trusted, "no partial write");

        let revoked = revoke_host(&app, "STATIC.dion.blue").unwrap();
        assert_eq!(revoked.trusted_hosts, ["b.example"]);
        assert_eq!(
            revoke_host(&app, "static.dion.blue").unwrap(),
            revoked,
            "idempotent"
        );
        assert_eq!(load_remote_image_trust(&app), revoked);
    }

    #[test]
    fn concurrent_trust_does_not_lose_updates() {
        let _guard = env_guard();
        let directory = tempdir().unwrap();
        let app = test_app(&directory);
        std::thread::scope(|scope| {
            for index in 0..8 {
                let app = &app;
                scope.spawn(move || trust_hosts(app, &[format!("host-{index}.example")]).unwrap());
            }
        });
        assert_eq!(load_remote_image_trust(&app).trusted_hosts.len(), 8);
    }
}
