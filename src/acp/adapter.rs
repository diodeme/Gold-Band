use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Result, anyhow, ensure};

use crate::config::AcpAdapterConfig;
use crate::process::{
    ManagedProcessGroup, background_command, find_executable_in_paths, resolved_child_path,
};

const REQUIRE_LOCAL_CLAUDE_ENV: &str = "GOLD_BAND_REQUIRE_LOCAL_CLAUDE";

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
    require_local_claude_executable: bool,
) -> Result<(ResolvedAcpAdapter, ManagedProcessGroup)> {
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
    match local_claude_executable_for_env(use_local_claude, &resolved_env) {
        Some(claude_path) => {
            command.env("CLAUDE_CODE_EXECUTABLE", claude_path);
        }
        None if should_require_local_claude_resolution(
            use_local_claude,
            require_local_claude_executable,
            &resolved_env,
        ) =>
        {
            return Err(anyhow!(
                "local Claude executable is required but could not be resolved"
            ));
        }
        None => {}
    }
    let child = ManagedProcessGroup::spawn(&mut command)
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
    let configured_path = configured_path(&env);
    if let Some(path) = resolved_child_path(configured_path.map(OsStr::new)) {
        remove_configured_path_keys(&mut env);
        env.insert("PATH".to_string(), path.to_string_lossy().into_owned());
    }
    env
}

fn configured_path(env: &BTreeMap<String, String>) -> Option<&str> {
    #[cfg(windows)]
    return env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.as_str());

    #[cfg(not(windows))]
    env.get("PATH").map(String::as_str)
}

fn remove_configured_path_keys(env: &mut BTreeMap<String, String>) {
    #[cfg(windows)]
    env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));

    #[cfg(not(windows))]
    {
        env.remove("PATH");
    }
}

fn resolve_command_with_path(command: &str, path: Option<&str>) -> String {
    if !command_requires_path_lookup(command) {
        return command.to_string();
    }
    find_executable_in_paths(command, path.map(OsStr::new))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| command.to_string())
}

fn local_claude_executable_for_env(
    use_local_claude: bool,
    resolved_env: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    if !use_local_claude || resolved_env.contains_key("CLAUDE_CODE_EXECUTABLE") {
        return None;
    }
    resolve_local_claude_executable(resolved_env.get("PATH").map(String::as_str))
}

fn should_require_local_claude_resolution(
    use_local_claude: bool,
    require_local_claude_executable: bool,
    resolved_env: &BTreeMap<String, String>,
) -> bool {
    use_local_claude
        && !resolved_env.contains_key("CLAUDE_CODE_EXECUTABLE")
        && (require_local_claude_executable
            || require_local_claude_env_enabled(resolved_env.get(REQUIRE_LOCAL_CLAUDE_ENV)))
}

fn require_local_claude_env_enabled(config_value: Option<&String>) -> bool {
    if let Some(value) = config_value {
        return bool_flag_enabled(value);
    }
    std::env::var(REQUIRE_LOCAL_CLAUDE_ENV)
        .ok()
        .is_some_and(|value| bool_flag_enabled(&value))
}

fn bool_flag_enabled(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
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

        let cmd = dir.join("claude.cmd");
        if let Some(native) = resolve_windows_cmd_to_exe(&cmd) {
            return Some(native);
        }
    }
    None
}

#[cfg(windows)]
fn resolve_windows_cmd_to_exe(cmd_path: &Path) -> Option<PathBuf> {
    if !cmd_path.is_file()
        || !cmd_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd"))
    {
        return None;
    }

    let contents = std::fs::read_to_string(cmd_path).ok()?;
    let cmd_dir = cmd_path.parent()?;
    for line in contents.lines().rev() {
        if let Some(path) = resolve_cmd_line_exe_reference(line, cmd_dir) {
            return Some(path);
        }
    }
    None
}

#[cfg(windows)]
fn resolve_cmd_line_exe_reference(line: &str, cmd_dir: &Path) -> Option<PathBuf> {
    let lower = line.to_ascii_lowercase();
    let token_start = lower.find("%dp0%").or_else(|| lower.find("%~dp0"))?;
    let exe_end = lower[token_start..].find(".exe")? + token_start + ".exe".len();
    let raw = extract_cmd_path_reference(line, token_start, exe_end);
    let raw_lower = raw.to_ascii_lowercase();
    let token = if raw_lower.contains("%dp0%") {
        "%dp0%"
    } else if raw_lower.contains("%~dp0") {
        "%~dp0"
    } else {
        return None;
    };
    let token_index = raw_lower.find(token)?;
    let suffix = &raw[token_index + token.len()..];
    let suffix = suffix.trim_start_matches(['\\', '/']);
    let resolved = if suffix.is_empty() {
        cmd_dir.to_path_buf()
    } else {
        cmd_dir.join(suffix)
    };
    resolved.is_file().then_some(resolved)
}

