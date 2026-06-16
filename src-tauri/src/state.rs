use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::events::current_timestamp;
use gold_band::app::App;
use gold_band::config::{
    ManagedAgentType, RuntimeConfig, SettingsConfig, StateConfig,
};
use gold_band::process::kill_process_tree;
use gold_band::provider::DoctorResult;
use gold_band::storage::{GoldBandPaths, active_storage_path_config, read_json, write_json};
use serde::{Deserialize, Serialize};

use crate::updater::{UpdateInfoVm, UpdateStatusVm, initial_update_status};

#[derive(Debug, Clone)]
pub struct DesktopContext {
    pub repo_root: Utf8PathBuf,
    pub config: RuntimeConfig,
    pub recent_workspaces: Vec<String>,
    pub needs_workspace: bool,
}

impl DesktopContext {
    pub fn from_current_dir() -> Result<Self> {
        let cwd = std::env::current_dir().context("failed to read current directory")?;
        let cwd = Utf8PathBuf::from_path_buf(cwd)
            .map_err(|_| anyhow::anyhow!("working directory is not valid UTF-8"))?;
        Self::from_workspace(resolve_initial_workspace(&cwd))
    }

    pub fn from_workspace(repo_root: Utf8PathBuf) -> Result<Self> {
        let paths = GoldBandPaths::new(repo_root.clone());
        let (settings, _) = load_configs(&paths);
        let needs_workspace = resolve_configured_workspace(&settings).is_none()
            && find_workspace_root(&repo_root).is_none();
        let repo_root = resolve_configured_workspace(&settings)
            .or_else(|| find_workspace_root(&repo_root))
            .unwrap_or(repo_root);
        let paths = GoldBandPaths::new(repo_root.clone());
        let (settings, state) = load_configs(&paths);
        let config = RuntimeConfig::default()
            .apply_settings(&settings)
            .apply_state(&state);
        let mut recent_workspaces = recent_workspaces(&state, &repo_root);
        if needs_workspace {
            recent_workspaces.retain(|w| w != repo_root.as_str());
        }
        Ok(Self {
            repo_root,
            config,
            recent_workspaces,
            needs_workspace,
        })
    }

    pub fn app(&self) -> App {
        App::with_config(self.repo_root.clone(), self.config.clone())
    }

    pub fn app_with_acp_live_update(
        &self,
        live_update: Arc<
            dyn Fn(
                    gold_band::app::AcpLiveEventContext,
                    gold_band::acp::events::AcpUiEvent,
                ) -> anyhow::Result<()>
                + Send
                + Sync,
        >,
        session_update: Arc<
            dyn Fn(gold_band::app::AcpLiveEventContext) -> anyhow::Result<()> + Send + Sync,
        >,
    ) -> App {
        self.app()
            .with_acp_live_update(live_update)
            .with_acp_session_update(session_update)
    }

    pub fn app_with_metrics(
        &self,
        live_update: Arc<
            dyn Fn(
                    gold_band::app::AcpLiveEventContext,
                    gold_band::acp::events::AcpUiEvent,
                ) -> anyhow::Result<()>
                + Send
                + Sync,
        >,
        session_update: Arc<
            dyn Fn(gold_band::app::AcpLiveEventContext) -> anyhow::Result<()> + Send + Sync,
        >,
        metrics_callback: Arc<
            dyn Fn(gold_band::app::MetricsEventContext, gold_band::app::MetricsEvent) + Send + Sync,
        >,
    ) -> App {
        self.app()
            .with_acp_live_update(live_update)
            .with_acp_session_update(session_update)
            .with_metrics_callback(metrics_callback)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDiagnosticState {
    pub available: bool,
    pub reason: Option<String>,
    pub checked_at: String,
    pub capabilities: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
pub enum UpdateBadgeSeenTarget {
    SettingsEntry,
    SettingsAdvanced,
    Announcement,
}

pub struct DesktopState {
    context: Mutex<DesktopContext>,
    agent_diagnostics: Mutex<BTreeMap<ManagedAgentType, AgentDiagnosticState>>,
    update_status: Mutex<UpdateStatusVm>,
    pending_critical_update: Mutex<Option<Utf8PathBuf>>,
}

impl DesktopState {
    pub fn new(context: DesktopContext) -> Self {
        let persisted_diagnostics = load_persisted_agent_diagnostics(&context);
        let updater_last_checked_at = context.config.desktop_updater_last_checked_at.clone();
        Self {
            context: Mutex::new(context),
            agent_diagnostics: Mutex::new(persisted_diagnostics),
            update_status: Mutex::new(initial_update_status(updater_last_checked_at)),
            pending_critical_update: Mutex::new(None),
        }
    }

    pub fn app(&self) -> Result<App> {
        Ok(self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .app())
    }

    pub fn context(&self) -> Result<DesktopContext> {
        Ok(self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .clone())
    }

    pub fn update_settings_config(&self, settings: &SettingsConfig) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let state: StateConfig =
            read_json(&GoldBandPaths::new(guard.repo_root.clone()).user_state_file())
                .unwrap_or_default();
        guard.config = RuntimeConfig::default()
            .apply_settings(settings)
            .apply_state(&state);
        drop(guard);
        self.prune_agent_diagnostics()?;
        Ok(())
    }

    pub fn agent_diagnostics(&self) -> Result<BTreeMap<ManagedAgentType, AgentDiagnosticState>> {
        Ok(self
            .agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .clone())
    }

    pub fn update_status(&self) -> Result<UpdateStatusVm> {
        Ok(self
            .update_status
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .clone())
    }

    pub fn set_update_status(&self, status: UpdateStatusVm) -> Result<()> {
        *self
            .update_status
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))? = status;
        Ok(())
    }

