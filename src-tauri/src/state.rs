use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU8, Ordering},
    },
};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::commands::{
    AcpCommandCatalog, AcpCommandItem, catalog_key, merge_native_skill_commands, workspace_key,
};
use gold_band::acp::events::current_timestamp;
use gold_band::app::observability::RuntimeLifecycleBus;
use gold_band::app::{App, NotificationDedup};
use gold_band::config::{
    ManagedAgentConfig, ManagedAgentId, ProviderDiagnosticSnapshot, RuntimeConfig, SettingsConfig,
    StateConfig,
};
use gold_band::process::kill_process_tree;
use gold_band::provider::DoctorResult;
use gold_band::storage::{
    GoldBandPaths, active_storage_path_config, load_settings_file, read_json, write_json,
};
use serde::{Deserialize, Serialize};

use crate::conversation_workspace::migrate_conversation_workspace_state;
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
        let (settings, _) = load_configs(&paths)?;
        let needs_workspace = resolve_configured_workspace(&settings).is_none()
            && find_workspace_root(&repo_root).is_none();
        let repo_root = resolve_configured_workspace(&settings)
            .or_else(|| find_workspace_root(&repo_root))
            .unwrap_or(repo_root);
        let paths = GoldBandPaths::new(repo_root.clone());
        let (settings, mut state) = load_configs(&paths)?;
        if migrate_conversation_workspace_state(
            (!needs_workspace).then_some(repo_root.as_path()),
            &mut state,
        ) {
            write_json(&paths.user_state_file(), &state)?;
        }
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

const SCHEDULER_SHUTDOWN_RUNNING: u8 = 0;
const SCHEDULER_SHUTDOWN_STARTED: u8 = 1;
const SCHEDULER_SHUTDOWN_COMPLETED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerExitAction {
    StartShutdown,
    WaitForShutdown,
    AllowExit,
}

pub struct DesktopState {
    context: Mutex<DesktopContext>,
    scheduled_service: Mutex<Option<Arc<crate::scheduled_service::ScheduledTaskService>>>,
    scheduler_coordinator: Mutex<Option<crate::scheduled_runtime::SchedulerCoordinatorHandle>>,
    scheduled_power: Mutex<
        crate::scheduled_runtime::power::ScheduledPowerManager<
            crate::scheduled_runtime::power::PlatformSleepInhibitor,
        >,
    >,
    scheduler_shutdown_phase: AtomicU8,
    agent_diagnostics: Arc<Mutex<BTreeMap<ManagedAgentId, AgentDiagnosticState>>>,
    agent_diagnostic_run_lock: Mutex<()>,
    agent_config_diagnostic_commit_lock: Mutex<()>,
    scheduled_agent_diagnostics: Mutex<BTreeMap<ManagedAgentId, u64>>,
    agent_command_catalogs: Mutex<BTreeMap<String, AcpCommandCatalog>>,
    update_status: Mutex<UpdateStatusVm>,
    pending_critical_update: Mutex<Option<Utf8PathBuf>>,
    notification_attention: Mutex<NotificationAttentionState>,
    /// 干预通知去重表（弹窗层统一管理，路径 A/B 共享同一实例）。
    notification_dedup: Arc<NotificationDedup>,
    lifecycle_bus: RuntimeLifecycleBus,
    /// MCP 服务器健康状态缓存（启动后台线程 + 手动诊断共同写入，列表读取）。
    mcp_health: Mutex<BTreeMap<String, gold_band::config::McpServerState>>,
}

