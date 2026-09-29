use gold_band::{
    config::ManagedAgentId,
    npx_cache::{self, RepairGuard, RepairItem},
};
use serde::Serialize;

use crate::{
    commands::{CommandResult, command_error},
    state::DesktopState,
};
use std::{path::PathBuf, str::FromStr};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheRepairPreview {
    pub token: String,
    pub paths: Vec<String>,
}

fn prepare(
    state: &DesktopState,
    id: &ManagedAgentId,
) -> anyhow::Result<(CacheRepairPreview, PathBuf, Vec<PathBuf>)> {
    let app = state.app()?;
    let config = app
        .managed_agents()
        .get(id)
        .cloned()
        .ok_or_else(|| npx_cache::error("npx-cache.changed"))?;
    let diagnostics = state.agent_diagnostics()?;
    let diagnostic = diagnostics
        .get(id)
        .filter(|d| !d.available)
        .ok_or_else(|| npx_cache::error("npx-cache.changed"))?;
    let reason = diagnostic
        .error
        .as_ref()
        .and_then(|e| e.params.get("reason"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let manifests = npx_cache::missing_manifests(reason);
    if manifests.is_empty() {
        return Err(npx_cache::error("npx-cache.unavailable"));
    }
    let cache = npx_cache::resolve_cache(&config.adapter, app.paths.repo_root.as_std_path())?;
    let targets = npx_cache::validate_targets(&cache, &manifests)?;
    let paths = targets
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let input = serde_json::to_vec(&(id, &config, diagnostic, &app.paths.repo_root, &paths))?;
    let token = blake3::hash(&input).to_hex().to_string();
    Ok((CacheRepairPreview { token, paths }, cache, targets))
}

#[tauri::command]
pub async fn preview_agent_cache_repair(
    app_handle: AppHandle,
    agent_type: String,
) -> CommandResult<CacheRepairPreview> {
    tauri::async_runtime::spawn_blocking(move || {
        let id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
        prepare(&app_handle.state::<DesktopState>(), &id)
            .map(|p| p.0)
            .map_err(command_error)
    })
    .await
    .map_err(|_| command_error(npx_cache::error("npx-cache.unavailable")))?
}

#[tauri::command]
pub async fn repair_agent_cache(
    app_handle: AppHandle,
    agent_type: String,
    token: String,
) -> CommandResult<Vec<RepairItem>> {
    tauri::async_runtime::spawn_blocking(move || {
        let id = ManagedAgentId::from_str(&agent_type).map_err(command_error)?;
        let state = app_handle.state::<DesktopState>();
        let guard = RepairGuard::acquire().map_err(command_error)?;
        // Serialize only this Agent’s diagnostics; running adapters remain untouched.
        let _diagnostic_guard = state.agent_diagnostic_guard(&id).map_err(command_error)?;
        let (preview, cache, targets) = prepare(&state, &id).map_err(command_error)?;
        if preview.token != token {
            return Err(command_error(npx_cache::error("npx-cache.changed")));
        }
        Ok(npx_cache::remove_targets(&cache, &targets, &guard))
    })
    .await
    .map_err(|_| command_error(npx_cache::error("npx-cache.cleanup-failed")))?
}
