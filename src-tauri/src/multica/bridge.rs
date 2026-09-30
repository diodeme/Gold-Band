//! multica lifecycle 桥接（开发设计 2.5 / 4.3）。
//!
//! 订阅 `RuntimeLifecycleBus`，把本地会话 lifecycle 事件转译为 multica 终态上报：
//! - `NodeCompleted`：读 `worker-ref.json` 采 ACP session_id → `pin_task_session` + 落
//!   `remote_task_conversations`（断点续跑依据）；session 变更才写（避免每节点重复 pin）。
//! - `RunCompleted`：按 `RunOutcome` 穷举上报（Success→complete（complete 送达后再用 PAT 把关联
//!   issue 流转到 done，接入方案 D2）+ completed 历史记 `completed` / Failure→fail + 历史记 `failed`
//!   / Killed→fail(timeout)，agent 真死；cancel 路径皆经 run_pause→Paused 从不产生 Killed，故无需
//!   cancel-detection 上下文消歧）。**M5-bp（开发设计 §12.55）：direct 模式 + issue 关联的 Success
//!   改为 completion-output 块门控**——最终回复提不出块 → 扣留完成（不上报终态、保留
//!   active_runs/task_conversations，远端保持 running 供对话继续）；非 direct / 非 issue 维持原
//!   fail-open 路径。在飞映射落空但 Success 的 run 是**终态后追问 run**：按
//!   local_task_id 反查 completed 历史，补交了 `completion-output` 时用不带 status 键的 PATCH
//!   只补写输出（`relay_late_completion_output`，issue 完成输出传递的多 run 场景）。
//! - `AcpTurnFinished`：Direct 后续追问在同一 attempt 内收尾、**不产生新的 RunCompleted**——
//!   M5-bp 起该事件兼作门控的 turn 级触发：命中在飞 direct+issue 任务且最终回复带块 → 完成路径
//!   （complete + issue done + finalize）；在飞但不属门控（workflow/auto/非 issue）→ 交 run 收尾
//!   路径负责；无在飞映射（任务已终态）→ `relay_late_completion_output` 迟到补发（同一补写通道，
//!   PUT 幂等覆盖；Failed/Cancelled 无交付语义不上报）。
//! - `RunPaused`/`InterventionRequested`：**绝对不上报终态**（multica 继续 running，本地处理
//!   elicitation/permission，开发设计 2.5 Paused 盲区）。
//!
//! 归属：本地 lifecycle 事件只带 display task_id/run_id（无 repo_root），靠 `active_runs`
//! 反查 (local_task_id, local_run_id) → remote_task_id（多 workspace/多 run 不串台）。
//! HTTP 调用经 `tauri::async_runtime::spawn` 异步执行（订阅器回调在 runtime 热路径，不可阻塞）。

use std::sync::Arc;

use gold_band::app::{AcpTurnOutcome, App, RuntimeLifecycleEvent};
use gold_band::config::{RemoteCompletedTask, RemoteTaskConversation, StateConfig};
use gold_band::domain::{PauseReason, RunOutcome};
use gold_band::runtime::WorkerRefState;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tracing::{info, warn};

use crate::multica::client::{MULTICA_ISSUE_DONE_STATUS, MulticaClient};
use crate::multica::config::{get_pat, multica_base_url, multica_settings};
use crate::multica::handoff;
use crate::multica::state::{ActiveRemoteRun, SharedMulticaState};
use crate::remote::{REMOTE_SOURCE_SETTINGS_UPDATED_EVENT, REMOTE_TASKS_UPDATED_EVENT};
use crate::state::{DesktopContext, DesktopState};

/// 通知前端远程任务状态已变更（事件常量定义在通用层 `remote` 模块，前端 sidebar 监听 →
/// re-fetch `get_remote_tasks` 刷新，开发设计 M5-b）。
///
/// 语义 = **任务生命周期**（claim/start/complete/fail/cancel、取消检测作废）。由 bridge 终态上报与
/// loop 取消检测 emit。连接态/工作空间绑定变更走 [`emit_remote_source_settings_updated`]。
///
/// 载荷为空——前端按「全量 re-fetch sidebar」处理（照搬 `emit_agent_registry_updated` 的 unit 载荷模式，
/// 避免在后端组装易腐化的部分 VM；前端单一数据源 `get_remote_tasks`）。
pub(crate) fn emit_remote_tasks_updated<R: Runtime>(app_handle: &AppHandle<R>) {
    let _ = app_handle.emit(REMOTE_TASKS_UPDATED_EVENT, ());
}

