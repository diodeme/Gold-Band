use super::{
    environment::EnvironmentTool,
    model::{SetupError, fingerprint},
};
use crate::{
    config::{
        AcpAdapterConfig, DiagnosticError, ManagedAgentConfig, ManagedAgentId,
        ProviderDiagnosticSnapshot, SettingsConfig,
    },
    platform::DesktopPlatform,
    process::{find_executable_in_paths, resolved_child_path},
    storage::{
        GoldBandPaths, configuration::with_configuration, load_settings_file, read_json,
        save_settings_file, write_json,
    },
};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const CUSTOM_AGENT_BATCH_SIZE: usize = 50;

#[derive(Debug, Clone)]
pub struct SetupCandidate {
    pub id: ManagedAgentId,
    pub config: ManagedAgentConfig,
    pub expected_fingerprint: Option<String>,
}

pub enum CandidateDetection<'a> {
    Reuse(&'a ProviderDiagnosticSnapshot),
    Probe,
    Unavailable(Option<&'a ProviderDiagnosticSnapshot>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetupLaunchKind {
    Npx,
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SetupUnavailableReason {
    NotInstalled,
    RuntimeRequired {
        tool: EnvironmentTool,
    },
    DoctorFailed {
        launch: SetupLaunchKind,
        error: Option<DiagnosticError>,
    },
}

pub enum PreparedAdapter {
    Ready(AcpAdapterConfig),
    Unavailable(SetupUnavailableReason),
}

pub fn launch_kind(candidate: &SetupCandidate) -> Result<SetupLaunchKind> {
    Ok(
        if crate::npx_cache::is_npx(&candidate.config.executable_adapter(&candidate.id)?) {
            SetupLaunchKind::Npx
        } else {
            SetupLaunchKind::Local
        },
    )
}

/// Configured Agents reuse the Agent management domain's canonical Doctor snapshot.
/// Only unconfigured catalog candidates are eligible for setup-owned probing.
pub fn candidate_detection<'a>(
    candidate: &SetupCandidate,
    diagnostics: &'a BTreeMap<ManagedAgentId, ProviderDiagnosticSnapshot>,
) -> CandidateDetection<'a> {
    if candidate.expected_fingerprint.is_none() {
        return CandidateDetection::Probe;
    }
    let expected_fingerprint = candidate.config.diagnostic_fingerprint().ok();
    match diagnostics.get(&candidate.id) {
        Some(diagnostic)
            if diagnostic.available
                && expected_fingerprint.is_some()
                && diagnostic.config_fingerprint == expected_fingerprint =>
        {
            CandidateDetection::Reuse(diagnostic)
        }
        // A previous successful Doctor result without the current config fingerprint is
        // stale, not a failed Agent. Setup may refresh it without overwriting that result.
        Some(diagnostic) if diagnostic.available => CandidateDetection::Probe,
        diagnostic => CandidateDetection::Unavailable(diagnostic),
    }
}

pub fn candidates(
    settings: &SettingsConfig,
    after: Option<&str>,
) -> Result<(Vec<SetupCandidate>, Option<String>)> {
    let empty = BTreeMap::new();
    let configured = settings.agents.as_ref().unwrap_or(&empty);
    let mut candidates = Vec::new();
    if after.is_none() {
        let platform = DesktopPlatform::current();
        for entry in &crate::agent_catalog::builtin_agent_catalog().agents {
            if !entry.supports_platform(platform) {
                continue;
            }
            let id: ManagedAgentId = entry.id.parse()?;
            let existing = configured.get(&id);
            candidates.push(SetupCandidate {
                id,
                config: existing
                    .cloned()
                    .unwrap_or_else(|| ManagedAgentConfig::from_catalog(entry)),
                expected_fingerprint: existing.map(fingerprint).transpose()?,
            });
        }
    }
    let custom: Vec<_> = configured
        .iter()
        .filter(|(id, _)| {
            crate::agent_catalog::builtin_agent(id.as_str()).is_none()
                && after.is_none_or(|after| id.as_str() > after)
        })
        .take(CUSTOM_AGENT_BATCH_SIZE + 1)
        .collect();
    let next = (custom.len() > CUSTOM_AGENT_BATCH_SIZE)
        .then(|| custom[CUSTOM_AGENT_BATCH_SIZE - 1].0.as_str().to_string());
    for (id, config) in custom.into_iter().take(CUSTOM_AGENT_BATCH_SIZE) {
        candidates.push(SetupCandidate {
            id: id.clone(),
            config: config.clone(),
            expected_fingerprint: Some(fingerprint(config)?),
        });
    }
    Ok((candidates, next))
}

