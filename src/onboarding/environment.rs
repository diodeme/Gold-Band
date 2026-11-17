use crate::process::{
    ManagedProcessGroup, background_command, find_executable_in_paths, resolved_child_path,
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: u64 = 64 * 1024;
const PROBE_PRELOAD: &str = include_str!("../../resources/onboarding-runtime-probe.cjs");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPolicy {
    pub version: u32,
    pub status: String,
    pub node: NodePolicy,
    pub npm: NpmPolicy,
    pub git_help_url: String,
    pub probe_timeouts: ProbeTimeoutPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTimeoutPolicy {
    pub native_command_ms: u64,
    pub package_manager_command_ms: u64,
}

impl EnvironmentPolicy {
    fn probe_timeout(&self, tool: EnvironmentTool) -> Duration {
        let milliseconds = match tool {
            EnvironmentTool::Git | EnvironmentTool::Node => self.probe_timeouts.native_command_ms,
            EnvironmentTool::Npm | EnvironmentTool::Npx => {
                self.probe_timeouts.package_manager_command_ms
            }
        };
        Duration::from_millis(milliseconds)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePolicy {
    pub ranges: Vec<String>,
    pub verified_majors: Vec<u64>,
    pub recommended_version: String,
    pub help_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmPolicy {
    pub ranges: Vec<String>,
    pub verified_majors: Vec<u64>,
    pub matrix_versions: Vec<String>,
}

pub fn environment_policy() -> &'static EnvironmentPolicy {
    static POLICY: OnceLock<EnvironmentPolicy> = OnceLock::new();
    POLICY.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../configs/onboarding-environment-policy.json"
        ))
        .expect("bundled onboarding environment policy is valid")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentTool {
    Git,
    Node,
    Npm,
    Npx,
}

impl EnvironmentTool {
    pub fn command(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Node => "node",
            Self::Npm => "npm",
            Self::Npx => "npx",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnvironmentStatus {
    Ready,
    Missing,
    Unsupported,
    Unverified,
    Incompatible,
    Failed,
    Timeout,
    Cancelled,
}

impl EnvironmentStatus {
    /// A stable version outside the verified policy window is a warning, not a
    /// proof that the local toolchain cannot run. The joint runtime checks
    /// below still have to confirm paths, versions and npm engines.
    fn is_runtime_usable(self) -> bool {
        matches!(self, Self::Ready | Self::Unverified)
    }

    fn is_launched(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Unverified | Self::Unsupported | Self::Incompatible
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentCheck {
    pub tool: EnvironmentTool,
    pub status: EnvironmentStatus,
    pub version: Option<String>,
    pub executable_path: Option<PathBuf>,
    pub node_executable_path: Option<PathBuf>,
    #[serde(default)]
    pub node_version: Option<String>,
    pub error: Option<EnvironmentError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}")]
pub struct EnvironmentError {
    pub code: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub checks: Vec<EnvironmentCheck>,
    pub npx_launchable: bool,
}

impl EnvironmentReport {
    pub fn from_checks(checks: Vec<EnvironmentCheck>) -> Self {
        let npx_launchable = checks
            .iter()
            .any(|check| check.tool == EnvironmentTool::Npx && check.status.is_launched());
        Self {
            checks,
            npx_launchable,
        }
    }

    #[cfg(test)]
    fn runtime_ready(&self) -> bool {
        [
            EnvironmentTool::Node,
            EnvironmentTool::Npm,
            EnvironmentTool::Npx,
        ]
        .into_iter()
        .all(|tool| {
            self.checks
                .iter()
                .any(|check| check.tool == tool && check.status.is_runtime_usable())
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum EnvironmentIncompatibility {
    ProbeUnavailable,
    NodeMismatch,
    NodeVersionMismatch,
    NpmMetadataMissing,
    NpmEnginesIncompatible,
    NpmVersionMismatch,
    NpmPackageMismatch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeObservation {
    pub node_version: String,
    pub node_path: PathBuf,
    pub script_path: Option<PathBuf>,
    pub npm: Option<NpmObservation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpmObservation {
    pub root: PathBuf,
    pub version: String,
    pub node_range: Option<String>,
    pub node_compatible: bool,
}

#[derive(Debug, Clone)]
pub struct ProbeOutput {
    pub stdout: String,
    pub observations: Vec<RuntimeObservation>,
}

/// Injectable at the process boundary; tests never depend on the developer's installed tools.
pub trait EnvironmentRunner: Sync {
    fn resolve(&self, tool: EnvironmentTool) -> Option<PathBuf>;
    fn probe(
        &self,
        tool: EnvironmentTool,
        path: &Path,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<ProbeOutput, EnvironmentStatus>;
}

pub struct LocalEnvironmentRunner {
    path: Option<OsString>,
}

impl Default for LocalEnvironmentRunner {
    fn default() -> Self {
        Self {
            path: resolved_child_path(None),
        }
    }
}

impl LocalEnvironmentRunner {
    pub fn with_path(path: OsString) -> Self {
        Self { path: Some(path) }
    }
}

struct ProbeDirectory(PathBuf);
impl ProbeDirectory {
    fn new() -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("gold-band-environment-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_bounded(path: &Path) -> std::io::Result<String> {
    let mut result = String::new();
    File::open(path)?
        .take(OUTPUT_LIMIT)
        .read_to_string(&mut result)?;
    Ok(result)
}

impl EnvironmentRunner for LocalEnvironmentRunner {
    fn resolve(&self, tool: EnvironmentTool) -> Option<PathBuf> {
        find_executable_in_paths(tool.command(), self.path.as_deref())
    }

    fn probe(
        &self,
        tool: EnvironmentTool,
        path: &Path,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<ProbeOutput, EnvironmentStatus> {
        let run = || -> Result<ProbeOutput, EnvironmentStatus> {
            let failed = |_| EnvironmentStatus::Failed;
            if cancelled.load(Ordering::Acquire) {
                return Err(EnvironmentStatus::Cancelled);
            }
            let directory = ProbeDirectory::new().map_err(failed)?;
            let stdout_path = directory.0.join("stdout");
            let stderr_path = directory.0.join("stderr");
            let observation_path = directory.0.join("runtime.jsonl");
            let stdout = File::create(&stdout_path).map_err(failed)?;
            let stderr = File::create(&stderr_path).map_err(failed)?;
            let mut command = background_command(path);
            command
                .current_dir(&directory.0)
                .stdin(Stdio::null())
                .stdout(stdout)
                .stderr(stderr);
            if let Some(path) = &self.path {
                command.env("PATH", path);
            }
            if tool == EnvironmentTool::Node {
                // --version bypasses Node preloads; eval runs the same harmless instrumentation.
                command.args(["--eval", "process.stdout.write(process.versions.node)"]);
            } else {
                command.arg("--version");
            }
            if tool != EnvironmentTool::Git {
                let preload = directory.0.join("runtime.cjs");
                fs::write(&preload, PROBE_PRELOAD).map_err(failed)?;
                let mut options = std::env::var_os("NODE_OPTIONS").unwrap_or_default();
                options.push(format!(
                    " --require=\"{}\"",
                    preload.to_string_lossy().replace('\\', "/")
                ));
                command
                    .env("NODE_OPTIONS", options)
                    .env("GOLD_BAND_RUNTIME_PROBE_OUTPUT", &observation_path);
            }
            // Suppress npm update checks; no package execution, install, auth or user configuration writes.
            command.env("NPM_CONFIG_UPDATE_NOTIFIER", "false");
            let mut child = ManagedProcessGroup::spawn(&mut command).map_err(failed)?;
            let started = Instant::now();
            let status = loop {
                let terminal = if cancelled.load(Ordering::Acquire) {
                    Some(EnvironmentStatus::Cancelled)
                } else if started.elapsed() >= timeout {
                    Some(EnvironmentStatus::Timeout)
                } else if [&stdout_path, &stderr_path, &observation_path]
                    .iter()
                    .any(|path| fs::metadata(path).is_ok_and(|m| m.len() > OUTPUT_LIMIT))
                {
                    Some(EnvironmentStatus::Failed)
                } else {
                    None
                };
                if let Some(status) = terminal {
                    let _ = child.force_kill();
                    return Err(status);
                }
                if let Some(status) = child.try_wait().map_err(failed)? {
                    break status;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            if !status.success() {
                return Err(EnvironmentStatus::Failed);
            }
            let stdout = read_bounded(&stdout_path).map_err(failed)?;
            let observations = if tool == EnvironmentTool::Git {
                Vec::new()
            } else {
                read_bounded(&observation_path)
                    .map_err(failed)?
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| EnvironmentStatus::Failed)?
            };
            Ok(ProbeOutput {
                stdout,
                observations,
            })
        };
        run()
    }
}

fn matches_policy(version: &Version, ranges: &[String]) -> bool {
    version.pre.is_empty()
        && ranges.iter().any(|range| {
            // Policy uses only comparator intersections, not arbitrary npm engines syntax.
            let range = range.split_whitespace().collect::<Vec<_>>().join(", ");
            VersionReq::parse(&range).is_ok_and(|requirement| requirement.matches(version))
        })
}

/// A version outside the accepted ranges is either older than every verified
/// major (unsupported) or newer than the verified majors (unverified). Keeping
/// the two apart prevents telling users to downgrade a newer official release.
fn version_status(
    version: &Version,
    ranges: &[String],
    verified_majors: &[u64],
) -> EnvironmentStatus {
    if matches_policy(version, ranges) {
        return EnvironmentStatus::Ready;
    }
    let older_than_verified = verified_majors
        .iter()
        .min()
        .is_some_and(|oldest| version.major < *oldest);
    if !version.pre.is_empty() || older_than_verified || verified_majors.contains(&version.major) {
        EnvironmentStatus::Unsupported
    } else {
        EnvironmentStatus::Unverified
    }
}

fn failure(check: &mut EnvironmentCheck, status: EnvironmentStatus) {
    check.status = status;
    let code = match status {
        EnvironmentStatus::Missing => "setup.runtime-missing",
        EnvironmentStatus::Unsupported => "setup.runtime-version-unsupported",
        EnvironmentStatus::Unverified => "setup.runtime-version-unverified",
        EnvironmentStatus::Incompatible => "setup.runtime-incompatible",
        EnvironmentStatus::Timeout => "setup.probe-timeout",
        EnvironmentStatus::Cancelled => "setup.probe-cancelled",
        _ => "setup.probe-failed",
    };
    check.error = Some(EnvironmentError {
        code: code.into(),
        params: serde_json::json!({ "tool": check.tool, "version": check.version }),
    });
}

fn incompatible(
    check: &mut EnvironmentCheck,
    reason: EnvironmentIncompatibility,
    params: serde_json::Value,
) {
    failure(check, EnvironmentStatus::Incompatible);
    let details = check
        .error
        .as_mut()
        .unwrap()
        .params
        .as_object_mut()
        .unwrap();
    details.insert("reason".into(), serde_json::to_value(reason).unwrap());
    if let serde_json::Value::Object(params) = params {
        details.extend(params);
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

/// Independent tools complete progressively; only the final report can establish joint readiness.
pub fn check_environment(
    runner: &impl EnvironmentRunner,
    policy: &EnvironmentPolicy,
    cancelled: &AtomicBool,
    on_checked: impl Fn(&EnvironmentCheck) + Sync,
) -> EnvironmentReport {
    let tools = [
        EnvironmentTool::Git,
        EnvironmentTool::Node,
        EnvironmentTool::Npm,
        EnvironmentTool::Npx,
    ];
    let results = std::thread::scope(|scope| {
        let handles = tools.map(|tool| {
            let on_checked = &on_checked;
            scope.spawn(move || {
                let mut check = EnvironmentCheck {
                    tool,
                    status: EnvironmentStatus::Ready,
                    version: None,
                    executable_path: runner.resolve(tool),
                    node_executable_path: None,
                    node_version: None,
                    error: None,
                };
                let mut observation = None;
                if let Some(path) = &check.executable_path {
                    match runner.probe(tool, path, policy.probe_timeout(tool), cancelled) {
                        Err(status) => failure(&mut check, status),
                        Ok(output) => {
                            if tool == EnvironmentTool::Git {
                                match crate::git::parse_git_version_from_output(&output.stdout, "")
                                {
                                    Some(version) => {
                                        check.version = Some(version.display);
                                        if !version.semantic.pre.is_empty()
                                            || version.semantic
                                                < Version::parse(
                                                    crate::git::MINIMUM_SUPPORTED_GIT_VERSION,
                                                )
                                                .unwrap()
                                        {
                                            failure(&mut check, EnvironmentStatus::Unsupported);
                                        }
                                    }
                                    None => failure(&mut check, EnvironmentStatus::Failed),
                                }
                            } else {
                                let version =
                                    Version::parse(output.stdout.trim().trim_start_matches('v'))
                                        .ok();
                                observation =
                                    output.observations.into_iter().rev().find(|observed| {
                                        if tool == EnvironmentTool::Node {
                                            observed.script_path.is_none()
                                        } else {
                                            observed
                                                .script_path
                                                .as_deref()
                                                .and_then(Path::file_name)
                                                == Some(OsStr::new(
                                                    if tool == EnvironmentTool::Npm {
                                                        "npm-cli.js"
                                                    } else {
                                                        "npx-cli.js"
                                                    },
                                                ))
                                        }
                                    });
                                check.node_executable_path =
                                    observation.as_ref().map(|o| o.node_path.clone());
                                check.node_version =
                                    observation.as_ref().map(|o| o.node_version.clone());
                                if let Some(version) = version {
                                    check.version = Some(version.to_string());
                                    let (ranges, verified_majors) = if tool == EnvironmentTool::Node
                                    {
                                        (&policy.node.ranges, &policy.node.verified_majors)
                                    } else {
                                        (&policy.npm.ranges, &policy.npm.verified_majors)
                                    };
                                    let status = version_status(&version, ranges, verified_majors);
                                    if status != EnvironmentStatus::Ready {
                                        failure(&mut check, status);
                                    } else if observation.is_none() {
                                        incompatible(
                                            &mut check,
                                            EnvironmentIncompatibility::ProbeUnavailable,
                                            serde_json::json!({}),
                                        );
                                    }
                                } else {
                                    failure(&mut check, EnvironmentStatus::Failed);
                                }
                            }
                        }
                    }
                } else {
                    failure(&mut check, EnvironmentStatus::Missing);
                }
                on_checked(&check);
                (check, observation)
            })
        });
        handles
            .into_iter()
            .map(|h| h.join().expect("environment probe worker panicked"))
            .collect::<Vec<_>>()
    });
    let mut checks: Vec<_> = results.iter().map(|(check, _)| check.clone()).collect();
    let node = results[1].1.as_ref();
    let npm = results[2].1.as_ref().and_then(|o| o.npm.as_ref());
    for index in 1..checks.len() {
        if !checks[index].status.is_runtime_usable() {
            continue;
        }
        let observed = results[index].1.as_ref();
        use EnvironmentIncompatibility as Reason;
        let issue = match (node, observed) {
            (Some(node), Some(observed)) => {
                if !same_path(&node.node_path, &observed.node_path) {
                    Some((
                        Reason::NodeMismatch,
                        serde_json::json!({"expectedNodePath": node.node_path, "actualNodePath": observed.node_path}),
                    ))
                } else if checks[1].version.as_deref() != Some(observed.node_version.as_str()) {
                    Some((
                        Reason::NodeVersionMismatch,
                        serde_json::json!({"expectedVersion": checks[1].version, "actualVersion": observed.node_version}),
                    ))
                } else if index == 1 {
                    // `node` may be a launcher/shim that executes a different process path.
                    // Node/npm/npx runtime identity is established by the observed process paths above.
                    None
                } else if let Some(package) = observed.npm.as_ref() {
                    if package.node_range.is_none() {
                        Some((Reason::NpmMetadataMissing, serde_json::json!({})))
                    } else if !package.node_compatible {
                        Some((
                            Reason::NpmEnginesIncompatible,
                            serde_json::json!({"nodeRange": package.node_range}),
                        ))
                    } else if checks[index].version.as_ref() != Some(&package.version) {
                        Some((
                            Reason::NpmVersionMismatch,
                            serde_json::json!({"expectedVersion": checks[index].version, "actualVersion": package.version}),
                        ))
                    } else if let Some(npm) = npm {
                        if !same_path(&npm.root, &package.root) || npm.version != package.version {
                            Some((
                                Reason::NpmPackageMismatch,
                                serde_json::json!({"expectedNpmRoot": npm.root, "actualNpmRoot": package.root}),
                            ))
                        } else {
                            None
                        }
                    } else {
                        Some((Reason::NpmMetadataMissing, serde_json::json!({})))
                    }
                } else {
                    Some((Reason::NpmMetadataMissing, serde_json::json!({})))
                }
            }
            _ => Some((Reason::ProbeUnavailable, serde_json::json!({}))),
        };
        if let Some((reason, params)) = issue {
            incompatible(&mut checks[index], reason, params);
        }
    }
    EnvironmentReport::from_checks(checks)
}

#[cfg(test)]
mod tests;