/// 通知前端远程来源设置/连接态已变更（任务列表 + 设置页 re-fetch；连接/断开/保存配置/工作空间绑定 CRUD）。
///
/// 语义 = **配置层变更**（非任务生命周期）。connect/disconnect/save/workspace CRUD 统一 emit；
/// 任务列表（`connected` 与已绑定工作空间均受影响）与设置页都订阅 → 任一处改动两端同步 re-fetch，
/// 杜绝「绑定发生在任务列表弹窗、设置页显示旧数据」之类的跨视图不一致。
pub(crate) fn emit_remote_source_settings_updated<R: Runtime>(app_handle: &AppHandle<R>) {
    let _ = app_handle.emit(REMOTE_SOURCE_SETTINGS_UPDATED_EVENT, ());
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
///
/// ACP 取消按 `(local_task_id, local_run_id)` **定点**（run 级）：单任务收尾不得波及同工作区
/// 其他会话/任务的活动 ACP 会话（workspace 级 `cancel_all_active_acp_attempts_best_effort`
/// 仅用于应用关闭/全局恢复，不可复用到 task 级生命周期）。
pub(crate) fn teardown_active_run(
    workspace_app: &App,
    shared: &SharedMulticaState,
    home_app: &App,
    remote_task_id: &str,
    local_task_id: &str,
    local_run_id: &str,
) {
    let _ = workspace_app.run_pause(local_task_id, local_run_id, PauseReason::ProcessInterrupted);
    workspace_app.cancel_active_acp_attempts_for_run_best_effort(local_task_id, local_run_id);
    if let Ok(mut guard) = shared.lock() {
        guard.drop_active_run(remote_task_id);
    }
    // 清断点续跑索引经 with_state 原子 RMW（防并发终态/取消收尾 lost-update）；dirty 由键是否存在决定。
    let _ = home_app.with_state(|state| {
        let dirty = state
            .remote_task_conversations
            .as_mut()
            .map(|convs| convs.remove(remote_task_id).is_some())
            .unwrap_or(false);
        (dirty, ())
    });
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

// ── M5-bp：direct 模式完成门控（开发设计 §12.55）──────────────────────────────

/// M5-bp 门控判定（纯函数）：在飞任务是否属 direct 模式 + issue 关联。
///
/// 两个维度都取自 `ActiveRemoteRun` 快照（lifecycle 事件不携带）：`run_mode`（发送时
/// `ConversationCreateInputVm.run_mode`）与 `issue_id`（claim 响应）。issue 空/纯空白视同非 issue
/// （与非 issue 来源任务的过滤惯例一致——协议块也不注入它们，无门控依据）。
fn is_gated_direct_issue_run(run: &ActiveRemoteRun) -> bool {
    run.run_mode == crate::multica::state::REMOTE_RUN_MODE_DIRECT
        && run
            .issue_id
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
}

/// M5-bp 决策点 1（run 收尾）的门控决策（纯函数）。
///
/// `block` = 从最终 assistant 回复提取的 `completion-output` 围栏块（提取动作含磁盘读，
/// 由调用方完成后传入——本函数只做无 IO 的分支决策，便于单测穷举）。
enum DirectCompletionDecision {
    /// 门控命中 + 块在 → complete（output = 块内容，即交付摘要；node_label 无下游语义）。
    Complete(String),
    /// 门控命中 + 无块 → 扣留完成：不上报终态、保留 active_runs/task_conversations，
    /// 远端保持 running（用户可继续对话推进，块出现后由 turn 收尾或下次 run 收尾完成）。
    Withhold,
    /// 非门控（非 direct / 非 issue）→ 既有 fail-open 路径（run 终态驱动完成，块尽力附带不门控）。
    Passthrough,
}

fn decide_run_completion(run: &ActiveRemoteRun, block: Option<String>) -> DirectCompletionDecision {
    if !is_gated_direct_issue_run(run) {
        return DirectCompletionDecision::Passthrough;
    }
    match block {
        Some(block) => DirectCompletionDecision::Complete(block),
        None => DirectCompletionDecision::Withhold,
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
                attempt_dir,
                ..
            } => {
                let Some((remote_task_id, run)) = lookup_active_run(&app_handle, &task_id, &run_id)
                else {
                    // 无在飞映射的本地 run：绝大多数是普通本地任务；唯一需要处理的例外是
                    // 「multica 任务终态后的追问 run」——首 run fail-open 标 done 时可能没
                    // 提出交付说明，用户追问后 agent 补交。反查 completed 历史含 state.json
                    // 磁盘读，故整体移入 spawn 异步执行（订阅器回调在热路径，不可阻塞）；
                    // Failure/Killed 无补发语义，不进 spawn。
                    if outcome == RunOutcome::Success {
                        let app_handle = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            relay_late_completion_output(
                                app_handle,
                                &task_id,
                                attempt_dir.as_deref(),
                            )
                            .await;
                        });
                    }
                    return;
                };
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    handle_run_completed(
                        app_handle,
                        remote_task_id,
                        run,
                        outcome,
                        node_label,
                        attempt_dir,
                    )
                    .await;
                });
            }
            RuntimeLifecycleEvent::AcpTurnFinished {
                task_id,
                run_id,
                outcome,
                batch_progress,
                attempt_dir,
                ..
            } => {
                // Failed/Cancelled 无交付语义。
                if outcome != AcpTurnOutcome::Completed {
                    return;
                }
                // Direct 排队的多条追问逐条触发 turn 事件，中间态 turn（batch_continues=true，
                // 后面还有排队 prompt）不是「最终回复」——此时提取的块可能是中间产物，跳过
                // 门控也跳过补发（终态 turn / run 收尾会再判）。
                if batch_progress.continues {
                    return;
                }
                // M5-bp 决策点 2：命中在飞 direct+issue 任务（含被扣留完成的）→ turn 收尾门控。
                // Direct 后续追问在同一 attempt 内收尾、不产生新的 RunCompleted，被扣留的任务
                // 只能靠本事件补判完成。
                if let Some((remote_task_id, run)) =
                    lookup_active_run(&app_handle, &task_id, &run_id)
                {
                    let app_handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        handle_turn_completion_gate(app_handle, remote_task_id, run, attempt_dir)
                            .await;
                    });
                    return;
                }
                // 无在飞映射（任务已终态 / 普通本地会话）：M5-bl/bn 迟到补发——按 completed
                // 历史反查，在飞任务天然不在历史里。事件逐消息触发，反查含磁盘读故移入 spawn。
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    relay_late_completion_output(app_handle, &task_id, attempt_dir.as_deref())
                        .await;
                });
            }
            // RunPaused / InterventionRequested / NodeStarted：不上报终态。
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