    pub fn store_pending_update(&self, path: Utf8PathBuf) -> Result<()> {
        self.pending_critical_update
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .replace(path);
        Ok(())
    }

    pub fn take_pending_update(&self) -> Option<Utf8PathBuf> {
        self.pending_critical_update
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
    }

    pub fn persist_updater_last_checked_at(&self, checked_at: Option<String>) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let app = guard.app();
        let state = app.set_user_desktop_updater_last_checked_at(checked_at)?;
        guard.config = guard.config.clone().apply_state(&state);
        Ok(())
    }

    pub fn mark_update_badge_seen(
        &self,
        target: UpdateBadgeSeenTarget,
        version: String,
    ) -> Result<RuntimeConfig> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let app = guard.app();
        let mut next_badges = guard.config.desktop_update_badges.clone();
        match target {
            UpdateBadgeSeenTarget::SettingsEntry => {
                next_badges.settings_entry_seen_version = Some(version);
            }
            UpdateBadgeSeenTarget::SettingsAdvanced => {
                next_badges.settings_advanced_seen_version = Some(version);
            }
            UpdateBadgeSeenTarget::Announcement => {
                next_badges.announcement_closed_version = Some(version);
            }
        }
        let state = app.set_user_desktop_update_badges(next_badges)?;
        guard.config = guard.config.clone().apply_state(&state);
        Ok(guard.config.clone())
    }

    pub fn persist_available_update(&self, update: Option<UpdateInfoVm>) -> Result<RuntimeConfig> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let app = guard.app();
        let available_update = update.map(|update| gold_band::config::DesktopAvailableUpdate {
            version: update.version,
            current_version: update.current_version,
            notes: update.notes,
            pub_date: update.pub_date,
        });
        let state = app.set_user_desktop_available_update(available_update)?;
        guard.config = guard.config.clone().apply_state(&state);
        Ok(guard.config.clone())
    }

    pub fn clear_agent_diagnostics(&self) -> Result<()> {
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.clear();
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)
    }

    pub fn clear_agent_diagnostic(&self, agent_type: ManagedAgentType) -> Result<()> {
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.remove(&agent_type);
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)
    }

    pub fn prune_agent_diagnostics(&self) -> Result<()> {
        let managed_agent_types = self
            .app()?
            .managed_agents()
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.retain(|agent_type, _| managed_agent_types.contains(agent_type));
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)
    }

    pub fn cleanup_agent_diagnostic_processes(&self) -> Result<()> {
        let repo_root = self.context()?.repo_root;
        let pid_path = GoldBandPaths::new(repo_root)
            .runtime_root
            .join("doctor/acp/provider.pid");
        let Some(pid) = std::fs::read_to_string(pid_path.as_std_path())
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            return Ok(());
        };
        let _ = kill_process_tree(pid);
        let _ = std::fs::remove_file(pid_path.as_std_path());
        Ok(())
    }

    pub fn refresh_agent_diagnostic(
        &self,
        agent_type: ManagedAgentType,
    ) -> Result<AgentDiagnosticState> {
        let app = self.app()?;
        let doctor = app.provider_doctor(agent_type.as_str())?;
        let diagnostic = diagnostic_state_from_result(doctor);
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.insert(agent_type, diagnostic.clone());
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)?;
        Ok(diagnostic)
    }

    pub fn refresh_all_agent_diagnostics(&self) -> Result<()> {
        let app = self.app()?;
        let agent_types = app.managed_agents().keys().copied().collect::<Vec<_>>();
        let mut diagnostics = BTreeMap::new();
        for agent_type in agent_types {
            let doctor = app.provider_doctor(agent_type.as_str())?;
            diagnostics.insert(agent_type, diagnostic_state_from_result(doctor));
        }
        *self
            .agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))? = diagnostics.clone();
        self.persist_agent_diagnostics(&diagnostics)
    }

    pub fn set_workspace(&self, repo_root: Utf8PathBuf) -> Result<DesktopContext> {
        let next_context = {
            let mut guard = self
                .context
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            let repo_root = find_workspace_root(&repo_root).unwrap_or(repo_root);
            let app = App::with_config(repo_root.clone(), guard.config.clone());
            let workspace = repo_root.to_string();
            let (settings, state) = app.set_user_desktop_workspace(&workspace)?;
            guard.repo_root = repo_root;
            guard.config = RuntimeConfig::default()
                .apply_settings(&settings)
                .apply_state(&state);
            guard.recent_workspaces = recent_workspaces(&state, &guard.repo_root);
            guard.needs_workspace = false;
            guard.clone()
        };
        let persisted_diagnostics = load_persisted_agent_diagnostics(&next_context);
        *self
            .agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))? = persisted_diagnostics;
        *self
            .update_status
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))? =
            initial_update_status(next_context.config.desktop_updater_last_checked_at.clone());
        Ok(next_context)
    }

    fn persist_agent_diagnostics(
        &self,
        diagnostics: &BTreeMap<ManagedAgentType, AgentDiagnosticState>,
    ) -> Result<()> {
        let repo_root = self.context()?.repo_root;
        let path = GoldBandPaths::new(repo_root).agent_diagnostics_file();
        write_json(&path, diagnostics)
    }
}

