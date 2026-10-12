//! npx cache access: direct launch of exact-version packages npx already installed, and
//! user-confirmed repair of explicitly reported npx installation directories.
use crate::{
    config::AcpAdapterConfig,
    process::{ManagedProcessGroup, background_command},
};
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::Duration,
};

const MAX_TARGETS: usize = 16;
const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: u64 = 64 * 1024;
/// Distinct (npx, cwd, env) combinations whose npm cache location is remembered.
const MAX_REMEMBERED_CACHES: usize = 32;
static REPAIR_ACTIVE: Mutex<bool> = Mutex::new(false);
type CacheKey = (PathBuf, PathBuf, BTreeMap<String, String>);
static RESOLVED_CACHES: LazyLock<Mutex<HashMap<CacheKey, PathBuf>>> =
    LazyLock::new(Default::default);

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

/// `node <bin> <args>` equivalent of an `npx -y <name>@<exact version> <args>` launch.
#[derive(Debug, PartialEq, Eq)]
pub struct CachedNpxLaunch {
    pub node: PathBuf,
    pub script: PathBuf,
    pub args: Vec<String>,
}

/// Resolves an npx adapter launch to the package npx already installed, so the adapter runs
/// without the long-lived npx/cmd wrapper processes. Returns `None` whenever the launch is not
/// a plain exact-version npx invocation or the cache entry cannot be fully verified; callers
/// then launch through npx, which also (re)populates the cache.
pub fn cached_launch(
    config: &AcpAdapterConfig,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Option<CachedNpxLaunch> {
    let (spec, rest) = exact_npx_spec(&config.command, args)?;
    let npx = PathBuf::from(crate::acp::adapter::resolve_command_with_path(
        &crate::acp::adapter::platform_adapter_command(&config.command),
        env.get("PATH").map(String::as_str),
    ));
    let node = npx
        .parent()?
        .join(if cfg!(windows) { "node.exe" } else { "node" });
    if !node.is_file() {
        return None;
    }
    let cache = remembered_cache(config, &npx, cwd, env)?;
    let script = installed_bin(&cache, &spec)?;
    Some(CachedNpxLaunch {
        node,
        script,
        args: rest.to_vec(),
    })
}

#[derive(Debug, PartialEq, Eq)]
struct NpxSpec {
    raw: String,
    name: String,
    version: semver::Version,
}

/// Accepts only `npx [-y|--yes]... <name>@<exact semver> [args]`: other launcher options or
/// version ranges could make npx resolve something other than what is cached.
fn exact_npx_spec<'a>(command: &str, args: &'a [String]) -> Option<(NpxSpec, &'a [String])> {
    let file = Path::new(command)
        .file_name()?
        .to_str()?
        .to_ascii_lowercase();
    if !matches!(file.as_str(), "npx" | "npx.cmd" | "npx.exe") {
        return None;
    }
    let start = args.iter().position(|a| a != "-y" && a != "--yes")?;
    let raw = &args[start];
    if raw.starts_with('-') {
        return None;
    }
    let at = raw.rfind('@').filter(|&i| i > 0)?;
    let version = semver::Version::parse(&raw[at + 1..]).ok()?;
    Some((
        NpxSpec {
            raw: raw.clone(),
            name: raw[..at].to_owned(),
            version,
        },
        &args[start + 1..],
    ))
}

fn remembered_cache(
    config: &AcpAdapterConfig,
    npx: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    let key = (npx.to_path_buf(), cwd.to_path_buf(), env.clone());
    if let Some(cache) = RESOLVED_CACHES.lock().ok()?.get(&key) {
        return Some(cache.clone());
    }
    let cache = resolve_cache(config, cwd).ok()?;
    let mut caches = RESOLVED_CACHES.lock().ok()?;
    if caches.len() >= MAX_REMEMBERED_CACHES {
        caches.clear();
    }
    caches.insert(key, cache.clone());
    Some(cache)
}