/// 原子认领完成路径（M5-bp 双事件竞态守卫，语义见 `MulticaRuntimeState::begin_completion`）。
///
/// 状态不可达（managed state 缺失/锁中毒）时放行（fail-open：守卫缺失退回旧行为，不让
/// 竞态守卫反过来卡死完成）。
fn begin_completion(app: &AppHandle, remote_task_id: &str) -> bool {
    shared_multica_state(app)
        .and_then(|shared| {
            shared
                .lock()
                .ok()
                .map(|mut guard| guard.begin_completion(remote_task_id))
        })
        .unwrap_or(true)
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
    // session 变更才落库 + pin（避免每节点重复 pin 同一 session）。RMW 经 with_state 原子化，
    // 防并发 NodeCompleted/RunCompleted 收尾 lost-update 覆盖此条 task_conversations。
    let changed = match context.app().with_state(|state| {
        let prev = state
            .remote_task_conversations
            .as_ref()
            .and_then(|m| m.get(&remote_task_id))
            .and_then(|c| c.session_id.as_deref());
        if prev == Some(session_id.as_str()) {
            return (false, false); // session 未变 → 无需写/pin。
        }
        let mut conversations = state.remote_task_conversations.take().unwrap_or_default();
        conversations.insert(
            remote_task_id.clone(),
            RemoteTaskConversation {
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
        state.remote_task_conversations = Some(conversations);
        (true, true)
    }) {
        Ok(c) => c,
        Err(e) => {
            warn!(%e, "multica pin: state rmw failed");
            return;
        }
    };
    if !changed {
        return; // session 未变 → 无需 pin。
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
    emit_remote_tasks_updated(&app);
}

/// RunCompleted：按 outcome 4 分支上报终态 + 清本地索引。
///
/// Success 分支在 `complete_task` 送达后，额外用码灵 PAT 把关联 issue 流转到 `done`（接入方案 D2：
/// 码灵作为中介），并在同一 PUT 内附带 `completion_output`——从 `attempt_dir` 的 acp.timeline 投影
/// 最终 assistant 回复、提取 `completion-output` 围栏块（issue 完成输出传递特性）。
///
/// **M5-bp（开发设计 §12.55）**：direct 模式 + issue 关联的 Success 由该块**门控**——无块 →
/// 扣留完成（不上报、不 finalize，远端保持 running 供对话继续，块出现后由 turn 收尾补判）。
/// 非 direct / 非 issue 维持既有 fail-open 路径（提取失败不带该键，issue 照常 done——写作可选、
/// 不门控）。
async fn handle_run_completed(
    app: AppHandle,
    remote_task_id: String,
    run: ActiveRemoteRun,
    outcome: RunOutcome,
    node_label: String,
    attempt_dir: Option<String>,
) {
    let Some(context) = desktop_context(&app) else {
        return;
    };
    let (session_id, work_dir) = current_session(context.app(), &remote_task_id);
    let action = classify_terminal(
        outcome,
        &node_label,
        session_id.as_deref(),
        work_dir.as_deref(),
    );
    let Some(client) = multica_client(&context) else {
        return;
    };
    let pending = match action {
        TerminalAction::Complete {
            output,
            session_id,
            work_dir,
        } => {
            // 块提取只在 issue 关联存在时进行（门控判定 ⊆ issue 非空；fail-open 附带同样需要
            // issue）——非 issue 任务不做这次大文件（acp.timeline）读，行为对齐旧实现。
            let probe = if run
                .issue_id
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                completion_output_for_issue_done(attempt_dir.as_deref())
            } else {
                handoff::CompletionBlockProbe::default()
            };
            // M5-bp 决策点 1（run 收尾）：direct + issue → completion-output 块门控。
            match decide_run_completion(&run, probe.block.clone()) {
                DirectCompletionDecision::Withhold => {
                    // 扣留完成：不上报终态、不 finalize（保留 active_runs / task_conversations），
                    // 远端保持 running——用户继续对话推进任务，块出现后由 turn 收尾（决策点 2）
                    // 补判完成。心跳在续，server sweeper 不会误杀 running。
                    log_completion_gate_miss(&remote_task_id, "run finished", &probe);
                    return;
                }
                DirectCompletionDecision::Complete(block) => {
                    // 门控命中 + 块在：完成路径（output = 块内容即交付摘要，node_label 无下游语义）。
                    // 同一终 turn 会并发触发 turn 收尾（AcpTurnFinished），认领守卫保证完成路径
                    // 单飞；认领失败 = turn 收尾先到 → 本 handler 整体退出（finalize 由认领方负责）。
                    if !begin_completion(&app, &remote_task_id) {
                        return;
                    }
                    complete_with_issue_done(
                        &client,
                        &remote_task_id,
                        &run,
                        &block,
                        Some(block.as_str()),
                        session_id.as_deref(),
                        work_dir.as_deref(),
                    )
                    .await;
                }
                DirectCompletionDecision::Passthrough => {
                    // 非 direct / 非 issue：既有 fail-open 路径——run 终态驱动完成，块尽力附带、
                    // 不门控。有 attempt（可读到执行体终态回复）却提不出块 → warn，用于内网联调
                    // 观察指令层遵从度。
                    if probe.block.is_none() && attempt_dir.is_some() {
                        if let Some(issue) =
                            run.issue_id.as_deref().filter(|s| !s.trim().is_empty())
                        {
                            warn!(
                                task = %remote_task_id,
                                %issue,
                                "multica completion-output: nothing extracted from final reply (issue still marked done)"
                            );
                        }
                    }
                    complete_with_issue_done(
                        &client,
                        &remote_task_id,
                        &run,
                        &output,
                        probe.block.as_deref(),
                        session_id.as_deref(),
                        work_dir.as_deref(),
                    )
                    .await;
                }
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
    emit_remote_tasks_updated(&app);
}

/// `complete_task` 送达 + 关联 issue 流转 done 的公共完成路径（M5-bp 起三处共用：run 收尾门控
/// 命中 / run 收尾 fail-open / turn 收尾门控命中——同一完成语义不得有多份实现）。
///
/// `completion_output` 随 issue done 一次 PUT 原子上送（None = 无块可附，fail-open：issue 照常
/// done）。`complete_task` 失败仅 warn 且**不流转 issue**（server 未收到 complete，issue 不得变
/// done）；issue done 失败仅 warn——任务终态已上报，issue 状态推进不阻断完成（issue 保持原状，
/// server 扫描器/用户兜底）；issue 关联缺失（issue_id 为空，如非 issue 来源任务）则跳过流转。
async fn complete_with_issue_done(
    client: &MulticaClient,
    remote_task_id: &str,
    run: &ActiveRemoteRun,
    output: &str,
    completion_output: Option<&str>,
    session_id: Option<&str>,
    work_dir: Option<&str>,
) {
    if let Err(e) = client
        .complete_task(remote_task_id, output, session_id, work_dir)
        .await
    {
        warn!(task = %remote_task_id, %e, "multica complete_task failed");
        return;
    }
    if let Some(issue) = run.issue_id.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Err(e) = client
            .update_issue_status(
                &run.workspace_id,
                issue,
                MULTICA_ISSUE_DONE_STATUS,
                completion_output,
            )
            .await
        {
            warn!(
                task = %remote_task_id,
                issue = %issue,
                %e,
                "multica update_issue_status(done) failed (task already completed; ignored)"
            );
        }
    }
}

/// M5-bp 决策点 2（turn 收尾）：被扣留/在飞的 direct+issue 任务带块 turn → 完成路径（开发设计
/// §12.55）。
///
/// 前置（订阅器已过滤）：turn `Completed`、非 batch 中间态、命中在飞映射。此处再判门控维度
/// （direct + issue）：不属门控（workflow / auto / 非 issue——run 收尾路径负责其终态）或无块
/// （对话继续，扣留状态不变）→ no-op；有块 → 与决策点 1 同一完成路径（complete + issue done +
/// finalize + 通知刷新）。
///
/// 与 run 收尾的并发（同 turn 双事件，几乎同时到达）：两个 handler 的归属查找都发生在各自
/// 订阅器回调、先于任一 finalize——**没有天然的先后互斥**（初版「先到者 finalize 后到者落空」
/// 的论证不成立）。由完成路径认领（`begin_completion`）保证单飞：先认领者执行 complete+
/// finalize，后到者整体退出；认领随 finalize 终结（`end_completion`），complete 失败也不泄漏。
async fn handle_turn_completion_gate(
    app: AppHandle,
    remote_task_id: String,
    run: ActiveRemoteRun,
    attempt_dir: Option<String>,
) {
    if !is_gated_direct_issue_run(&run) {
        return;
    }
    let probe = completion_output_for_issue_done(attempt_dir.as_deref());
    let Some(block) = probe.block else {
        log_completion_gate_miss(&remote_task_id, "turn finished", &probe);
        return;
    };
    // 双事件竞态守卫：run 收尾（RunCompleted）可能已认领同一任务的完成路径——认领失败
    // = 对方在完成中，本 handler 整体退出（其 finalize 会清 active_runs 并通知刷新）。
    // 认领放在 context/client 取得**之后**：断连等早退路径不认领（认领后无 finalize 释放，
    // 会阻塞该任务后续事件的完成路径直到重启）。
    let Some(context) = desktop_context(&app) else {
        return;
    };
    let Some(client) = multica_client(&context) else {
        return;
    };
    if !begin_completion(&app, &remote_task_id) {
        return;
    }
    let (session_id, work_dir) = current_session(context.app(), &remote_task_id);
    complete_with_issue_done(
        &client,
        &remote_task_id,
        &run,
        &block,
        Some(block.as_str()),
        session_id.as_deref(),
        work_dir.as_deref(),
    )
    .await;
    finalize_terminal(&app, &remote_task_id, &run, PendingUpdate::ClearOnSuccess);
    emit_remote_tasks_updated(&app);
}

/// 终态后追问的迟到 `completion-output` 补发（issue 完成输出传递特性，多 run/多 turn 场景）。
///
/// 背景：首 run Success 时 fail-open——issue 已 done 但交付说明未写入（agent 没按协议产出
/// 围栏块）；用户在本地会话继续追问、agent 补交了块。触发源有两个（同一补写通道，PUT 幂等
/// 覆盖）：终态后追问 **run**（`RunCompleted` 反查不到 active run）与 Direct 后续追问
/// **turn**（`AcpTurnFinished`——追问在同一 attempt 内收尾、不产生新的 RunCompleted）。
/// 均按 local_task_id 从「最近完成」历史找回 issue 关联，用**不带 status 键**的 PATCH 只补写
/// `completion_output`（服务端字段级 merge：issue 状态可能已被 reopen/人工流转，迟到写入
/// 不得拉回 done）。
///
/// - 未连接 / 历史/issue 关联缺失 → 静默跳过：普通本地会话的常态出口（`AcpTurnFinished`
///   逐消息触发，不刷日志）。
/// - 命中 multica 终态任务但提不出块 → info 日志：这正是「补发触发过但无内容可送」的
///   诊断分界（M5-bl 排查时四个静默 return 让触发缺失不可见）。
/// - PATCH 失败 → warn：迟到的自愈写入，不重试不阻断（下次补交整体覆盖，服务端
///   「done 后补写自愈」语义）。
/// - 本地状态零改动（completed 历史与 issue 状态均不动）。
///
/// 过滤顺序：内存检查（连接态）→ state.json（小文件，completed 历史反查）→ attempt
/// timeline（大文件，提块）——turn 级触发逐消息发生，先用小文件过滤非 multica 任务，
/// 避免每条追问消息都读大 timeline。
async fn relay_late_completion_output(
    app: AppHandle,
    local_task_id: &str,
    attempt_dir: Option<&str>,
) {
    // 未连接时无处可发（multica_client 是纯内存检查，先挡掉最便宜）。
    let Some(context) = desktop_context(&app) else {
        return;
    };
    let Some(client) = multica_client(&context) else {
        return;
    };
    // 先按 completed 历史过滤（state.json 小文件）：非 multica 终态任务（普通本地会话的
    // 常态）直接跳过，不为它们做大体积 timeline 读取。
    let Ok(state) = context.app().load_state() else {
        info!(task = %local_task_id, "multica late relay skip: state read failed");
        return;
    };
    let Some((workspace_id, issue, remote_task_id)) =
        find_completed_issue_task(&state, local_task_id)
    else {
        return;
    };
    // 命中 multica 终态任务后再提块（attempt timeline 磁盘读）。
    let probe = completion_output_for_issue_done(attempt_dir);
    let Some(output) = probe.block else {
        match probe.near_miss_reason {
            Some(reason) => warn!(
                task = %remote_task_id,
                reason,
                "multica late relay skip: near-miss completion-output fence rejected (info string must sit on the ``` opening line)"
            ),
            None => info!(
                task = %remote_task_id,
                "multica late relay skip: no completion-output block in final reply"
            ),
        }
        return;
    };
    if let Err(e) = client
        .update_issue_completion_output(&workspace_id, &issue, &output)
        .await
    {
        warn!(
            task = %remote_task_id,
            issue = %issue,
            %e,
            "multica late completion-output relay failed (ignored; next relay overwrites)"
        );
        return;
    }
    info!(
        task = %remote_task_id,
        issue = %issue,
        chars = output.chars().count(),
        "multica late completion-output relayed"
    );
}

/// 终态本地收尾：移 active_runs + 清 task_conversations + 记 completed 历史快照（status 由 pending 决定）。
fn finalize_terminal(
    app: &AppHandle,
    remote_task_id: &str,
    run: &ActiveRemoteRun,
    pending: PendingUpdate,
) {
    if let Some(shared) = shared_multica_state(app) {
        if let Ok(mut g) = shared.lock() {
            g.drop_active_run(remote_task_id);
            // 完成路径认领随 finalize 终结（M5-bp：认领生命周期 = [begin, finalize]，
            // 成功/失败皆然——complete 失败也走本地终态，认领不得泄漏阻塞后续事件）。
            g.end_completion(remote_task_id);
        }
    }
    let Some(context) = desktop_context(app) else {
        return;
    };
    let status = match pending {
        PendingUpdate::ClearOnSuccess => "completed",
        PendingUpdate::AddOnFailure => "failed",
    };
    // 终态本地收尾经 with_state 原子 RMW（清 task_conversations + 记 completed 历史）；
    // 防并发终态/取消收尾 lost-update（同一 remote 的 NodeCompleted 与 RunCompleted 并发 save 互不覆盖）。
    if let Err(e) = context.app().with_state(|state| {
        // 清断点续跑索引（任务已终态，不再续跑此 remote task）。
        if let Some(convs) = state.remote_task_conversations.as_mut() {
            convs.remove(remote_task_id);
        }
        // 「最近完成」历史快照（Issue 3C）：active→completed，保留 remote↔local 链接供远程 tab 回看。
        // task_conversations 此处已清（续跑语义不变），但 completed 历史独立常驻，供用户回看本地会话。
        record_completed_task(
            state,
            completed_task_from_run(remote_task_id, run, status, chrono::Utc::now().to_rfc3339()),
        );
        (true, ())
    }) {
        warn!(%e, "multica finalize: state rmw failed");
    }
}

/// 由在飞 run 快照构造终态历史条目（纯函数：不碰 AppHandle/StateConfig，便于单测固化字段来源）。
///
/// `kind`（中立词汇，翻译自 wire `issue_kind`）必须**取自 `run`**（claim 响应落盘的类型快照）——
/// multica C1 验收：终态行的类型徽标不丢。编译器只强制该字段被赋值、不强制取值来源，
/// 故用单测锁住「来源是 run 而非空值」。
/// `title` 为空/纯空白时回退为 remote_task_id（行标签缺失的兜底，与 claim 时的 thread_name 语义一致）。
fn completed_task_from_run(
    remote_task_id: &str,
    run: &ActiveRemoteRun,
    status: &str,
    completed_at: String,
) -> RemoteCompletedTask {
    RemoteCompletedTask {
        remote_task_id: remote_task_id.to_string(),
        local_task_id: run.local_task_id.clone(),
        local_run_id: run.local_run_id.clone(),
        workspace_id: run.workspace_id.clone(),
        local_project_id: run.local_project_id.clone(),
        issue_ref: run.issue_id.clone(),
        // 类型快照自 ActiveRemoteRun（claim 响应落盘的值）→ 终态行类型徽标不丢（multica C1）。
        kind: run.issue_kind.clone(),
        status: status.to_string(),
        title: run
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| remote_task_id.to_string()),
        completed_at,
    }
}

/// 「最近完成」历史容量上限（最新在前，超出截断）。
const MAX_MULTICA_COMPLETED_HISTORY: usize = 50;

/// 把终态任务快照写入「最近完成」历史（去重 by remote_task_id，最新在前，截断至上限）。
///
/// 每次终态 finalize 调用一次；同 remote_task_id 重复终态（理论上不应发生）时覆盖为最新而非重复堆积。
fn record_completed_task(state: &mut StateConfig, entry: RemoteCompletedTask) {
    let list = &mut state.remote_completed_tasks;
    list.retain(|c| c.remote_task_id != entry.remote_task_id);
    list.insert(0, entry);
    if list.len() > MAX_MULTICA_COMPLETED_HISTORY {
        list.truncate(MAX_MULTICA_COMPLETED_HISTORY);
    }
}

/// 从「最近完成」历史删除指定 remote_task_id 的条目（`record_completed_task` 的逆操作）。
///
/// 终态行的唯一数据源是本地历史（`get_remote_tasks` 三源合并，服务端不回传终态），故终态行的
/// 「移出列表」必须是真删除而非视图过滤——否则任何一次刷新都会把行「复活」。返回是否删到
/// （false = 本就不在历史，幂等 no-op，调用方据此不落盘）。
pub(crate) fn remove_completed_task_entry(state: &mut StateConfig, remote_task_id: &str) -> bool {
    let before = state.remote_completed_tasks.len();
    state
        .remote_completed_tasks
        .retain(|c| c.remote_task_id != remote_task_id);
    before != state.remote_completed_tasks.len()
}

/// 终态对「最近完成」历史快照 status 的处置（success→completed / failure→failed）。
enum PendingUpdate {
    /// Success：completed 历史记 `completed`。
    ClearOnSuccess,
    /// Failure：completed 历史记 `failed`。
    AddOnFailure,
}

/// Success 分支 issue done 流转附带的交付说明提取（issue 完成输出传递特性，fail-open）。
///
/// attempt 缺失（非 ACP 完成路径）/ timeline 不可读 → 空 probe；最终回复无
/// `completion-output` 围栏块 → `block` None 且按近似形态探测给出 `near_miss_reason`
/// （仅诊断，门控只看 `block`）。读取与提取均为纯本地操作（一次 timeline 读，探测复用
/// 已读回复文本，不产生额外 I/O）。
fn completion_output_for_issue_done(attempt_dir: Option<&str>) -> handoff::CompletionBlockProbe {
    attempt_dir
        .and_then(handoff::final_assistant_reply)
        .map(|reply| handoff::completion_block_probe(&reply))
        .unwrap_or_default()
}

/// 完成门控未命中日志（开发设计 §12.55 根因修复）：回复里存在近似 completion-output 围栏
/// （info 串未与开栏同行等形态滑落）时升级 WARN 并带机器可检索原因——与「完全未输出块」的
/// 正常扣留区分，内网日志一眼定位格式滑落；无近似形态时维持 info 级（对话继续属预期行为）。
fn log_completion_gate_miss(
    task: &str,
    phase: &'static str,
    probe: &handoff::CompletionBlockProbe,
) {
    match probe.near_miss_reason {
        Some(reason) => warn!(
            task,
            phase,
            reason,
            "multica completion gate: near-miss completion-output fence rejected — info string must sit on the ``` opening line (task stays running)"
        ),
        None => info!(
            task,
            phase,
            "multica completion gate: {} without completion-output block (task stays running)",
            phase
        ),
    }
}

/// 按 local_task_id 从「最近完成」历史反查迟到补发目标 `(workspace_id, issue_id, remote_task_id)`
/// （纯函数）。命中条件：status == `completed`（与 [`PendingUpdate::ClearOnSuccess`] 写入的字面量
/// 一致——failed 任务无 done 流转、无输出可补）且 issue_ref 非空（无 issue 来源任务没有补写
/// 对象）。多条命中取最新（历史最新在前）。
fn find_completed_issue_task(
    state: &StateConfig,
    local_task_id: &str,
) -> Option<(String, String, String)> {
    state
        .remote_completed_tasks
        .iter()
        .find(|c| {
            c.local_task_id == local_task_id
                && c.status == "completed"
                && c.issue_ref.as_deref().is_some_and(|s| !s.trim().is_empty())
        })
        .map(|c| {
            (
                c.workspace_id.clone(),
                c.issue_ref.clone().unwrap_or_default(),
                c.remote_task_id.clone(),
            )
        })
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
        .remote_task_conversations
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

    fn completed(remote: &str, completed_at: &str) -> RemoteCompletedTask {
        RemoteCompletedTask {
            remote_task_id: remote.into(),
            local_task_id: format!("task-{remote}"),
            local_run_id: format!("run-{remote}"),
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            issue_ref: None,
            kind: Some("dev".into()),
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
            .remote_completed_tasks
            .iter()
            .map(|c| c.remote_task_id.as_str())
            .collect();
        assert_eq!(ids, vec!["rt-2", "rt-1"]);

        // 同 remote_task_id 重复终态 → 覆盖为最新（去重，不堆积）。
        record_completed_task(&mut state, completed("rt-1", "2026-08-06T03:00:00Z"));
        let ids: Vec<&str> = state
            .remote_completed_tasks
            .iter()
            .map(|c| c.remote_task_id.as_str())
            .collect();
        assert_eq!(ids, vec!["rt-1", "rt-2"], "rt-1 应前移并去重");
        assert_eq!(
            state.remote_completed_tasks[0].completed_at,
            "2026-08-06T03:00:00Z"
        );
    }

    fn active_run(title: Option<&str>, issue_kind: Option<&str>) -> ActiveRemoteRun {
        ActiveRemoteRun {
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            local_task_id: "local-task-1".into(),
            local_run_id: "local-run-1".into(),
            issue_id: Some("issue-1".into()),
            title: title.map(str::to_string),
            started_at: "2026-09-10T00:00:00Z".into(),
            issue_kind: issue_kind.map(str::to_string),
            run_mode: "direct".into(),
        }
    }

    #[test]
    fn remove_completed_task_entry_deletes_by_remote_id_and_is_noop_when_absent() {
        // 终态行的唯一数据源是本地 remote_completed_tasks 历史（get_remote_tasks 三源合并，
        // 服务端不回传终态）——「移出列表」若只做视图过滤，任何一次刷新都会把行「复活」。
        // 此处锁住真删除语义：按 remote_task_id 删条目；不存在时 no-op（幂等，不标脏）。
        let mut state = StateConfig::default();
        state.remote_completed_tasks = vec![
            completed("rt-1", "2026-09-20T01:00:00Z"),
            completed("rt-2", "2026-09-20T02:00:00Z"),
        ];

        assert!(remove_completed_task_entry(&mut state, "rt-1"));
        let ids: Vec<&str> = state
            .remote_completed_tasks
            .iter()
            .map(|c| c.remote_task_id.as_str())
            .collect();
        assert_eq!(ids, vec!["rt-2"], "只删目标条目，其余历史保序保留");

        assert!(
            !remove_completed_task_entry(&mut state, "rt-missing"),
            "不存在 → 未删任何条目"
        );
        assert_eq!(state.remote_completed_tasks.len(), 1);
    }

    // ===== story dev/test 拆分：C1「终态历史保留类型快照」的接口层固化 =====

    #[test]
    fn completed_task_from_run_keeps_claim_kind_snapshot() {
        // 终态行的类型徽标唯一来源是 claim 落盘的 run.issue_kind（multica C1）。
        // 该字段若被改成 None / 常量，看板终态行会静默失去类型徽标——此处锁住「来源 = run」。
        for kind in ["dev", "test", "bug", "general"] {
            let entry = completed_task_from_run(
                "rt-1",
                &active_run(Some("标题"), Some(kind)),
                "completed",
                "2026-09-10T01:00:00Z".into(),
            );
            assert_eq!(entry.kind.as_deref(), Some(kind), "类型快照必须透传");
            assert_eq!(entry.status, "completed");
            assert_eq!(entry.title, "标题");
        }

        // 旧历史 / 无 issue 任务（claim 响应无 issue_kind）→ None：徽标不渲染，不臆造类型。
        let legacy = completed_task_from_run(
            "rt-2",
            &active_run(Some("标题"), None),
            "failed",
            "2026-09-10T01:00:00Z".into(),
        );
        assert!(legacy.kind.is_none());
        assert_eq!(legacy.status, "failed");

        // 行标签缺失/纯空白 → 回退 remote_task_id（终态行仍有可读标签）。
        let untitled = completed_task_from_run(
            "rt-3",
            &active_run(Some("   "), Some("dev")),
            "completed",
            "2026-09-10T01:00:00Z".into(),
        );
        assert_eq!(untitled.title, "rt-3");
        let no_title = completed_task_from_run(
            "rt-4",
            &active_run(None, None),
            "completed",
            "2026-09-10T01:00:00Z".into(),
        );
        assert_eq!(no_title.title, "rt-4");
    }

    #[test]
    fn record_completed_task_caps_history_at_limit() {
        // 超 MAX 的旧条目被截断（最新 MAX 条保留，仍按时间倒序）。
        let mut state = StateConfig::default();
        for i in 0..(MAX_MULTICA_COMPLETED_HISTORY + 5) {
            record_completed_task(
                &mut state,
                completed(&format!("rt-{i}"), "2026-08-06T00:00:00Z"),
            );
        }
        assert_eq!(
            state.remote_completed_tasks.len(),
            MAX_MULTICA_COMPLETED_HISTORY
        );
        // 最新写入（rt-(MAX+4)）在最前，最老被截断。
        assert_eq!(
            state.remote_completed_tasks[0].remote_task_id,
            format!("rt-{}", MAX_MULTICA_COMPLETED_HISTORY + 4)
        );
    }

    // ===== issue 完成输出传递：Success 分支 completion_output 提取组合的接口层固化 =====

    #[test]
    fn completion_output_for_issue_done_extracts_fenced_block_from_timeline() {
        // attempt_dir 指向含 completion-output 围栏块的最终 assistant 回复 → Some（随 done 一次 PUT 上送）。
        let (attempt_dir, _temp) =
            timeline_attempt_dir("工作完成。\n```completion-output\n交付说明\n```\n");
        assert_eq!(
            completion_output_for_issue_done(Some(attempt_dir.as_str())).block,
            Some("交付说明".to_string())
        );
    }

    #[test]
    fn completion_output_for_issue_done_without_block_or_dir_is_none() {
        // agent 未按协议产出块 / 非 ACP 完成路径（attempt_dir None）→ None：issue 照常 done（fail-open）。
        let (attempt_dir, _temp) = timeline_attempt_dir("工作完成，无交付说明块。");
        assert_eq!(
            completion_output_for_issue_done(Some(attempt_dir.as_str())).block,
            None
        );
        assert_eq!(completion_output_for_issue_done(None).block, None);
    }

    #[test]
    fn completion_output_probe_flags_near_miss_fence_from_timeline() {
        // §12.55 内网实测形态：裸围栏 + info 串另起一行 → 块缺失（门控扣留、任务保持进行中）
        // 但近似原因可检索——门控日志升级 WARN，内网不再需要导出 timeline 取证定位格式问题。
        let (attempt_dir, _temp) = timeline_attempt_dir(
            "```\ncompletion-output\n任务性质：测试性工作项，无实质代码变更。\n当前分支：sit\n```",
        );
        let probe = completion_output_for_issue_done(Some(attempt_dir.as_str()));
        assert_eq!(probe.block, None);
        assert_eq!(
            probe.near_miss_reason,
            Some(handoff::NEAR_MISS_INFO_ON_OWN_LINE)
        );
    }

    // ===== 终态后追问 run 的迟到 completion-output 补发（多 run 场景，纯函数固化）=====

    /// 直造一条 completed 历史条目（local_task_id / status / issue_ref 可指定——
    /// 迟到补发反查的三个命中维度）。
    fn history_entry(
        local_task_id: &str,
        status: &str,
        issue_ref: Option<&str>,
    ) -> RemoteCompletedTask {
        RemoteCompletedTask {
            remote_task_id: format!("rt-{local_task_id}"),
            local_task_id: local_task_id.into(),
            local_run_id: format!("run-{local_task_id}"),
            workspace_id: "ws-1".into(),
            local_project_id: "proj-1".into(),
            issue_ref: issue_ref.map(str::to_string),
            kind: Some("dev".into()),
            status: status.into(),
            title: format!("title-{local_task_id}"),
            completed_at: "2026-09-20T00:00:00Z".into(),
        }
    }

    #[test]
    fn find_completed_issue_task_returns_relay_target_for_completed_issue_entry() {
        // 首 run fail-open 标 done 后，追问 run 的 RunCompleted 已无在飞映射 → 按
        // local_task_id 反查 completed 历史，命中 completed + issue 关联条目 → 补发目标三元组。
        let mut state = StateConfig::default();
        state
            .remote_completed_tasks
            .push(history_entry("lt-1", "completed", Some("iss-1")));
        assert_eq!(
            find_completed_issue_task(&state, "lt-1"),
            Some(("ws-1".into(), "iss-1".into(), "rt-lt-1".into()))
        );
    }

    #[test]
    fn find_completed_issue_task_skips_failed_blank_issue_and_foreign_local_tasks() {
        // failed 任务无 done 流转、issue_ref 空/空白无补写对象、local_task_id 不同是别的任务
        // ——三者都必须落空，否则会把迟到输出补发到错误对象（含普通本地 run 误发）。
        for (status, issue) in [
            ("failed", Some("iss-1")),
            ("completed", None),
            ("completed", Some("   ")),
        ] {
            let mut state = StateConfig::default();
            state
                .remote_completed_tasks
                .push(history_entry("lt-1", status, issue));
            assert_eq!(
                find_completed_issue_task(&state, "lt-1"),
                None,
                "status={status} issue={issue:?} 不命中"
            );
        }
        // local_task_id 不同（普通本地 run / 其他任务）→ 落空。
        let mut state = StateConfig::default();
        state
            .remote_completed_tasks
            .push(history_entry("lt-other", "completed", Some("iss-1")));
        assert_eq!(find_completed_issue_task(&state, "lt-1"), None);
    }

    #[test]
    fn find_completed_issue_task_prefers_newest_match() {
        // 同一 local_task_id 对过多条远程任务时取最新（历史最新在前；旧条目的输出已被后续
        // 覆盖，补发目标应与「最近完成」分区展示一致）。
        let mut state = StateConfig::default();
        let mut older = history_entry("lt-1", "completed", Some("iss-old"));
        older.remote_task_id = "rt-old".into();
        let mut newer = history_entry("lt-1", "completed", Some("iss-new"));
        newer.remote_task_id = "rt-new".into();
        state.remote_completed_tasks = vec![newer, older];
        assert_eq!(
            find_completed_issue_task(&state, "lt-1"),
            Some(("ws-1".into(), "iss-new".into(), "rt-new".into()))
        );
    }

    // ===== M5-bp：direct 模式远程任务完成门控（开发设计 §12.55，纯函数固化）=====

    /// 造一个在飞 run（`active_run` 基础上覆写 run_mode / issue_id——门控判定的两个维度）。
    fn gated_run(run_mode: &str, issue_id: Option<&str>) -> ActiveRemoteRun {
        let mut run = active_run(Some("标题"), Some("dev"));
        run.run_mode = run_mode.to_string();
        run.issue_id = issue_id.map(str::to_string);
        run
    }

    #[test]
    fn is_gated_direct_issue_run_requires_direct_mode_and_issue_link() {
        // direct + issue 关联 → 门控命中（run 收尾无块扣留、turn 收尾有块完成）。
        assert!(is_gated_direct_issue_run(&gated_run(
            "direct",
            Some("iss-1")
        )));
        // workflow / auto → 非 direct：graph/计划终态即 definition of done，走既有 fail-open 路径。
        assert!(!is_gated_direct_issue_run(&gated_run(
            "workflow",
            Some("iss-1")
        )));
        assert!(!is_gated_direct_issue_run(&gated_run(
            "auto",
            Some("iss-1")
        )));
        // 非 issue 来源（server chat/autopilot/quick-create）→ 协议块不注入、无门控依据。
        assert!(!is_gated_direct_issue_run(&gated_run("direct", None)));
        assert!(!is_gated_direct_issue_run(&gated_run(
            "direct",
            Some("   ")
        )));
    }

    #[test]
    fn decide_run_completion_withholds_gated_run_without_block() {
        // 决策点 1 核心：direct + issue + 最终回复无 completion-output 块 → 扣留完成
        // （不上报终态、保留 active_runs/task_conversations，远端保持 running 供对话继续）。
        assert!(matches!(
            decide_run_completion(&gated_run("direct", Some("iss-1")), None),
            DirectCompletionDecision::Withhold
        ));
    }

    #[test]
    fn decide_run_completion_completes_gated_run_with_block() {
        // 有块 → complete，且 output 用块内容（交付摘要）而非 node_label（run 级标签无下游语义）。
        assert!(matches!(
            decide_run_completion(
                &gated_run("direct", Some("iss-1")),
                Some("交付说明".to_string())
            ),
            DirectCompletionDecision::Complete(ref output) if output == "交付说明"
        ));
    }

    #[test]
    fn decide_run_completion_passes_through_non_direct_and_non_issue() {
        // workflow / auto / 非 issue → 既有 fail-open 路径（run 终态驱动完成，块尽力附带不门控）。
        for run_mode in ["workflow", "auto"] {
            assert!(matches!(
                decide_run_completion(&gated_run(run_mode, Some("iss-1")), None),
                DirectCompletionDecision::Passthrough
            ));
        }
        assert!(matches!(
            decide_run_completion(&gated_run("direct", None), None),
            DirectCompletionDecision::Passthrough
        ));
    }

    /// 造一个 attempt 目录，timeline 内含一条最终 assistant textDelta（`reply`）。
    /// 返回 (attempt_dir, TempDir guard)——guard 由调用方持有，目录在测试结束才清理。
    fn timeline_attempt_dir(reply: &str) -> (camino::Utf8PathBuf, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let attempt_dir =
            camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 temp path");
        let paths = gold_band::acp::events::AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        gold_band::acp::events::write_timeline_items(
            &paths.timeline,
            &[gold_band::acp::events::AcpUiEvent {
                id: "assistant-final".into(),
                seq: 1,
                timestamp: "1Z".into(),
                kind: "textDelta".into(),
                session_id: None,
                content: Some(reply.to_string()),
                title: None,
                tool_call_id: None,
                status: None,
                started_seq: Some(1),
                ended_seq: Some(1),
                started_at: Some("1Z".into()),
                ended_at: Some("1Z".into()),
                timing: None,
                raw: None,
            }],
        )
        .expect("write timeline");
        (attempt_dir, temp)
    }
}
