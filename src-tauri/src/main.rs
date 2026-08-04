#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod avatar;
mod builtin_mcp;
mod channel;
mod commands;
mod commands_conversation;
mod conversation_workspace;
mod feedback;
mod i18n;
mod metrics;
mod notifications;
mod state;
mod updater;
mod view_models;
mod view_models_conversation;
#[cfg(any(test, all(debug_assertions, target_os = "windows")))]
mod webview_heap_diagnostics;
mod window_chrome;
mod workspace_files;

use anyhow::Context;
use commands::{
    add_mcp_server, cancel_acp_session, check_local_claude, check_mcp_server_health,
    check_skill_name_conflict, check_update_manual, choose_workspace, clear_desktop_avatar,
    continue_run, create_agent, create_profile, create_task, delete_agent, delete_auto_template,
    delete_mcp_server, delete_profile, delete_skill, delete_workflow_template,
    dismiss_update_announcement, doctor_agent, download_and_install_update,
    get_acp_activity_detail, get_acp_raw_frames, get_acp_session, get_acp_tool_detail,
    get_agent_command_catalog, get_agent_registry, get_app_bootstrap, get_auto_templates,
    get_file_comparison, get_log_page, get_metrics_settings, get_profile, get_profiles,
    get_round_detail, get_run_detail, get_skill_sync_status, get_system_fonts, get_task_detail,
    get_task_list, get_turn_file_change_set, get_update_status, get_workflow,
    get_workflow_templates, list_conversation_directory, list_mcp_servers, list_mcp_tools,
    list_project_skills, list_skills, mark_settings_advanced_update_seen,
    mark_settings_update_seen, open_conversation_directory_path_in_file_manager,
    open_in_file_manager, pause_run, prepare_app_exit, read_conversation_directory_file,
    read_skill, remove_recent_workspace, renew_acp_session_lease, replace_auto_templates,
    respond_acp_permission, respond_elicitation, retry_run, save_auto_template,
    save_desktop_avatar, save_desktop_avatar_shape, save_desktop_preferences,
    save_metrics_settings, save_task_workflow, save_updater_settings, save_workflow_template,
    search_acp_prompts, search_acp_sessions, search_tasks, select_recent_desktop_avatar,
    select_recent_workspace, send_acp_prompt, set_acp_session_config_option, set_acp_session_model,
    set_acp_session_permission_mode, show_artifact, show_attachment, show_worker_ref, start_run,
    stop_active_session, submit_conversation_prompt, submit_manual_check, toggle_mcp_server,
    update_agent, update_auto_template, update_mcp_server, update_notification_attention,
    update_profile, update_skill_sync_targets, update_workflow_template, write_skill,
};
use commands_conversation::{
    add_conversation_workspace, choose_conversation_workspace, create_conversation_run,
    delete_conversation_task, get_conversation_run, get_conversation_run_mode,
    get_conversation_sidebar, get_conversation_workspaces, get_supported_attachment_extensions,
    materialize_conversation_attachments, pin_conversation, remove_conversation_workspace,
    reorder_pinned_conversations, rerun_conversation_task, save_conversation_preference,
    save_conversation_run_mode, save_desktop_ui_mode, save_last_conversation_workspace,
    search_conversation_tasks, show_conversation_attachment, show_conversation_message_attachment,
    stat_attachment_files, switch_conversation_session, sync_conversation_workspace,
    unpin_conversation, update_task_metadata, validate_conversation_create,
};
use gold_band::observability::{init_tracing, touch_log_file_best_effort};
use gold_band::storage::configure_storage_paths;
use gold_band::storage::sqlite::init_search_index;
use metrics::start_heartbeat_polling;
use state::{DesktopContext, DesktopState};
use tauri::Manager;
use tracing::info;
use updater::{retry_pending_startup_install, start_update_polling};
use workspace_files::{WorkspaceFileRuntime, WorkspaceFileWatchRuntime};

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "failed to start {} desktop: {error:?}",
            channel::current_channel_config().app_name
        );
    }
}