fn diagnostic_state_from_result(result: DoctorResult) -> AgentDiagnosticState {
    AgentDiagnosticState {
        available: result.available,
        reason: result.reason,
        checked_at: current_timestamp(),
        capabilities: result.capabilities,
    }
}

fn load_persisted_agent_diagnostics(
    context: &DesktopContext,
) -> BTreeMap<ManagedAgentType, AgentDiagnosticState> {
    read_json(&GoldBandPaths::new(context.repo_root.clone()).agent_diagnostics_file())
        .unwrap_or_default()
}

fn resolve_initial_workspace(cwd: &Utf8Path) -> Utf8PathBuf {
    find_workspace_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

fn resolve_configured_workspace(settings: &SettingsConfig) -> Option<Utf8PathBuf> {
    settings
        .desktop_workspace
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Utf8PathBuf::from)
        .filter(|path| path.is_dir())
}

fn find_workspace_root(start: &Utf8Path) -> Option<Utf8PathBuf> {
    nearest_parent_containing(start, ".git")
        .or_else(|| nearest_parent_containing(start, active_storage_path_config().config_dir_name))
}

fn nearest_parent_containing(start: &Utf8Path, marker: &str) -> Option<Utf8PathBuf> {
    let mut current = start;
    loop {
        if current.join(marker).is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn load_configs(paths: &GoldBandPaths) -> (SettingsConfig, StateConfig) {
    let settings: SettingsConfig = read_json(&paths.user_settings_file()).unwrap_or_default();
    let state: StateConfig = read_json(&paths.user_state_file()).unwrap_or_default();
    (settings, state)
}

fn recent_workspaces(state: &StateConfig, repo_root: &Utf8Path) -> Vec<String> {
    let current = repo_root.to_string();
    let mut workspaces = vec![current.clone()];
    for workspace in &state.recent_desktop_workspaces {
        let workspace = workspace.trim();
        if !workspace.is_empty() && workspace != current && Utf8Path::new(workspace).is_dir() {
            workspaces.push(workspace.to_string());
        }
    }
    workspaces
}
