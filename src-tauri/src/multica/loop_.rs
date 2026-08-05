//! multica 运行期循环（开发设计 2.6 / 4.4 / 接入方案 3.2.4 / 4.1 / 4.2 / C5）。
//!
//! 职责：
//! - **启动全量 register**：`verify_pat` 有效后，遍历 `desktop_multica_workspaces` 逐个 register，
//!   取回 runtime_id 缓存到 `MulticaRuntimeState`。
//! - **recover-orphans**：启动 register 后，对每个 runtime_id 清残留的在飞任务。
//! - **启动 reconcile**：recover-orphans 后，崩溃残留的 task_conversations 条目 remote cancelled/404
//!   → 作废本地 Paused run（不在 failed 上作废，保断点续跑，开发设计 4.4）。
//! - **执行期 15s 心跳 + 取消检测**：claim→complete 期间维持 runtime 在线；同 tick 取消检测——
//!   在飞 active_run 的 remote failed/cancelled/404 → 作废本地 run（接入方案 C5）。
//!
//! 复用 `metrics::start_heartbeat_polling` 骨架：`tauri::async_runtime::spawn` + 三层 guard
//! （try_state → context → multica_settings）+ 每 tick 重读配置（用户改配置即时生效）。

use std::time::Duration;

use camino::Utf8PathBuf;
use gold_band::config::MulticaWorkspaceRef;
use tauri::{AppHandle, Manager, Runtime};

use crate::metrics::get_system_username;
use crate::multica::bridge::{emit_multica_task_updated, teardown_active_run};
use crate::multica::client::{MulticaClient, RegisterRequest, RuntimeSpec};
use crate::multica::config::{binding_for_multica, get_daemon_id, get_pat, multica_base_url, multica_settings};
use crate::multica::error::MulticaError;
use crate::multica::state::SharedMulticaState;
use crate::state::DesktopState;

/// 执行期心跳间隔（秒）。≠ metrics 的 15min；两套并存，互不干扰（开发设计 2.6 / 第 6 章表3）。
pub const MULTICA_HEARTBEAT_INTERVAL_SECS: u64 = 15;

/// 启动 multica 运行期循环（main.rs setup，`start_heartbeat_polling` 之后挂载）。
///
/// PAT 无效 / 未配置 / 无 workspace → 静默跳过（首启绝不自动弹登录，等用户在远程任务列表
/// 点【连接 Multica】触发，开发设计 4.1）。
pub fn start_multica_loop<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        run_startup_registration(&app).await;
        run_heartbeat_loop(app).await;
    });
}

/// 启动一次性注册：verify_pat → 全量 register → recover-orphans（开发设计 4.1 / 4.2）。
async fn run_startup_registration<R: Runtime>(app: &AppHandle<R>) {
    let Some((client, workspaces, daemon_id)) = resolve_startup_params(app) else {
        return;
    };

    // 4.1: verify_pat → 200 才全量 register；否则静默（首启不弹登录）。
    if let Err(error) = client.verify_pat().await {
        tracing::info!(
            %error,
            "multica pat invalid or unreachable, skip startup register"
        );
        return;
    }

    let device_name = get_system_username();
    let cli_version = env!("CARGO_PKG_VERSION").to_string();
    let shared = app.try_state::<SharedMulticaState>();

    for workspace in &workspaces {
        let provider = workspace.provider.clone();
        let request = RegisterRequest {
            workspace_id: workspace.id.clone(),
            daemon_id: daemon_id.clone(),
            device_name: device_name.clone(),
            cli_version: cli_version.clone(),
            runtimes: vec![RuntimeSpec {
                name: provider.clone(),
                runtime_type: provider,
                version: cli_version.clone(),
                status: "ready".to_string(),
            }],
        };
        match client.register(&request).await {
            Ok(response) => match response.runtimes.first() {
                Some(row) => {
                    tracing::info!(
                        workspace = %workspace.id,
                        runtime_id = %row.id,
                        "multica register ok"
                    );
                    if let Some(state) = shared.as_ref() {
                        if let Ok(mut guard) = state.lock() {
                            guard.set_runtime_id(&workspace.id, &row.id);
                        }
                    }
                }
                None => tracing::warn!(
                    workspace = %workspace.id,
                    "multica register returned no runtime"
                ),
            },
            Err(error) => tracing::warn!(
                workspace = %workspace.id,
                %error,
                "multica register failed"
            ),
        }
    }

    // recover-orphans：对每个已注册 runtime 清残留任务（失败不阻断启动）。
    if let Some(state) = shared.as_ref() {
        let runtime_ids = state.lock().map(|guard| guard.runtime_ids()).unwrap_or_default();
        for runtime_id in runtime_ids {
            if let Err(error) = client.recover_orphans(&runtime_id).await {
                tracing::warn!(
                    runtime_id = %runtime_id,
                    %error,
                    "multica recover-orphans failed (non-fatal)"
                );
            }
        }
    }

    // 启动 reconcile（开发设计 4.4）：崩溃残留的 task_conversations 条目，remote cancelled/404 →
    // 作废本地 Paused run。**不在 failed 上作废**——retryable 失败会被 server 重派，由 re-claim 续跑
    // （断点续跑）；作废 failed 会丢失续跑索引。terminal-failed（agent_error）的本地 Paused run 由
    // strict_continue fallback 兜底（用户续跑死 session → 自动降级 fresh）。
    reconcile_startup_orphans(app, &client).await;
}