impl DesktopState {
    pub fn new(context: DesktopContext) -> Self {
        let persisted_diagnostics = load_persisted_agent_diagnostics(&context);
        let persisted_command_catalogs = load_persisted_agent_command_catalogs(&context);
        let updater_last_checked_at = context.config.desktop_updater_last_checked_at.clone();
        Self {
            context: Mutex::new(context),
            scheduled_service: Mutex::new(None),
            scheduler_coordinator: Mutex::new(None),
            scheduled_power: Mutex::new(
                crate::scheduled_runtime::power::ScheduledPowerManager::new(
                    crate::scheduled_runtime::power::PlatformSleepInhibitor::default(),
                ),
            ),
            scheduler_shutdown_phase: AtomicU8::new(SCHEDULER_SHUTDOWN_RUNNING),
            agent_diagnostics: Arc::new(Mutex::new(persisted_diagnostics)),
            agent_diagnostic_run_lock: Mutex::new(()),
            agent_config_diagnostic_commit_lock: Mutex::new(()),
            scheduled_agent_diagnostics: Mutex::new(BTreeMap::new()),
            agent_command_catalogs: Mutex::new(persisted_command_catalogs),
            update_status: Mutex::new(initial_update_status(updater_last_checked_at)),
            pending_critical_update: Mutex::new(None),
            notification_attention: Mutex::new(NotificationAttentionState::default()),
            notification_dedup: Arc::new(NotificationDedup::new()),
            lifecycle_bus: RuntimeLifecycleBus::new(),
            mcp_health: Mutex::new(BTreeMap::new()),
        }
    }

    /// 读取 MCP 健康状态缓存快照（供列表 VM 附加展示）。
    pub fn mcp_health_snapshot(
        &self,
    ) -> Result<BTreeMap<String, gold_band::config::McpServerState>> {
        Ok(self
            .mcp_health
            .lock()
            .map_err(|_| anyhow::anyhow!("mcp health lock poisoned"))?
            .clone())
    }

