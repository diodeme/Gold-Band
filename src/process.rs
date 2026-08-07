use anyhow::{Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

#[cfg(unix)]
use std::io::{Read, Seek};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::sync::OnceLock;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(unix)]
const LOGIN_SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const LOGIN_SHELL_OUTPUT_LIMIT: u64 = 1024 * 1024;
#[cfg(unix)]
const LOGIN_SHELL_ENV_START: &[u8] = b"_GOLD_BAND_SHELL_ENV_START_";
#[cfg(unix)]
const LOGIN_SHELL_ENV_END: &[u8] = b"_GOLD_BAND_SHELL_ENV_END_";
#[cfg(unix)]
const LOGIN_SHELL_ENV_COMMAND: &str = "command printf '_GOLD_BAND_SHELL_ENV_START_'; command env; command printf '_GOLD_BAND_SHELL_ENV_END_'; exit";

#[cfg(unix)]
static LOGIN_SHELL_PATH: OnceLock<Option<OsString>> = OnceLock::new();

/// Resolves the PATH inherited by an external desktop child process.
///
/// Explicit adapter entries always have the highest priority. Unix desktop
/// builds then use the user's cached login-shell PATH before the current
/// process PATH. Windows refreshes the user and machine PATH values from the
/// registry for every launch. Common platform locations are appended last.
pub fn resolved_child_path(configured_path: Option<&OsStr>) -> Option<OsString> {
    let current_path = std::env::var_os("PATH");

    #[cfg(unix)]
    let login_shell_path = login_shell_path();
    #[cfg(not(unix))]
    let login_shell_path: Option<OsString> = None;

    #[cfg(windows)]
    let platform_paths = windows_registry_paths();
    #[cfg(not(windows))]
    let platform_paths = Vec::new();

    #[cfg(not(windows))]
    let suggested_dirs = suggested_unix_path_dirs();
    #[cfg(windows)]
    let suggested_dirs: Vec<PathBuf> = Vec::new();

    resolved_child_path_from_sources(
        configured_path,
        login_shell_path.as_deref(),
        current_path.as_deref(),
        &platform_paths,
        &suggested_dirs,
        !cfg!(windows),
    )
}

fn resolved_child_path_from_sources(
    configured_path: Option<&OsStr>,
    login_shell_path: Option<&OsStr>,
    current_path: Option<&OsStr>,
    platform_paths: &[OsString],
    appended_entries: &[PathBuf],
    case_sensitive: bool,
) -> Option<OsString> {
    let mut sources = Vec::new();
    if let Some(path) = configured_path.filter(|path| !path.is_empty()) {
        sources.push(path.to_os_string());
    }
    if let Some(path) = login_shell_path.filter(|path| !path.is_empty()) {
        sources.push(path.to_os_string());
    }
    if let Some(path) = current_path.filter(|path| !path.is_empty()) {
        sources.push(path.to_os_string());
    }
    sources.extend(platform_paths.iter().cloned());

    merge_path_sources(&sources, appended_entries, case_sensitive)
}

fn merge_path_sources(
    sources: &[OsString],
    appended_entries: &[PathBuf],
    case_sensitive: bool,
) -> Option<OsString> {
    let mut entries = Vec::new();
    for source in sources {
        for entry in std::env::split_paths(source) {
            push_unique_path(&mut entries, entry, case_sensitive);
        }
    }
    for entry in appended_entries {
        push_unique_path(&mut entries, entry.clone(), case_sensitive);
    }

    (!entries.is_empty())
        .then(|| std::env::join_paths(entries).ok())
        .flatten()
}

fn push_unique_path(entries: &mut Vec<PathBuf>, entry: PathBuf, case_sensitive: bool) {
    if entry.as_os_str().is_empty()
        || entries
            .iter()
            .any(|existing| paths_equal(existing, &entry, case_sensitive))
    {
        return;
    }
    entries.push(entry);
}

