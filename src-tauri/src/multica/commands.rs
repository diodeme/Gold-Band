//! multica 远程任务命令（开发设计 2.4 / 第 6 章表）。
//!
//! - [`get_multica_tasks`]：按 workspace 分组的远程 pending 列表 + 本地失败回显（`multica_pending_issues`）。
//! - [`claim_multica_task`]：selective claim（点击即领取，claim-at-click）。claim 成功后登记 prepare lease
//!   （心跳循环续期防 45s 回收）并回填 `requirement`（composer 预填输入框用）。
//! - [`start_multica_conversation_run`]：发送预填好的远程任务——**复用** 本地会话创建链路
//!   （`create_conversation_run_vm`：建工作流 + 建任务 + 写 conversation.json + 启动 run），
//!   仅叠加 multica 专属簿记（active_run + 断点续跑索引 + start_task + 释放 lease）。
//! - [`cancel_multica_prepare_lease`]：放弃 compose 时释放 lease（兜底是 45s 自然过期回收）。
//!
//! `pending_issues` 语义 = 失败待重试 issue（"失败回显"）：由 M4 终态 fail 写入、complete/rerun
//! 清除；**claim 不写**（刚领取的 running 任务不是 failed/retryable，写入会让 VM 把 in-flight 误显为可重试）。

use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use gold_band::config::{ConversationWorkspaceEntry, MulticaTaskConversation, MulticaWorkspaceRef};
use tauri::{AppHandle, State};
use tracing::{info, warn};

use crate::commands::{
    CommandErrorVm, CommandResult, acp_live_update_emitter_for_app, acp_session_update_emitter,
    command_error,
};
use crate::conversation_workspace::{project_id_for_workspace, project_ids_match, workspace_entry_for_project};
use crate::multica::client::{MulticaClient, RegisterRequest, RuntimeSpec, WorkspaceInfo};
use crate::multica::config::{
    MulticaSettingsVm, binding_for_multica, get_daemon_id, get_pat, multica_base_url, multica_settings,
};
use crate::multica::error::MulticaError;
use crate::multica::state::{ActiveRemoteRun, SharedMulticaState};
use crate::multica::vm::{MulticaCompletedTaskVm, RemoteConversationSidebarVm, RemoteTaskVm};
use crate::state::DesktopState;
use crate::view_models_conversation::{
    ConversationCreateInputVm, ConversationRunVm, create_conversation_run_vm,
    validate_conversation_create_vm,
};

