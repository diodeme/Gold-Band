use crate::{
    commands::{CommandErrorVm, resolve_command_app},
    state::DesktopState,
};
use gold_band::memory::{MemoryService, Snapshot, WriteCommand};
use tauri::{AppHandle, Manager};

async fn execute(
    handle: AppHandle,
    project_id: String,
    command: Option<WriteCommand>,
) -> Result<Snapshot, CommandErrorVm> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<DesktopState>();
        let app = resolve_command_app(&state, Some(&project_id))?;
        let service = MemoryService::new(app.paths.clone(), &project_id, None);
        service
            .and_then(|service| match command {
                Some(command) => service.write(command),
                None => service.read(),
            })
            .map_err(|error| CommandErrorVm {
                code: error.code.into(),
                params: error.params,
            })
    })
    .await
    .map_err(|_| CommandErrorVm {
        code: "memory.worker".into(),
        params: serde_json::json!({}),
    })?
}

#[tauri::command]
pub async fn read_project_memory(
    handle: AppHandle,
    project_id: String,
) -> Result<Snapshot, CommandErrorVm> {
    execute(handle, project_id, None).await
}

#[tauri::command]
pub async fn write_project_memory(
    handle: AppHandle,
    project_id: String,
    command: WriteCommand,
) -> Result<Snapshot, CommandErrorVm> {
    execute(handle, project_id, Some(command)).await
}
