mod commands;
mod i18n;
mod state;
mod view_models;

use anyhow::Context;
use commands::{
    cancel_acp_session, choose_workspace, continue_run, create_agent, create_profile, create_task,
    delete_agent, delete_workflow_template, doctor_agent, get_acp_raw_frames, get_acp_session,
    get_agent_registry, get_app_bootstrap, get_log_page, get_profile, get_profiles, get_round_detail,
    get_run_detail, get_system_fonts, get_task_detail, get_task_list, get_workflow,
    get_workflow_templates, kill_run, respond_acp_permission, retry_run, save_desktop_preferences,
    save_task_workflow, save_workflow_template, select_recent_workspace, send_acp_prompt,
    show_artifact, show_attachment, show_worker_ref, start_run, submit_manual_check, update_agent,
    update_profile, update_workflow_template,
};
use state::{DesktopContext, DesktopState};
use tauri::{Manager, WindowEvent};

fn main() {
    if let Err(error) = run() {
        eprintln!("failed to start Gold Band desktop: {error:?}");
    }
}

fn run() -> anyhow::Result<()> {
    let context = DesktopContext::from_current_dir()?;
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(DesktopState::new(context))
        .setup(|app| {
            let state = app.state::<DesktopState>();
            let _ = state.cleanup_agent_diagnostic_processes();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    let state = handle.state::<DesktopState>();
                    let _ = state.refresh_all_agent_diagnostics();
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                let state = window.state::<DesktopState>();
                if let Ok(app) = state.app() {
                    let _ = app.stop_all_running_sessions();
                }
                let _ = state.cleanup_agent_diagnostic_processes();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_bootstrap,
            get_system_fonts,
            get_agent_registry,
            create_agent,
            update_agent,
            delete_agent,
            doctor_agent,
            get_task_list,
            get_profiles,
            get_profile,
            create_profile,
            update_profile,
            choose_workspace,
            select_recent_workspace,
            get_task_detail,
            create_task,
            save_task_workflow,
            get_workflow,
            get_workflow_templates,
            save_workflow_template,
            update_workflow_template,
            delete_workflow_template,
            get_run_detail,
            get_round_detail,
            get_log_page,
            get_acp_session,
            send_acp_prompt,
            respond_acp_permission,
            cancel_acp_session,
            get_acp_raw_frames,
            start_run,
            continue_run,
            submit_manual_check,
            retry_run,
            kill_run,
            show_artifact,
            show_attachment,
            show_worker_ref,
            save_desktop_preferences,
        ])
        .run(tauri::generate_context!())
        .context("tauri runtime failed")?;
    Ok(())
}