/// 远程任务列表（按 workspace 分组，对齐 `ConversationSidebarVm` 形状）。
///
/// 数据源：远程 queued（`list_pending_tasks`，逐已注册 workspace）+ 本地失败回显
/// （`multica_pending_issues` → `pinned_tasks`，retryable=true）。未连接（无 PAT）→ 空状态 sidebar
/// （`connected=false`，前端展示连接入口，不报错、不另查 patSet）。
#[tauri::command]
pub async fn get_multica_tasks(
    state: State<'_, DesktopState>,
    shared: State<'_, SharedMulticaState>,
) -> CommandResult<RemoteConversationSidebarVm> {
    let context = state.context().map_err(command_error)?;
    let settings = multica_settings(&context.config);
    let workspaces = context.config.desktop_multica_workspaces.clone();
    let last_active_workspace_id = context.config.desktop_multica_active_workspace_id.clone();

    // 未连接 → 空状态（前端展示连接入口，开发设计 2.4）。
    if !settings.connected {
        return Ok(RemoteConversationSidebarVm {
            workspaces,
            tasks_by_workspace: BTreeMap::new(),
            pinned_tasks: Vec::new(),
            recently_completed: Vec::new(),
            last_active_workspace_id,
            connected: false,
        });
    }

    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;

    // 远程 pending：逐 workspace（已注册 runtime_id）拉取并分组。失败仅跳过该 workspace（不阻断列表）。
    let mut tasks_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>> = BTreeMap::new();
    for workspace in &workspaces {
        let runtime_id = shared
            .lock()
            .ok()
            .and_then(|guard| guard.runtime_id(&workspace.id).map(str::to_string));
        let Some(runtime_id) = runtime_id else {
            continue; // workspace 未注册（无 runtime_id）→ 跳过。
        };
        match client.list_pending_tasks(&runtime_id).await {
            Ok(tasks) => {
                let rows = tasks
                    .iter()
                    .map(|task| RemoteTaskVm::from_pending(task, &workspace.id))
                    .collect();
                tasks_by_workspace.insert(workspace.id.clone(), rows);
            }
            Err(error) => warn!(
                workspace = %workspace.id,
                %error,
                "multica list_pending failed (skipped workspace)"
            ),
        }
    }

    // 本地失败回显 + 「最近完成」回看：同一次 load_state 读取
    // （multica_pending_issues / multica_completed_tasks）。读失败不阻断列表（仅记日志返回空）。
    let (pinned_tasks, recently_completed) = match context.app().load_state() {
        Ok(state_cfg) => {
            let pinned: Vec<RemoteTaskVm> = state_cfg
                .multica_pending_issues
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|issue_id| RemoteTaskVm::from_failed_issue(issue_id))
                .collect();
            // 「最近完成」（Issue 3C）：按 workspaces 解析 workspace_id → local_project_id，
            // 供前端 onSelectRun(project_id, local_task_id, run_id) 直达本地会话。
            // 未绑定/已解绑的 workspace（无 project_id）则跳过（无可直达的本地会话）。
            let completed: Vec<MulticaCompletedTaskVm> = state_cfg
                .multica_completed_tasks
                .iter()
                .filter_map(|c| {
                    let project_id = workspaces
                        .iter()
                        .find(|w| w.id == c.workspace_id)
                        .map(|w| w.local_project_id.clone())?;
                    Some(MulticaCompletedTaskVm {
                        remote_task_id: c.remote_task_id.clone(),
                        local_task_id: c.local_task_id.clone(),
                        run_id: c.local_run_id.clone(),
                        workspace_id: c.workspace_id.clone(),
                        project_id,
                        title: c.title.clone(),
                        status: c.status.clone(),
                        completed_at: c.completed_at.clone(),
                    })
                })
                .collect();
            (pinned, completed)
        }
        Err(error) => {
            warn!(%error, "multica load_state for pending/completed failed");
            (Vec::new(), Vec::new())
        }
    };

    Ok(RemoteConversationSidebarVm {
        workspaces,
        tasks_by_workspace,
        pinned_tasks,
        recently_completed,
        last_active_workspace_id,
        connected: true,
    })
}

/// selective claim：点哪领哪（claim-at-click，开发设计 2.4 / 4.4）。
///
/// 命中本地 `multica_task_conversations[task_id].session_id` 时带 `prior_session_id` 续跑同一 ACP
/// session。claim 成功后：① 登记 prepare lease（心跳循环续期，防 compose 期间 45s 回收；同时缓存
/// 任务身份 `issue_id`/`title` 供 start 消费）；② 回填 `requirement`（来自 `requirement_text()`），
/// 供前端 composer 预填输入框。`auth_token` 不回显（执行凭证不进 VM）。
#[tauri::command]
pub async fn claim_multica_task(
    state: State<'_, DesktopState>,
    shared: State<'_, SharedMulticaState>,
    task_id: String,
    workspace_id: String,
) -> CommandResult<RemoteTaskVm> {
    let context = state.context().map_err(command_error)?;
    if !multica_settings(&context.config).connected {
        return Err(command_error(MulticaError::NotConfigured.into()));
    }
    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;

    // runtime_id 来自该 workspace 的启动注册缓存（未注册 → runtime-offline）。
    let runtime_id = shared
        .lock()
        .ok()
        .and_then(|guard| guard.runtime_id(&workspace_id).map(str::to_string))
        .ok_or(MulticaError::RuntimeOffline)
        .map_err(|e| command_error(e.into()))?;

    // 续跑命中：本地 task_conversations[task_id].session_id 非空 → 带 prior_session_id 续跑。
    let prior_session_id = context
        .app()
        .load_state()
        .ok()
        .and_then(|state_cfg| {
            state_cfg
                .multica_task_conversations
                .as_ref()
                .and_then(|map| map.get(&task_id))
                .and_then(|conv| conv.session_id.clone())
        });

    let task = client
        .claim_specific_task(&runtime_id, &task_id, prior_session_id.as_deref())
        .await
        .map_err(|e| command_error(e.into()))?;

    // claim 成功 → 登记 prepare lease（心跳循环续期 + 缓存任务身份供 start 消费）。
    // issue_id/title 取自 claim 响应（start 命令无权再读 claim，靠 lease 传递）。
    if let Ok(mut guard) = shared.lock() {
        guard.register_prepare_lease(
            &task_id,
            runtime_id,
            task.issue_id.clone(),
            task.title.clone(),
        );
    }

    Ok(RemoteTaskVm::from_claimed(&task, &workspace_id))
}