/// Locates the entry script of `spec` in the npx directory npx itself would use, verifying the
/// directory was installed for exactly this spec and finished installing.
fn installed_bin(cache: &Path, spec: &NpxSpec) -> Option<PathBuf> {
    // npx keys single-package installs by the first 16 hex chars of sha512(spec).
    let digest = Sha512::digest(spec.raw.as_bytes());
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let root = cache.join("_npx").join(hash);
    let manifest = read_json(&root.join("package.json"))?;
    if manifest.pointer("/_npx/packages")? != &Value::from(vec![spec.raw.clone()]) {
        return None;
    }
    // Written by npm only after the install tree was fully reified.
    if !root.join("node_modules/.package-lock.json").is_file() {
        return None;
    }
    let package = root.join("node_modules").join(&spec.name);
    let manifest = read_json(&package.join("package.json"))?;
    if semver::Version::parse(manifest.get("version")?.as_str()?).ok()? != spec.version {
        return None;
    }
    let bin = match manifest.get("bin")? {
        Value::String(bin) => bin.as_str(),
        Value::Object(bins) => {
            let short = spec.name.rsplit('/').next().unwrap_or(&spec.name);
            match bins.get(short) {
                Some(bin) => bin.as_str()?,
                None if bins.len() == 1 => bins.values().next()?.as_str()?,
                None => return None,
            }
        }
        _ => return None,
    };
    let relative = Path::new(bin);
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let script = package.join(relative);
    script.is_file().then_some(script)
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
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

    const SPEC: &str = "@scope/fixture-acp@1.2.3";

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    fn spec() -> NpxSpec {
        exact_npx_spec("npx", &args(&["-y", SPEC])).unwrap().0
    }

    /// Lays out an npx install of `SPEC` the way npm 11 does and returns its root directory.
    fn install(cache: &Path, bin: Value) -> PathBuf {
        let hash: String = Sha512::digest(SPEC.as_bytes())[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let root = cache.join("_npx").join(hash);
        let package = root.join("node_modules/@scope/fixture-acp");
        fs::create_dir_all(package.join("dist")).unwrap();
        fs::write(
            root.join("package.json"),
            serde_json::json!({"dependencies": {"@scope/fixture-acp": "^1.2.3"}, "_npx": {"packages": [SPEC]}}).to_string(),
        )
        .unwrap();
        fs::write(root.join("node_modules/.package-lock.json"), "{}").unwrap();
        fs::write(
            package.join("package.json"),
            serde_json::json!({"name": "@scope/fixture-acp", "version": "1.2.3", "bin": bin})
                .to_string(),
        )
        .unwrap();
        fs::write(package.join("dist/index.js"), "").unwrap();
        root
    }

    #[test]
    fn only_plain_exact_version_npx_launches_are_eligible() {
        let launch = args(&["-y", SPEC, "--acp"]);
        let (spec, rest) = exact_npx_spec("C:/node/npx.cmd", &launch).unwrap();
        assert_eq!(spec.name, "@scope/fixture-acp");
        assert_eq!(spec.version, semver::Version::new(1, 2, 3));
        assert_eq!(rest, ["--acp"]);
        assert_eq!(
            exact_npx_spec("npx", &args(&["unscoped@0.1.0"]))
                .unwrap()
                .0
                .name,
            "unscoped"
        );

        for (command, values) in [
            ("npx", &["-y", "@scope/fixture-acp@^1.2.3"][..]),
            ("npx", &["-y", "@scope/fixture-acp@latest"]),
            ("npx", &["-y", "@scope/fixture-acp"]),
            ("npx", &["--registry", "https://example.invalid", SPEC]),
            ("npx", &["-y"]),
            ("npm", &["exec", "-y", SPEC]),
            ("node", &[SPEC]),
        ] {
            assert!(
                exact_npx_spec(command, &args(values)).is_none(),
                "{command} {values:?}"
            );
        }
    }

    #[test]
    fn locates_the_bin_of_the_npx_install_for_exactly_this_spec() {
        let temp = tempfile::tempdir().unwrap();
        let root = install(
            temp.path(),
            serde_json::json!({"fixture-acp": "dist/index.js", "other": "x.js"}),
        );
        let script = root.join("node_modules/@scope/fixture-acp/dist/index.js");
        assert_eq!(installed_bin(temp.path(), &spec()), Some(script.clone()));

        install(temp.path(), Value::from("dist/index.js"));
        assert_eq!(installed_bin(temp.path(), &spec()), Some(script.clone()));
        install(temp.path(), serde_json::json!({"renamed": "dist/index.js"}));
        assert_eq!(installed_bin(temp.path(), &spec()), Some(script));
    }

    #[test]
    fn unverifiable_installs_fall_back_to_npx() {
        let temp = tempfile::tempdir().unwrap();
        type BreakInstall = fn(&Path);
        let cases: [(&str, BreakInstall); 6] = [
            ("dependency of another npx install", |root| {
                fs::write(
                    root.join("package.json"),
                    r#"{"_npx":{"packages":["other@1.0.0"]}}"#,
                )
                .unwrap();
            }),
            ("install not finished", |root| {
                fs::remove_file(root.join("node_modules/.package-lock.json")).unwrap();
            }),
            ("different installed version", |root| {
                fs::write(
                    root.join("node_modules/@scope/fixture-acp/package.json"),
                    r#"{"version":"1.2.4","bin":"dist/index.js"}"#,
                )
                .unwrap();
            }),
            ("missing entry script", |root| {
                fs::remove_file(root.join("node_modules/@scope/fixture-acp/dist/index.js"))
                    .unwrap();
            }),
            ("ambiguous bin map", |root| {
                fs::write(
                    root.join("node_modules/@scope/fixture-acp/package.json"),
                    r#"{"version":"1.2.3","bin":{"a":"dist/index.js","b":"dist/index.js"}}"#,
                )
                .unwrap();
            }),
            ("bin escaping the package", |root| {
                fs::write(
                    root.join("node_modules/@scope/fixture-acp/package.json"),
                    r#"{"version":"1.2.3","bin":"../../../package.json"}"#,
                )
                .unwrap();
            }),
        ];
        for (name, break_install) in cases {
            let root = install(temp.path(), Value::from("dist/index.js"));
            break_install(&root);
            assert_eq!(installed_bin(temp.path(), &spec()), None, "{name}");
        }
        assert_eq!(installed_bin(&temp.path().join("absent"), &spec()), None);
    }

    #[cfg(windows)]
    #[test]
    fn cached_launch_runs_node_next_to_npx_with_the_remaining_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let root = install(&cache, Value::from("dist/index.js"));
        fs::write(temp.path().join("node.exe"), "").unwrap();
        fs::write(
            temp.path().join("npm.cmd"),
            "@echo off\r\necho %npm_config_cache%\r\n",
        )
        .unwrap();
        let config = AcpAdapterConfig {
            command: temp.path().join("npx.cmd").to_string_lossy().into_owned(),
            args: args(&["-y", SPEC, "--acp"]),
            display_name: "fixture".into(),
            env: [(
                "npm_config_cache".into(),
                cache.to_string_lossy().into_owned(),
            )]
            .into(),
        };
        let env = crate::acp::adapter::resolved_adapter_env(&config.env);
        assert_eq!(
            cached_launch(&config, &config.args, temp.path(), &env),
            Some(CachedNpxLaunch {
                node: temp.path().join("node.exe"),
                script: root.join("node_modules/@scope/fixture-acp/dist/index.js"),
                args: args(&["--acp"]),
            })
        );
        fs::remove_file(temp.path().join("node.exe")).unwrap();
        assert_eq!(
            cached_launch(&config, &config.args, temp.path(), &env),
            None
        );
    }

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
