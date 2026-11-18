use crate::commands::{CommandErrorVm, CommandResult, resolve_command_app, spawn_blocking_command};
use crate::state::DesktopState;
use gold_band::provider::workspace_files::{FileReferenceTarget, resolve_file_reference_target};
use gold_band::provider::{PromptWorkspaceFileRef, resolve_prompt_workspace_files};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn resolve_right_workspace_file_target(
    state: State<'_, DesktopState>,
    source: PromptWorkspaceFileRef,
) -> CommandResult<FileReferenceTarget> {
    let app = resolve_command_app(state.inner(), Some(source.project_id()))?.clone_for_background();
    spawn_blocking_command(move || {
        let roots = app.prompt_workspace_roots(&app.paths.repo_root);
        resolve_file_reference_target(&roots, &source)
            .map_err(|error| CommandErrorVm::new(error.code(), error.params()))
    })
    .await
}

#[tauri::command]
pub async fn reveal_right_workspace_file(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    source: PromptWorkspaceFileRef,
) -> CommandResult<()> {
    let app = resolve_command_app(state.inner(), Some(source.project_id()))?.clone_for_background();
    let path = spawn_blocking_command(move || {
        let roots = app.prompt_workspace_roots(&app.paths.repo_root);
        let files = resolve_prompt_workspace_files(&roots, &[source], 0)
            .map_err(|error| CommandErrorVm::new(error.code(), error.params()))?;
        Ok(files[0].canonical_path.clone())
    })
    .await?;
    app_handle.opener().reveal_item_in_dir(path).map_err(|_| {
        CommandErrorVm::new(
            "workspace-file.file-manager-open-failed",
            serde_json::json!({}),
        )
    })
}