/// 发送预填好的远程任务：复用本地会话创建链路 + 叠加 multica 簿记（开发设计 2.5 / 2.8 / 4.3）。
///
/// 与本地工作空间「+」号进入的是**同一创建链路**：解析 workspace 绑定目录 → 构造 workspace-bound App
/// （注入与 `create_conversation_run` 同款的 ACP emitter，NodeCompleted/RunCompleted 流向前端）→
/// `validate_conversation_create_vm` → **复用** `create_conversation_run_vm`（建工作流 + 建任务 +
/// 写 conversation.json + 拷附件 + 启动 run）。远程任务预填的需求即 `input.requirement`，用户在 composer
/// 已选好模型/模式（与本地完全一致）。
///
/// multica 专属叠加（成功建 run 后）：① `register_active_run`（真实 run.id，先于事件归属反查）；
/// ② 持久化 `multica_task_conversations`（断点续跑索引，session_id 待 bridge 回填）；③ `start_task`
/// （dispatched→running，claim 后 5min 内不 start 会被超时 fail）；④ 释放 prepare lease（compose 期间
/// 心跳续期使命完成）。
///
/// lease 读取用 peek（不移除）：失败路径（校验/建 run/HTTP）下 lease 留存，心跳继续续期，任务保持已领取态
/// 可重试发送；仅成功建 run + start 后才 drop。
///
/// 库层 sync App 调用经 `spawn_blocking` 执行；HTTP（start/fail）留在 async 上下文。
#[tauri::command]
pub async fn start_multica_conversation_run(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    shared: State<'_, SharedMulticaState>,
    input: ConversationCreateInputVm,
    remote_task_id: String,
    workspace_id: String,
) -> CommandResult<ConversationRunVm> {
    let context = state.context().map_err(command_error)?;
    if !multica_settings(&context.config).connected {
        return Err(command_error(MulticaError::NotConfigured.into()));
    }
    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;

    // ① 消费 claim 时缓存的 lease 身份（runtime_id + issue_id + thread_name）；未 claim 直接 send → 拒绝。
    // peek 不移除：失败路径 lease 留存续期，成功后才 drop。
    let lease = shared
        .lock()
        .ok()
        .and_then(|guard| guard.prepare_lease(&remote_task_id))
        .ok_or(MulticaError::TaskNotFound)
        .map_err(|e| command_error(e.into()))?;

    // ② resolve workspace（input.project_id = 绑定 workspace 的 local_project_id，前端预填时已确定）。
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &input.project_id)
    else {
        return Err(CommandErrorVm::new(
            "workspace.not-found",
            serde_json::json!({ "projectId": input.project_id }),
        ));
    };

    // ③ workspace-bound App + 与 create_conversation_run 同款的 ACP emitter 注入（事件流向前端）。
    let workspace_app = state
        .app()
        .map_err(command_error)?
        .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
    let mut input = input;
    input.project_id = resolved_project_id.clone();
    // 先算 emitter（借 workspace_app）再 move——避免在 with_acp_live_update 表达式内同时借与 move。
    let live_update = acp_live_update_emitter_for_app(
        &workspace_app,
        app_handle.clone(),
        Some(resolved_project_id.clone()),
    );
    let app = workspace_app
        .with_acp_live_update(live_update)
        .with_acp_session_update(acp_session_update_emitter(
            app_handle.clone(),
            state
                .app()
                .map_err(command_error)?
                .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone()),
            Some(resolved_project_id.clone()),
        ));

    // ④ 复用本地链路校验（与本地「+」同一校验：模型/agent 可用性等）。
    let validation = validate_conversation_create_vm(&app, &input).map_err(command_error)?;
    if !validation.valid {
        return Err(CommandErrorVm::new(
            "conversation.validation-failed",
            serde_json::json!({
                "codes": validation
                    .missing_items
                    .iter()
                    .map(|item| item.code.clone())
                    .collect::<Vec<_>>()
            }),
        ));
    }

    // 搬运到 spawn_blocking 闭包的 multica 簿记数据。
    let remote = remote_task_id.clone();
    let ws_id = workspace_id.clone();
    let rt_id = lease.runtime_id.clone();
    let issue = lease.issue_id.clone();
    let title = lease.title.clone();
    let ws_path = workspace_path.clone();
    let shared_clone = shared.inner().clone();
    let ctx_clone = context.clone();

    // ⑤ 复用 create_conversation_run_vm（建工作流 + 建任务 + 写 conversation.json + 启动 run）+ 叠加簿记。
    let result = tauri::async_runtime::spawn_blocking(
        move || -> anyhow::Result<ConversationRunVm> {
            let run = create_conversation_run_vm(&app, &input)?;
            // 登记 active_run（真实 run.id，先于 NodeCompleted/RunCompleted 归属反查）。
            if let Ok(mut guard) = shared_clone.lock() {
                guard.register_active_run(
                    &remote,
                    ActiveRemoteRun {
                        runtime_id: rt_id,
                        workspace_id: ws_id,
                        local_task_id: run.task_id.clone(),
                        local_run_id: run.run_id.clone(),
                        issue_id: issue,
                        title,
                        started_at: chrono::Utc::now().to_rfc3339(),
                    },
                );
                // 建成 run + 登记 → 释放 lease（compose 期间续期使命完成）。失败时已在上面 ? 返回，lease 留存。
                guard.drop_prepare_lease(&remote);
            }
            // 落断点续跑索引（home-repo StateConfig）：新 run 的 local ids + work_dir；session_id 待 bridge 回填。
            let mut state_cfg = ctx_clone.app().load_state()?;
            let mut conversations = state_cfg.multica_task_conversations.clone().unwrap_or_default();
            let entry = conversations.entry(remote.clone()).or_insert(MulticaTaskConversation {
                local_task_id: run.task_id.clone(),
                local_run_id: run.run_id.clone(),
                session_id: None,
                work_dir: Some(ws_path.clone()),
            });
            entry.local_task_id = run.task_id.clone();
            entry.local_run_id = run.run_id.clone();
            entry.work_dir = Some(ws_path);
            state_cfg.multica_task_conversations = Some(conversations);
            ctx_clone.app().save_state(&state_cfg)?;
            Ok(run)
        },
    )
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?;

    match result {
        Ok(run) => {
            // 本地 run 已登记 → 通知 server dispatched→running（claim 后 5min 内不 start 会被超时 fail）。
            // composer 流总是 fresh（与本地「+」一致；断点续跑由 server 重派 + bridge 兜底，不在此分支）。
            client
                .start_task(&remote_task_id, false)
                .await
                .map_err(|e| command_error(e.into()))?;
            Ok(run)
        }
        Err(error) => {
            // 本地启动失败：best-effort fail_task，避免 server 干等 5min 超时（reason=agent_error，resume-unsafe）。
            let _ = client
                .fail_task(&remote_task_id, "local start failed", "agent_error")
                .await;
            Err(command_error(error))
        }
    }
}