/// 执行期心跳循环（15s）：仅对有在飞任务的 runtime 发心跳 + 取消检测（开发设计 2.6 / 4.4）。
///
/// 每 tick：心跳维持 runtime 在线，随后取消检测——在飞 active_run 的 remote 若已 terminal
/// （failed/cancelled/404）→ 作废本地 run（接入方案 C5）。无在飞任务则空转。
async fn run_heartbeat_loop<R: Runtime>(app: AppHandle<R>) {
    loop {
        let runtime_ids = collect_active_runtime_ids(&app);
        if !runtime_ids.is_empty() {
            if let Some(client) = build_client(&app) {
                for runtime_id in &runtime_ids {
                    if let Err(error) = client.heartbeat(runtime_id).await {
                        tracing::warn!(
                            runtime_id = %runtime_id,
                            %error,
                            "multica heartbeat failed (will retry next tick)"
                        );
                    }
                }
                // 取消检测：active run 的 remote terminal → 作废本地（client.rs:524 C5）。
                detect_cancelled_active_runs(&app, &client).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(MULTICA_HEARTBEAT_INTERVAL_SECS)).await;
    }
}

/// 三层 guard + 取启动注册参数（未启用 / 无 PAT / 无 workspace → None，静默跳过）。
fn resolve_startup_params<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<(MulticaClient, Vec<MulticaWorkspaceRef>, String)> {
    let state = app.try_state::<DesktopState>()?;
    let context = state.context().ok()?;
    if !multica_settings(&context.config).enabled {
        return None;
    }
    let base_url = multica_base_url(&context.config)?;
    let pat = get_pat(&context.config)?;
    let daemon_id = get_daemon_id(&context.config)?;
    if context.config.desktop_multica_workspaces.is_empty() {
        return None;
    }
    let client = MulticaClient::new(base_url, Some(pat)).ok()?;
    Some((
        client,
        context.config.desktop_multica_workspaces.clone(),
        daemon_id,
    ))
}

/// 三层 guard + 构造已认证 client（心跳 tick 用，每 tick 重读配置）。
fn build_client<R: Runtime>(app: &AppHandle<R>) -> Option<MulticaClient> {
    let state = app.try_state::<DesktopState>()?;
    let context = state.context().ok()?;
    if !multica_settings(&context.config).enabled {
        return None;
    }
    let base_url = multica_base_url(&context.config)?;
    let pat = get_pat(&context.config)?;
    MulticaClient::new(base_url, Some(pat)).ok()
}

/// 三层 guard + 取有在飞任务的 runtime_id 集合（心跳遍历用）。
fn collect_active_runtime_ids<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let Some(desktop) = app.try_state::<DesktopState>() else {
        return Vec::new();
    };
    let Some(shared) = app.try_state::<SharedMulticaState>() else {
        return Vec::new();
    };
    let Ok(context) = desktop.context() else {
        return Vec::new();
    };
    if !multica_settings(&context.config).enabled {
        return Vec::new();
    }
    shared
        .lock()
        .map(|guard| guard.active_runtime_ids())
        .unwrap_or_default()
}

// ── 取消检测 / 启动 reconcile（开发设计 4.4 / 接入方案 C5）──────────────────────

/// 在飞 active_run 的 remote 是否为终态（作废本地）：failed/cancelled。
fn is_active_terminal(status: &str) -> bool {
    matches!(status, "failed" | "cancelled")
}

/// 崩溃残留 orphan 的 remote 是否为终态（作废本地）：仅 cancelled。
///
/// 不含 failed——retryable failed 会被 server 重派由 re-claim 续跑（断点续跑），作废会丢索引。
fn is_orphan_terminal(status: &str) -> bool {
    matches!(status, "cancelled")
}

/// 取消检测（接入方案 C5）：遍历在飞 active_run，remote failed/cancelled/404 → 作废本地 run。
///
/// active run 场景：本地正在执行，remote 已 terminal → 停本地（省算力、同步状态）。无 resume
/// 冲突——resume 是崩溃后 Paused run 的 re-claim 路径，与在飞 active run 互斥。
async fn detect_cancelled_active_runs<R: Runtime>(app: &AppHandle<R>, client: &MulticaClient) {
    for remote in active_run_remote_ids(app) {
        let invalidate = match client.get_task_status(&remote).await {
            Ok(status) => is_active_terminal(&status),
            Err(MulticaError::TaskNotFound) => true, // 404：task 已删
            Err(_) => false,                          // 暂态网络/解码 → 下 tick 重试
        };
        if invalidate {
            spawn_invalidate(app, &remote).await;
            // 本地 run 已作废（remote terminal / 404）→ 通知前端刷新 sidebar。
            emit_multica_task_updated(app);
        }
    }
}

