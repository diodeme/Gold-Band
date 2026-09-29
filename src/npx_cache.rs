//! User-confirmed repair of explicitly reported npx installation directories.
use crate::{
    config::AcpAdapterConfig,
    process::{ManagedProcessGroup, background_command},
};
use anyhow::{Result, bail};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};

const MAX_TARGETS: usize = 16;
const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: u64 = 64 * 1024;
static REPAIR_ACTIVE: Mutex<bool> = Mutex::new(false);

#[derive(Debug, thiserror::Error)]
#[error("{code}: {msg}")]
pub struct CacheError {
    pub code: &'static str,
    pub msg: String,
}
pub fn error(code: &'static str) -> anyhow::Error {
    CacheError {
        code,
        msg: code.to_owned(),
    }
    .into()
}

pub fn is_npx(config: &AcpAdapterConfig) -> bool {
    matches!(
        Path::new(&config.command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "npx" | "npx.cmd" | "npx.exe"
    ) || (matches!(
        Path::new(&config.command)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "npm" | "npm.cmd" | "npm.exe"
    ) && config.args.first().is_some_and(|a| a == "exec"))
}

pub struct RepairGuard {
    _private: (),
}
impl RepairGuard {
    pub fn acquire() -> Result<Self> {
        let mut s = REPAIR_ACTIVE.lock().map_err(|_| error("npx-cache.busy"))?;
        if *s {
            return Err(error("npx-cache.busy"));
        }
        *s = true;
        Ok(Self { _private: () })
    }
}
impl Drop for RepairGuard {
    fn drop(&mut self) {
        if let Ok(mut s) = REPAIR_ACTIVE.lock() {
            *s = false;
        }
    }
}

/// Only missing root manifests qualify; arbitrary stack-frame paths never do.
pub fn missing_manifests(reason: &str) -> Vec<PathBuf> {
    if !reason.lines().any(|line| {
        matches!(
            line.trim(),
            "npm error code ENOENT" | "npm ERR! code ENOENT"
        )
    }) {
        return vec![];
    }
    let mut paths = BTreeSet::new();
    for line in reason.lines() {
        let line = line.trim();
        let path = line
            .strip_prefix("npm error path ")
            .or_else(|| line.strip_prefix("npm ERR! path "))
            .or_else(|| {
                if !line.contains("ENOENT: no such file or directory, open '") {
                    return None;
                }
                line.split_once("ENOENT: no such file or directory, open '")?
                    .1
                    .strip_suffix('\'')
            });
        if let Some(path) = path {
            let path = PathBuf::from(path);
            if path.file_name().is_some_and(|s| s == "package.json")
                && path
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .is_some_and(|s| s == "_npx")
            {
                paths.insert(path);
            }
        }
    }
    if paths.len() > MAX_TARGETS {
        return vec![];
    }
    paths.into_iter().collect()
}

fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            m.is_dir() && m.file_attributes() & 0x400 == 0
        }
        #[cfg(not(windows))]
        {
            m.is_dir() && !m.file_type().is_symlink()
        }
    })
}

pub fn validate_targets(cache: &Path, manifests: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let root = fs::canonicalize(cache).map_err(|_| error("npx-cache.unavailable"))?;
    let npx = root.join("_npx");
    if !plain_directory(&npx) {
        return Err(error("npx-cache.unavailable"));
    }
    let mut targets = BTreeSet::new();
    for manifest in manifests {
        if !manifest.is_absolute()
            || manifest
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            continue;
        }
        let Some(target) = manifest.parent() else {
            continue;
        };
        let Some(parent) = target.parent() else {
            continue;
        };
        if !plain_directory(target) || fs::canonicalize(parent).ok().as_ref() != Some(&npx) {
            continue;
        }
        if manifest.file_name().is_none_or(|s| s != "package.json") {
            continue;
        }
        if !matches!(fs::symlink_metadata(manifest), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let target = fs::canonicalize(target).map_err(|_| error("npx-cache.unavailable"))?;
        if target.parent() == Some(npx.as_path()) {
            targets.insert(target);
        }
    }
    if targets.is_empty() || targets.len() > MAX_TARGETS {
        return Err(error("npx-cache.unavailable"));
    }
    Ok(targets.into_iter().collect())
}