/// 放弃 compose 时释放 prepare lease（开发设计 2.5 / 接入方案 Req D.2）。
///
/// 用户在预填页返回/新建其它会话 → 调此命令丢弃 lease，心跳停止续期；server 的 45s prepare lease
/// 自然过期后由 `ReclaimStaleDispatchedTaskForRuntime` 回收回 pending 可领取态（兜底）。幂等：无 lease 返回 Ok。
#[tauri::command]
pub fn cancel_multica_prepare_lease(
    shared: State<'_, SharedMulticaState>,
    remote_task_id: String,
) -> CommandResult<()> {
    if let Ok(mut guard) = shared.lock() {
        guard.drop_prepare_lease(&remote_task_id);
    }
    Ok(())
}

/// 中断 multica 远程任务的本地 run（开发设计 2.8 / 4.4 取消检测）。
///
/// 取消检测命中（remote cancelled/failed/404）或用户手动取消时调用：`run_pause(ProcessInterrupted)`
/// + 杀 ACP 子进程（复用 gold-band stop session），并清本地索引（`active_runs` + 该 remote task 的
/// `multica_task_conversations` 条目）——cancelled task 不再断点续跑。bridge 对 RunPaused 不上报终态
/// （Paused 盲区），multica 侧已 terminal，无需 complete/fail。
#[tauri::command]
pub async fn cancel_multica_task(
    state: State<'_, DesktopState>,
    shared: State<'_, SharedMulticaState>,
    task_id: String,
) -> CommandResult<()> {
    let context = state.context().map_err(command_error)?;
    // 反查在飞映射（remote_task_id → 本地 task/run + workspace）。
    let run = shared
        .lock()
        .ok()
        .and_then(|guard| guard.active_run(&task_id))
        .ok_or(MulticaError::TaskNotFound)
        .map_err(|e| command_error(e.into()))?;
    // 解析 workspace 目录（run.workspace_id → 绑定 workspace_path）。
    let home_state = context.app().load_state().map_err(command_error)?;
    let (workspace_path, _) =
        binding_for_multica(&context.config, &home_state, &run.workspace_id)
            .ok_or(MulticaError::TaskNotFound)
            .map_err(|e| command_error(e.into()))?;
    let workspace_app = state
        .app()
        .map_err(command_error)?
        .with_repo_root(Utf8PathBuf::from(workspace_path), context.config.clone());

    let local_task_id = run.local_task_id.clone();
    let local_run_id = run.local_run_id.clone();
    let remote = task_id.clone();
    let shared_clone = shared.inner().clone();
    let home_app = context.app();

    tauri::async_runtime::spawn_blocking(
        move || {
            // 作废本地 run（Paused，bridge 不上报）+ 杀 ACP + 清 active_runs/task_conversations。
            // 复用 bridge::teardown_active_run（取消检测 / 启动 reconcile 共用同一收尾）。
            crate::multica::bridge::teardown_active_run(
                &workspace_app,
                &shared_clone,
                &home_app,
                &remote,
                &local_task_id,
                &local_run_id,
            );
        },
    )
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?;
    Ok(())
}

