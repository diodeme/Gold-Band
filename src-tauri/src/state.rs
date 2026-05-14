use std::sync::Mutex;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gold_band::app::App;
use gold_band::config::{RuntimeConfig, UserConfig};
use gold_band::storage::{GoldBandPaths, read_json};

#[derive(Debug, Clone)]
pub struct DesktopContext {
    pub repo_root: Utf8PathBuf,
    pub config: RuntimeConfig,
    pub recent_workspaces: Vec<String>,
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
        let repo_root = resolve_configured_workspace(&user_config)
            .or_else(|| find_workspace_root(&repo_root))
            .unwrap_or(repo_root);
        let paths = GoldBandPaths::new(repo_root.clone());
        let user_config = load_user_config(&paths);
        let config = RuntimeConfig::default().apply_user_config(&user_config);
        let recent_workspaces = recent_workspaces(&user_config, &repo_root);
        Ok(Self {
            repo_root,
            config,
            recent_workspaces,
        })
    }

    pub fn app(&self) -> App {
        App::with_config(self.repo_root.clone(), self.config.clone())
    }
}

pub struct DesktopState {
    context: Mutex<DesktopContext>,
}

impl DesktopState {
    pub fn new(context: DesktopContext) -> Self {
        Self {
            context: Mutex::new(context),
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
        Ok(())
    }

    pub fn set_workspace(&self, repo_root: Utf8PathBuf) -> Result<DesktopContext> {
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
        Ok(guard.clone())
    }
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
    nearest_parent_containing(start, ".git").or_else(|| nearest_parent_containing(start, ".gold-band"))
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
