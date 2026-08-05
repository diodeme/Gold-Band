//! multica lifecycle 桥接（开发设计 2.5 / 4.3）。
//!
//! 订阅 `RuntimeLifecycleBus`，把本地会话 lifecycle 事件转译为 multica 终态上报：
//! - `NodeCompleted`：读 `worker-ref.json` 采 ACP session_id → `pin_task_session` + 落
//!   `multica_task_conversations`（断点续跑依据）；session 变更才写（避免每节点重复 pin）。
//! - `RunCompleted`：按 `RunOutcome` 穷举上报（Success→complete / Failure→fail+记
//!   pending_issues / Killed→fail(timeout)，agent 真死；cancel 路径皆经 run_pause→Paused
//!   从不产生 Killed，故无需 cancel-detection 上下文消歧）。
//! - `RunPaused`/`InterventionRequested`：**绝对不上报终态**（multica 继续 running，本地处理
//!   elicitation/permission，开发设计 2.5 Paused 盲区）。
//!
//! 归属：本地 lifecycle 事件只带 display task_id/run_id（无 repo_root），靠 `active_runs`
//! 反查 (local_task_id, local_run_id) → remote_task_id（多 workspace/多 run 不串台）。
//! HTTP 调用经 `tauri::async_runtime::spawn` 异步执行（订阅器回调在 runtime 热路径，不可阻塞）。

// M5 前端接入前，订阅器虽已注册但无 multica 任务在飞（active_runs 由 M4-c start 填充）。
// M5 完成后审查移除该 allow。
#![allow(dead_code)]

use std::sync::Arc;

use gold_band::app::{App, RuntimeLifecycleEvent};
use gold_band::config::{MulticaCompletedTask, MulticaTaskConversation, StateConfig};
use gold_band::domain::{PauseReason, RunOutcome};
use gold_band::runtime::WorkerRefState;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tracing::warn;

use crate::multica::client::MulticaClient;
use crate::multica::config::{get_pat, multica_base_url, multica_settings};
use crate::multica::state::{ActiveRemoteRun, SharedMulticaState};
use crate::state::{DesktopContext, DesktopState};

/// multica 远程任务状态变更事件名（前端 sidebar 监听 → re-fetch `get_multica_tasks` 刷新，开发设计 M5-b）。
///
/// 语义 = **任务生命周期**（claim/start/complete/fail/cancel、取消检测作废）。由 bridge 终态上报与
/// loop 取消检测 emit。连接态/工作空间绑定变更走 [`MULTICA_SETTINGS_UPDATED_EVENT`]。
pub(crate) const MULTICA_TASK_UPDATED_EVENT: &str = "gold-band://multica-task-updated";

/// 通知前端 multica 远程任务状态已变更。
///
/// 载荷为空——前端按「全量 re-fetch sidebar」处理（照搬 `emit_agent_registry_updated` 的 unit 载荷模式，
/// 避免在后端组装易腐化的部分 VM；前端单一数据源 `get_multica_tasks`）。
pub(crate) fn emit_multica_task_updated<R: Runtime>(app_handle: &AppHandle<R>) {
    let _ = app_handle.emit(MULTICA_TASK_UPDATED_EVENT, ());
}

/// multica 设置/连接态变更事件名（连接/断开/保存配置/工作空间绑定 CRUD）。
///
/// 语义 = **配置层变更**（非任务生命周期）。connect/disconnect/save/workspace CRUD 统一 emit；
/// 任务列表（`connected` 与已绑定工作空间均受影响）与设置页都订阅 → 任一处改动两端同步 re-fetch，
/// 杜绝「绑定发生在任务列表弹窗、设置页显示旧数据」之类的跨视图不一致。
pub(crate) const MULTICA_SETTINGS_UPDATED_EVENT: &str = "gold-band://multica-settings-updated";