fn paths_equal(left: &Path, right: &Path, case_sensitive: bool) -> bool {
    if case_sensitive {
        left == right
    } else {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }
}

#[cfg(windows)]
fn windows_registry_paths() -> Vec<OsString> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    const USER_ENVIRONMENT: &str = "Environment";
    const MACHINE_ENVIRONMENT: &str =
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

    [
        (HKEY_CURRENT_USER, USER_ENVIRONMENT),
        (HKEY_LOCAL_MACHINE, MACHINE_ENVIRONMENT),
    ]
    .into_iter()
    .filter_map(|(hive, key)| {
        let value: String = RegKey::predef(hive)
            .open_subkey(key)
            .ok()?
            .get_value("Path")
            .ok()?;
        let expanded = expand_windows_environment_variables(&value);
        (!expanded.is_empty()).then(|| OsString::from(expanded))
    })
    .collect()
}

#[cfg(windows)]
fn expand_windows_environment_variables(value: &str) -> String {
    let environment = std::env::vars()
        .map(|(key, value)| (key.to_ascii_uppercase(), value))
        .collect::<std::collections::HashMap<_, _>>();
    expand_windows_environment_variables_with(value, |name| {
        environment.get(&name.to_ascii_uppercase()).cloned()
    })
}

#[cfg(windows)]
fn expand_windows_environment_variables_with(
    value: &str,
    resolve: impl Fn(&str) -> Option<String>,
) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut remainder = value;
    while let Some(start) = remainder.find('%') {
        expanded.push_str(&remainder[..start]);
        let after_start = &remainder[start + 1..];
        let Some(end) = after_start.find('%') else {
            expanded.push_str(&remainder[start..]);
            return expanded;
        };
        let name = &after_start[..end];
        if let Some(resolved) = (!name.is_empty()).then(|| resolve(name)).flatten() {
            expanded.push_str(&resolved);
        } else {
            expanded.push('%');
            expanded.push_str(name);
            expanded.push('%');
        }
        remainder = &after_start[end + 1..];
    }
    expanded.push_str(remainder);
    expanded
}

#[cfg(not(windows))]
fn suggested_unix_path_dirs() -> Vec<PathBuf> {
    suggested_unix_path_dirs_with_home(dirs::home_dir().as_deref())
}

#[cfg(not(windows))]
fn suggested_unix_path_dirs_with_home(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = home {
        push_dir_if_exists(&mut dirs, home.join(".local/bin"));
        push_dir_if_exists(&mut dirs, home.join(".cargo/bin"));
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

#[cfg(not(windows))]
fn nvm_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let versions_dir = home.join(".nvm/versions/node");
    let Ok(entries) = std::fs::read_dir(versions_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path().join("bin")))
        .collect()
}

#[cfg(not(windows))]
fn push_dir_if_exists(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dir.is_dir() && !dirs.iter().any(|existing| existing == &dir) {
        dirs.push(dir);
    }
}

#[cfg(unix)]
fn login_shell_path() -> Option<OsString> {
    LOGIN_SHELL_PATH
        .get_or_init(|| {
            discover_login_shell_path(
                &login_shell_candidates(),
                dirs::home_dir().as_deref(),
                LOGIN_SHELL_PATH_TIMEOUT,
            )
        })
        .clone()
}

#[cfg(unix)]
fn login_shell_candidates() -> Vec<PathBuf> {
    use uzers::os::unix::UserExt;

    let account_shell =
        uzers::get_user_by_uid(uzers::get_current_uid()).map(|user| user.shell().to_path_buf());
    let environment_shell = std::env::var_os("SHELL").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let fallbacks = ["/bin/zsh", "/bin/bash", "/bin/sh"];
    #[cfg(not(target_os = "macos"))]
    let fallbacks = ["/bin/bash", "/bin/zsh", "/bin/sh"];

    login_shell_candidates_from(
        account_shell,
        environment_shell,
        &fallbacks.map(PathBuf::from),
    )
}

