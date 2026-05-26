use std::{collections::BTreeMap, sync::Mutex};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::acp::events::current_timestamp;
use gold_band::app::App;
use gold_band::config::{ManagedAgentType, RuntimeConfig, UserConfig};
use gold_band::process::kill_process_tree;
use gold_band::provider::DoctorResult;
use gold_band::storage::{GoldBandPaths, active_storage_path_config, read_json, write_json};
use serde::{Deserialize, Serialize};

use crate::updater::{UpdateStatusVm, initial_update_status};

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
        let user_config = load_user_config(&paths);
        let needs_workspace = resolve_configured_workspace(&user_config).is_none()
            && find_workspace_root(&repo_root).is_none();
        let repo_root = resolve_configured_workspace(&user_config)
            .or_else(|| find_workspace_root(&repo_root))
            .unwrap_or(repo_root);
        let paths = GoldBandPaths::new(repo_root.clone());
        let user_config = load_user_config(&paths);
        let config = RuntimeConfig::default().apply_user_config(&user_config);
        let mut recent_workspaces = recent_workspaces(&user_config, &repo_root);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDiagnosticState {
    pub available: bool,
    pub reason: Option<String>,
    pub checked_at: String,
    pub capabilities: Option<serde_json::Value>,
}

pub struct DesktopState {
    context: Mutex<DesktopContext>,
    agent_diagnostics: Mutex<BTreeMap<ManagedAgentType, AgentDiagnosticState>>,
    update_status: Mutex<UpdateStatusVm>,
}

impl DesktopState {
    pub fn new(context: DesktopContext) -> Self {
        let persisted_diagnostics = load_persisted_agent_diagnostics(&context);
        let updater_last_checked_at = context.config.desktop_updater_last_checked_at.clone();
        Self {
            context: Mutex::new(context),
            agent_diagnostics: Mutex::new(persisted_diagnostics),
            update_status: Mutex::new(initial_update_status(updater_last_checked_at)),
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

    pub fn update_config(&self, config: RuntimeConfig) -> Result<()> {
        self.context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?
            .config = config;
        self.clear_agent_diagnostics()?;
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

    pub fn persist_updater_last_checked_at(&self, checked_at: Option<String>) -> Result<()> {
        let mut guard = self
            .context
            .lock()
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))?;
        let app = guard.app();
        let user_config = app.set_user_desktop_updater_last_checked_at(checked_at)?;
        guard.config = RuntimeConfig::default().apply_user_config(&user_config);
        Ok(())
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
            let user_config = app.set_user_desktop_workspace(&workspace)?;
            guard.repo_root = repo_root;
            guard.config = RuntimeConfig::default().apply_user_config(&user_config);
            guard.recent_workspaces = recent_workspaces(&user_config, &guard.repo_root);
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
            .map_err(|_| anyhow::anyhow!("desktop state lock poisoned"))? = initial_update_status(
            next_context.config.desktop_updater_last_checked_at.clone(),
        );
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

fn resolve_configured_workspace(user_config: &UserConfig) -> Option<Utf8PathBuf> {
    user_config
        .desktop_workspace
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Utf8PathBuf::from)
        .filter(|path| path.is_dir())
}

fn find_workspace_root(start: &Utf8Path) -> Option<Utf8PathBuf> {
    nearest_parent_containing(start, ".git").or_else(|| {
        nearest_parent_containing(start, active_storage_path_config().config_dir_name)
    })
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

fn load_user_config(paths: &GoldBandPaths) -> UserConfig {
    read_json(&paths.user_config_file()).unwrap_or_default()
}

fn recent_workspaces(user_config: &UserConfig, repo_root: &Utf8Path) -> Vec<String> {
    let current = repo_root.to_string();
    let mut workspaces = vec![current.clone()];
    for workspace in &user_config.recent_desktop_workspaces {
        let workspace = workspace.trim();
        if !workspace.is_empty() && workspace != current && Utf8Path::new(workspace).is_dir() {
            workspaces.push(workspace.to_string());
        }
    }
    workspaces
}