/// 通知前端 multica 设置/连接态已变更（任务列表 + 设置页 re-fetch）。
pub(crate) fn emit_multica_settings_updated<R: Runtime>(app_handle: &AppHandle<R>) {
    let _ = app_handle.emit(MULTICA_SETTINGS_UPDATED_EVENT, ());
}

// ── 纯函数（可单测）──────────────────────────────────────────────────────────

/// 终态动作（`RunOutcome` → 上报决策，开发设计 2.5 终态 4 分支表）。
enum TerminalAction {
    /// 成功 → `complete(output, session_id, work_dir)`。
    Complete {
        output: String,
        session_id: Option<String>,
        work_dir: Option<String>,
    },
    /// 失败 → `fail(error, failure_reason)`（reason=agent_error，resume-unsafe，用户 rerun）。
    Fail { error: String, reason: String },
}

/// 作废 remote task 对应的本地 run（取消检测 / 手动取消 / 启动 reconcile 共用，开发设计 4.4）。
///
/// `run_pause(ProcessInterrupted)` + 杀 ACP + 清 `active_runs` + 清 `task_conversations[remote]`。
/// 纯本地收尾，**不上报 multica 终态**（调用场景下 remote 已 terminal，或用户已主导取消）。
/// 取 `workspace_app`（run_pause/杀 ACP）与 `home_app`（task_conversations 落 home-repo StateConfig）
/// 两个不同 App 实例——索引与执行分属不同 repo root（开发设计 2.5）。
pub(crate) fn teardown_active_run(
    workspace_app: &App,
    shared: &SharedMulticaState,
    home_app: &App,
    remote_task_id: &str,
    local_task_id: &str,
    local_run_id: &str,
) {
    let _ = workspace_app.run_pause(local_task_id, local_run_id, PauseReason::ProcessInterrupted);
    workspace_app.cancel_all_active_acp_attempts_best_effort();
    if let Ok(mut guard) = shared.lock() {
        guard.drop_active_run(remote_task_id);
    }
    if let Ok(mut state) = home_app.load_state() {
        if let Some(convs) = state.multica_task_conversations.as_mut() {
            convs.remove(remote_task_id);
        }
        let _ = home_app.save_state(&state);
    }
}

/// 按 `RunOutcome` 分类终态动作（纯函数，开发设计 2.5 终态表）。
fn classify_terminal(
    outcome: RunOutcome,
    node_label: &str,
    session_id: Option<&str>,
    work_dir: Option<&str>,
) -> TerminalAction {
    match outcome {
        RunOutcome::Success => TerminalAction::Complete {
            output: node_label.to_string(),
            session_id: session_id.map(str::to_string),
            work_dir: work_dir.map(str::to_string),
        },
        RunOutcome::Failure => TerminalAction::Fail {
            error: node_label.to_string(),
            reason: "agent_error".to_string(),
        },
        RunOutcome::Killed => TerminalAction::Fail {
            error: node_label.to_string(),
            // Killed = agent 进程真死（cancel 路径皆经 run_pause→Paused，从不产生 Killed，
            // 故无需「cancel-detection 上下文」消歧）。timeout 为 resume-safe，server 可 auto-retry。
            reason: "timeout".to_string(),
        },
    }
}

/// 从 `WorkerRefState` 提取 ACP session_id + work_dir（断点续跑依据）。
///
/// `continue_ref.acpSessionId` 为 session_id，`continue_ref.cwd` 为 work_dir；缺失/空 → None。
fn extract_session(state: &WorkerRefState) -> Option<(String, String)> {
    let cont = state.continue_ref.as_ref()?;
    let session_id = cont.get("acpSessionId")?.as_str()?.to_string();
    if session_id.is_empty() {
        return None;
    }
    let work_dir = cont
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some((session_id, work_dir))
}

/// 从 `attempt_dir/worker-ref.json` 读 session_id + work_dir（磁盘薄封装）。
fn read_worker_ref_session(attempt_dir: &str) -> Option<(String, String)> {
    let path = std::path::Path::new(attempt_dir).join("worker-ref.json");
    let data = std::fs::read(&path).ok()?;
    let state: WorkerRefState = serde_json::from_slice(&data).ok()?;
    extract_session(&state)
}

