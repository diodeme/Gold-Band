//! multica 运行期循环（开发设计 2.6 / 4.4 / 接入方案 3.2.4 / 4.1 / 4.2 / C5）。
//!
//! 职责：
//! - **启动全量 register**：`verify_pat` 有效后，遍历 `desktop_multica_workspaces` 逐个 register，
//!   取回 runtime_id 缓存到 `MulticaRuntimeState`。
//! - **recover-orphans**：启动 register 后，对每个 runtime_id 清残留的在飞任务。
//! - **启动 reconcile**：recover-orphans 后，崩溃残留的 task_conversations 条目 remote cancelled/404
//!   -> 作废本地 Paused run（不在 failed 上作废，保断点续跑，开发设计 4.4）。
//! - **常驻 15s 心跳 + 自愈注册 + 取消检测**：与 multica 建立连接后始终保持心跳（不再仅在任务执行时）；
//!   每 tick 先自愈注册（已连接但 runtime_ids 缺失的已绑定 workspace -> 重注册，根因修复 Bug 1），
//!   再对 `(workspace_id, runtime_id)` 发心跳（runtime 行失效 404 -> 清缓存触发下 tick 自愈重注册），
//!   再对在飞 active_run 做取消检测--remote failed/cancelled/404 -> 作废本地 run（接入方案 C5）。
//!
//! 复用 `metrics::start_heartbeat_polling` 骨架：`tauri::async_runtime::spawn` + 三层 guard
//! （try_state -> context -> multica_settings）+ 每 tick 重读配置（用户改配置即时生效）。

use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use gold_band::config::MulticaWorkspaceRef;
use tauri::{AppHandle, Manager, Runtime};

use crate::channel::current_channel_config;
use crate::conversation_workspace::workspace_entry_for_project;
use crate::metrics::get_system_username;
use crate::multica::bridge::{emit_multica_task_updated, teardown_active_run};
use crate::multica::client::{MulticaClient, RegisterRequest, RuntimeSpec};
use crate::multica::config::{get_daemon_id, get_pat, multica_base_url, multica_settings};
use crate::multica::error::MulticaError;
use crate::multica::state::SharedMulticaState;
use crate::state::DesktopState;

/// 常驻心跳间隔（秒）。≠ metrics 的 15min；两套并存，互不干扰（开发设计 2.6 / 第 6 章表3）。
pub const MULTICA_HEARTBEAT_INTERVAL_SECS: u64 = 15;

/// 注册单个 workspace 并缓存 runtime_id（启动全量 / 自愈增量 / connect 触发 / 绑定即时 共用底层）。
///
/// 统一 register 逻辑，杜绝四处复制：构造 `RegisterRequest`（device_name/cli_version 内部推导）
/// -> `client.register` 或 `client.register_once`（由 `retried` 选）-> 取首个 runtime -> `set_runtime_id` 缓存。
/// 返回 runtime_id 供调用方日志；失败返回错误，调用方按场景定日志级别
/// （启动 warn / 自愈 warn-and-retry / 绑定 best-effort）。
///
/// `retried` 区分一次性 vs 循环驱动：启动 / connect / 绑定即时传 `true`（无上层循环兜底，走 client 内
/// `with_network_retry` 3 次）；常驻心跳自愈传 `false`（循环即重试，走单次 `register_once` + liveness
/// 短超时——避免弱网下单 tick 嵌套 3×30s 退避阻塞后续取消检测）。
pub(crate) async fn register_workspace(
    client: &MulticaClient,
    workspace_id: &str,
    provider: &str,
    daemon_id: &str,
    shared: &SharedMulticaState,
    retried: bool,
) -> Result<String, MulticaError> {
    let device_name = get_system_username();
    let cli_version = env!("CARGO_PKG_VERSION").to_string();
    let request = RegisterRequest {
        workspace_id: workspace_id.to_string(),
        daemon_id: daemon_id.to_string(),
        device_name,
        cli_version: cli_version.clone(),
        runtimes: vec![RuntimeSpec {
            // name = 客户端展示名（channel app_name，默认 "Gold Band"）；runtime_type = provider 路由键，二者分离。
            name: current_channel_config().app_name.to_string(),
            runtime_type: provider.to_string(),
            version: cli_version,
            status: "ready".to_string(),
        }],
    };
    let response = if retried {
        client.register(&request).await?
    } else {
        client.register_once(&request).await?
    };
    let row = response
        .runtimes
        .first()
        .ok_or_else(|| MulticaError::RegisterFailed("register returned no runtime".into()))?;
    let runtime_id = row.id.clone();
    if let Ok(mut guard) = shared.lock() {
        guard.set_runtime_id(workspace_id, &runtime_id);
    }
    Ok(runtime_id)
}