/// 用户手动重跑失败任务（开发设计 4.4 / 接入方案 D1）。
///
/// `POST /api/issues/{id}/rerun`（X-Workspace-ID 路由）→ server 创建全新 queued 任务
/// （force_fresh_session，整任务重跑，不续跑旧 session）。成功后清本地 `multica_pending_issues`
/// 中该 issue 的失败回显（rerun 已消费）。新 task 进列表由用户 fresh claim（新本地 task/run，
/// 与旧 session 无关）。
#[tauri::command]
pub async fn rerun_multica_task(
    state: State<'_, DesktopState>,
    issue_id: String,
    workspace_id: String,
) -> CommandResult<()> {
    let context = state.context().map_err(command_error)?;
    if !multica_settings(&context.config).connected {
        return Err(command_error(MulticaError::NotConfigured.into()));
    }
    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;

    // D1：force_fresh_session 整任务重跑。失败按错误码返回（前端查文案）。
    client
        .rerun_issue(&workspace_id, &issue_id)
        .await
        .map_err(|e| command_error(e.into()))?;

    // 清本地失败回显（rerun 已消费该 issue；新 task 完成时 complete 会再清，幂等）。
    let app = context.app();
    if let Ok(mut state_cfg) = app.load_state() {
        if let Some(list) = state_cfg.multica_pending_issues.as_mut() {
            list.retain(|i| i != &issue_id);
        }
        app.save_state(&state_cfg).map_err(command_error)?;
    }
    Ok(())
}