#[cfg(unix)]
fn login_shell_candidates_from(
    account_shell: Option<PathBuf>,
    environment_shell: Option<PathBuf>,
    fallback_shells: &[PathBuf],
) -> Vec<PathBuf> {
    let mut shells = Vec::new();
    for shell in account_shell
        .into_iter()
        .chain(environment_shell)
        .chain(fallback_shells.iter().cloned())
    {
        if shell.is_file() && !shells.iter().any(|existing| existing == &shell) {
            shells.push(shell);
        }
    }
    shells
}

#[cfg(unix)]
fn discover_login_shell_path(
    shells: &[PathBuf],
    home: Option<&Path>,
    timeout: Duration,
) -> Option<OsString> {
    let deadline = Instant::now().checked_add(timeout)?;
    for shell in shells {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match run_login_shell_path(shell, home, remaining) {
            Ok(Some(path)) => return Some(path),
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(shell = %shell.display(), %error, "failed to read login-shell PATH");
            }
        }
    }
    None
}

#[cfg(unix)]
fn run_login_shell_path(
    shell: &Path,
    home: Option<&Path>,
    timeout: Duration,
) -> std::io::Result<Option<OsString>> {
    use wait_timeout::ChildExt;

    let mut output_file = tempfile::tempfile()?;
    let child_output = output_file.try_clone()?;
    let mut command = background_command(shell);
    command
        .args(["-ilc", LOGIN_SHELL_ENV_COMMAND])
        .stdin(Stdio::null())
        .stdout(Stdio::from(child_output))
        .stderr(Stdio::null())
        .env("DISABLE_AUTO_UPDATE", "true")
        .env("ZSH_TMUX_AUTOSTARTED", "true")
        .env("ZSH_TMUX_AUTOSTART", "false");
    if let Some(home) = home.filter(|path| path.is_dir()) {
        command.current_dir(home);
    }

    let mut child = command.spawn()?;
    let Some(status) = child.wait_timeout(timeout)? else {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(None);
    };
    if !status.success() {
        return Ok(None);
    }

    output_file.rewind()?;
    let mut output = Vec::new();
    output_file
        .take(LOGIN_SHELL_OUTPUT_LIMIT)
        .read_to_end(&mut output)?;
    Ok(parse_login_shell_path_output(&output))
}

#[cfg(unix)]
fn parse_login_shell_path_output(output: &[u8]) -> Option<OsString> {
    let start = find_bytes(output, LOGIN_SHELL_ENV_START)? + LOGIN_SHELL_ENV_START.len();
    let end = find_bytes(&output[start..], LOGIN_SHELL_ENV_END)? + start;
    let environment = &output[start..end];
    for line in environment.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(path) = line.strip_prefix(b"PATH=")
            && !path.is_empty()
        {
            return Some(OsString::from_vec(path.to_vec()));
        }
    }
    None
}

#[cfg(unix)]
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    (!needle.is_empty())
        .then(|| {
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        })
        .flatten()
}

pub fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = resolved_child_path(None);
    find_executable_in_paths(name, path_var.as_deref())
}