/// 连接建立后即时注册所有已绑定 workspace（`connect_multica` 触发）。
///
/// 用户刚连上就想看到 runtime 在线、任务可领取--不等 15s 心跳 tick 自愈（根因修复 Bug 1：旧实现
/// `connect_multica` 不注册，首连后要等启动 loop 或重启才注册，期间心跳空转）。best-effort：逐条尝试，
/// 失败仅 warn（自愈 tick 兜底重试）。复用 [`resolve_startup_params`] 取 client+workspaces+daemon_id。
pub(crate) async fn register_all_bound_workspaces<R: Runtime>(app: &AppHandle<R>) {
    let Some((client, workspaces, daemon_id)) = resolve_startup_params(app) else {
        return;
    };
    let Some(shared) = app.try_state::<SharedMulticaState>() else {
        return;
    };
    for workspace in &workspaces {
        match register_workspace(
            &client,
            &workspace.id,
            &workspace.provider,
            &daemon_id,
            shared.inner(),
            true,
        )
        .await
        {
            Ok(runtime_id) => tracing::info!(
                workspace = %workspace.id,
                runtime_id = %runtime_id,
                "multica connect-triggered register ok"
            ),
            Err(error) => tracing::warn!(
                workspace = %workspace.id,
                %error,
                "multica connect-triggered register failed (self-heal will retry)"
            ),
        }
    }
}

/// 启动 multica 运行期循环（main.rs setup，`start_heartbeat_polling` 之后挂载）。
///
/// PAT 无效 / 未配置 / 无 workspace -> 静默跳过（首启绝不自动弹登录，等用户在远程任务列表
/// 点【连接 Multica】触发，开发设计 4.1）。
pub fn start_multica_loop<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        run_startup_registration(&app).await;
        run_heartbeat_loop(app).await;
    });
}

/// 启动一次性注册：verify_pat -> 全量 register -> recover-orphans（开发设计 4.1 / 4.2）。
async fn run_startup_registration<R: Runtime>(app: &AppHandle<R>) {
    let Some((client, workspaces, daemon_id)) = resolve_startup_params(app) else {
        return;
    };

    // 4.1: verify_pat -> 200 才全量 register；否则静默（首启不弹登录）。
    if let Err(error) = client.verify_pat().await {
        tracing::info!(
            %error,
            "multica pat invalid or unreachable, skip startup register"
        );
        return;
    }

    let Some(shared) = app.try_state::<SharedMulticaState>() else {
        return; // 无共享状态：无法缓存 runtime_id，启动注册无意义。
    };

    for workspace in &workspaces {
        match register_workspace(
            &client,
            &workspace.id,
            &workspace.provider,
            &daemon_id,
            shared.inner(),
            true,
        )
        .await
        {
            Ok(runtime_id) => tracing::info!(
                workspace = %workspace.id,
                runtime_id = %runtime_id,
                "multica register ok"
            ),
            Err(error) => tracing::warn!(
                workspace = %workspace.id,
                %error,
                "multica register failed"
            ),
        }
    }

    // recover-orphans：对每个已注册 runtime 清残留任务（失败不阻断启动）。
    let runtime_ids = shared
        .lock()
        .map(|guard| guard.runtime_ids())
        .unwrap_or_default();
    for runtime_id in runtime_ids {
        if let Err(error) = client.recover_orphans(&runtime_id).await {
            tracing::warn!(
                runtime_id = %runtime_id,
                %error,
                "multica recover-orphans failed (non-fatal)"
            );
        }
    }

    // 启动 reconcile（开发设计 4.4）：崩溃残留的 task_conversations 条目，remote cancelled/404 ->
    // 作废本地 Paused run。**不在 failed 上作废**--retryable 失败会被 server 重派，由 re-claim 续跑
    // （断点续跑）；作废 failed 会丢失续跑索引。terminal-failed（agent_error）的本地 Paused run 由
    // strict_continue fallback 兜底（用户续跑死 session -> 自动降级 fresh）。
    reconcile_startup_orphans(app, &client).await;
}