// ===== M3.5 / M5-c：multica workspace 绑定 CRUD（开发设计 2.2.1 / 2.5）=====
//
// 一个 multica workspace 绑定一个本地目录（`local_project_id` 指向 `conversation_workspaces`
// 条目）+ 一个执行 provider（绑定后不可变）。绑定目录复用 `conversation_workspace` 的 project_id
// 派生与去重，**不在 multica 侧重复存 path**（避免双份不一致，见 binding_for_multica）。
// 文件夹选择在前端经 `@tauri-apps/plugin-dialog` 完成（同 `add_conversation_workspace`），命令只接 path。

/// 列出 multica server 侧可见的 workspace（下拉单选用，id+name）。
///
/// 未连接（无 PAT）-> `multica.not-configured`，前端引导连接。
#[tauri::command]
pub async fn list_server_multica_workspaces(
    state: State<'_, DesktopState>,
) -> CommandResult<Vec<WorkspaceInfo>> {
    let context = state.context().map_err(command_error)?;
    if !multica_settings(&context.config).connected {
        return Err(command_error(MulticaError::NotConfigured.into()));
    }
    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;
    client
        .list_workspaces()
        .await
        .map_err(|e| command_error(e.into()))
}

/// 绑定一个 multica workspace 到本地目录（首个绑定自动设为 active），并即时 register。
///
/// `slug`：server `list_workspaces` 不暴露 slug，用 `id` 兜底（与 `config.rs` 测试约定一致，
/// slug 不在任何关键路径）。`provider` 绑定后不可变（改绑见 [`rebind_multica_workspace`]）。
///
/// 绑定后即时 register（取回 runtime_id）——「绑定即可用」，无需重启等启动 loop 全量 register
/// （复刻 `loop_.rs::run_startup_registration` 单 workspace 逻辑，失败非致命，启动 loop 兜底）。
#[tauri::command]
pub async fn add_multica_workspace(
    state: State<'_, DesktopState>,
    shared: State<'_, SharedMulticaState>,
    app_handle: AppHandle,
    workspace_id: String,
    workspace_name: String,
    workspace_path: String,
    provider: String,
) -> CommandResult<MulticaSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let workspace_path_str = Utf8PathBuf::from(&workspace_path).as_str().to_string();
    let project_id = project_id_for_workspace(&workspace_path_str);
    let local_name = std::path::Path::new(&workspace_path_str)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path_str.clone());

    // ① 落 conversation_workspaces 条目（该目录尚未添加才写，去重 by project_id）
    let mut state_cfg = app.load_state().map_err(command_error)?;
    if !state_cfg
        .conversation_workspaces
        .iter()
        .any(|w| project_ids_match(&w.project_id, &project_id))
    {
        state_cfg.conversation_workspaces.push(ConversationWorkspaceEntry {
            project_id: project_id.clone(),
            workspace_path: workspace_path_str,
            name: local_name,
            added_at: chrono::Utc::now().to_rfc3339(),
        });
        app.save_state(&state_cfg).map_err(command_error)?;
    }

    // ② 落 multica 绑定（去重 by multica workspace id；SettingsConfig 字段为 Option<Vec>）
    let mut settings = app.load_settings().map_err(command_error)?;
    let workspaces = settings.desktop_multica_workspaces.get_or_insert_with(Vec::new);
    if workspaces.iter().any(|w| w.id == workspace_id) {
        return Err(CommandErrorVm::new(
            "multica.workspace-already-bound",
            serde_json::json!({ "workspaceId": workspace_id }),
        ));
    }
    workspaces.push(MulticaWorkspaceRef {
        id: workspace_id.clone(),
        name: workspace_name,
        slug: workspace_id.clone(),
        local_project_id: project_id,
        provider: provider.clone(),
    });
    if settings.desktop_multica_active_workspace_id.is_none() {
        settings.desktop_multica_active_workspace_id = Some(workspace_id.clone());
    }
    app.save_settings(&settings).map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;

    // ③ 即时 register：绑定后立即可用（无需重启等启动 loop）。失败非致命——启动 loop 兜底重试。
    register_workspace_best_effort(&state, &shared, &workspace_id, &provider).await;

    // ④ 工作空间绑定变更 → 通知任务列表 + 设置页 re-fetch（跨视图同步）。
    crate::multica::bridge::emit_multica_settings_updated(&app_handle);

    let updated_context = state.context().map_err(command_error)?;
    Ok(multica_settings(&updated_context.config))
}