// ── 订阅器 ────────────────────────────────────────────────────────────────────

/// 构造 multica lifecycle 订阅器（注册于 `register_lifecycle_subscribers`）。
///
/// `Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync>`，回调在 runtime 热路径——只做轻量
/// 归属查找，命中 multica 在飞任务后 `spawn` 异步 HTTP（pin/complete/fail）。
pub fn create_multica_subscriber(
    app_handle: AppHandle,
) -> Arc<dyn Fn(RuntimeLifecycleEvent) + Send + Sync> {
    Arc::new(move |event| {
        match event {
            RuntimeLifecycleEvent::NodeCompleted {
                task_id,
                run_id,
                attempt_dir,
                ..
            } => {
                let Some((remote_task_id, run)) = lookup_active_run(&app_handle, &task_id, &run_id)
                else {
                    return; // 非 multica 在飞任务（本地普通 run）→ 不处理。
                };
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    handle_node_completed(app_handle, remote_task_id, run, attempt_dir).await;
                });
            }
            RuntimeLifecycleEvent::RunCompleted {
                task_id,
                run_id,
                outcome,
                node_label,
                ..
            } => {
                let Some((remote_task_id, run)) = lookup_active_run(&app_handle, &task_id, &run_id)
                else {
                    return;
                };
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    handle_run_completed(app_handle, remote_task_id, run, outcome, node_label)
                        .await;
                });
            }
            // RunPaused / InterventionRequested / NodeStarted / AcpTurnFinished：不上报终态。
            _ => {}
        }
    })
}

/// 按 (local_task_id, local_run_id) 反查在飞 multica 任务（锁内 clone 后释放）。
fn lookup_active_run(
    app: &AppHandle,
    local_task_id: &str,
    local_run_id: &str,
) -> Option<(String, ActiveRemoteRun)> {
    let shared = shared_multica_state(app)?;
    let guard = shared.lock().ok()?;
    guard.find_active_run_by_local(local_task_id, local_run_id)
}

// ── 异步处理（spawn 内执行）────────────────────────────────────────────────────

/// NodeCompleted：采 session_id → 变更则 pin + 落 task_conversations。
async fn handle_node_completed(
    app: AppHandle,
    remote_task_id: String,
    run: ActiveRemoteRun,
    attempt_dir: String,
) {
    let Some((session_id, work_dir)) = read_worker_ref_session(&attempt_dir) else {
        return; // worker-ref 未就绪/非 ACP 节点 → 跳过。
    };
    let Some(context) = desktop_context(&app) else {
        return;
    };
    // session 变更才落库 + pin（避免每节点重复 pin 同一 session）。
    let mut state = match context.app().load_state() {
        Ok(s) => s,
        Err(e) => {
            warn!(%e, "multica pin: load_state failed");
            return;
        }
    };
    let prev = state
        .multica_task_conversations
        .as_ref()
        .and_then(|m| m.get(&remote_task_id))
        .and_then(|c| c.session_id.clone());
    if prev.as_deref() == Some(session_id.as_str()) {
        return; // session 未变 → 无需重复 pin。
    }
    let mut conversations = state.multica_task_conversations.clone().unwrap_or_default();
    conversations.insert(
        remote_task_id.clone(),
        MulticaTaskConversation {
            local_task_id: run.local_task_id,
            local_run_id: run.local_run_id,
            session_id: Some(session_id.clone()),
            work_dir: if work_dir.is_empty() {
                None
            } else {
                Some(work_dir.clone())
            },
        },
    );
    state.multica_task_conversations = Some(conversations);
    if let Err(e) = context.app().save_state(&state) {
        warn!(%e, "multica pin: save_state failed");
    }
    let Some(client) = multica_client(&context) else {
        return;
    };
    let work_dir_ref = if work_dir.is_empty() {
        None
    } else {
        Some(work_dir.as_str())
    };
    if let Err(e) = client
        .pin_task_session(&remote_task_id, &session_id, work_dir_ref)
        .await
    {
        warn!(task = %remote_task_id, %e, "multica pin_task_session failed (ignored; next node retries)");
    }
    // session 已落盘 + pin 上报 → 通知前端刷新（远程任务进入 running / 续跑上下文就绪）。
    emit_multica_task_updated(&app);
}

