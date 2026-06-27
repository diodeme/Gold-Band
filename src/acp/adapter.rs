use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Child, Stdio},
};

use anyhow::{Result, anyhow, ensure};

use crate::config::AcpAdapterConfig;
use crate::process::{background_command, find_executable_in_paths};

#[derive(Debug, Clone)]
pub struct ResolvedAcpAdapter {
    pub adapter_id: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
}

pub fn resolve_adapter(config: &AcpAdapterConfig) -> Result<ResolvedAcpAdapter> {
    ensure!(
        !config.command.trim().is_empty(),
        "ACP adapter command cannot be empty"
    );
    Ok(ResolvedAcpAdapter {
        adapter_id: config.command.clone(),
        display_name: config.display_name.clone(),
        command: config.command.clone(),
        args: normalize_args(&config.args),
    })
}

pub fn spawn_adapter(
    config: &AcpAdapterConfig,
    cwd: &std::path::Path,
    use_local_claude: bool,
) -> Result<(ResolvedAcpAdapter, Child)> {
    let adapter = resolve_adapter(config)?;
    let executable = platform_adapter_command(&adapter.command);
    let resolved_env = resolved_adapter_env(&config.env);
    let resolved_command =
        resolve_command_with_path(&executable, resolved_env.get("PATH").map(String::as_str));
    let mut command = background_command(&resolved_command);
    command
        .args(&adapter.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &resolved_env {
        command.env(key, value);
    }
    if use_local_claude && !resolved_env.contains_key("CLAUDE_CODE_EXECUTABLE") {
        if let Some(claude_path) =
            resolve_local_claude_executable(resolved_env.get("PATH").map(String::as_str))
        {
            command.env("CLAUDE_CODE_EXECUTABLE", claude_path);
        }
    }
    let child = command
        .spawn()
        .map_err(|error| anyhow!("failed to start ACP adapter `{}`: {error}", executable))?;
    Ok((adapter, child))
}

fn normalize_args(args: &[String]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| arg.split_whitespace().map(str::to_string))
        .collect()
}

#[cfg(windows)]
fn platform_adapter_command(command: &str) -> String {
    if command.eq_ignore_ascii_case("npx") {
        "npx.cmd".to_string()
    } else {
        command.to_string()
    }
}

#[cfg(not(windows))]
fn platform_adapter_command(command: &str) -> String {
    command.to_string()
}

fn resolved_adapter_env(config_env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env = config_env.clone();
    let suggested_dirs = suggested_path_dirs();
    let current_path = std::env::var("PATH").ok();
    let base_path = env
        .get("PATH")
        .map(String::as_str)
        .or(current_path.as_deref());
    if let Some(path) = augment_path_with_dirs(base_path, &suggested_dirs) {
        env.insert("PATH".to_string(), path);
    }
    env
}

fn resolve_command_with_path(command: &str, path: Option<&str>) -> String {
    if !command_requires_path_lookup(command) {
        return command.to_string();
    }
    find_executable_in_paths(command, path.map(OsStr::new))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| command.to_string())
}

fn resolve_local_claude_executable(path: Option<&str>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        resolve_local_claude_executable_windows(path)
    }
    #[cfg(not(windows))]
    {
        find_executable_in_paths("claude", path.map(OsStr::new))
    }
}

#[cfg(windows)]
fn resolve_local_claude_executable_windows(path: Option<&str>) -> Option<PathBuf> {
    let path_var = path?;
    for dir in std::env::split_paths(OsStr::new(path_var)) {
        let native = dir.join("claude.exe");
        if native.is_file() {
            return Some(native);
        }

        let has_claude_shim = dir.join("claude.cmd").is_file() || dir.join("claude").is_file();
        if has_claude_shim {
            let npm_native = dir.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
            if npm_native.is_file() {
                return Some(npm_native);
            }
        }
    }
    None
}

fn augment_path_with_dirs(base_path: Option<&str>, suggested_dirs: &[PathBuf]) -> Option<String> {
    let mut path_entries = base_path
        .map(|value| std::env::split_paths(OsStr::new(value)).collect::<Vec<_>>())
        .unwrap_or_default();
    for dir in suggested_dirs {
        if !path_entries.iter().any(|existing| existing == dir) {
            path_entries.push(dir.clone());
        }
    }
    if path_entries.is_empty() {
        return None;
    }
    std::env::join_paths(path_entries)
        .ok()
        .map(|value| value.to_string_lossy().into_owned())
}