/// Discovers local ACP executables and prepared packages independently of saved instances.
/// A locally installed Agent CLI can use its catalog adapter; an npx launcher alone cannot.
pub fn prepared_adapter(
    candidate: &SetupCandidate,
    cwd: &Path,
    npx_launchable: bool,
) -> Result<PreparedAdapter> {
    let not_installed = || {
        Ok(PreparedAdapter::Unavailable(
            SetupUnavailableReason::NotInstalled,
        ))
    };
    let mut adapter = candidate.config.executable_adapter(&candidate.id)?;
    let path = resolved_child_path(adapter.env.get("PATH").map(OsStr::new));
    if let Some(path) = path.as_ref() {
        adapter
            .env
            .insert("PATH".into(), path.to_string_lossy().into_owned());
    }
    if crate::npx_cache::is_npx(&adapter) {
        if let Some(launch) =
            crate::npx_cache::cached_launch(&adapter, &adapter.args, cwd, &adapter.env)
        {
            adapter.command = launch.node.to_string_lossy().into_owned();
            adapter.args = std::iter::once(launch.script.to_string_lossy().into_owned())
                .chain(launch.args)
                .collect();
            return Ok(PreparedAdapter::Ready(adapter));
        }
        let installed = crate::agent_catalog::builtin_agent(candidate.id.as_str())
            .and_then(|entry| entry.installed_command.as_deref())
            .and_then(|command| find_executable_in_paths(command, path.as_deref()));
        if installed.is_none() {
            return not_installed();
        }
        if !npx_launchable {
            return Ok(PreparedAdapter::Unavailable(
                SetupUnavailableReason::RuntimeRequired {
                    tool: EnvironmentTool::Npx,
                },
            ));
        }
    }
    let executable = PathBuf::from(&adapter.command);
    if matches!(
        executable.file_stem().and_then(OsStr::to_str),
        Some("uv" | "uvx" | "bunx" | "pnpm" | "yarn")
    ) {
        return not_installed();
    }
    let executable = if executable.is_absolute() {
        executable.is_file().then_some(executable)
    } else {
        find_executable_in_paths(&adapter.command, path.as_deref())
    };
    let Some(executable) = executable else {
        return not_installed();
    };
    adapter.command = executable.to_string_lossy().into_owned();
    Ok(PreparedAdapter::Ready(adapter))
}

/// The caller also holds the existing per-Agent/diagnostic commit guard. A failed write is never ready.
pub fn register_success(
    paths: &GoldBandPaths,
    candidate: &SetupCandidate,
    diagnostic: &ProviderDiagnosticSnapshot,
    cancelled: &AtomicBool,
) -> Result<SettingsConfig> {
    with_configuration(&paths.user_gold_band_dir(), || {
        if cancelled.load(Ordering::Acquire) {
            return Err(SetupError::new("setup.probe-cancelled").into());
        }
        let mut settings = load_settings_file(&paths.user_settings_file())?;
        let existing = settings
            .agents
            .as_ref()
            .and_then(|agents| agents.get(&candidate.id));
        let actual = existing.map(fingerprint).transpose()?;
        let candidate_fingerprint = fingerprint(&candidate.config)?;
        let own_registration = candidate.expected_fingerprint.is_none()
            && actual.as_deref() == Some(candidate_fingerprint.as_str());
        if actual != candidate.expected_fingerprint && !own_registration {
            return Err(SetupError::new("setup.configuration-conflict").into());
        }
        if !diagnostic.available
            || diagnostic.config_fingerprint.as_deref()
                != Some(candidate.config.diagnostic_fingerprint()?.as_str())
        {
            return Err(SetupError::new("setup.agent-unavailable").into());
        }
        if actual.as_deref() != Some(candidate_fingerprint.as_str()) {
            settings
                .agents
                .get_or_insert_with(Default::default)
                .insert(candidate.id.clone(), candidate.config.clone());
            save_settings_file(&paths.user_settings_file(), &settings)?;
        }
        let path = paths.agent_diagnostics_file();
        let mut diagnostics: BTreeMap<String, ProviderDiagnosticSnapshot> = if path.exists() {
            read_json(&path)?
        } else {
            BTreeMap::new()
        };
        diagnostics.insert(candidate.id.as_str().to_string(), diagnostic.clone());
        write_json(&path, &diagnostics)?;
        load_settings_file(&paths.user_settings_file())
    })
    .map_err(|error| {
        if error.is::<SetupError>()
            || error.is::<crate::storage::configuration::ConfigurationError>()
        {
            error
        } else {
            SetupError::new("setup.commit-failed").into()
        }
    })
}

pub struct IsolatedProbeDirectory(pub camino::Utf8PathBuf);
impl IsolatedProbeDirectory {
    pub fn new() -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("gold-band-onboarding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)?;
        Ok(Self(
            camino::Utf8PathBuf::from_path_buf(path)
                .map_err(|_| SetupError::new("setup.probe-failed"))?,
        ))
    }
}
impl Drop for IsolatedProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