/// 启动 reconcile：崩溃残留的 task_conversations 条目，remote cancelled/404 → 作废本地 Paused run。
async fn reconcile_startup_orphans<R: Runtime>(app: &AppHandle<R>, client: &MulticaClient) {
    for remote in orphan_remote_ids(app) {
        let invalidate = match client.get_task_status(&remote).await {
            Ok(status) => is_orphan_terminal(&status),
            Err(MulticaError::TaskNotFound) => true,
            Err(_) => false,
        };
        if invalidate {
            spawn_invalidate(app, &remote).await;
            // 本地 run 已作废（remote terminal / 404）→ 通知前端刷新 sidebar。
            emit_multica_task_updated(app);
        }
    }
}

/// 在飞 active_run 的 remote_task_id 集合（取消检测遍历用）。
fn active_run_remote_ids<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    app.try_state::<SharedMulticaState>()
        .and_then(|shared| shared.lock().ok().map(|g| g.active_runs.keys().cloned().collect()))
        .unwrap_or_default()
}

/// 崩溃残留 task_conversations 的 remote_task_id 集合（启动 reconcile 遍历用）。
fn orphan_remote_ids<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    app.try_state::<DesktopState>()
        .and_then(|desktop| desktop.context().ok())
        .and_then(|context| context.app().load_state().ok())
        .and_then(|state| state.multica_task_conversations)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// 异步包装：sync 作废本地 run 经 spawn_blocking 执行（不阻塞 async runtime）。
async fn spawn_invalidate<R: Runtime>(app: &AppHandle<R>, remote: &str) {
    let app = app.clone();
    let remote = remote.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || invalidate_remote_task(&app, &remote)).await;
}

/// 作废 remote task 对应本地 run（取消检测 / 启动 reconcile 共用）。
///
/// 解析 `(workspace_path, local_task_id, local_run_id)`：active_run 优先（在飞场景，经
/// `binding_for_multica` 解析 workspace_id→path）；回退 `task_conversations[remote]`（崩溃 orphan
/// 场景，active_runs 已丢，用持久化 work_dir + local ids）。再交 `bridge::teardown_active_run` 收尾。
fn invalidate_remote_task<R: Runtime>(app: &AppHandle<R>, remote: &str) {
    let Some(desktop) = app.try_state::<DesktopState>() else { return };
    let Ok(context) = desktop.context() else { return };
    let Some(shared) = app.try_state::<SharedMulticaState>() else { return };
    let home_app = context.app();

    // active_run 优先（在飞）→ 回退 task_conversations（崩溃 orphan）。
    let active = shared.lock().ok().and_then(|g| g.active_run(remote));
    let Some((workspace_path, local_task_id, local_run_id)) = active
        .and_then(|run| {
            let home_state = home_app.load_state().ok()?;
            let (wp, _) = binding_for_multica(&context.config, &home_state, &run.workspace_id)?;
            Some((wp, run.local_task_id, run.local_run_id))
        })
        .or_else(|| {
            let state = home_app.load_state().ok()?;
            let conv = state.multica_task_conversations.as_ref()?.get(remote)?;
            let wp = conv.work_dir.clone()?;
            Some((wp, conv.local_task_id.clone(), conv.local_run_id.clone()))
        })
    else {
        return;
    };

    tracing::info!(task = %remote, "multica cancel-detection: invalidating local run");
    let workspace_app = home_app.with_repo_root(Utf8PathBuf::from(workspace_path), context.config.clone());
    teardown_active_run(&workspace_app, &shared, &home_app, remote, &local_task_id, &local_run_id);
}

#[cfg(test)]
mod tests {
    use super::{is_active_terminal, is_orphan_terminal};

    #[test]
    fn active_terminal_flags_failed_and_cancelled() {
        // 在飞 active run：remote failed/cancelled → 作废本地（接入方案 C5）。
        assert!(is_active_terminal("failed"));
        assert!(is_active_terminal("cancelled"));
        // 非终态/已成功 → 不作废。
        assert!(!is_active_terminal("running"));
        assert!(!is_active_terminal("queued"));
        assert!(!is_active_terminal("dispatched"));
        assert!(!is_active_terminal("completed"));
    }

    #[test]
    fn orphan_terminal_flags_only_cancelled_not_failed() {
        // 崩溃残留 orphan：仅 cancelled 作废。failed 不作废——retryable 由 re-claim 续跑（断点续跑）。
        assert!(is_orphan_terminal("cancelled"));
        assert!(!is_orphan_terminal("failed"));
        assert!(!is_orphan_terminal("running"));
        assert!(!is_orphan_terminal("queued"));
    }
}