/// 常驻心跳循环（15s）：自愈注册 + 心跳 + 取消检测。
///
/// 与 multica 建立连接后即持续维持在线（不再仅任务执行期间）。每 tick 顺序：
/// 1. 自愈注册：已连接但 runtime_ids 缺失的已绑定 workspace -> 重注册（根因修复 Bug 1）；
/// 2. 心跳：遍历 `(workspace_id, runtime_id)`，runtime 行失效 404 -> 清缓存触发自愈重注册；
/// 3. 取消检测：在飞 active_run 的 remote 若已 terminal（failed/cancelled/404）-> 作废本地 run（C5）。
async fn run_heartbeat_loop<R: Runtime>(app: AppHandle<R>) {
    loop {
        // tick 耗时埋点：量化退化网络下单 tick 耗时，观测是否威胁常驻心跳的 runtime 在线判定（S2 验证依据）。
        let tick_start = Instant::now();
        if let Some(client) = build_client(&app) {
            // ① 自愈注册：已连接但 runtime_ids 缺失的已绑定 workspace -> 重注册（根因修复 Bug 1）。
            //    旧实现心跳源 runtime_ids 缺失时永久空转 -> runtime 离线 -> 任务被 server 回收回 pending。
            let stage = Instant::now();
            self_heal_registration(&app, &client).await;
            trace_stage("self_heal", stage);

            // ② 心跳：遍历 (workspace_id, runtime_id)。runtime 行失效 404 -> 清缓存，下 tick 自愈重注册。
            let stage = Instant::now();
            let pairs = collect_runtime_id_pairs(&app);
            let shared = app.try_state::<SharedMulticaState>();
            for (workspace_id, runtime_id) in &pairs {
                if let Err(error) = client.heartbeat(runtime_id).await {
                    if matches!(error, MulticaError::TaskNotFound) {
                        // runtime 行已被 server 删除/失效：旧 runtime_id 永久 404，清缓存触发自愈重注册。
                        tracing::warn!(
                            workspace_id = %workspace_id,
                            runtime_id = %runtime_id,
                            "multica runtime gone (404), clearing for re-register"
                        );
                        if let Some(shared) = shared.as_ref() {
                            if let Ok(mut guard) = shared.lock() {
                                guard.clear_runtime_id(workspace_id);
                            }
                        }
                    } else {
                        tracing::warn!(
                            runtime_id = %runtime_id,
                            %error,
                            "multica heartbeat failed (will retry next tick)"
                        );
                    }
                }
            }
            trace_stage("heartbeat", stage);

            // ③ 取消检测：active run 的 remote terminal -> 作废本地（接入方案 C5）。
            let stage = Instant::now();
            detect_cancelled_active_runs(&app, &client).await;
            trace_stage("cancel_detect", stage);
        }
        // tick 总耗时：正常 <1s；超 30s 视为 overrun（warn），便于弱网回归观测 S2 改善（单 tick 不再阻塞数分钟）。
        let tick_elapsed = tick_start.elapsed();
        if tick_elapsed >= Duration::from_secs(30) {
            tracing::warn!(
                elapsed_ms = tick_elapsed.as_millis() as u64,
                "multica heartbeat tick overrun (degraded network? stages blocking)"
            );
        } else {
            tracing::debug!(
                elapsed_ms = tick_elapsed.as_millis() as u64,
                "multica heartbeat tick done"
            );
        }
        tokio::time::sleep(Duration::from_secs(MULTICA_HEARTBEAT_INTERVAL_SECS)).await;
    }
}

/// 单阶段耗时埋点（trace 级，低噪声）：观测心跳 tick 内哪一阶段在退化网络下变慢。
fn trace_stage(stage: &'static str, start: Instant) {
    tracing::trace!(
        stage,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "multica heartbeat tick stage"
    );
}

/// 三层 guard + 取启动注册参数（未启用 / 无 PAT / 无 workspace -> None，静默跳过）。
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