    /// 写入/更新单个 MCP 服务器的健康状态（启动后台线程与诊断命令共用）。
    pub fn record_mcp_health(
        &self,
        id: String,
        state: gold_band::config::McpServerState,
    ) -> Result<()> {
        self.mcp_health
            .lock()
            .map_err(|_| anyhow::anyhow!("mcp health lock poisoned"))?
            .insert(id, state);
        Ok(())
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
        Ok(App::with_config(context.repo_root, context.config)
            .with_lifecycle_bus(self.lifecycle_bus.clone())
            .with_provider_diagnostics_source(Arc::new(move || {
                Ok(diagnostics
                    .lock()
                    .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
                    .iter()
                    .map(|(agent_type, diagnostic)| {
                        (agent_type.as_str().to_string(), diagnostic.clone())
                    })
                    .collect())
            })))
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

    pub fn install_scheduled_service(
        &self,
        service: Arc<crate::scheduled_service::ScheduledTaskService>,
    ) -> Result<()> {
        *self
            .scheduled_service
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduled service lock poisoned"))? = Some(service);
        Ok(())
    }

    pub fn scheduled_service(&self) -> Result<Arc<crate::scheduled_service::ScheduledTaskService>> {
        self.scheduled_service
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduled service lock poisoned"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("scheduled service is not initialized"))
    }

    pub fn install_scheduler_coordinator(
        &self,
        coordinator: crate::scheduled_runtime::SchedulerCoordinatorHandle,
    ) -> Result<()> {
        *self
            .scheduler_coordinator
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduler coordinator lock poisoned"))? =
            Some(coordinator);
        Ok(())
    }

    pub fn scheduler_coordinator(
        &self,
    ) -> Result<crate::scheduled_runtime::SchedulerCoordinatorHandle> {
        self.scheduler_coordinator
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduler coordinator lock poisoned"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("scheduler coordinator is not initialized"))
    }

    pub fn reconcile_scheduled_power(
        &self,
        enabled_job_count: usize,
        app_is_running: bool,
    ) -> Result<crate::scheduled_runtime::power::ScheduledPowerStatus> {
        let keep_awake_enabled = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .config
            .scheduled_keep_awake_enabled;
        Ok(self
            .scheduled_power
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduled power lock poisoned"))?
            .reconcile(keep_awake_enabled, enabled_job_count, app_is_running))
    }

    pub fn reconcile_scheduled_power_setting(
        &self,
    ) -> Result<crate::scheduled_runtime::power::ScheduledPowerStatus> {
        let enabled_job_count = self.scheduled_power_status()?.enabled_job_count;
        self.reconcile_scheduled_power(enabled_job_count, true)
    }

    pub fn scheduled_power_status(
        &self,
    ) -> Result<crate::scheduled_runtime::power::ScheduledPowerStatus> {
        Ok(self
            .scheduled_power
            .lock()
            .map_err(|_| anyhow::anyhow!("scheduled power lock poisoned"))?
            .status())
    }

    pub fn begin_scheduler_shutdown(&self) -> SchedulerExitAction {
        match self.scheduler_shutdown_phase.compare_exchange(
            SCHEDULER_SHUTDOWN_RUNNING,
            SCHEDULER_SHUTDOWN_STARTED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => SchedulerExitAction::StartShutdown,
            Err(SCHEDULER_SHUTDOWN_STARTED) => SchedulerExitAction::WaitForShutdown,
            Err(SCHEDULER_SHUTDOWN_COMPLETED) => SchedulerExitAction::AllowExit,
            Err(_) => SchedulerExitAction::WaitForShutdown,
        }
    }

    pub fn complete_scheduler_shutdown(&self) {
        self.scheduler_shutdown_phase
            .store(SCHEDULER_SHUTDOWN_COMPLETED, Ordering::Release);
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
        if let Ok(coordinator) = self.scheduler_coordinator() {
            let _ = coordinator.send(crate::scheduled_runtime::SchedulerCommand::SettingsChanged);
        }
        Ok(())
    }

    pub fn agent_diagnostics(&self) -> Result<BTreeMap<ManagedAgentId, AgentDiagnosticState>> {
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

    pub fn clear_agent_diagnostic(&self, agent_id: &ManagedAgentId) -> Result<()> {
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.remove(agent_id);
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)
    }

    pub fn prune_agent_diagnostics(&self) -> Result<()> {
        let managed_agent_ids = self
            .app()?
            .managed_agents()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.retain(|agent_id, _| managed_agent_ids.contains(agent_id));
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)?;
        let catalogs = {
            let mut catalogs = self
                .agent_command_catalogs
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            catalogs.retain(|_, catalog| {
                ManagedAgentId::from_str(&catalog.agent_type)
                    .ok()
                    .is_some_and(|agent_id| managed_agent_ids.contains(&agent_id))
            });
            catalogs.clone()
        };
        self.persist_agent_command_catalogs(&catalogs)
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

    pub fn agent_diagnostic_guard(&self) -> Result<MutexGuard<'_, ()>> {
        self.agent_diagnostic_run_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("agent diagnostic lock poisoned"))
    }

    pub fn agent_config_diagnostic_commit_guard(&self) -> Result<MutexGuard<'_, ()>> {
        self.agent_config_diagnostic_commit_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("agent config/diagnostic commit lock poisoned"))
    }

    pub fn queue_agent_diagnostic(&self, agent_id: &ManagedAgentId) -> Result<bool> {
        let mut scheduled = self
            .scheduled_agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?;
        if let Some(generation) = scheduled.get_mut(agent_id) {
            *generation = generation.saturating_add(1);
            Ok(false)
        } else {
            scheduled.insert(agent_id.clone(), 1);
            Ok(true)
        }
    }

    pub fn cancel_queued_agent_diagnostic(&self, agent_id: &ManagedAgentId) -> Result<()> {
        self.scheduled_agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?
            .remove(agent_id);
        Ok(())
    }

    pub fn run_queued_agent_diagnostic(
        &self,
        agent_id: &ManagedAgentId,
    ) -> Result<AgentDiagnosticState> {
        loop {
            let requested_generation = self
                .scheduled_agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?
                .get(agent_id)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("agent diagnostic request was cancelled"))?;
            let result = {
                let _run_guard = self.agent_diagnostic_guard()?;
                self.refresh_agent_diagnostic_unlocked(agent_id)
            };
            let should_retry = {
                let mut scheduled = self
                    .scheduled_agent_diagnostics
                    .lock()
                    .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?;
                match scheduled.get(agent_id).copied() {
                    Some(current_generation) if current_generation != requested_generation => true,
                    Some(_) => {
                        scheduled.remove(agent_id);
                        false
                    }
                    None => false,
                }
            };
            if should_retry {
                continue;
            }
            return result;
        }
    }

    pub fn refresh_agent_diagnostic(
        &self,
        agent_id: &ManagedAgentId,
    ) -> Result<AgentDiagnosticState> {
        let _run_guard = self.agent_diagnostic_guard()?;
        self.refresh_agent_diagnostic_unlocked(agent_id)
    }

    fn refresh_agent_diagnostic_unlocked(
        &self,
        agent_id: &ManagedAgentId,
    ) -> Result<AgentDiagnosticState> {
        let expected_config = self.managed_agent_config_revision(agent_id)?;
        let app = self.app()?;
        let probe = app.provider_doctor_probe(agent_id.as_str())?;
        let _commit_guard = self.agent_config_diagnostic_commit_guard()?;
        if self.managed_agent_config_revision(agent_id)? != expected_config {
            anyhow::bail!("agent configuration changed during diagnostic");
        }
        if probe.doctor.available {
            self.record_agent_commands(agent_id, &app.paths.repo_root, probe.commands.clone())?;
        }
        let diagnostic = diagnostic_state_from_result(probe.doctor);
        let snapshot = {
            let mut diagnostics = self
                .agent_diagnostics
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            diagnostics.insert(agent_id.clone(), diagnostic.clone());
            diagnostics.clone()
        };
        self.persist_agent_diagnostics(&snapshot)?;
        Ok(diagnostic)
    }

    pub fn refresh_all_agent_diagnostics(&self) -> Result<()> {
        let _run_guard = self.agent_diagnostic_guard()?;
        let app = self.app()?;
        let agent_ids = app.managed_agents().keys().cloned().collect::<Vec<_>>();
        let scheduled = self
            .scheduled_agent_diagnostics
            .lock()
            .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        for agent_id in agent_ids {
            if scheduled.contains(&agent_id) {
                continue;
            }
            if let Err(error) = self.refresh_agent_diagnostic_unlocked(&agent_id) {
                let queued = self
                    .scheduled_agent_diagnostics
                    .lock()
                    .map_err(|_| anyhow::anyhow!("agent diagnostic schedule lock poisoned"))?
                    .contains_key(&agent_id);
                if !queued {
                    return Err(error);
                }
            }
        }
        self.prune_agent_diagnostics()
    }

    fn managed_agent_config_revision(&self, agent_id: &ManagedAgentId) -> Result<Option<Vec<u8>>> {
        self.context()?
            .config
            .agents
            .get(agent_id)
            .map(serde_json::to_vec)
            .transpose()
            .map_err(Into::into)
    }

    pub fn agent_command_catalog(
        &self,
        agent_id: &ManagedAgentId,
        workspace: &Utf8Path,
    ) -> Result<Option<AcpCommandCatalog>> {
        let workspace_key = workspace_key(workspace);
        let key = catalog_key(agent_id.as_str(), &workspace_key);
        let policy = self
            .context()?
            .config
            .agents
            .get(agent_id)
            .map(ManagedAgentConfig::skill_directory_policy);
        let (catalog, catalogs_to_persist) = {
            let mut catalogs = self
                .agent_command_catalogs
                .lock()
                .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
            let Some(mut catalog) = catalogs.get(&key).cloned() else {
                return Ok(None);
            };
            let mut changed = false;
            if let Some(policy) = policy {
                // 旧目录文件没有 acp_commands，只能暂用旧 commands 迁移；下一次 Doctor
                // 会写入 Some(...)，之后所有扫描都从原始 ACP 列表开始，Skill 删除不会残留。
                let raw_commands = catalog
                    .acp_commands
                    .as_ref()
                    .unwrap_or(&catalog.commands)
                    .clone();
                let commands = merge_native_skill_commands(&policy, workspace, raw_commands);
                if commands != catalog.commands {
                    catalog.commands = commands;
                    catalog.updated_at = current_timestamp();
                    catalogs.insert(key, catalog.clone());
                    changed = true;
                }
            }
            let snapshot = changed.then(|| catalogs.clone());
            (catalog, snapshot)
        };
        if let Some(catalogs) = catalogs_to_persist {
            self.persist_agent_command_catalogs(&catalogs)?;
        }
        Ok(Some(catalog))
    }

    pub fn record_agent_commands(
        &self,
        agent_id: &ManagedAgentId,
        workspace: &Utf8Path,
        commands: Vec<AcpCommandItem>,
    ) -> Result<AcpCommandCatalog> {
        let acp_commands = commands;
        let commands = self
            .context()?
            .config
            .agents
            .get(agent_id)
            .map(|config| {
                merge_native_skill_commands(
                    &config.skill_directory_policy(),
                    workspace,
                    acp_commands.clone(),
                )
            })
            .unwrap_or_else(|| acp_commands.clone());
        let workspace_key = workspace_key(workspace);
        let catalog = AcpCommandCatalog {
            agent_type: agent_id.as_str().to_string(),
            workspace_key: workspace_key.clone(),
            acp_commands: Some(acp_commands),
            commands,
            updated_at: current_timestamp(),
        };
        let mut catalogs = self
            .agent_command_catalogs
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        catalogs.insert(
            catalog_key(agent_id.as_str(), &workspace_key),
            catalog.clone(),
        );
        while catalogs.len() > 256 {
            let oldest = catalogs
                .iter()
                .min_by_key(|(_, catalog)| catalog.updated_at.as_str())
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            catalogs.remove(&oldest);
        }
        self.persist_agent_command_catalogs(&catalogs)?;
        Ok(catalog)
    }

    pub fn refresh_agent_command_catalog_for_workspace(
        &self,
        agent_id: &ManagedAgentId,
        workspace: Utf8PathBuf,
    ) -> Result<()> {
        let _run_guard = self.agent_diagnostic_guard()?;
        self.refresh_agent_command_catalog_for_workspace_unlocked(agent_id, workspace)
    }

    fn refresh_agent_command_catalog_for_workspace_unlocked(
        &self,
        agent_id: &ManagedAgentId,
        workspace: Utf8PathBuf,
    ) -> Result<()> {
        let expected_config = self.managed_agent_config_revision(agent_id)?;
        let config = self.context()?.config;
        let app = App::with_config(workspace, config);
        let probe = app.provider_doctor_probe(agent_id.as_str())?;
        let _commit_guard = self.agent_config_diagnostic_commit_guard()?;
        if self.managed_agent_config_revision(agent_id)? != expected_config {
            anyhow::bail!("agent configuration changed during command catalog refresh");
        }
        if probe.doctor.available {
            self.record_agent_commands(agent_id, &app.paths.repo_root, probe.commands)?;
        }
        Ok(())
    }

    pub fn refresh_all_agent_command_catalogs_for_workspace(
        &self,
        workspace: Utf8PathBuf,
    ) -> Result<()> {
        let _run_guard = self.agent_diagnostic_guard()?;
        let config = self.context()?.config;
        let app = App::with_config(workspace.clone(), config);
        let agent_ids = app.managed_agents().keys().cloned().collect::<Vec<_>>();
        for agent_id in agent_ids {
            let _ = self
                .refresh_agent_command_catalog_for_workspace_unlocked(&agent_id, workspace.clone());
        }
        Ok(())
    }

    pub fn refresh_agent_command_catalogs_for_active_workspaces(&self) -> Result<()> {
        let context = self.context()?;
        let app = context.app();
        let persisted_state = app.load_state()?;
        let current_workspace_key = workspace_key(&context.repo_root);
        let mut workspaces = std::collections::BTreeSet::new();
        if let Some(active_project_id) = persisted_state.last_conversation_workspace.as_deref() {
            if let Some(active_workspace) = persisted_state
                .conversation_workspaces
                .iter()
                .find(|workspace| workspace.project_id == active_project_id)
            {
                let active_workspace = Utf8PathBuf::from(&active_workspace.workspace_path);
                if workspace_key(&active_workspace) != current_workspace_key {
                    workspaces.insert(active_workspace);
                }
            }
        }
        for workspace in workspaces {
            self.refresh_all_agent_command_catalogs_for_workspace(workspace)?;
        }
        Ok(())
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

    pub fn remove_recent_workspace(&self, workspace: &str) -> Result<DesktopContext> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let app = guard.app();
        let state = app.remove_user_recent_desktop_workspace(workspace)?;
        guard.config = guard.config.clone().apply_state(&state);
        guard.recent_workspaces = recent_workspaces(&state, &guard.repo_root);
        if guard.needs_workspace {
            let current = guard.repo_root.to_string();
            guard.recent_workspaces.retain(|w| w != &current);
        }
        Ok(guard.clone())
    }

    fn persist_agent_diagnostics(
        &self,
        diagnostics: &BTreeMap<ManagedAgentId, AgentDiagnosticState>,
    ) -> Result<()> {
        let repo_root = self.context()?.repo_root;
        let path = GoldBandPaths::new(repo_root).agent_diagnostics_file();
        write_json(&path, diagnostics)
    }

    fn persist_agent_command_catalogs(
        &self,
        catalogs: &BTreeMap<String, AcpCommandCatalog>,
    ) -> Result<()> {
        let repo_root = self.context()?.repo_root;
        let path = GoldBandPaths::new(repo_root).agent_command_catalogs_file();
        write_json(&path, &catalogs.values().cloned().collect::<Vec<_>>())
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
) -> BTreeMap<ManagedAgentId, AgentDiagnosticState> {
    read_json(&GoldBandPaths::new(context.repo_root.clone()).agent_diagnostics_file())
        .unwrap_or_default()
}