pub fn resolve_cache(config: &AcpAdapterConfig, cwd: &Path) -> Result<PathBuf> {
    if !is_npx(config) {
        return Err(error("npx-cache.unavailable"));
    }
    let env = crate::acp::adapter::resolved_adapter_env(&config.env);
    let executable = crate::acp::adapter::platform_adapter_command(&config.command);
    let resolved = crate::acp::adapter::resolve_command_with_path(
        &executable,
        env.get("PATH").map(String::as_str),
    );
    let resolved = PathBuf::from(resolved);
    // Use npm from the same installation, never an unrelated npm from PATH.
    let npm = resolved
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| error("npx-cache.unavailable"))?
        .join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    if !npm.is_file() {
        return Err(error("npx-cache.unavailable"));
    }
    let mut cmd = background_command(npm);
    cmd.args(["config", "get", "cache"])
        .envs(&env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Unknown launcher options may change npm configuration: fail closed.
    let args = if config.args.first().is_some_and(|a| a == "exec") {
        &config.args[1..]
    } else {
        &config.args[..]
    };
    let mut args = args.iter().peekable();
    while let Some(arg) = args.next() {
        if arg == "--" || !arg.starts_with('-') {
            break;
        }
        if matches!(arg.as_str(), "-y" | "--yes") {
            continue;
        }
        if ["--cache=", "--userconfig=", "--globalconfig="]
            .iter()
            .any(|p| arg.starts_with(p))
        {
            cmd.arg(arg);
            continue;
        }
        if matches!(arg.as_str(), "--cache" | "--userconfig" | "--globalconfig") {
            cmd.arg(arg)
                .arg(args.next().ok_or_else(|| error("npx-cache.unavailable"))?);
            continue;
        }
        return Err(error("npx-cache.unavailable"));
    }
    let mut child =
        ManagedProcessGroup::spawn(&mut cmd).map_err(|_| error("npx-cache.unavailable"))?;
    let stdout = child
        .take_stdout()
        .ok_or_else(|| error("npx-cache.unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout
            .take(MAX_OUTPUT + 1)
            .read_to_string(&mut text)
            .map(|_| text)
    });
    let status = child
        .wait_timeout(CONFIG_TIMEOUT)
        .map_err(|_| error("npx-cache.unavailable"))?;
    if status.is_none() {
        let _ = child.force_kill();
    }
    drop(child);
    let output = reader
        .join()
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| error("npx-cache.unavailable"))?;
    if !status.is_some_and(|s| s.success()) || output.len() as u64 > MAX_OUTPUT {
        return Err(error("npx-cache.unavailable"));
    }
    let path = PathBuf::from(output.trim());
    if !path.is_absolute() {
        bail!(error("npx-cache.unavailable"));
    }
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairItem {
    pub path: String,
    pub error_code: Option<&'static str>,
}

