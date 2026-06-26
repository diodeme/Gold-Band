use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::events::current_timestamp;
use gold_band::app::{App, NotificationDedup};
use gold_band::config::{
    ManagedAgentType, ProviderDiagnosticSnapshot, RuntimeConfig, SettingsConfig, StateConfig,
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
}

pub type AgentDiagnosticState = ProviderDiagnosticSnapshot;

#[derive(Debug, Clone, Copy)]
pub enum UpdateBadgeSeenTarget {
    SettingsEntry,
    SettingsAdvanced,
    Announcement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationAttentionInput {
    pub window_focused: bool,
    pub window_minimized: bool,
    pub window_visible: bool,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub round_id: Option<String>,
    pub node_id: Option<String>,
    pub attempt_id: Option<String>,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NotificationAttentionTarget<'a> {
    pub task_id: &'a str,
    pub run_id: &'a str,
    pub round_id: &'a str,
    pub node_id: &'a str,
    pub attempt_id: &'a str,
}

#[derive(Debug, Clone)]
pub struct NotificationAttentionState {
    window_focused: bool,
    window_minimized: bool,
    window_visible: bool,
    project_id: Option<String>,
    task_id: Option<String>,
    run_id: Option<String>,
    round_id: Option<String>,
    node_id: Option<String>,
    attempt_id: Option<String>,
    outer_node_id: Option<String>,
    outer_attempt_id: Option<String>,
}

impl Default for NotificationAttentionState {
    fn default() -> Self {
        Self {
            window_focused: false,
            window_minimized: true,
            window_visible: false,
            project_id: None,
            task_id: None,
            run_id: None,
            round_id: None,
            node_id: None,
            attempt_id: None,
            outer_node_id: None,
            outer_attempt_id: None,
        }
    }
}

impl NotificationAttentionState {
    fn update(&mut self, input: NotificationAttentionInput) {
        self.window_focused = input.window_focused;
        self.window_minimized = input.window_minimized;
        self.window_visible = input.window_visible;
        self.project_id = input.project_id;
        self.task_id = input.task_id;
        self.run_id = input.run_id;
        self.round_id = input.round_id;
        self.node_id = input.node_id;
        self.attempt_id = input.attempt_id;
        self.outer_node_id = input.outer_node_id;
        self.outer_attempt_id = input.outer_attempt_id;
    }

    pub fn should_notify(
        &self,
        target: &NotificationAttentionTarget<'_>,
        require_session_match: bool,
    ) -> bool {
        if !self.window_focused || self.window_minimized || !self.window_visible {
            return true;
        }
        if self.task_id.as_deref() != Some(target.task_id)
            || self.run_id.as_deref() != Some(target.run_id)
        {
            return true;
        }
        if !require_session_match {
            return false;
        }
        self.round_id.as_deref() != Some(target.round_id)
            || self.node_id.as_deref() != Some(target.node_id)
            || self.attempt_id.as_deref() != Some(target.attempt_id)
    }
}

pub struct DesktopState {
    context: Mutex<DesktopContext>,
    agent_diagnostics: Arc<Mutex<BTreeMap<ManagedAgentType, AgentDiagnosticState>>>,
    update_status: Mutex<UpdateStatusVm>,
    pending_critical_update: Mutex<Option<Utf8PathBuf>>,
    notification_attention: Mutex<NotificationAttentionState>,
    /// 干预通知去重表（弹窗层统一管理，路径 A/B 共享同一实例）。
    notification_dedup: Arc<NotificationDedup>,
}

impl DesktopState {
    pub fn new(context: DesktopContext) -> Self {
        let persisted_diagnostics = load_persisted_agent_diagnostics(&context);
        let updater_last_checked_at = context.config.desktop_updater_last_checked_at.clone();
        Self {
            context: Mutex::new(context),
            agent_diagnostics: Arc::new(Mutex::new(persisted_diagnostics)),
            update_status: Mutex::new(initial_update_status(updater_last_checked_at)),
            pending_critical_update: Mutex::new(None),
            notification_attention: Mutex::new(NotificationAttentionState::default()),
            notification_dedup: Arc::new(NotificationDedup::new()),
        }
    }

    /// 干预通知去重表（共享实例）。路径 A/B 与 dismiss 命令均经此访问。
    pub fn notification_dedup(&self) -> Arc<NotificationDedup> {
        self.notification_dedup.clone()
    }

    pub fn update_notification_attention(&self, input: NotificationAttentionInput) -> Result<()> {
        self.notification_attention
            .lock()
            .map_err(|_| anyhow::anyhow!("notification attention lock poisoned"))?
            .update(input);
        Ok(())
    }

    pub fn should_send_notification(
        &self,
        target: &NotificationAttentionTarget<'_>,
        require_session_match: bool,
    ) -> bool {
        self.notification_attention
            .lock()
            .map(|state| state.should_notify(target, require_session_match))
            .unwrap_or(true)
    }

    pub fn app(&self) -> Result<App> {
        let context = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .clone();
        let diagnostics = self.agent_diagnostics.clone();
        Ok(
            App::with_config(context.repo_root, context.config).with_provider_diagnostics_source(
                Arc::new(move || {
                    Ok(diagnostics
                        .lock()
                        .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
                        .iter()
                        .map(|(agent_type, diagnostic)| {
                            (agent_type.as_str().to_string(), diagnostic.clone())
                        })
                        .collect())
                }),
            ),
        )
    }

    pub fn provider_diagnostic_snapshots(
        &self,
    ) -> Result<BTreeMap<String, ProviderDiagnosticSnapshot>> {
        Ok(self
            .agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .iter()
            .map(|(agent_type, diagnostic)| (agent_type.as_str().to_string(), diagnostic.clone()))
            .collect())
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

    #[allow(dead_code)]
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
        let pid_path = GoldBandPaths::new(repo_root).doctor_acp_provider_pid_file();
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
    ProviderDiagnosticSnapshot {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> NotificationAttentionTarget<'static> {
        NotificationAttentionTarget {
            task_id: "task-1",
            run_id: "run-1",
            round_id: "round-1",
            node_id: "node-1",
            attempt_id: "attempt-1",
        }
    }

    fn input() -> NotificationAttentionInput {
        NotificationAttentionInput {
            window_focused: true,
            window_minimized: false,
            window_visible: true,
            project_id: Some("project-1".to_string()),
            task_id: Some("task-1".to_string()),
            run_id: Some("run-1".to_string()),
            round_id: Some("round-1".to_string()),
            node_id: Some("node-1".to_string()),
            attempt_id: Some("attempt-1".to_string()),
            outer_node_id: None,
            outer_attempt_id: None,
        }
    }

    #[test]
    fn notification_attention_suppresses_visible_selected_session() {
        let mut state = NotificationAttentionState::default();
        state.update(input());
        assert!(!state.should_notify(&target(), true));
    }

    #[test]
    fn notification_attention_notifies_when_minimized_or_different_session() {
        let mut state = NotificationAttentionState::default();
        let mut minimized = input();
        minimized.window_minimized = true;
        state.update(minimized);
        assert!(state.should_notify(&target(), true));

        let mut other = input();
        other.attempt_id = Some("attempt-2".to_string());
        state.update(other);
        assert!(state.should_notify(&target(), true));
    }
}