/// 自愈注册（常驻心跳 tick 内）：已连接但 runtime_ids 缺失的已绑定 workspace -> 重注册。
///
/// 根因修复 Bug 1：旧实现注册是 one-shot（启动 + 绑定），`connect_multica` 不注册，心跳循环不重注册。
/// runtime_ids 缺失/失效的任意场景（启动时 server 不可达 / PAT 暂态校验失败 / runtime 行被 server
/// 清理后心跳 404 已清缓存）-> 心跳永久空转 -> runtime 离线 -> 任务被 server 回收回 pending。
/// 每 tick 对缺失 workspace 试注册一次，失败下 tick 重试（开发设计 4.1 自愈）。
async fn self_heal_registration<R: Runtime>(app: &AppHandle<R>, client: &MulticaClient) {
    let Some((workspaces, daemon_id, shared)) = resolve_self_heal_inputs(app) else {
        return;
    };
    for workspace in &workspaces {
        // 仅注册缺失 runtime_id 的 workspace（已注册的不重复 register，避免无谓 HTTP）。
        let already = shared
            .lock()
            .map(|g| g.runtime_id(&workspace.id).is_some())
            .unwrap_or(false);
        if already {
            continue;
        }
        match register_workspace(
            client,
            &workspace.id,
            &workspace.provider,
            &daemon_id,
            &shared,
            false,
        )
        .await
        {
            Ok(runtime_id) => tracing::info!(
                workspace = %workspace.id,
                runtime_id = %runtime_id,
                "multica self-heal register ok"
            ),
            Err(error) => tracing::warn!(
                workspace = %workspace.id,
                %error,
                "multica self-heal register failed (will retry next tick)"
            ),
        }
    }
}

/// 自愈注册输入：已启用 + 有 PAT（`connected`）+ 有 daemon_id + 有绑定 workspace + SharedMulticaState。
///
/// 不复用 [`resolve_startup_params`]（其返回 client，自愈已有 client）。仅校验 connected：无 PAT 则
/// register 必失败，跳过避免无谓 HTTP（用户未连接时心跳 tick 整体由 `build_client` 返回 None 拦截）。
fn resolve_self_heal_inputs<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<(Vec<MulticaWorkspaceRef>, String, SharedMulticaState)> {
    let desktop = app.try_state::<DesktopState>()?;
    let context = desktop.context().ok()?;
    let settings = multica_settings(&context.config);
    if !settings.enabled || !settings.connected {
        return None;
    }
    let daemon_id = get_daemon_id(&context.config)?;
    let shared = app.try_state::<SharedMulticaState>()?;
    if context.config.desktop_multica_workspaces.is_empty() {
        return None;
    }
    Some((
        context.config.desktop_multica_workspaces.clone(),
        daemon_id,
        shared.inner().clone(),
    ))
}

/// 已注册 `(workspace_id, runtime_id)` 对（常驻心跳遍历用）。
///
/// 由 `build_client` 已确认 enabled+PAT，这里仅读共享状态。返回 workspace_id 以便心跳 404 时按
/// workspace 清缓存触发自愈重注册（[`MulticaRuntimeState::clear_runtime_id`]）。
fn collect_runtime_id_pairs<R: Runtime>(app: &AppHandle<R>) -> Vec<(String, String)> {
    app.try_state::<SharedMulticaState>()
        .and_then(|shared| shared.lock().ok().map(|g| g.runtime_id_pairs()))
        .unwrap_or_default()
}

// ── 取消检测 / 启动 reconcile（开发设计 4.4 / 接入方案 C5）──────────────────────

/// 在飞 active_run 的 remote 是否为终态（作废本地）：failed/cancelled。
fn is_active_terminal(status: &str) -> bool {
    matches!(status, "failed" | "cancelled")
}

/// 崩溃残留 orphan 的 remote 是否为终态（作废本地）：仅 cancelled。
///
/// 不含 failed--retryable failed 会被 server 重派由 re-claim 续跑（断点续跑），作废会丢索引。
fn is_orphan_terminal(status: &str) -> bool {
    matches!(status, "cancelled")
}