/// 绑定后即时 register 单个 workspace（取回 runtime_id 缓存到 [`SharedMulticaState`]）。
///
/// 复刻 `loop_.rs::run_startup_registration` 的单 workspace 注册逻辑。未连接（无 PAT/daemon_id）
/// 或 register 失败 → 静默跳过（非致命：启动 loop 会兜底全量 register）。
async fn register_workspace_best_effort(
    state: &State<'_, DesktopState>,
    shared: &State<'_, SharedMulticaState>,
    workspace_id: &str,
    provider: &str,
) {
    let Ok(context) = state.context() else {
        return;
    };
    let Some(base_url) = multica_base_url(&context.config) else {
        return;
    };
    let Some(pat) = get_pat(&context.config) else {
        return;
    };
    let Some(daemon_id) = get_daemon_id(&context.config) else {
        return;
    };
    let Ok(client) = MulticaClient::new(base_url, Some(pat)) else {
        return;
    };
    let device_name = crate::metrics::get_system_username();
    let cli_version = env!("CARGO_PKG_VERSION").to_string();
    let request = RegisterRequest {
        workspace_id: workspace_id.to_string(),
        daemon_id: daemon_id.clone(),
        device_name: device_name.clone(),
        cli_version: cli_version.clone(),
        runtimes: vec![RuntimeSpec {
            name: provider.to_string(),
            runtime_type: provider.to_string(),
            version: cli_version.clone(),
            status: "ready".to_string(),
        }],
    };
    match client.register(&request).await {
        Ok(response) => match response.runtimes.first() {
            Some(row) => {
                if let Ok(mut guard) = shared.lock() {
                    guard.set_runtime_id(workspace_id, &row.id);
                }
                info!(
                    workspace = %workspace_id,
                    runtime_id = %row.id,
                    "multica register-on-add ok"
                );
            }
            None => warn!(
                workspace = %workspace_id,
                "multica register-on-add returned no runtime"
            ),
        },
        Err(error) => warn!(
            workspace = %workspace_id,
            %error,
            "multica register-on-add failed (non-fatal, startup loop will retry)"
        ),
    }
}

/// 改绑 multica workspace 的本地目录（provider 绑定后不可变，仅换 path）。
#[tauri::command]
pub fn rebind_multica_workspace(
    state: State<'_, DesktopState>,
    app_handle: AppHandle,
    workspace_id: String,
    workspace_path: String,
) -> CommandResult<MulticaSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let workspace_path_str = Utf8PathBuf::from(&workspace_path).as_str().to_string();
    let project_id = project_id_for_workspace(&workspace_path_str);
    let local_name = std::path::Path::new(&workspace_path_str)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| workspace_path_str.clone());

    // ① 落/去重 conversation_workspaces 条目（新目录尚未添加才写）
    let mut state_cfg = app.load_state().map_err(command_error)?;
    if !state_cfg
        .conversation_workspaces
        .iter()
        .any(|w| project_ids_match(&w.project_id, &project_id))
    {
        state_cfg.conversation_workspaces.push(ConversationWorkspaceEntry {
            project_id: project_id.clone(),
            workspace_path: workspace_path_str,
            name: local_name,
            added_at: chrono::Utc::now().to_rfc3339(),
        });
        app.save_state(&state_cfg).map_err(command_error)?;
    }

    // ② 改绑 local_project_id（provider 不动；SettingsConfig 字段为 Option<Vec>）
    let mut settings = app.load_settings().map_err(command_error)?;
    let mref = settings
        .desktop_multica_workspaces
        .get_or_insert_with(Vec::new)
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| {
            CommandErrorVm::new(
                "multica.workspace-not-found",
                serde_json::json!({ "workspaceId": workspace_id }),
            )
        })?;
    mref.local_project_id = project_id;
    app.save_settings(&settings).map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    // 工作空间绑定变更 → 通知任务列表 + 设置页 re-fetch（跨视图同步）。
    crate::multica::bridge::emit_multica_settings_updated(&app_handle);
    let updated_context = state.context().map_err(command_error)?;
    Ok(multica_settings(&updated_context.config))
}