#[cfg(windows)]
fn extract_cmd_path_reference(line: &str, token_start: usize, exe_end: usize) -> &str {
    if let Some(quote_start) = line[..token_start].rfind('"')
        && let Some(quote_end) = line[exe_end..].find('"')
    {
        return &line[quote_start + 1..exe_end + quote_end];
    }
    &line[token_start..exe_end]
}

fn command_requires_path_lookup(command: &str) -> bool {
    let path = Path::new(command);
    !path.is_absolute() && path.components().count() == 1
}

#[cfg(test)]
mod tests {
    use super::{
        local_claude_executable_for_env, resolve_command_with_path,
        resolve_local_claude_executable, should_require_local_claude_resolution, spawn_adapter,
    };
    use crate::config::AcpAdapterConfig;
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolve_command_uses_resolved_path() {
        let temp = tempdir().unwrap();
        let adapter_bin = temp.path().join("adapter-bin");
        fs::create_dir_all(&adapter_bin).unwrap();
        let executable_name = if cfg!(windows) { "npx.exe" } else { "npx" };
        fs::write(adapter_bin.join(executable_name), "").unwrap();

        let path = std::env::join_paths([adapter_bin.clone()]).unwrap();
        let resolved = resolve_command_with_path("npx", Some(path.to_str().unwrap()));

        assert_eq!(
            resolved,
            adapter_bin.join(executable_name).to_string_lossy()
        );
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
        let cmd_path = temp.path().join("claude.cmd");
        fs::write(temp.path().join("claude"), "").unwrap();
        let npm_bin = temp
            .path()
            .join("node_modules/@anthropic-ai/claude-code/bin");
        fs::create_dir_all(&npm_bin).unwrap();
        fs::write(npm_bin.join("claude.exe"), "").unwrap();
        fs::write(
            cmd_path,
            r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0
"%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe"   %*
"#,
        )
        .unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, Some(npm_bin.join("claude.exe")));
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_env_uses_npm_package_binary_on_windows() {
        let temp = tempdir().unwrap();
        let cmd_path = temp.path().join("claude.cmd");
        fs::write(temp.path().join("claude"), "").unwrap();
        let npm_bin = temp
            .path()
            .join("node_modules/@anthropic-ai/claude-code/bin");
        fs::create_dir_all(&npm_bin).unwrap();
        fs::write(npm_bin.join("claude.exe"), "").unwrap();
        fs::write(
            cmd_path,
            r#""%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe"   %*"#,
        )
        .unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let env = BTreeMap::from([("PATH".to_string(), path.to_string_lossy().into_owned())]);
        let resolved = local_claude_executable_for_env(true, &env);

        assert_eq!(resolved, Some(npm_bin.join("claude.exe")));
    }

    #[test]
    fn local_claude_env_respects_override_and_disabled_flag() {
        let env = BTreeMap::from([(
            "CLAUDE_CODE_EXECUTABLE".to_string(),
            "/custom/claude".to_string(),
        )]);

        assert!(local_claude_executable_for_env(true, &env).is_none());
        assert!(local_claude_executable_for_env(false, &BTreeMap::new()).is_none());
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
    fn local_claude_skips_bare_windows_shim_without_cmd_wrapper() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("claude"), "").unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, None);
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_skips_cmd_wrapper_when_target_is_missing() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("claude.cmd"),
            r#""%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe"   %*"#,
        )
        .unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, None);
    }

    #[cfg(windows)]
    #[test]
    fn local_claude_resolves_cmd_wrapper_with_tilde_dp0_token() {
        let temp = tempdir().unwrap();
        let npm_bin = temp
            .path()
            .join("node_modules/@anthropic-ai/claude-code/bin");
        fs::create_dir_all(&npm_bin).unwrap();
        fs::write(npm_bin.join("claude.exe"), "").unwrap();
        fs::write(
            temp.path().join("claude.cmd"),
            r#"%~dp0\node_modules\@anthropic-ai\claude-code\bin\claude.exe %*"#,
        )
        .unwrap();

        let path = std::env::join_paths([temp.path()]).unwrap();
        let resolved = resolve_local_claude_executable(path.to_str());

        assert_eq!(resolved, Some(npm_bin.join("claude.exe")));
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

        let error = spawn_adapter(&config, temp.path(), false, false).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("missing-acp-command-for-test"));
        assert_ne!(
            message,
            "failed to start ACP adapter `missing-acp-command-for-test`"
        );
    }

    #[test]
    fn local_claude_resolution_can_be_required_by_config() {
        let temp = tempdir().unwrap();
        let env = BTreeMap::from([(
            "PATH".to_string(),
            temp.path().to_string_lossy().into_owned(),
        )]);

        assert!(should_require_local_claude_resolution(true, true, &env));
    }
}