/// RunCompleted：按 outcome 4 分支上报终态 + 清本地索引。
async fn handle_run_completed(
    app: AppHandle,
    remote_task_id: String,
    run: ActiveRemoteRun,
    outcome: RunOutcome,
    node_label: String,
) {
    let Some(context) = desktop_context(&app) else {
        return;
    };
    let (session_id, work_dir) = current_session(context.app(), &remote_task_id);
    let action = classify_terminal(outcome, &node_label, session_id.as_deref(), work_dir.as_deref());
    let Some(client) = multica_client(&context) else {
        return;
    };
    let pending = match action {
        TerminalAction::Complete {
            output,
            session_id,
            work_dir,
        } => {
            if let Err(e) = client
                .complete_task(&remote_task_id, &output, session_id.as_deref(), work_dir.as_deref())
                .await
            {
                warn!(task = %remote_task_id, %e, "multica complete_task failed");
            }
            PendingUpdate::ClearOnSuccess
        }
        TerminalAction::Fail { error, reason } => {
            if let Err(e) = client.fail_task(&remote_task_id, &error, &reason).await {
                warn!(task = %remote_task_id, %e, "multica fail_task failed");
            }
            PendingUpdate::AddOnFailure
        }
    };
    finalize_terminal(&app, &remote_task_id, &run, pending);
    // 远程任务终态（complete/fail）已上报 + 本地索引已清 → 通知前端刷新 sidebar。
    emit_multica_task_updated(&app);
}