fn suggested_path_dirs() -> Vec<PathBuf> {
    suggested_path_dirs_with_home(dirs::home_dir().as_deref())
}

fn suggested_path_dirs_with_home(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        push_dir_if_exists(&mut dirs, home.join(".local/bin"));
        push_dir_if_exists(&mut dirs, home.join(".cargo/bin"));
        push_dir_if_exists(&mut dirs, home.join(".opencode/bin"));
        push_dir_if_exists(&mut dirs, home.join(".volta/bin"));
        for dir in nvm_bin_dirs(home) {
            push_dir_if_exists(&mut dirs, dir);
        }
    }
    push_dir_if_exists(&mut dirs, PathBuf::from("/opt/homebrew/bin"));
    push_dir_if_exists(&mut dirs, PathBuf::from("/opt/homebrew/sbin"));
    push_dir_if_exists(&mut dirs, PathBuf::from("/usr/local/bin"));
    push_dir_if_exists(&mut dirs, PathBuf::from("/usr/local/sbin"));
    dirs
}

fn nvm_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let versions_dir = home.join(".nvm/versions/node");
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path().join("bin")))
        .collect()
}

fn push_dir_if_exists(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.is_dir() && !dirs.iter().any(|existing| existing == &dir) {
        dirs.push(dir);
    }
}

fn command_requires_path_lookup(command: &str) -> bool {
    let path = Path::new(command);
    !path.is_absolute() && path.components().count() == 1
}

#[cfg(test)]
mod tests {
    use super::{
        augment_path_with_dirs, resolve_command_with_path, resolve_local_claude_executable,
        spawn_adapter, suggested_path_dirs_with_home,
    };
    use crate::config::AcpAdapterConfig;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn suggested_path_dirs_include_nvm_bins() {
        let temp = tempdir().unwrap();
        let nvm_bin = temp.path().join(".nvm/versions/node/v24.14.1/bin");
        fs::create_dir_all(&nvm_bin).unwrap();

        let dirs = suggested_path_dirs_with_home(Some(temp.path()));

        assert!(dirs.iter().any(|dir| dir == &nvm_bin));
    }

    #[test]
    fn resolve_command_uses_augmented_path() {
        let temp = tempdir().unwrap();
        let adapter_bin = temp.path().join("adapter-bin");
        fs::create_dir_all(&adapter_bin).unwrap();
        fs::write(adapter_bin.join("npx"), "").unwrap();

        let path = augment_path_with_dirs(Some("/usr/bin:/bin"), &[adapter_bin.clone()]).unwrap();
        let resolved = resolve_command_with_path("npx", Some(path.as_str()));

        assert_eq!(resolved, adapter_bin.join("npx").to_string_lossy());
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_prefers_native_exe_on_windows() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("claude.exe"), "").unwrap();
        fs::write(temp.path().join("claude.cmd"), "").unwrap();
        fs::write(temp.path().join("claude"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, Some(temp.path().join("claude.exe")));
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_resolves_npm_package_binary_on_windows() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("claude.cmd"), "").unwrap();
        fs::write(temp.path().join("claude"), "").unwrap();
        let npm_bin = temp
            .path()
            .join("node_modules/@anthropic-ai/claude-code/bin");
        fs::create_dir_all(&npm_bin).unwrap();
        fs::write(npm_bin.join("claude.exe"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, Some(npm_bin.join("claude.exe")));
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_requires_windows_shim_before_using_npm_binary() {
        let temp = tempdir().unwrap();
        let npm_bin = temp
            .path()
            .join("node_modules/@anthropic-ai/claude-code/bin");
        fs::create_dir_all(&npm_bin).unwrap();
        fs::write(npm_bin.join("claude.exe"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, None);
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_skips_windows_shims_without_native_binary() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("claude.cmd"), "").unwrap();
        fs::write(temp.path().join("claude"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, None);
    }

    #[cfg(not(windows))]
    #[test]
    fn local_claude_uses_path_entry_on_unix() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("claude"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, Some(temp.path().join("claude")));
    }

    #[test]
    fn spawn_adapter_error_includes_os_failure_details() {
        let temp = tempdir().unwrap();
        let config = AcpAdapterConfig {
            command: "missing-acp-command-for-test".to_string(),
            args: Vec::new(),
            display_name: "Missing".to_string(),
            env: Default::default(),
        };

        let error = spawn_adapter(&config, temp.path(), false).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("missing-acp-command-for-test"));
        assert_ne!(
            message,
            "failed to start ACP adapter `missing-acp-command-for-test`"
        );
    }
}