fn run() -> anyhow::Result<()> {
    configure_storage_paths(channel::storage_path_config());
    let context = DesktopContext::from_current_dir()?;
    #[cfg(all(debug_assertions, target_os = "windows"))]
    let webview_heap_diagnostics = webview_heap_diagnostics::initialize(&context)?;
    let mut tauri_context = tauri::generate_context!();
    #[cfg(target_os = "windows")]
    let desktop_window_chrome = window_chrome::desktop_window_chrome_vm();
    #[cfg(target_os = "windows")]
    if let Some(window) = tauri_context.config_mut().app.windows.first_mut() {
        // WebView2's opaque controller visibly lags behind Win32 edge resizing and exposes
        // black/white bars. Composition mode avoids that artifact while the CSS root still
        // paints an opaque application surface. Windows 11 keeps the DWM shadow for native
        // rounding; Windows 10 disables TAO's asymmetric undecorated frame and uses the
        // application-owned inset outline instead.
        window.transparent = true;
        window.shadow = desktop_window_chrome.native_shadow;
        #[cfg(debug_assertions)]
        {
            window.additional_browser_args =
                Some(webview_heap_diagnostics::additional_browser_arguments(
                    window.additional_browser_args.as_deref(),
                    &webview_heap_diagnostics.snapshot(),
                ));
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(window) = tauri_context.config_mut().app.windows.first_mut() {
        window.decorations = true;
        window.shadow = true;
        window.title_bar_style = tauri::TitleBarStyle::Overlay;
        window.hidden_title = true;
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(DesktopState::new(context))
        .manage(WorkspaceFileRuntime::default())
        .manage(WorkspaceFileWatchRuntime::default());
    #[cfg(all(debug_assertions, target_os = "windows"))]
    let builder = builder.manage(webview_heap_diagnostics);
    builder
        .register_asynchronous_uri_scheme_protocol(
            workspace_files::WORKSPACE_FILE_PREVIEW_PROTOCOL,
            |protocol_context, request, responder| {
                let runtime = protocol_context
                    .app_handle()
                    .state::<WorkspaceFileRuntime>()
                    .inner()
                    .clone();
                let request_path = request.uri().path().to_string();
                std::thread::spawn(move || {
                    responder.respond(workspace_files::preview_protocol_response(
                        &runtime,
                        &request_path,
                    ));
                });
            },
        )
        .setup(|app| {
            let state = app.state::<DesktopState>();
            let _ = state.cleanup_agent_diagnostic_processes();
            if let Ok(runtime_app) = state.app() {
                commands::register_lifecycle_subscribers(&runtime_app, app.handle());
                let _ = runtime_app.recover_interrupted_running_sessions();
            }
            // Initialize SQLite search index (best-effort; failures are non-fatal).
            // On first run (empty DB), a background thread backfills existing tasks/sessions.
            if let Ok(ctx) = state.context() {
                let paths = gold_band::storage::GoldBandPaths::new(ctx.repo_root);
                touch_log_file_best_effort(&paths);
                init_tracing(&paths, &ctx.config, true);
                info!(
                    repo_root = %paths.repo_root,
                    project_id = %paths.project_id,
                    needs_workspace = ctx.needs_workspace,
                    "desktop runtime initialized"
                );
                builtin_mcp::inject_builtin_mcp_servers(&state);
                let _ = init_search_index(&paths.sqlite_db_path(), &paths.projects_dir());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    let state = handle.state::<DesktopState>();
                    let diagnostics_refreshed = state.refresh_all_agent_diagnostics().is_ok();
                    let commands_refreshed = state
                        .refresh_agent_command_catalogs_for_active_workspaces()
                        .is_ok();
                    if diagnostics_refreshed {
                        commands::emit_agent_registry_updated(&handle);
                    }
                    if diagnostics_refreshed || commands_refreshed {
                        commands::emit_agent_commands_updated(&handle, None);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });
            // 启动后台线程预探测 MCP 服务健康状态（独立线程，避免阻塞 webview 主线程）。
            // 客户端启动后即开始检测，进入 MCP 管理页时状态已就绪，无需手动诊断。
            let health_handle = app.handle().clone();
            std::thread::spawn(move || {
                let state = health_handle.state::<DesktopState>();
                builtin_mcp::refresh_all_mcp_health(&state);
            });
            retry_pending_startup_install(&app.handle().clone());
            start_update_polling(app.handle().clone());
            start_heartbeat_polling(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_bootstrap,
            prepare_app_exit,
            get_system_fonts,
            check_local_claude,
            get_agent_registry,
            get_agent_command_catalog,
            create_agent,
            update_agent,
            delete_agent,
            doctor_agent,
            get_task_list,
            get_profiles,
            get_profile,
            create_profile,
            update_profile,
            delete_profile,
            choose_workspace,
            select_recent_workspace,
            remove_recent_workspace,
            get_task_detail,
            create_task,
            save_task_workflow,
            get_workflow,
            get_workflow_templates,
            save_workflow_template,
            update_workflow_template,
            delete_workflow_template,
            get_auto_templates,
            save_auto_template,
            update_auto_template,
            delete_auto_template,
            replace_auto_templates,
            get_run_detail,
            get_round_detail,
            get_log_page,
            get_acp_session,
            get_turn_file_change_set,
            get_file_comparison,
            get_acp_activity_detail,
            get_acp_tool_detail,
            renew_acp_session_lease,
            submit_conversation_prompt,
            send_acp_prompt,
            set_acp_session_model,
            set_acp_session_config_option,
            set_acp_session_permission_mode,
            respond_acp_permission,
            respond_elicitation,
            cancel_acp_session,
            get_acp_raw_frames,
            start_run,
            continue_run,
            pause_run,
            stop_active_session,
            submit_manual_check,
            retry_run,
            show_artifact,
            show_attachment,
            show_worker_ref,
            save_desktop_preferences,
            save_desktop_avatar,
            select_recent_desktop_avatar,
            save_desktop_avatar_shape,
            clear_desktop_avatar,
            save_updater_settings,
            get_metrics_settings,
            update_notification_attention,
            save_metrics_settings,
            get_update_status,
            mark_settings_update_seen,
            mark_settings_advanced_update_seen,
            dismiss_update_announcement,
            check_update_manual,
            download_and_install_update,
            search_acp_prompts,
            search_acp_sessions,
            search_tasks,
            // Conversation UI
            save_desktop_ui_mode,
            get_conversation_sidebar,
            get_conversation_workspaces,
            get_conversation_run,
            validate_conversation_create,
            create_conversation_run,
            rerun_conversation_task,
            switch_conversation_session,
            stat_attachment_files,
            materialize_conversation_attachments,
            show_conversation_attachment,
            show_conversation_message_attachment,
            update_task_metadata,
            delete_conversation_task,
            pin_conversation,
            unpin_conversation,
            reorder_pinned_conversations,
            search_conversation_tasks,
            get_conversation_run_mode,
            save_conversation_run_mode,
            choose_conversation_workspace,
            add_conversation_workspace,
            remove_conversation_workspace,
            sync_conversation_workspace,
            save_conversation_preference,
            save_last_conversation_workspace,
            get_supported_attachment_extensions,
            open_in_file_manager,
            list_conversation_directory,
            open_conversation_directory_path_in_file_manager,
            read_conversation_directory_file,
            workspace_files::list_workspace_directory,
            workspace_files::open_workspace_path_in_file_manager,
            workspace_files::search_workspace_files,
            workspace_files::resolve_workspace_file_link,
            workspace_files::read_file_resource,
            workspace_files::resolve_markdown_image,
            workspace_files::write_file_resource,
            workspace_files::release_workspace_file_preview,
            workspace_files::renew_external_file_access,
            workspace_files::release_external_file_access,
            workspace_files::start_workspace_file_watch,
            workspace_files::stop_workspace_file_watch,
            // MCP & SKILL management
            list_mcp_servers,
            add_mcp_server,
            update_mcp_server,
            delete_mcp_server,
            toggle_mcp_server,
            check_mcp_server_health,
            list_mcp_tools,
            list_skills,
            list_project_skills,
            read_skill,
            write_skill,
            delete_skill,
            update_skill_sync_targets,
            get_skill_sync_status,
            check_skill_name_conflict,
            feedback::submit_feedback,
            feedback::preview_feedback_session_archive,
            #[cfg(all(debug_assertions, target_os = "windows"))]
            webview_heap_diagnostics::get_webview_heap_diagnostic,
        ])
        .run(tauri_context)
        .context("tauri runtime failed")?;
    Ok(())
}