pub fn find_executable_in_paths(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path_var = path_var?;
    for dir in std::env::split_paths(&path_var) {
        #[cfg(windows)]
        {
            for candidate in windows_executable_candidates(&dir, name) {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    const EXECUTABLE_EXTENSIONS: [&str; 4] = ["exe", "com", "cmd", "bat"];

    if Path::new(name).extension().is_some() {
        return vec![dir.join(name)];
    }

    EXECUTABLE_EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("{name}.{extension}")))
        .collect()
}

/// Creates a command for non-interactive background work.
///
/// Desktop runtime callers should use this for helper CLI processes such as Git,
/// MCP stdio checks, and Windows shell fallbacks so the app does not surface a
/// transient console window while the command runs.
pub fn background_command(program: impl AsRef<OsStr>) -> ProcessCommand {
    let mut command = ProcessCommand::new(program);
    apply_background_process_flags(&mut command);
    command
}

pub fn apply_background_process_flags(_command: &mut ProcessCommand) {
    #[cfg(windows)]
    _command.creation_flags(CREATE_NO_WINDOW);
}

pub fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let status = background_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process tree for pid {pid}");
        }
    }
    #[cfg(not(windows))]
    {
        let status = background_command("kill")
            .args(["-TERM", &pid.to_string()])
            .status()?;
        if !status.success() {
            bail!("failed to kill provider process for pid {pid}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{merge_path_sources, paths_equal, resolved_child_path_from_sources};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[test]
    fn path_sources_preserve_priority_and_remove_duplicates() {
        let configured = std::env::join_paths(["configured-bin", "shared-bin"]).unwrap();
        let current = std::env::join_paths(["current-bin", "shared-bin"]).unwrap();
        let user = std::env::join_paths(["user-bin", "shared-bin"]).unwrap();
        let system = std::env::join_paths(["system-bin", "current-bin"]).unwrap();

        let merged = merge_path_sources(
            &[configured, current, user, system],
            &["fallback-bin".into()],
            true,
        )
        .unwrap();
        let entries = std::env::split_paths(&merged).collect::<Vec<_>>();

        assert_eq!(
            entries,
            vec![
                Path::new("configured-bin"),
                Path::new("shared-bin"),
                Path::new("current-bin"),
                Path::new("user-bin"),
                Path::new("system-bin"),
                Path::new("fallback-bin"),
            ]
        );
    }

    #[test]
    fn windows_path_comparison_is_ascii_case_insensitive() {
        assert!(paths_equal(
            Path::new(r"C:\Users\Example\bin"),
            Path::new(r"c:\users\example\BIN"),
            false,
        ));
        assert!(!paths_equal(
            Path::new(r"C:\Users\Example\bin"),
            Path::new(r"c:\users\example\BIN"),
            true,
        ));
    }

    #[test]
    fn empty_sources_do_not_create_an_empty_path() {
        assert_eq!(merge_path_sources(&[], &[], true), None::<OsString>);
    }

    #[test]
    fn resolved_child_path_preserves_cross_platform_source_priority() {
        let configured = std::env::join_paths(["configured-bin", "shared-bin"]).unwrap();
        let login = std::env::join_paths(["login-bin", "shared-bin"]).unwrap();
        let current = std::env::join_paths(["current-bin", "login-bin"]).unwrap();
        let platform = std::env::join_paths(["platform-bin", "current-bin"]).unwrap();

        let resolved = resolved_child_path_from_sources(
            Some(configured.as_os_str()),
            Some(login.as_os_str()),
            Some(current.as_os_str()),
            &[platform],
            &[PathBuf::from("fallback-bin")],
            true,
        )
        .unwrap();

        assert_eq!(
            std::env::split_paths(&resolved).collect::<Vec<_>>(),
            vec![
                Path::new("configured-bin"),
                Path::new("shared-bin"),
                Path::new("login-bin"),
                Path::new("current-bin"),
                Path::new("platform-bin"),
                Path::new("fallback-bin"),
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_lookup_prefers_native_binary_over_npm_shims() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        fs::write(temp.path().join("opencode"), "#!/bin/sh\n").unwrap();
        fs::write(temp.path().join("opencode.cmd"), "@ECHO off\r\n").unwrap();
        fs::write(temp.path().join("opencode.exe"), "").unwrap();
        let path = std::env::join_paths([temp.path()]).unwrap();

        let resolved = super::find_executable_in_paths("opencode", Some(path.as_os_str()));

        assert_eq!(resolved, Some(temp.path().join("opencode.exe")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_lookup_uses_cmd_instead_of_extensionless_npm_shim() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        fs::write(temp.path().join("opencode"), "#!/bin/sh\n").unwrap();
        fs::write(temp.path().join("opencode.cmd"), "@ECHO off\r\n").unwrap();
        let path = std::env::join_paths([temp.path()]).unwrap();

        let resolved = super::find_executable_in_paths("opencode", Some(path.as_os_str()));

        assert_eq!(resolved, Some(temp.path().join("opencode.cmd")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_bare_command_ignores_extensionless_unix_shim() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        fs::write(temp.path().join("opencode"), "#!/bin/sh\n").unwrap();
        let path = std::env::join_paths([temp.path()]).unwrap();

        let resolved = super::find_executable_in_paths("opencode", Some(path.as_os_str()));

        assert_eq!(resolved, None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_explicit_command_extension_is_preserved() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        fs::write(temp.path().join("opencode.cmd"), "@ECHO off\r\n").unwrap();
        fs::write(temp.path().join("opencode.cmd.exe"), "").unwrap();
        let path = std::env::join_paths([temp.path()]).unwrap();

        let resolved = super::find_executable_in_paths("opencode.cmd", Some(path.as_os_str()));

        assert_eq!(resolved, Some(temp.path().join("opencode.cmd")));
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_output_parser_ignores_profile_noise() {
        use std::os::unix::ffi::OsStrExt;

        let output = b"profile banner\n_GOLD_BAND_SHELL_ENV_START_HOME=/tmp\nPATH=/login/bin:/shared/bin\n_GOLD_BAND_SHELL_ENV_END_trailing";

        let path = super::parse_login_shell_path_output(output).unwrap();

        assert_eq!(path.as_os_str().as_bytes(), b"/login/bin:/shared/bin");
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_runner_reads_path_from_an_executable_shell() {
        use std::fs;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let shell = temp.path().join("test-shell");
        fs::write(
            &shell,
            "#!/bin/sh\nprintf 'profile banner\\n_GOLD_BAND_SHELL_ENV_START_HOME=/tmp\\nPATH=/login/bin:/shared/bin\\n_GOLD_BAND_SHELL_ENV_END_'\n",
        )
        .unwrap();
        fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();

        let path = super::discover_login_shell_path(
            std::slice::from_ref(&shell),
            Some(temp.path()),
            Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(path.as_os_str().as_bytes(), b"/login/bin:/shared/bin");
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_runner_stops_at_the_discovery_timeout() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let shell = temp.path().join("blocking-shell");
        fs::write(&shell, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
        fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();

        let path = super::discover_login_shell_path(
            std::slice::from_ref(&shell),
            Some(temp.path()),
            Duration::from_millis(50),
        );

        assert!(path.is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_candidates_prefer_account_shell_and_deduplicate() {
        use std::fs;
        use tempfile::tempdir;

        let temp = tempdir().unwrap();
        let account = temp.path().join("account-shell");
        let environment = temp.path().join("environment-shell");
        fs::write(&account, "").unwrap();
        fs::write(&environment, "").unwrap();

        let candidates = super::login_shell_candidates_from(
            Some(account.clone()),
            Some(environment.clone()),
            &[account.clone(), PathBuf::from("missing-shell")],
        );

        assert_eq!(candidates, vec![account, environment]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_registry_variables_expand_case_insensitively() {
        let expanded = super::expand_windows_environment_variables_with(
            r"%SystemRoot%\System32;%USERPROFILE%\bin;%UNKNOWN%\bin",
            |name| match name.to_ascii_uppercase().as_str() {
                "SYSTEMROOT" => Some(r"C:\Windows".to_string()),
                "USERPROFILE" => Some(r"C:\Users\Example".to_string()),
                _ => None,
            },
        );

        assert_eq!(
            expanded,
            r"C:\Windows\System32;C:\Users\Example\bin;%UNKNOWN%\bin"
        );
    }
}