fn load_persisted_agent_command_catalogs(
    context: &DesktopContext,
) -> BTreeMap<String, AcpCommandCatalog> {
    read_json::<Vec<AcpCommandCatalog>>(
        &GoldBandPaths::new(context.repo_root.clone()).agent_command_catalogs_file(),
    )
    .unwrap_or_default()
    .into_iter()
    .map(|catalog| {
        (
            catalog_key(&catalog.agent_type, &catalog.workspace_key),
            catalog,
        )
    })
    .collect()
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

fn load_configs(paths: &GoldBandPaths) -> Result<(SettingsConfig, StateConfig)> {
    let settings = load_settings_file(&paths.user_settings_file())?;
    let state: StateConfig = read_json(&paths.user_state_file()).unwrap_or_default();
    Ok((settings, state))
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
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    fn desktop_state() -> (tempfile::TempDir, DesktopState) {
        let root = tempfile::tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
        let context = DesktopContext {
            repo_root,
            config: RuntimeConfig::default(),
            recent_workspaces: Vec::new(),
            needs_workspace: false,
        };
        (root, DesktopState::new(context))
    }

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
    fn scheduler_exit_waits_for_shutdown_completion_and_allows_only_the_retry() {
        let (_root, state) = desktop_state();

        assert_eq!(
            state.begin_scheduler_shutdown(),
            SchedulerExitAction::StartShutdown
        );
        assert_eq!(
            state.begin_scheduler_shutdown(),
            SchedulerExitAction::WaitForShutdown
        );
        state.complete_scheduler_shutdown();
        assert_eq!(
            state.begin_scheduler_shutdown(),
            SchedulerExitAction::AllowExit
        );
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

    #[test]
    fn agent_diagnostic_queue_coalesces_duplicate_save_requests() {
        let (_root, state) = desktop_state();
        let agent_id = ManagedAgentId::from_str("claude-acp").unwrap();

        assert!(state.queue_agent_diagnostic(&agent_id).unwrap());
        assert!(!state.queue_agent_diagnostic(&agent_id).unwrap());

        state.cancel_queued_agent_diagnostic(&agent_id).unwrap();
        assert!(state.queue_agent_diagnostic(&agent_id).unwrap());
    }

    #[test]
    fn agent_config_commit_does_not_wait_for_running_doctor() {
        let (_root, state) = desktop_state();
        let state = Arc::new(state);
        let (doctor_locked_tx, doctor_locked_rx) = mpsc::channel();
        let (release_doctor_tx, release_doctor_rx) = mpsc::channel();
        let doctor_state = state.clone();
        let doctor_thread = std::thread::spawn(move || {
            let _guard = doctor_state.agent_diagnostic_guard().unwrap();
            doctor_locked_tx.send(()).unwrap();
            release_doctor_rx.recv().unwrap();
        });
        doctor_locked_rx.recv().unwrap();

        let (commit_acquired_tx, commit_acquired_rx) = mpsc::channel();
        let commit_state = state.clone();
        let commit_thread = std::thread::spawn(move || {
            let _guard = commit_state.agent_config_diagnostic_commit_guard().unwrap();
            commit_acquired_tx.send(()).unwrap();
        });

        let acquired_without_waiting = commit_acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .is_ok();
        release_doctor_tx.send(()).unwrap();
        doctor_thread.join().unwrap();
        commit_thread.join().unwrap();
        assert!(acquired_without_waiting);
    }
}