/// 取消检测（接入方案 C5）：遍历在飞 active_run，remote failed/cancelled/404 -> 作废本地 run。
///
/// active run 场景：本地正在执行，remote 已 terminal -> 停本地（省算力、同步状态）。无 resume
/// 冲突--resume 是崩溃后 Paused run 的 re-claim 路径，与在飞 active run 互斥。
async fn detect_cancelled_active_runs<R: Runtime>(app: &AppHandle<R>, client: &MulticaClient) {
    for remote in active_run_remote_ids(app) {
        let invalidate = match client.get_task_status(&remote).await {
            Ok(status) => is_active_terminal(&status),
            Err(MulticaError::TaskNotFound) => true, // 404：task 已删
            Err(_) => false,                         // 暂态网络/解码 -> 下 tick 重试
        };
        if invalidate {
            spawn_invalidate(app, &remote).await;
            // 本地 run 已作废（remote terminal / 404）-> 通知前端刷新 sidebar。
            emit_multica_task_updated(app);
        }
    }
}

/// 启动 reconcile：崩溃残留的 task_conversations 条目，remote cancelled/404 -> 作废本地 Paused run。
async fn reconcile_startup_orphans<R: Runtime>(app: &AppHandle<R>, client: &MulticaClient) {
    for remote in orphan_remote_ids(app) {
        let invalidate = match client.get_task_status(&remote).await {
            Ok(status) => is_orphan_terminal(&status),
            Err(MulticaError::TaskNotFound) => true,
            Err(_) => false,
        };
        if invalidate {
            spawn_invalidate(app, &remote).await;
            // 本地 run 已作废（remote terminal / 404）-> 通知前端刷新 sidebar。
            emit_multica_task_updated(app);
        }
    }
}

/// 在飞 active_run 的 remote_task_id 集合（取消检测遍历用）。
fn active_run_remote_ids<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    app.try_state::<SharedMulticaState>()
        .and_then(|shared| {
            shared
                .lock()
                .ok()
                .map(|g| g.active_runs.keys().cloned().collect())
        })
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
    let _ =
        tauri::async_runtime::spawn_blocking(move || invalidate_remote_task(&app, &remote)).await;
}

/// 作废 remote task 对应本地 run（取消检测 / 启动 reconcile 共用）。
///
/// 解析 `(workspace_path, local_task_id, local_run_id)`：active_run 优先（在飞场景，按任务级
/// `local_project_id` 经 `workspace_entry_for_project` 解析路径——绑定模型已下沉到任务级）；
/// 回退 `task_conversations[remote]`（崩溃 orphan 场景，active_runs 已丢，用持久化 work_dir + local ids）。
/// 再交 `bridge::teardown_active_run` 收尾。
fn invalidate_remote_task<R: Runtime>(app: &AppHandle<R>, remote: &str) {
    let Some(desktop) = app.try_state::<DesktopState>() else {
        return;
    };
    let Ok(context) = desktop.context() else {
        return;
    };
    let Some(shared) = app.try_state::<SharedMulticaState>() else {
        return;
    };
    let home_app = context.app();

    // active_run 优先（在飞）-> 回退 task_conversations（崩溃 orphan）。
    let active = shared.lock().ok().and_then(|g| g.active_run(remote));
    let Some((workspace_path, local_task_id, local_run_id)) = active
        .and_then(|run| {
            let home_state = home_app.load_state().ok()?;
            let (wp, _) = workspace_entry_for_project(&home_state, &run.local_project_id)?;
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
    let workspace_app =
        home_app.with_repo_root(Utf8PathBuf::from(workspace_path), context.config.clone());
    teardown_active_run(
        &workspace_app,
        &shared,
        &home_app,
        remote,
        &local_task_id,
        &local_run_id,
    );
}

#[cfg(test)]
mod tests {
    use super::{is_active_terminal, is_orphan_terminal};

    #[test]
    fn active_terminal_flags_failed_and_cancelled() {
        // 在飞 active run：remote failed/cancelled -> 作废本地（接入方案 C5）。
        assert!(is_active_terminal("failed"));
        assert!(is_active_terminal("cancelled"));
        // 非终态/已成功 -> 不作废。
        assert!(!is_active_terminal("running"));
        assert!(!is_active_terminal("queued"));
        assert!(!is_active_terminal("dispatched"));
        assert!(!is_active_terminal("completed"));
    }

    #[test]
    fn orphan_terminal_flags_only_cancelled_not_failed() {
        // 崩溃残留 orphan：仅 cancelled 作废。failed 不作废--retryable 由 re-claim 续跑（断点续跑）。
        assert!(is_orphan_terminal("cancelled"));
        assert!(!is_orphan_terminal("failed"));
        assert!(!is_orphan_terminal("running"));
        assert!(!is_orphan_terminal("queued"));
    }
}