/// 移除 multica workspace 绑定（不删 conversation_workspaces 条目--其可能被本地会话复用）。
/// 移除的是 active -> 回退到首个绑定（无则清空）。
#[tauri::command]
pub fn remove_multica_workspace(
    state: State<'_, DesktopState>,
    app_handle: AppHandle,
    workspace_id: String,
) -> CommandResult<MulticaSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let mut settings = app.load_settings().map_err(command_error)?;
    let workspaces = settings.desktop_multica_workspaces.get_or_insert_with(Vec::new);
    let before = workspaces.len();
    workspaces.retain(|w| w.id != workspace_id);
    if workspaces.len() == before {
        return Err(CommandErrorVm::new(
            "multica.workspace-not-found",
            serde_json::json!({ "workspaceId": workspace_id }),
        ));
    }
    if settings.desktop_multica_active_workspace_id.as_deref() == Some(workspace_id.as_str()) {
        settings.desktop_multica_active_workspace_id = workspaces.first().map(|w| w.id.clone());
    }
    app.save_settings(&settings).map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    // 工作空间绑定变更 → 通知任务列表 + 设置页 re-fetch（跨视图同步）。
    crate::multica::bridge::emit_multica_settings_updated(&app_handle);
    let updated_context = state.context().map_err(command_error)?;
    Ok(multica_settings(&updated_context.config))
}

/// 设当前活跃 multica workspace（纯视图切换，不触发 register--register 由 loop 全量/增量处理）。
#[tauri::command]
pub fn set_active_multica_workspace(
    state: State<'_, DesktopState>,
    app_handle: AppHandle,
    workspace_id: String,
) -> CommandResult<MulticaSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();
    let mut settings = app.load_settings().map_err(command_error)?;
    if !settings
        .desktop_multica_workspaces
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|w| w.id == workspace_id)
    {
        return Err(CommandErrorVm::new(
            "multica.workspace-not-found",
            serde_json::json!({ "workspaceId": workspace_id }),
        ));
    }
    settings.desktop_multica_active_workspace_id = Some(workspace_id);
    app.save_settings(&settings).map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;
    // 工作空间绑定变更 → 通知任务列表 + 设置页 re-fetch（跨视图同步）。
    crate::multica::bridge::emit_multica_settings_updated(&app_handle);
    let updated_context = state.context().map_err(command_error)?;
    Ok(multica_settings(&updated_context.config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_prepare_lease_is_idempotent() {
        // 放弃 compose：有 lease → 丢弃；无 lease → 幂等返回 Ok（兜底是 45s 自然过期回收）。
        let shared = crate::multica::state::shared_state();
        // 无 lease：不报错。
        assert!(shared.lock().unwrap().drop_prepare_lease("remote-x").is_none());
        // 登记后丢弃：返回 Some（携带身份），再丢返回 None。
        shared
            .lock()
            .unwrap()
            .register_prepare_lease("remote-1", "rt-a".into(), None, None);
        assert!(shared.lock().unwrap().drop_prepare_lease("remote-1").is_some());
        assert!(shared.lock().unwrap().drop_prepare_lease("remote-1").is_none());
    }

    #[test]
    fn prepare_lease_peek_does_not_consume() {
        // start 命令用 peek 读身份：失败路径 lease 留存（心跳续期），成功后才 drop。
        let shared = crate::multica::state::shared_state();
        shared.lock().unwrap().register_prepare_lease(
            "remote-1",
            "rt-a".into(),
            Some("issue-1".into()),
            Some("Thread name".into()),
        );
        // peek 不移除 → snapshot 仍含该条（心跳仍续期）。
        let peeked = shared.lock().unwrap().prepare_lease("remote-1").expect("应命中");
        assert_eq!(peeked.runtime_id, "rt-a");
        assert_eq!(peeked.issue_id.as_deref(), Some("issue-1"));
        assert_eq!(peeked.title.as_deref(), Some("Thread name"));
        assert!(!shared.lock().unwrap().prepare_leases_snapshot().is_empty());
        // 显式 drop 才移除（成功建 run 后）。
        shared.lock().unwrap().drop_prepare_lease("remote-1");
        assert!(shared.lock().unwrap().prepare_lease("remote-1").is_none());
    }
}