pub fn remove_targets(cache: &Path, approved: &[PathBuf], _guard: &RepairGuard) -> Vec<RepairItem> {
    approved
        .iter()
        .map(|target| {
            let valid = validate_targets(cache, &[target.join("package.json")])
                .is_ok_and(|v| v.len() == 1 && &v[0] == target);
            let error_code = if !valid {
                Some("npx-cache.changed")
            } else if fs::remove_dir_all(target).is_err() {
                Some("npx-cache.cleanup-failed")
            } else {
                None
            };
            RepairItem {
                path: target.to_string_lossy().into_owned(),
                error_code,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn running_npx_adapter_does_not_block_user_confirmed_repair() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp
            .path()
            .join(if cfg!(windows) { "npx.cmd" } else { "npx" });
        #[cfg(windows)]
        fs::write(&executable, "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&executable, "#!/bin/sh\nsleep 30\n").unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = AcpAdapterConfig {
            command: executable.to_string_lossy().into_owned(),
            args: vec![],
            display_name: "running fixture".into(),
            env: Default::default(),
        };
        let (_, mut child) =
            crate::acp::adapter::spawn_adapter(&config, temp.path(), false, false).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        let guard =
            RepairGuard::acquire().expect("running adapters must not block confirmed repair");
        assert!(
            RepairGuard::acquire().is_err(),
            "duplicate repair must be rejected"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "repair must not terminate adapters"
        );
        drop(guard);
        drop(child);
    }
    #[test]
    fn repair_only_missing_root_manifests_and_preserve_other_cache() {
        let temp = tempfile::tempdir().unwrap();
        let cache = fs::canonicalize(temp.path()).unwrap();
        let a = cache.join("_npx/a");
        let b = cache.join("_npx/b");
        let healthy = cache.join("_npx/healthy");
        for dir in [&a, &b, &healthy, &cache.join("_cacache")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(healthy.join("package.json"), "{}").unwrap();
        let reason = format!(
            "npm error code ENOENT\nnpm error path {}\nnpm error path {}\nnpm error enoent Could not read package.json: Error: ENOENT: no such file or directory, open '{}'",
            a.join("package.json").display(),
            b.join("package.json").display(),
            a.join("package.json").display()
        );
        let paths = missing_manifests(&reason);
        assert_eq!(paths.len(), 2);
        let targets = validate_targets(&cache, &paths).unwrap();
        let guard = RepairGuard::acquire().unwrap();
        assert!(RepairGuard::acquire().is_err());
        let result = remove_targets(&cache, &targets, &guard);
        assert!(result.iter().all(|r| r.error_code.is_none()));
        assert!(!a.exists() && !b.exists());
        assert!(healthy.join("package.json").exists());
        assert!(cache.join("_cacache").exists());
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("package.json"), "{}").unwrap();
        let changed = remove_targets(&cache, &[a.clone()], &guard);
        assert_eq!(changed[0].error_code, Some("npx-cache.changed"));
        assert!(a.join("package.json").exists());
    }
    #[test]
    fn rejects_non_enoent_stack_paths_outside_and_restored_files() {
        assert!(
            missing_manifests("npm error code EACCES\nnpm error path /cache/_npx/a/package.json")
                .is_empty()
        );
        assert!(
            missing_manifests("npm error code ENOENT\nat /cache/_npx/a/package.json").is_empty()
        );
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        let target = root.join("_npx/a");
        fs::create_dir_all(&target).unwrap();
        assert!(validate_targets(&root, &[temp.path().join("outside/package.json")]).is_err());
        fs::write(target.join("package.json"), "{}").unwrap();
        assert!(validate_targets(&root, &[target.join("package.json")]).is_err());
        assert!(validate_targets(&root, &[target.join("../a/package.json")]).is_err());
    }
    #[test]
    fn rejects_linked_installations() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("cache");
        let external = temp.path().join("external");
        fs::create_dir_all(root.join("_npx")).unwrap();
        fs::create_dir_all(&external).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, root.join("_npx/a")).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&external, root.join("_npx/a")).is_err() {
                return;
            }
        }
        assert!(validate_targets(&root, &[root.join("_npx/a/package.json")]).is_err());
        assert!(external.exists());
    }

    #[cfg(windows)]
    #[test]
    fn resolves_cache_using_sibling_npm_and_agent_environment() {
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("custom-cache");
        let npm = temp.path().join("npm.cmd");
        fs::write(&npm, "@echo off\r\necho %npm_config_cache%\r\n").unwrap();
        let config = AcpAdapterConfig {
            command: temp.path().join("npx.cmd").to_string_lossy().into_owned(),
            args: vec!["-y".into(), "fixture@1".into()],
            display_name: "fixture".into(),
            env: [(
                "npm_config_cache".into(),
                expected.to_string_lossy().into_owned(),
            )]
            .into(),
        };
        assert_eq!(resolve_cache(&config, temp.path()).unwrap(), expected);
        let unsupported = AcpAdapterConfig {
            args: vec!["--unknown-option".into()],
            ..config
        };
        assert!(resolve_cache(&unsupported, temp.path()).is_err());
    }
}