/// 终态本地收尾：移 active_runs + 清 task_conversations + 按 pending 调整失败回显。
fn finalize_terminal(
    app: &AppHandle,
    remote_task_id: &str,
    run: &ActiveRemoteRun,
    pending: PendingUpdate,
) {
    if let Some(shared) = shared_multica_state(app) {
        if let Ok(mut g) = shared.lock() {
            g.drop_active_run(remote_task_id);
        }
    }
    let Some(context) = desktop_context(app) else {
        return;
    };
    let mut state = match context.app().load_state() {
        Ok(s) => s,
        Err(e) => {
            warn!(%e, "multica finalize: load_state failed");
            return;
        }
    };
    // 清断点续跑索引（任务已终态，不再续跑此 remote task）。
    if let Some(convs) = state.multica_task_conversations.as_mut() {
        convs.remove(remote_task_id);
    }
    // 失败回显：Success 清除（重试成功）、Failure 记录（供 rerun）。
    if let Some(issue) = run.issue_id.as_deref() {
        let mut list = state.multica_pending_issues.clone().unwrap_or_default();
        match pending {
            PendingUpdate::ClearOnSuccess => list.retain(|i| i != issue),
            PendingUpdate::AddOnFailure => {
                if !list.iter().any(|i| i == issue) {
                    list.push(issue.to_string());
                }
            }
        }
        state.multica_pending_issues = Some(list);
    }
    // 「最近完成」历史快照（Issue 3C）：active→completed，保留 remote↔local 链接供远程 tab 回看。
    // task_conversations 此处已清（续跑语义不变），但 completed 历史独立常驻，供用户回看本地会话。
    let status = match pending {
        PendingUpdate::ClearOnSuccess => "completed",
        PendingUpdate::AddOnFailure => "failed",
    };
    record_completed_task(
        &mut state,
        MulticaCompletedTask {
            remote_task_id: remote_task_id.to_string(),
            local_task_id: run.local_task_id.clone(),
            local_run_id: run.local_run_id.clone(),
            workspace_id: run.workspace_id.clone(),
            issue_id: run.issue_id.clone(),
            status: status.to_string(),
            title: run
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| remote_task_id.to_string()),
            completed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    if let Err(e) = context.app().save_state(&state) {
        warn!(%e, "multica finalize: save_state failed");
    }
}

/// 「最近完成」历史容量上限（最新在前，超出截断）。
const MAX_MULTICA_COMPLETED_HISTORY: usize = 50;

/// 把终态任务快照写入「最近完成」历史（去重 by remote_task_id，最新在前，截断至上限）。
///
/// 每次终态 finalize 调用一次；同 remote_task_id 重复终态（理论上不应发生）时覆盖为最新而非重复堆积。
fn record_completed_task(state: &mut StateConfig, entry: MulticaCompletedTask) {
    let list = &mut state.multica_completed_tasks;
    list.retain(|c| c.remote_task_id != entry.remote_task_id);
    list.insert(0, entry);
    if list.len() > MAX_MULTICA_COMPLETED_HISTORY {
        list.truncate(MAX_MULTICA_COMPLETED_HISTORY);
    }
}

/// 终态对失败回显（pending_issues）的处置。
enum PendingUpdate {
    /// Success：清除该 issue 的失败回显（重试成功）。
    ClearOnSuccess,
    /// Failure：记录该 issue 供用户 rerun。
    AddOnFailure,
}

// ── 配置/状态访问 helper ────────────────────────────────────────────────────────

fn desktop_context(app: &AppHandle) -> Option<DesktopContext> {
    Some(app.try_state::<DesktopState>()?.context().ok()?)
}

fn shared_multica_state(app: &AppHandle) -> Option<SharedMulticaState> {
    Some(app.try_state::<SharedMulticaState>()?.inner().clone())
}

fn multica_client(context: &DesktopContext) -> Option<MulticaClient> {
    if !multica_settings(&context.config).connected {
        return None;
    }
    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    MulticaClient::new(base_url, Some(pat)).ok()
}

/// 取 task_conversations[remote] 的 (session_id, work_dir)（complete 上报用）。
fn current_session(app: App, remote_task_id: &str) -> (Option<String>, Option<String>) {
    let Ok(state) = app.load_state() else {
        return (None, None);
    };
    let Some(conv) = state
        .multica_task_conversations
        .as_ref()
        .and_then(|m| m.get(remote_task_id))
    else {
        return (None, None);
    };
    (conv.session_id.clone(), conv.work_dir.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::domain::SessionMode;
    use serde_json::json;

    fn worker_ref(session_id: Option<&str>, cwd: Option<&str>) -> WorkerRefState {
        let continue_ref = match (session_id, cwd) {
            (Some(s), Some(w)) => Some(json!({ "acpSessionId": s, "cwd": w })),
            (Some(s), None) => Some(json!({ "acpSessionId": s })),
            _ => None,
        };
        WorkerRefState {
            version: "1".into(),
            provider: "claude-acp".into(),
            mode: SessionMode::Continue,
            supports_open_session: true,
            supports_continue_session: true,
            continue_ref,
            open_command: None,
        }
    }

    #[test]
    fn extract_session_reads_acp_session_and_cwd() {
        let (sid, wd) = extract_session(&worker_ref(Some("sess-9"), Some("/repo"))).unwrap();
        assert_eq!(sid, "sess-9");
        assert_eq!(wd, "/repo");
    }

    #[test]
    fn extract_session_missing_cwd_yields_empty_workdir() {
        // cwd 缺失 → session 仍取到，work_dir 空（complete 上报时 work_dir=None）。
        let (sid, wd) = extract_session(&worker_ref(Some("sess-9"), None)).unwrap();
        assert_eq!(sid, "sess-9");
        assert_eq!(wd, "");
    }

    #[test]
    fn extract_session_none_when_session_missing_or_empty() {
        assert!(extract_session(&worker_ref(None, None)).is_none());
        assert!(extract_session(&worker_ref(Some(""), Some("/repo"))).is_none());
    }

    #[test]
    fn classify_terminal_success_emits_complete_with_session() {
        let action = classify_terminal(RunOutcome::Success, "执行", Some("sess-9"), Some("/repo"));
        match action {
            TerminalAction::Complete {
                output,
                session_id,
                work_dir,
            } => {
                assert_eq!(output, "执行");
                assert_eq!(session_id.as_deref(), Some("sess-9"));
                assert_eq!(work_dir.as_deref(), Some("/repo"));
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn classify_terminal_failure_emits_agent_error() {
        let action = classify_terminal(RunOutcome::Failure, "agent exit 1", None, None);
        match action {
            TerminalAction::Fail { error, reason } => {
                assert_eq!(error, "agent exit 1");
                assert_eq!(reason, "agent_error"); // resume-unsafe → 用户 rerun，不触发 auto-retry
            }
            _ => panic!("expected Fail"),
        }
    }

    #[test]
    fn classify_terminal_killed_fails_with_timeout_reason() {
        // Killed = agent 进程真死（cancel 路径皆经 run_pause→Paused 从不产生 Killed）→
        // fail(timeout)，resume-safe，server 可 auto-retry。
        assert!(matches!(
            classify_terminal(RunOutcome::Killed, "killed", None, None),
            TerminalAction::Fail { ref reason, .. } if reason == "timeout"
        ));
    }

    fn completed(remote: &str, completed_at: &str) -> MulticaCompletedTask {
        MulticaCompletedTask {
            remote_task_id: remote.into(),
            local_task_id: format!("task-{remote}"),
            local_run_id: format!("run-{remote}"),
            workspace_id: "ws-1".into(),
            issue_id: None,
            status: "completed".into(),
            title: format!("title-{remote}"),
            completed_at: completed_at.into(),
        }
    }

    #[test]
    fn record_completed_task_prepends_newest_and_dedups() {
        // 终态按发生顺序写入 → 最新在前（前端「最近完成」分区直接渲染顺序即时间倒序）。
        let mut state = StateConfig::default();
        record_completed_task(&mut state, completed("rt-1", "2026-08-06T01:00:00Z"));
        record_completed_task(&mut state, completed("rt-2", "2026-08-06T02:00:00Z"));
        let ids: Vec<&str> = state
            .multica_completed_tasks
            .iter()
            .map(|c| c.remote_task_id.as_str())
            .collect();
        assert_eq!(ids, vec!["rt-2", "rt-1"]);

        // 同 remote_task_id 重复终态 → 覆盖为最新（去重，不堆积）。
        record_completed_task(&mut state, completed("rt-1", "2026-08-06T03:00:00Z"));
        let ids: Vec<&str> = state
            .multica_completed_tasks
            .iter()
            .map(|c| c.remote_task_id.as_str())
            .collect();
        assert_eq!(ids, vec!["rt-1", "rt-2"], "rt-1 应前移并去重");
        assert_eq!(
            state.multica_completed_tasks[0].completed_at,
            "2026-08-06T03:00:00Z"
        );
    }

    #[test]
    fn record_completed_task_caps_history_at_limit() {
        // 超 MAX 的旧条目被截断（最新 MAX 条保留，仍按时间倒序）。
        let mut state = StateConfig::default();
        for i in 0..(MAX_MULTICA_COMPLETED_HISTORY + 5) {
            record_completed_task(&mut state, completed(&format!("rt-{i}"), "2026-08-06T00:00:00Z"));
        }
        assert_eq!(
            state.multica_completed_tasks.len(),
            MAX_MULTICA_COMPLETED_HISTORY
        );
        // 最新写入（rt-(MAX+4)）在最前，最老被截断。
        assert_eq!(
            state.multica_completed_tasks[0].remote_task_id,
            format!("rt-{}", MAX_MULTICA_COMPLETED_HISTORY + 4)
        );
    }
}
