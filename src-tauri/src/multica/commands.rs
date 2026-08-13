//! multica 远程任务命令（开发设计 2.4 / 第 6 章表）。
//!
//! - [`get_multica_tasks`]：按 workspace 分组的远程 pending 列表 + 本地终态历史（`multica_completed_tasks`）。
//! - [`get_multica_task_requirement`]：claim-at-send 只读取——拉任务详情 + 需求正文预填 composer、绑定 chip，
//!   不改 server 状态（任务仍 queued）。删除 chip 即解绑回普通会话。
//! - [`start_multica_conversation_run`]：发送预填好的远程任务——发送即事务边界：先 claim（pending→dispatched）
//!   再**复用**本地会话创建链路（`create_conversation_run_vm`：建工作流 + 建任务 + 写 conversation.json + 启动 run）
//!   + start_task（dispatched→running）；claim 后、running 前任意失败由 release 回滚（dispatched→queued）。

use std::collections::{BTreeMap, HashSet};

use camino::Utf8PathBuf;
use gold_band::app::{App, is_run_continuable};
use gold_band::config::{MulticaTaskConversation, MulticaWorkspaceRef};
use tauri::{AppHandle, State};
use tracing::{info, warn};

use crate::commands::{
    CommandErrorVm, CommandResult, acp_live_update_emitter_for_app, acp_session_update_emitter,
    command_error,
};
use crate::conversation_workspace::workspace_entry_for_project;
use crate::multica::client::{MULTICA_ISSUE_IN_PROGRESS_STATUS, MulticaClient, WorkspaceInfo};
use crate::multica::config::{
    MulticaSettingsVm, get_daemon_id, get_pat, multica_base_url, multica_settings,
};
use crate::multica::error::MulticaError;
use crate::multica::state::{ActiveRemoteRun, SharedMulticaState};
use crate::multica::vm::{RemoteConversationSidebarVm, RemoteTaskVm};
use crate::state::DesktopState;
use crate::view_models_conversation::{
    ConversationCreateInputVm, ConversationRunVm, conversation_run_vm, create_conversation_run_vm,
    validate_conversation_create_vm,
};

/// 远程任务列表（按 workspace 分组，对齐 `ConversationSidebarVm` 形状）。
///
/// 数据源：远程 queued（`list_pending_tasks`，逐已注册 workspace）+ 本地终态历史
/// （`multica_completed_tasks`，按 workspace 归组）。未连接（无 PAT）→ 空状态 sidebar
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
            last_active_workspace_id,
            connected: false,
        });
    }

    let base_url = multica_base_url(&context.config).unwrap_or_default();
    let pat = get_pat(&context.config).unwrap_or_default();
    let client = MulticaClient::new(base_url, Some(pat)).map_err(|e| command_error(e.into()))?;

    // 本地终态历史（multica_completed_tasks，最新在前）：同一次 load_state 读取。读失败不阻断列表（仅记日志返回空）。
    // 改动六：终态行不再进扁平全局「最近完成」桶，改为按 workspace_id 归入对应工作空间组。
    let completed_by_workspace = match context.app().load_state() {
        Ok(state_cfg) => {
            // 终态行按 workspace_id 分组。local_project_id 在 finalize 时从 ActiveRemoteRun 快照到
            // MulticaCompletedTask（绑定模型下沉到任务级），terminal 行据此做本地深链，无需再查工作区绑定。
            let mut by_ws: BTreeMap<String, Vec<RemoteTaskVm>> = BTreeMap::new();
            for c in state_cfg.multica_completed_tasks.iter() {
                by_ws
                    .entry(c.workspace_id.clone())
                    .or_default()
                    .push(RemoteTaskVm::from_completed(c, &c.local_project_id));
            }
            by_ws
        }
        Err(error) => {
            warn!(%error, "multica load_state for completed failed");
            BTreeMap::new()
        }
    };

    // 远程任务（running + pending + 终态）按 workspace 分组：每个 workspace 单次取锁取在飞 running 行
    // （active_runs，改动七）与 runtime_id，再按 runtime_id 拉 server pending，最后并入本地终态行
    // （按 remote_task_id 去重：running > pending > terminal，避免同一任务同时显多个状态）。
    let mut tasks_by_workspace: BTreeMap<String, Vec<RemoteTaskVm>> = BTreeMap::new();
    for workspace in &workspaces {
        // 单次取锁：在飞 running 行（active_runs，改动七：执行中任务不再从侧栏消失）+ workspace 的 runtime_id。
        let (running, runtime_id) = match shared.lock() {
            Ok(guard) => {
                let running: Vec<RemoteTaskVm> = guard
                    .active_runs
                    .iter()
                    .filter(|(_, r)| r.workspace_id == workspace.id)
                    .map(|(rid, r)| RemoteTaskVm::from_active_run(rid, r, &r.local_project_id))
                    .collect();
                let runtime_id = guard.runtime_id(&workspace.id).map(str::to_string);
                (running, runtime_id)
            }
            Err(_) => (Vec::new(), None),
        };
        // pending 行：server 可领取队列（逐已注册 workspace；未注册 workspace 无 pending，仍有 running/终态）。
        let pending: Vec<RemoteTaskVm> = match runtime_id {
            Some(runtime_id) => match client.list_pending_tasks(&runtime_id).await {
                Ok(tasks) => tasks
                    .iter()
                    .map(|task| RemoteTaskVm::from_pending(task, &workspace.id))
                    .collect(),
                Err(error) => {
                    warn!(
                        workspace = %workspace.id,
                        %error,
                        "multica list_pending failed (skipped workspace)"
                    );
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let terminal = completed_by_workspace.get(&workspace.id).map(Vec::as_slice).unwrap_or(&[]);
        tasks_by_workspace.insert(
            workspace.id.clone(),
            merge_workspace_tasks(running, pending, terminal),
        );
    }

    Ok(RemoteConversationSidebarVm {
        workspaces,
        tasks_by_workspace,
        last_active_workspace_id,
        connected: true,
    })
}

/// 合并某工作空间的 running / pending / 终态行（改动六 + 改动七，纯逻辑可单测）。
///
/// 顺序：running（在跑的最重要）→ pending（可领取）→ 终态历史（最新在前，由 `record_completed_task` 保证）。
/// 按 `id`（remote_task_id）去重，优先级 running > pending > terminal：重派/竞态场景下同一任务可能
/// 同时落在多个来源，running 是当前真相（在执行），优先保留，避免一行同时显多个状态。
fn merge_workspace_tasks(
    mut running: Vec<RemoteTaskVm>,
    pending: Vec<RemoteTaskVm>,
    terminal: &[RemoteTaskVm],
) -> Vec<RemoteTaskVm> {
    let mut seen: HashSet<String> = running.iter().map(|t| t.id.clone()).collect();
    for vm in pending {
        if seen.insert(vm.id.clone()) {
            running.push(vm);
        }
    }
    for vm in terminal {
        if seen.insert(vm.id.clone()) {
            running.push(vm.clone());
        }
    }
    running
}

/// 断点续跑判定结果（开发设计 改动三）。
#[derive(Debug)]
enum ResumeDecision {
    /// 续既有本地 run：冷重连 ACP session，接着被中断的编排往下跑（不新建 run、不新增一轮）。
    ///
    /// 只携带本地 task/run id——ACP session 由 run 自身状态恢复（`run_continue_background` →
    /// `session/load`），不经此字段。命中此分支的前提（checkpoint 解析出本地 ids、run 可续）
    /// 已在 [`classify_resume_from`] 内校验。
    Resume {
        local_task_id: String,
        local_run_id: String,
    },
    /// 新建本地 run（与本地工作空间「+」一致）。
    Fresh,
}

/// 纯判定（无 I/O，可单测）：给定断点 checkpoint 与本地 run 状态，决定续跑 vs 新建。
///
/// 判定规则（与 `is_run_continuable` 对齐）：
/// - 无 checkpoint / 本地 run 不存在 → [`ResumeDecision::Fresh`]。
/// - 本地 run 处于可续态（Paused + outcome None + pause_reason ∈ {ProcessInterrupted,
///   RuntimeAbnormal, WaitingForUserInput} + round/node/attempt 齐）→ [`ResumeDecision::Resume`]。
///   其中 ProcessInterrupted 是崩溃重启后启动自愈（`pause_all_running_sessions`）写入的态。
/// - 其余（Running / 已终态 / 缺 locator）→ [`ResumeDecision::Fresh`]。
///
/// **不以 checkpoint 的 `session_id` 作为续跑门**：ACP session 由 run 自身恢复——
/// [`App::run_continue_background`] 直接读 `worker-ref.json` 的 `continue_ref.acpSessionId`，
/// 不经 checkpoint 的 `session_id` 字段。checkpoint 的 `session_id` 由 bridge 在 `NodeCompleted`
/// 回填（仅供 server `pin_task_session`），但崩溃常发生在首个节点完成前、该字段尚未回填——若以其为
/// 续跑门，会把一个实际可续（locator 齐 → attempt 已起 → worker-ref 已写 session）的 run 误判新建。
/// 可续性以 run 的真实状态（`is_run_continuable`）为准；真无可续 session 时续跑执行会失败并落 Fresh 兜底。
fn classify_resume_from(
    conv: Option<&MulticaTaskConversation>,
    run: Option<&gold_band::runtime::RunState>,
) -> ResumeDecision {
    let Some(conv) = conv else {
        return ResumeDecision::Fresh;
    };
    let Some(run) = run else {
        return ResumeDecision::Fresh;
    };
    if is_run_continuable(run) {
        ResumeDecision::Resume {
            local_task_id: conv.local_task_id.clone(),
            local_run_id: conv.local_run_id.clone(),
        }
    } else {
        ResumeDecision::Fresh
    }
}

/// 纯逻辑（可单测）：从续跑索引按「字面 id → 父任务 id」两级解析出 checkpoint。
///
/// 镜像 [`classify_resume`] 的索引解析顺序：
/// 1. 先查 `remote_task_id`（同 id 场景：dispatched lease 过期后同 row 重派回本机）；
/// 2. miss 且有 `parent_task_id` → 查父任务（auto-retry 子任务场景：server 克隆新 id 子任务 T'，
///    父任务 T 的本地索引才是续跑指针——「崩溃/关闭重启后领取重试子任务」续跑的关键）。
fn resolve_resume_checkpoint(
    map: &std::collections::HashMap<String, MulticaTaskConversation>,
    remote_task_id: &str,
    parent_task_id: Option<&str>,
) -> Option<MulticaTaskConversation> {
    map.get(remote_task_id)
        .cloned()
        .or_else(|| parent_task_id.and_then(|pid| map.get(pid).cloned()))
}

/// 断点续跑判定（start 续跑分支用，决定续既有 run vs 新建）。
///
/// 读 home-repo `multica_task_conversations` → [`resolve_resume_checkpoint`] 两级解析出 checkpoint →
/// 由 `work_dir` 构造 workspace-bound App 查本地 run 状态 → 委托 [`classify_resume_from`]
/// （`is_run_continuable` 校验）。仅判定，不改状态：start 命中 Resume 才真正续跑。
fn classify_resume(
    home_app: &App,
    remote_task_id: &str,
    parent_task_id: Option<&str>,
) -> ResumeDecision {
    let map = home_app
        .load_state()
        .ok()
        .and_then(|state_cfg| state_cfg.multica_task_conversations);
    // checkpoint 解析路径（诊断用）：literal = 子 task id 直接命中；parent = 经 parent_task_id 反查命中。
    let (conv, resolved_via) = match map.as_ref() {
        Some(m) => {
            if m.get(remote_task_id).is_some() {
                (resolve_resume_checkpoint(m, remote_task_id, parent_task_id), "literal")
            } else if parent_task_id.is_some_and(|pid| m.get(pid).is_some()) {
                (resolve_resume_checkpoint(m, remote_task_id, parent_task_id), "parent")
            } else {
                (None, "none")
            }
        }
        None => (None, "no-map"),
    };
    let session_present = conv
        .as_ref()
        .and_then(|c| c.session_id.as_deref())
        .is_some_and(|s| !s.trim().is_empty());
    let run = conv.as_ref().and_then(|conv| {
        let work_dir = conv.work_dir.as_deref()?.trim();
        if work_dir.is_empty() {
            return None;
        }
        let workspace_app =
            home_app.with_repo_root(Utf8PathBuf::from(work_dir), home_app.config.clone());
        workspace_app
            .run_status(&conv.local_task_id, &conv.local_run_id)
            .ok()
    });
    let continuable = run.as_ref().is_some_and(is_run_continuable);
    let decision = classify_resume_from(conv.as_ref(), run.as_ref());
    info!(
        task = remote_task_id,
        parent = parent_task_id,
        resolved_via,
        session_present,
        run_status = ?run.as_ref().map(|r| r.status),
        continuable,
        decision = ?decision,
        "multica classify_resume decision"
    );
    decision
}

/// 纯逻辑（可单测）：把续跑索引从父任务迁移到子任务（断点续跑方案 §3.3，返回新 map 不原地改）。
///
/// 插入 `child_task_id` 条目（继承父条目的 `session_id`/`work_dir`，`local_task_id`/`local_run_id`
/// 用本次续跑的实际 run）；移除被取代的 `parent_task_id` 条目。父条目缺失时仍插入 child
/// （local ids 已知，session/work_dir 置 None 待 bridge 回填），保证多次重试 T→T'→T'' 链式可续。
fn migrate_resume_index_map(
    mut map: std::collections::HashMap<String, MulticaTaskConversation>,
    child_task_id: &str,
    parent_task_id: &str,
    local_task_id: &str,
    local_run_id: &str,
) -> std::collections::HashMap<String, MulticaTaskConversation> {
    let (session_id, work_dir) = map
        .get(parent_task_id)
        .map(|p| (p.session_id.clone(), p.work_dir.clone()))
        .unwrap_or((None, None));
    map.insert(
        child_task_id.into(),
        MulticaTaskConversation {
            local_task_id: local_task_id.into(),
            local_run_id: local_run_id.into(),
            session_id,
            work_dir,
        },
    );
    map.remove(parent_task_id);
    map
}

/// 续跑成功后把续跑索引从父任务迁到子任务（断点续跑方案 §3.3）。
///
/// auto-retry 子任务 T' 续的是父 T 的本地 run/session，索引原挂 T 名下；迁到 T' 后后续再次重试
/// （T'→T''）仍能链式反查。**best-effort**：续跑已成功（run 正在继续），迁移失败（盘 I/O）仅 `warn!`
/// 不阻断——退化为本轮可续、下次崩溃落 Fresh（与修复前等价，不劣化）。
fn migrate_resume_index(
    home_app: &App,
    child_task_id: &str,
    parent_task_id: &str,
    local_task_id: &str,
    local_run_id: &str,
) {
    // RMW 经 with_state 原子化：迁移 task_conversations 与终态/取消收尾并发 save 互不覆盖（lost-update）。
    if let Err(error) = home_app.with_state(|state| {
        let conversations = state.multica_task_conversations.take().unwrap_or_default();
        let migrated = migrate_resume_index_map(
            conversations,
            child_task_id,
            parent_task_id,
            local_task_id,
            local_run_id,
        );
        state.multica_task_conversations = Some(migrated);
        true
    }) {
        warn!(
            child = child_task_id,
            parent = parent_task_id,
            %error,
            "multica migrate_resume_index: state rmw failed (resume continues; next crash falls back to fresh)"
        );
    }
}

/// 纯逻辑（可单测）：从续跑索引汇总需要启动自愈的 work_dir 集合。
///
/// 取 `multica_task_conversations` 全部条目的 `work_dir`，trim 后去重、丢空。返回值即 multica 子系统
/// 管理 run 的全部独立 repo——启动自愈需逐个覆盖（home repo 自愈够不到这些 work_dir）。
fn collect_multica_work_dirs(
    map: &std::collections::HashMap<String, MulticaTaskConversation>,
) -> Vec<String> {
    let mut dirs: Vec<String> = map
        .values()
        .filter_map(|conv| conv.work_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// 启动自愈：把 multica 远程任务落在各 `work_dir` 的孤儿 Running run pause 成 ProcessInterrupted。
///
/// **根因修复**：`main.rs` 启动时的 `recover_interrupted_running_sessions()` 只跑在 home repo 上，其
/// `pause_all_running_sessions` 仅遍历单一 repo 的 `task_list`；而 multica 远程任务的 run 落在 task 自身
/// `work_dir`（独立 repo）→ 重启后残留 stale `Running` → `classify_resume` 读 `is_run_continuable` = false
/// → 误落 Fresh（断点续跑失效）。此函数遍历 `multica_task_conversations` 的全部 work_dir 逐个自愈，
/// 与 `classify_resume` 读同一张权威表，保证被判定为可续的 run 在判定前已被 pause。
///
/// 安全前提：启动瞬间磁盘上所有 `Running` 都是上一轮崩溃遗留的孤儿态（进程刚起，无在飞 run）。
/// 单 work_dir 自愈失败（盘 I/O）仅 `warn!` 不阻断其余。
pub fn recover_multica_work_dir_sessions(home_app: &App) {
    // 无条件打一条 beacon：存在即证明「改动十三」代码已编进二进制（区分旧 binary 仍落 Fresh）。
    let conv_count = home_app
        .load_state()
        .ok()
        .and_then(|state_cfg| state_cfg.multica_task_conversations)
        .map(|c| c.len())
        .unwrap_or(0);
    let convs = home_app
        .load_state()
        .ok()
        .and_then(|state_cfg| state_cfg.multica_task_conversations);
    let work_dirs = convs
        .as_ref()
        .map(collect_multica_work_dirs)
        .unwrap_or_default();
    if work_dirs.is_empty() {
        info!(
            conv_count,
            "multica startup recovery: no multica work_dir to sweep (断点续跑根因修复代码已加载)"
        );
        return;
    }
    let mut recovered = 0usize;
    for work_dir in &work_dirs {
        let workspace_app = home_app.with_repo_root(Utf8PathBuf::from(work_dir), home_app.config.clone());
        match workspace_app.recover_interrupted_running_sessions() {
            Ok(paused) => {
                recovered += paused.len();
                if !paused.is_empty() {
                    info!(
                        work_dir,
                        count = paused.len(),
                        "multica startup recovery: paused orphaned running sessions in work_dir"
                    );
                }
            }
            Err(error) => warn!(
                work_dir,
                %error,
                "multica startup recovery: recover work_dir failed (skipped)"
            ),
        }
    }
    info!(
        work_dir_count = work_dirs.len(),
        conv_count,
        recovered,
        "multica startup recovery: swept multica work_dir repos for orphaned runs"
    );
}

/// 只读取远程任务需求（claim-at-send，接入方案 B3）。
///
/// 点击「认领执行」只读拉任务详情 + 需求正文，预填 composer、绑定 chip——**不**改 server 任务状态
/// （任务仍 queued、可被其它 runtime 领取；删除 chip 即解绑回普通会话，不涉 server）。真正 claim
/// （pending→dispatched）推迟到用户点「发送」时由 [`start_multica_conversation_run`] 执行——兑现
/// 「删除 chip 不影响任务待办态、点发送才开始」的契约。`auth_token` 不回显（执行凭证不进 VM）。
#[tauri::command]
pub async fn get_multica_task_requirement(
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

    // 只读取：GET 任务详情（裸 AgentTaskResponse → RemoteTask），不改 server 状态。
    let task = client
        .get_task_requirement(&runtime_id, &task_id)
        .await
        .map_err(|e| command_error(e.into()))?;

    Ok(RemoteTaskVm::from_detail(&task, &workspace_id))
}

/// claim-at-send 失败回滚（开发设计 2.5 / 接入方案 B4）。
///
/// claim 已把任务置 dispatched，但其后任意一步（workspace 解析 / 模型校验 / 本地建 run）失败、任务尚未真正
/// 进入 running → best-effort `release_task`（CAS dispatched→queued）把任务还回可领取态。失败仅 `warn!`：
/// server 侧 dispatched 任务有 `FailStaleTasks`（`dispatched_at + 300s`）+ `FailTasksForOfflineRuntimes`
/// （daemon 离线）兜底，且 release 对「已非 dispatched」幂等返回 200。
///
/// **仅适用于「任务确实还在 dispatched」的失败点**（claim 后、start 前）。`start_task` 之后的失败点不能调本函数
/// ——start 响应可能在传输层丢失而 server 已 running，此时 release 是 no-op、任务会永久卡 running（见
/// [`decide_start_failure_action`] / [`fail_after_run_start_failure`]）。
///
/// 注：server 在 claim 时仍写 `prepare_lease_expires_at = now()+45s`，码灵不续约该 lease（仅用一次性的
/// `dispatched_at + 300s` 硬超时兜底），故不存在「lease 自然过期」的旧回收路径。
async fn release_after_run_start_failure(client: &MulticaClient, runtime_id: &str, task_id: &str) {
    if let Err(e) = client.release_task(runtime_id, task_id).await {
        warn!(
            task = %task_id,
            %e,
            "multica release after run-start failure failed (server backstop will recover)"
        );
    }
}

/// `start_task` 失败后的处置决策（纯函数，可单测）。
///
/// `start_task` 的 HTTP 响应可能在传输层丢失：server 侧 `dispatched→running` 已落库，但码灵拿到网络错误
/// （`NetworkFailed`）。此时**不能**假定「start 未生效」直接 release+teardown——release 对 running 是 no-op，
/// 而 running 任务**无 per-task liveness**（webank `FailStaleTasks` 的 running 分支要求 daemon 非 online 才兜底），
/// 只要码灵存活并在心跳（哪怕为别的任务），该任务就永久卡 running。用 [`MulticaClient::get_task_status`] 消歧：
///
/// - 查询返回 `running` → start 实际成功（响应丢失）→ [`StartFailureAction::Continue`]：本地 run 正在执行、
///   server 也 running，两者一致，继续即可（尤其续跑分支已 `run_continue` 的进度不浪费）。
/// - 查询返回其他非 running（dispatched/failed/cancelled/…）→ [`StartFailureAction::RollbackRelease`]：start 未生效
///   或任务已终态 → `release` 回滚（对 dispatched 正确；对终态幂等 no-op）+ 本地 teardown。
/// - 查询本身失败（`None`，无法确认）→ [`StartFailureAction::Terminate`]：不能 release（可能 running）→
///   `fail_task`（reason=`timeout`，resume-safe、可 auto-retry）保证 server 侧任务终结，杜绝任何卡 running。
#[derive(Debug, PartialEq, Eq)]
enum StartFailureAction {
    /// start 已生效（server running）→ 调用方继续执行已建好的 run。
    Continue,
    /// start 未生效 / 已终态（非 running）→ `release` 回滚 + 本地 teardown。
    RollbackRelease,
    /// 无法确认（status 查询失败）→ `fail_task` 终结 + 本地 teardown，保证不卡 running。
    Terminate,
}

/// `status` = `get_task_status` 的结果：`Some(s)` 成功、`None` 查询失败。
fn decide_start_failure_action(status: Option<&str>) -> StartFailureAction {
    match status {
        Some("running") => StartFailureAction::Continue,
        Some(_) => StartFailureAction::RollbackRelease,
        None => StartFailureAction::Terminate,
    }
}

/// start 后无法确认任务状态时的兜底终结（与 [`release_after_run_start_failure`] 对称的最佳努力上报）。
///
/// `get_task_status` 查询失败（无法区分 running vs dispatched）时调用：`fail_task`（reason=`timeout`，
/// resume-safe、可 auto-retry）保证 server 侧任务终结，杜绝「start 已成功但码灵以为失败」导致的永久卡 running。
/// 走 `fail_task` 自带的终态严格重试；最终仍失败仅 `warn!`（dispatched 子情形仍有 5min backstop 兜底）。
async fn fail_after_run_start_failure(client: &MulticaClient, task_id: &str) {
    if let Err(e) = client
        .fail_task(task_id, "start ack lost; terminated to avoid orphan run", "timeout")
        .await
    {
        warn!(
            task = %task_id,
            %e,
            "multica fail after run-start failure failed (server backstop will recover)"
        );
    }
}

/// 把关联 issue 流转到「进行中」（改动五，与完成时 `done` 流转对称）。
///
/// `start_task` 成功后调用——server 的 start 只把 **task** 推进到 running（看板「正在进行」badge 据此），
/// 但从不改 `issue.status`（设计上交给 agent 管）；而看板**列**由 `issue.status` 派生，故卡片停在「待办」列。
/// 码灵作为中介用自身 PAT 补这条流转，把卡片移到「进行中」列。最佳努力：失败仅 `warn!`，不让 run 失败
/// （start_task 已成功，issue 列推进是看板一致性，由 server 失败路径 in_progress→todo 兜底）。
/// issue 关联缺失（非 issue 来源任务）则跳过。与 `bridge.rs` done 路径同构。
async fn mark_issue_in_progress(client: &MulticaClient, workspace_id: &str, issue_id: Option<&str>) {
    if let Some(issue) = in_progress_target(issue_id) {
        if let Err(e) = client
            .update_issue_status(workspace_id, issue, MULTICA_ISSUE_IN_PROGRESS_STATUS)
            .await
        {
            warn!(
                issue = %issue,
                %e,
                "multica update_issue_status(in_progress) failed (task already started; ignored)"
            );
        }
    }
}

/// 纯判定（无 I/O，可单测）：是否需要把 issue 流转到「进行中」，返回应推进的 issue id 引用。
///
/// None / 空 / 纯空白 → None（跳过：非 issue 来源任务，或脏数据，避免对 issue-less 任务发空 id 的
/// `PUT /api/issues/`）。与 `bridge.rs` done 路径的 issue 过滤同构。
fn in_progress_target(issue_id: Option<&str>) -> Option<&str> {
    issue_id.filter(|s| !s.trim().is_empty())
}

/// 发送预填好的远程任务：claim-at-send（点发送才 claim+start）+ 复用本地会话创建链路 + multica 簿记
/// （开发设计 2.5 / 2.8 / 4.3，接入方案 B2/C2/B4）。
///
/// 与本地工作空间「+」号进入的是**同一创建链路**：解析 workspace 绑定目录 → 构造 workspace-bound App
/// （注入与 `create_conversation_run` 同款的 ACP emitter，NodeCompleted/RunCompleted 流向前端）→
/// `validate_conversation_create_vm` → **复用** `create_conversation_run_vm`（建工作流 + 建任务 +
/// 写 conversation.json + 拷附件 + 启动 run）。远程任务预填的需求即 `input.requirement`，用户在 composer
/// 已选好模型/模式（与本地完全一致）。
///
/// **claim-at-send**：发送即事务边界——先 `claim_specific_task`（pending→dispatched），再走本地创建链路 +
/// `start_task`（dispatched→running）。点击「认领执行」时不 claim（只 [`get_multica_task_requirement`]
/// 只读拉正文），故删除 chip 不影响任务待办态。
///
/// **失败回滚**：claim 成功后、任务尚未进入 running 前的任意失败（workspace 解析 / 模型校验 / 本地建 run /
/// start_task）→ [`release_after_run_start_failure`] 把任务 CAS 回 queued（替代旧 prepare-lease 的
/// 45s 自然过期兜底，lease 已移除）。claim 本身失败（404/409）则任务未被领取，直接报错、无须回滚。
///
/// multica 专属叠加（成功建 run 后）：① `register_active_run`（真实 run.id，先于事件归属反查）；
/// ② 持久化 `multica_task_conversations`（断点续跑索引，session_id 待 bridge 回填）；③ `start_task`
/// （dispatched→running）。
///
/// 库层 sync App 调用经 `spawn_blocking` 执行；HTTP（claim/start/release）留在 async 上下文。
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

    // ① claim-at-send：发送即领取。先解析 runtime_id（该 workspace 启动注册缓存），再 claim
    //    （pending→dispatched）。claim 失败（404 任务已不在 / 409 已被领取 / 网络）→ 任务未被本端领取，
    //    直接报错；其后任意失败才走 release 回滚（见 release_after_run_start_failure）。
    let runtime_id = shared
        .lock()
        .ok()
        .and_then(|guard| guard.runtime_id(&workspace_id).map(str::to_string))
        .ok_or(MulticaError::RuntimeOffline)
        .map_err(|e| command_error(e.into()))?;
    let task = client
        .claim_specific_task(&runtime_id, &remote_task_id)
        .await
        .map_err(|e| command_error(e.into()))?;
    // claim 响应携带的任务身份（替代旧 lease 缓存）：供后续 register_active_run / 续跑判定 / issue 流转消费。
    // prior_session_id 当前未消费（保留 claim 响应既有字段，不扩范围）。
    let issue_id = task.issue_id.clone();
    let title = task.title.clone();
    let parent_task_id = task.parent_task_id.clone();

    // ② resolve workspace（input.project_id = composer 下拉选定的本地工作区，执行时由用户决定；
    //    绑定模型已下沉到任务级，工作区不再绑本地目录）。
    let global_app = context.app();
    let app_state = global_app.load_state().map_err(command_error)?;
    let Some((workspace_path, resolved_project_id)) =
        workspace_entry_for_project(&app_state, &input.project_id)
    else {
        // claim 已成功、workspace 解析失败 → 回滚（dispatched→queued），任务回可领取态。
        release_after_run_start_failure(&client, &runtime_id, &remote_task_id).await;
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
        // claim 已成功、模型/agent 校验失败 → 回滚（dispatched→queued），任务回可领取态。
        release_after_run_start_failure(&client, &runtime_id, &remote_task_id).await;
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

    // ⑤ 断点续跑判定（改动三）：命中可续跑 checkpoint → 续既有本地 run（冷重连 ACP session，
    //    prompt=None 纯续跑，沿用既有模型/模式）；续跑失败或无可续 checkpoint → 落下面的 Fresh 分支。
    //    决策依据：D1=纯续跑(prompt=None)、D2=仅 is_run_continuable 时续（不主动 pause-then-resume）。
    if let ResumeDecision::Resume {
        local_task_id,
        local_run_id,
    } = classify_resume(
        &context.app(),
        &remote_task_id,
        parent_task_id.as_deref(),
    )
    {
        let prior_task_id = local_task_id;
        let prior_run_id = local_run_id;
        let register_task_id = prior_task_id.clone();
        let register_run_id = prior_run_id.clone();
        let resume_project_id = resolved_project_id.clone();
        // 续跑索引迁移所需：子任务 id（= remote_task_id）、父任务 id（claim 响应血缘）、home-repo App。
        // 仅当续跑经父任务反查解析（parent_task_id 有且 ≠ 子 id）时迁；同 id 场景索引已挂正确键，跳过。
        let resume_remote_task_id = remote_task_id.clone();
        let resume_parent_task_id = parent_task_id.clone();
        let resume_home_app = context.app();
        // clone_for_background 保留全部字段（含 ACP emitter）→ 续跑事件仍流向前端；原 `app` 留给 Fresh 兜底。
        let resume_app = app.clone_for_background();
        let join = tauri::async_runtime::spawn_blocking(
            move || -> anyhow::Result<ConversationRunVm> {
                // 纯续跑：prompt=None，接着被中断的编排往下跑（不新增一轮用户消息、不换模型/模式）。
                resume_app.run_continue_background(&prior_task_id, &prior_run_id, None, None)?;
                // 续跑成功 → 迁移续跑索引到子任务（断点续跑方案 §3.3）：子任务 T' 续的是父 T 的本地
                // run/session，索引原挂 T 名下；迁到 T' 后后续再次重试（T'→T''）仍能链式反查。
                if let Some(parent) = resume_parent_task_id.as_deref() {
                    if parent != resume_remote_task_id.as_str() {
                        migrate_resume_index(
                            &resume_home_app,
                            &resume_remote_task_id,
                            parent,
                            &prior_task_id,
                            &prior_run_id,
                        );
                    }
                }
                // 从既有 run 还原 VM（导航到既有会话，非新建）。
                conversation_run_vm(&resume_app, &resume_project_id, &prior_task_id, &prior_run_id, None)
            },
        )
        .await
        .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?;

        match join {
            Ok(vm) => {
                // 登记 active_run（既有 local ids，先于 NodeCompleted/RunCompleted 归属反查）。
                if let Ok(mut guard) = shared.lock() {
                    guard.register_active_run(
                        &remote_task_id,
                        ActiveRemoteRun {
                            runtime_id: runtime_id.clone(),
                            workspace_id: workspace_id.clone(),
                            local_project_id: resolved_project_id.clone(),
                            local_task_id: register_task_id.clone(),
                            local_run_id: register_run_id.clone(),
                            issue_id: issue_id.clone(),
                            title: title.clone(),
                            started_at: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
                // 通知 server dispatched→running（续跑与 Fresh 都 false；force_fresh 仅整任务重跑）。
                if let Err(start_err) = client.start_task(&remote_task_id, false).await {
                    // start 响应可能在传输层丢失而 server 已 running：盲目 release 会致任务永久卡 running
                    // （release 对 running 是 no-op）。用 get_task_status 消歧，决策见 decide_start_failure_action。
                    let action = decide_start_failure_action(
                        client.get_task_status(&remote_task_id).await.ok().as_deref(),
                    );
                    if action == StartFailureAction::Continue {
                        // start 实际成功（响应丢失）：续跑 run 正在执行、server running，一致 → 继续。
                        mark_issue_in_progress(&client, &workspace_id, issue_id.as_deref()).await;
                        crate::multica::bridge::emit_multica_task_updated(&app_handle);
                        return Ok(vm);
                    }
                    // 未生效（release）或无法确认（fail）→ 回滚 server，再本地 teardown。
                    match action {
                        StartFailureAction::RollbackRelease => {
                            release_after_run_start_failure(&client, &runtime_id, &remote_task_id)
                                .await
                        }
                        StartFailureAction::Terminate => {
                            fail_after_run_start_failure(&client, &remote_task_id).await
                        }
                        StartFailureAction::Continue => unreachable!(),
                    }
                    let home_app = context.app();
                    let workspace_app = home_app
                        .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
                    crate::multica::bridge::teardown_active_run(
                        &workspace_app,
                        shared.inner(),
                        &home_app,
                        &remote_task_id,
                        &register_task_id,
                        &register_run_id,
                    );
                    crate::multica::bridge::emit_multica_task_updated(&app_handle);
                    return Err(command_error(start_err.into()));
                }
                // start 成功 → 把关联 issue 流转到「进行中」（改动五：与完成时 done 对称）。
                mark_issue_in_progress(&client, &workspace_id, issue_id.as_deref()).await;
                // 通知侧栏刷新：active_runs 已登记，前端即时显示 running 行（改动七）。
                crate::multica::bridge::emit_multica_task_updated(&app_handle);
                return Ok(vm);
            }
            Err(error) => {
                // 续跑失败（session 死 / strict_continue 失败 / run 已不可续）→ 落 Fresh。
                // checkpoint 由下面 Fresh 分支覆盖（session_id 重置 None），无需单独清理。
                warn!(
                    task = %remote_task_id,
                    %error,
                    "multica resume failed; falling back to fresh"
                );
            }
        }
    }

    // 搬运到 spawn_blocking 闭包的 multica 簿记数据。
    let remote = remote_task_id.clone();
    let ws_id = workspace_id.clone();
    let rt_id = runtime_id.clone();
    let issue = issue_id.clone();
    let title = title.clone();
    let local_project = resolved_project_id.clone();
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
                        local_project_id: local_project,
                        local_task_id: run.task_id.clone(),
                        local_run_id: run.run_id.clone(),
                        issue_id: issue,
                        title,
                        started_at: chrono::Utc::now().to_rfc3339(),
                    },
                );
                // 建成 run + 登记 active_run（claim-at-send 无 lease 需释放）。
            }
            // 落断点续跑索引（home-repo StateConfig）：新 run 的 local ids + work_dir；session_id 待 bridge 回填。
            // RMW 经 with_state 原子化：与 bridge NodeCompleted/终态收尾并发 save 互不覆盖（lost-update）。
            ctx_clone.app().with_state(|state| {
                let mut conversations = state.multica_task_conversations.take().unwrap_or_default();
                let entry = conversations.entry(remote.clone()).or_insert(MulticaTaskConversation {
                    local_task_id: run.task_id.clone(),
                    local_run_id: run.run_id.clone(),
                    session_id: None,
                    work_dir: Some(ws_path.clone()),
                });
                entry.local_task_id = run.task_id.clone();
                entry.local_run_id = run.run_id.clone();
                // 命中 stale checkpoint 时重置 session_id（旧 session 随旧 run 失效；新 run 的 session_id
                // 待 bridge 在 NodeCompleted 回填）。修旧漏：此前 Fresh 覆盖既有 checkpoint 时漏清 session_id。
                entry.session_id = None;
                entry.work_dir = Some(ws_path.clone());
                state.multica_task_conversations = Some(conversations);
                true
            })?;
            Ok(run)
        },
    )
    .await
    .map_err(|_| CommandErrorVm::new("app.task-join-failed", serde_json::json!({})))?;

    match result {
        Ok(run) => {
            // 本地 run 已登记 → 通知 server dispatched→running。
            // composer 流总是 fresh（与本地「+」一致；断点续跑由 server 重派 + bridge 兜底，不在此分支）。
            if let Err(start_err) = client.start_task(&remote_task_id, false).await {
                // start 响应可能在传输层丢失而 server 已 running：盲目 release 会致任务永久卡 running
                // （release 对 running 是 no-op）。用 get_task_status 消歧，决策见 decide_start_failure_action。
                let action = decide_start_failure_action(
                    client.get_task_status(&remote_task_id).await.ok().as_deref(),
                );
                if action == StartFailureAction::Continue {
                    // start 实际成功（响应丢失）：本地 run 正在执行、server running，一致 → 继续。
                    mark_issue_in_progress(&client, &workspace_id, issue_id.as_deref()).await;
                    crate::multica::bridge::emit_multica_task_updated(&app_handle);
                    return Ok(run);
                }
                // 未生效（release）或无法确认（fail）→ 回滚 server，再本地 teardown。
                match action {
                    StartFailureAction::RollbackRelease => {
                        release_after_run_start_failure(&client, &runtime_id, &remote_task_id).await
                    }
                    StartFailureAction::Terminate => {
                        fail_after_run_start_failure(&client, &remote_task_id).await
                    }
                    StartFailureAction::Continue => unreachable!(),
                }
                let home_app = context.app();
                let workspace_app = home_app
                    .with_repo_root(Utf8PathBuf::from(&workspace_path), context.config.clone());
                crate::multica::bridge::teardown_active_run(
                    &workspace_app,
                    shared.inner(),
                    &home_app,
                    &remote_task_id,
                    &run.task_id,
                    &run.run_id,
                );
                crate::multica::bridge::emit_multica_task_updated(&app_handle);
                return Err(command_error(start_err.into()));
            }
            // start 成功 → 把关联 issue 流转到「进行中」（改动五：与完成时 done 对称）。
            mark_issue_in_progress(&client, &workspace_id, issue_id.as_deref()).await;
            // 通知侧栏刷新：active_runs 已登记，前端即时显示 running 行（改动七）。
            crate::multica::bridge::emit_multica_task_updated(&app_handle);
            Ok(run)
        }
        Err(error) => {
            // 本地建 run 失败：release 回滚（dispatched→queued），任务回可领取态供重试（替代 fail_task 终态化——
            // 本地环境失败非任务本身失败，requeue 比 terminal 更合理）。
            release_after_run_start_failure(&client, &runtime_id, &remote_task_id).await;
            Err(command_error(error))
        }
    }
}

/// 用户手动中断 multica 远程任务（前端看板 running 列「取消」按钮触发）。
///
/// 双通道收尾，缺一即致 running 孤儿（🔴 根因：webank 对 running 任务无逐任务 liveness，
/// 码灵静默 drop 会让它永久卡 running）：
/// 1. **远端终态上报**：`fail_task(reason=agent_error)` 把 remote running 任务终态化为 failed。
///    bare `agent_error` 是**不可重试**（webank retryableReasons 不含），用户主动取消的任务不自动 requeue。
///    best-effort：任务已被 sweeper/他端终态化时 fail 返回 4xx，吞掉即可（与 fail_after_run_start_failure
///    同语义；失败不阻断本地收尾，本地 run 无论如何都要作废）。
/// 2. **本地收尾**：`run_pause(ProcessInterrupted)` + 杀 ACP + 清 `active_runs`/`task_conversations`，
///    cancelled task 不再断点续跑。bridge 对 RunPaused 不上报终态（Paused 盲区）——远端终态由通道 1 负责，
///    本地 pause 事件不复用为远端上报，两通道职责分离。
///
/// 注：取消检测（remote 已 cancelled/failed/404，loop 命中）是**独立路径**，不经本命令；那里 remote 已
/// terminal，无需 fail。本命令恒为「码灵侧 running 任务的主动取消」。
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
    // 解析 workspace 目录（任务级 local_project_id → workspace_path；绑定模型已下沉到任务级，
    // 工作区不再绑本地目录，故从在飞 run 取 local_project_id 而非 workspace 绑定）。
    let home_state = context.app().load_state().map_err(command_error)?;
    let Some((workspace_path, _)) = workspace_entry_for_project(&home_state, &run.local_project_id)
    else {
        return Err(command_error(MulticaError::TaskNotFound.into()));
    };
    let workspace_app = state
        .app()
        .map_err(command_error)?
        .with_repo_root(Utf8PathBuf::from(workspace_path), context.config.clone());

    // 通道 1：远端终态上报（best-effort）。码灵主动取消的 running 任务须 fail 化，否则永久卡 running。
    // bare agent_error 非重试——用户取消不应被 webank 自动 requeue。
    if let (Some(base_url), Some(pat)) =
        (multica_base_url(&context.config), get_pat(&context.config))
    {
        if let Ok(client) = MulticaClient::new(base_url, Some(pat)) {
            if let Err(fail_err) = client
                .fail_task(&task_id, "cancelled by user (manual cancel)", "agent_error")
                .await
            {
                warn!(
                    error = %fail_err,
                    task_id = %task_id,
                    "multica cancel: best-effort fail_task failed (task may already be terminal); proceeding with local teardown"
                );
            }
        }
    }

    let local_task_id = run.local_task_id.clone();
    let local_run_id = run.local_run_id.clone();
    let remote = task_id.clone();
    let shared_clone = shared.inner().clone();
    let home_app = context.app();

    tauri::async_runtime::spawn_blocking(
        move || {
            // 通道 2：作废本地 run（Paused，bridge 不上报）+ 杀 ACP + 清 active_runs/task_conversations。
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

// ===== M3.5 / M5-c：multica workspace 绑定 CRUD（开发设计 2.2.1 / 2.5）=====
//
// 一个 multica workspace 只绑定一个执行 provider（绑定后不可变）；**本地工作目录不在工作区级
// 绑定**，推迟到每次任务执行时由用户在 composer 下拉选定，并随任务生命周期落到任务级结构体
// （`ActiveRemoteRun` / `MulticaCompletedTask`，见 Multica远程任务管理设计 §3）。故此处不再派生
// project_id、不再落 conversation_workspaces 条目。

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

/// 添加一个 multica 远程工作区绑定（首个自动设为 active），并即时 register。
///
/// 仅绑定 provider（执行器类型，绑定后不可变）；本地工作目录推迟到任务执行时由 composer 下拉
/// 选定，不在工作区级绑定（见 Multica远程任务管理设计 §3）。
///
/// `slug`：server `list_workspaces` 不暴露 slug，用 `id` 兜底（slug 不在任何关键路径）。
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
    provider: String,
) -> CommandResult<MulticaSettingsVm> {
    let context = state.context().map_err(command_error)?;
    let app = context.app();

    // 落 multica 绑定（去重 by multica workspace id；SettingsConfig 字段为 Option<Vec>）
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
        provider: provider.clone(),
    });
    if settings.desktop_multica_active_workspace_id.is_none() {
        settings.desktop_multica_active_workspace_id = Some(workspace_id.clone());
    }
    app.save_settings(&settings).map_err(command_error)?;
    state.update_settings_config(&settings).map_err(command_error)?;

    // 即时 register：绑定后立即可用（无需重启等启动 loop）。失败非致命——启动 loop 兜底重试。
    register_workspace_best_effort(&state, &shared, &workspace_id, &provider).await;

    // 工作空间绑定变更 → 通知任务列表 + 设置页 re-fetch（跨视图同步）。
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
    // 委托共享 register 逻辑（与启动全量 / 自愈 / connect 触发同一实现，杜绝复制）。
    match crate::multica::loop_::register_workspace(
        &client,
        workspace_id,
        provider,
        &daemon_id,
        shared.inner(),
        true,
    )
    .await
    {
        Ok(runtime_id) => info!(
            workspace = %workspace_id,
            runtime_id = %runtime_id,
            "multica register-on-add ok"
        ),
        Err(error) => warn!(
            workspace = %workspace_id,
            %error,
            "multica register-on-add failed (non-fatal, startup loop will retry)"
        ),
    }
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

    // ---- 改动三：断点续跑判定（classify_resume_from 纯逻辑固化）----
    use gold_band::domain::{PauseReason, RunStatus};
    use gold_band::runtime::RunState;

    fn checkpoint(session_id: Option<&str>, work_dir: Option<&str>) -> MulticaTaskConversation {
        MulticaTaskConversation {
            local_task_id: "local-task".into(),
            local_run_id: "local-run".into(),
            session_id: session_id.map(String::from),
            work_dir: work_dir.map(String::from),
        }
    }

    /// 构造 RunState：仅 status / pause_reason / locator 影响判定，其余填占位（outcome 恒 None）。
    fn run_state(
        status: RunStatus,
        pause_reason: Option<PauseReason>,
        with_locator: bool,
    ) -> RunState {
        RunState {
            version: "1".into(),
            id: "run-1".into(),
            task_id: "local-task".into(),
            task_uuid: None,
            status,
            outcome: None,
            started_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            workflow_snapshot: "{}".into(),
            current_round: with_locator.then(|| "round-1".into()),
            current_node: with_locator.then(|| "node-1".into()),
            current_attempt: with_locator.then(|| "attempt-1".into()),
            new_rounds_opened: 0,
            pause_reason,
            uuid: None,
            last_executed_node: None,
        }
    }

    #[test]
    fn resume_fresh_when_no_checkpoint() {
        // 无断点索引 → 新建。
        assert!(matches!(classify_resume_from(None, None), ResumeDecision::Fresh));
    }

    #[test]
    fn resume_when_session_id_missing_or_blank() {
        // session_id None / 纯空白 **不再阻断续跑**：ACP session 由 run 自身状态恢复
        // （`run_continue_background` 读 `worker-ref.json` 的 `continue_ref.acpSessionId`），
        // 不经 checkpoint 的 session_id 字段。崩溃常发生在首个 NodeCompleted 前（bridge 未回填
        // session_id），若以其为门会把可续 run 误判新建。这里 run 可续（Paused + ProcessInterrupted
        // + locator 齐）→ Resume，无视 checkpoint 的 session_id 缺失/空白。
        let conv = checkpoint(None, Some("/ws"));
        let run = run_state(
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
            true,
        );
        assert!(matches!(
            classify_resume_from(Some(&conv), Some(&run)),
            ResumeDecision::Resume { .. }
        ));
        let conv_blank = checkpoint(Some("   "), Some("/ws"));
        assert!(matches!(
            classify_resume_from(Some(&conv_blank), Some(&run)),
            ResumeDecision::Resume { .. }
        ));
    }

    #[test]
    fn resume_fresh_when_run_unreachable() {
        // 有 session_id 但本地 run 读不到（work_dir 缺/不存在）→ 新建。
        let conv = checkpoint(Some("acp-session-1"), None);
        assert!(matches!(
            classify_resume_from(Some(&conv), None),
            ResumeDecision::Fresh
        ));
    }

    #[test]
    fn resume_when_run_paused_process_interrupted() {
        // 崩溃重启后启动自愈写入的态：Paused + ProcessInterrupted + locator 齐 + outcome None → 续跑。
        let conv = checkpoint(Some("acp-session-1"), Some("/ws"));
        let run = run_state(
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
            true,
        );
        match classify_resume_from(Some(&conv), Some(&run)) {
            ResumeDecision::Resume {
                local_task_id,
                local_run_id,
            } => {
                assert_eq!(local_task_id, "local-task");
                assert_eq!(local_run_id, "local-run");
            }
            other => panic!("期望 Resume，实际 {other:?}"),
        }
    }

    #[test]
    fn resume_fresh_when_run_running() {
        // run 还卡 Running（未重启自愈 / 未 Paused）→ is_run_continuable 不满足 → 新建。
        // 对应 D2：不主动 pause-then-resume，仅 is_run_continuable 命中才续。
        let conv = checkpoint(Some("acp-session-1"), Some("/ws"));
        let run = run_state(RunStatus::Running, None, true);
        assert!(matches!(
            classify_resume_from(Some(&conv), Some(&run)),
            ResumeDecision::Fresh
        ));
    }

    #[test]
    fn resume_fresh_when_paused_but_non_continuable_reason() {
        // Paused 但 pause_reason 不在可续集合（ErrorBlocked）→ 新建。
        let conv = checkpoint(Some("acp-session-1"), Some("/ws"));
        let run = run_state(RunStatus::Paused, Some(PauseReason::ErrorBlocked), true);
        assert!(matches!(
            classify_resume_from(Some(&conv), Some(&run)),
            ResumeDecision::Fresh
        ));
    }

    #[test]
    fn resume_fresh_when_paused_but_missing_locator() {
        // Paused + ProcessInterrupted 但缺 round/node/attempt locator → is_run_continuable 不满足 → 新建。
        let conv = checkpoint(Some("acp-session-1"), Some("/ws"));
        let run = run_state(
            RunStatus::Paused,
            Some(PauseReason::ProcessInterrupted),
            false,
        );
        assert!(matches!(
            classify_resume_from(Some(&conv), Some(&run)),
            ResumeDecision::Fresh
        ));
    }

    // ---- 断点续跑方案 §3.3：父系反查 + 索引迁移（纯逻辑固化）----
    #[test]
    fn resolve_resume_checkpoint_prefers_literal_id_then_parent() {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "t-parent".into(),
            checkpoint(Some("sess-parent"), Some("/ws-parent")),
        );
        map.insert(
            "t-child".into(),
            checkpoint(Some("sess-child"), Some("/ws-child")),
        );

        // 1. 字面 id 命中（同 id 场景）→ 用 child 自己的 entry，不看 parent。
        let hit = resolve_resume_checkpoint(&map, "t-child", Some("t-parent"));
        assert_eq!(
            hit.as_ref().and_then(|c| c.session_id.as_deref()),
            Some("sess-child")
        );

        // 2. 字面 id miss、parent 命中（auto-retry 子任务场景）→ 用父 entry（父的 run/session 才是要续的）。
        let via_parent = resolve_resume_checkpoint(&map, "t-child-new", Some("t-parent"));
        assert_eq!(
            via_parent.as_ref().and_then(|c| c.session_id.as_deref()),
            Some("sess-parent")
        );

        // 3. 字面 miss 且 parent 也 miss / parent 为 None → None（落 Fresh）。
        assert!(resolve_resume_checkpoint(&map, "t-unknown", Some("t-missing")).is_none());
        assert!(resolve_resume_checkpoint(&map, "t-unknown", None).is_none());
    }

    #[test]
    fn migrate_resume_index_map_moves_parent_entry_to_child() {
        // 子任务 T' 续父 T 的 run：索引从 T 迁到 T'（继承 session/work_dir，local ids 用本次 run）。
        let mut map = std::collections::HashMap::new();
        map.insert(
            "t-parent".into(),
            MulticaTaskConversation {
                local_task_id: "old-task".into(),
                local_run_id: "old-run".into(),
                session_id: Some("acp-sess".into()),
                work_dir: Some("/ws".into()),
            },
        );
        let migrated = migrate_resume_index_map(map, "t-child", "t-parent", "new-task", "new-run");

        // 子条目：继承父 session/work_dir，local ids 为本次续跑 run。
        let child = migrated.get("t-child").expect("子条目应存在");
        assert_eq!(child.local_task_id, "new-task");
        assert_eq!(child.local_run_id, "new-run");
        assert_eq!(child.session_id.as_deref(), Some("acp-sess"));
        assert_eq!(child.work_dir.as_deref(), Some("/ws"));
        // 父条目已清（被取代）。
        assert!(!migrated.contains_key("t-parent"), "父条目应被移除");
    }

    #[test]
    fn migrate_resume_index_map_inserts_child_even_if_parent_missing() {
        // 兜底：父 entry 已被 finalize 清掉但续跑仍成功 → 插入 child（local ids 已知，
        // session/work_dir 置 None 待 bridge 回填），保证下次重试仍可链式续。
        let map = std::collections::HashMap::new();
        let migrated = migrate_resume_index_map(map, "t-child", "t-parent", "task-1", "run-1");
        let child = migrated.get("t-child").expect("子条目应插入");
        assert_eq!(child.local_task_id, "task-1");
        assert_eq!(child.local_run_id, "run-1");
        assert!(child.session_id.is_none());
        assert!(child.work_dir.is_none());
    }

    // ---- 改动十三：启动自愈覆盖 multica work_dir（collect_multica_work_dirs 纯逻辑固化）----
    #[test]
    fn collect_multica_work_dirs_dedup_trims_and_drops_empty() {
        // 续跑索引汇总待自愈 work_dir：去重 + trim + 丢空/None（home 自愈够不到这些独立 repo）。
        let mut map = std::collections::HashMap::new();
        map.insert("t-1".into(), checkpoint(Some("s-1"), Some("/ws-a")));
        map.insert("t-2".into(), checkpoint(Some("s-2"), Some("  /ws-b  "))); // trim
        map.insert("t-3".into(), checkpoint(Some("s-3"), Some("/ws-a"))); // 与 t-1 重复
        map.insert("t-4".into(), checkpoint(Some("s-4"), Some("   "))); // 纯空白 → 丢
        map.insert("t-5".into(), checkpoint(Some("s-5"), None)); // None → 丢

        let dirs = collect_multica_work_dirs(&map);
        assert_eq!(dirs, vec!["/ws-a".to_string(), "/ws-b".to_string()]);
    }

    #[test]
    fn collect_multica_work_dirs_empty_map_yields_empty() {
        // 无续跑索引 → 无 work_dir 待自愈（recover_multica_work_dir_sessions 提前返回）。
        let map = std::collections::HashMap::<String, MulticaTaskConversation>::new();
        assert!(collect_multica_work_dirs(&map).is_empty());
    }

    #[test]
    fn in_progress_status_constant_is_in_progress() {
        // 锁定状态字符串（改动五）：与 server validIssueStatuses 的 in_progress 一致。
        assert_eq!(MULTICA_ISSUE_IN_PROGRESS_STATUS, "in_progress");
    }

    #[test]
    fn in_progress_target_skips_absent_or_blank_issue() {
        // 非 issue 来源任务（issue_id=None）→ 跳过，不发空 id 的 PUT。
        assert_eq!(in_progress_target(None), None);
        // 空字符串 / 纯空白 → 跳过（脏数据兜底）。
        assert_eq!(in_progress_target(Some("")), None);
        assert_eq!(in_progress_target(Some("   ")), None);
        // 合法 issue id → 返回该 id（应推进到 in_progress）。
        assert_eq!(in_progress_target(Some("iss-42")), Some("iss-42"));
    }

    #[test]
    fn merge_workspace_tasks_orders_running_pending_terminal_with_dedup() {
        // 顺序：running（在跑）→ pending（可领取）→ terminal（历史）。
        let running = vec![task_vm("rt-1", "running")];
        let pending = vec![task_vm("rt-2", "queued")];
        let terminal = vec![task_vm("rt-3", "completed"), task_vm("rt-4", "failed")];
        let merged = merge_workspace_tasks(running, pending, &terminal);
        assert_eq!(
            merged.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
            vec!["rt-1", "rt-2", "rt-3", "rt-4"]
        );

        // 去重优先级 running > pending > terminal：同 id 多源时 running（当前真相）胜出。
        let running2 = vec![task_vm("rt-1", "running")];
        let pending2 = vec![task_vm("rt-1", "queued"), task_vm("rt-5", "queued")];
        let terminal2 = vec![task_vm("rt-1", "completed"), task_vm("rt-2", "completed")];
        let merged2 = merge_workspace_tasks(running2, pending2, &terminal2);
        assert_eq!(
            merged2.iter().map(|t| (t.id.clone(), t.status.clone())).collect::<Vec<_>>(),
            vec![
                ("rt-1".into(), "running".into()),
                ("rt-5".into(), "queued".into()),
                ("rt-2".into(), "completed".into())
            ]
        );

        // running 与 pending 都空 → 仅终态行（未注册 workspace 仍展示已完成历史）。
        let merged3 = merge_workspace_tasks(Vec::new(), Vec::new(), &terminal);
        assert_eq!(
            merged3.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
            vec!["rt-3", "rt-4"]
        );
    }

    /// 构造最小 RemoteTaskVm（仅 id + status 影响 merge 判定）。
    fn task_vm(id: &str, status: &str) -> RemoteTaskVm {
        RemoteTaskVm {
            id: id.into(),
            issue_id: None,
            status: status.into(),
            workspace_id: String::new(),
            title: id.into(),
            last_activity_at: None,
            requirement: None,
            local_task_id: None,
            run_id: None,
            project_id: None,
        }
    }

    // ---- 改动十八（🔴）：start 响应丢失消歧决策（decide_start_failure_action 纯逻辑固化）----
    // 决策表：get_task_status 返回值 → 如何收尾，避免盲目 release 致 running 永久卡死。
    #[test]
    fn start_failure_action_continue_when_running() {
        // start 实际成功（响应丢失）→ server 已 running，与本地 run 一致 → 继续，不收尾。
        assert_eq!(
            decide_start_failure_action(Some("running")),
            StartFailureAction::Continue
        );
    }

    #[test]
    fn start_failure_action_rollback_when_other_non_running_status() {
        // 仍 dispatched（start 未生效）/ queued（已被 requeue）/ 终态化（failed/cancelled）→
        // release 回滚（dispatched→queued CAS；对非 dispatched 幂等 no-op）。
        for status in ["dispatched", "queued", "failed", "cancelled", "completed"] {
            assert_eq!(
                decide_start_failure_action(Some(status)),
                StartFailureAction::RollbackRelease,
                "status={status} 应回滚 release"
            );
        }
    }

    #[test]
    fn start_failure_action_terminate_when_status_unknown() {
        // 查询本身失败（None）→ 无法确认是否已 running → 终态化（fail_task），不能 release
        // （release 对 running 是 no-op 会致孤儿）。fail 是唯一能终结 running 的确定动作。
        assert_eq!(
            decide_start_failure_action(None),
            StartFailureAction::Terminate
        );
    }

    #[test]
    fn start_failure_action_is_status_sensitive_not_truthy() {
        // 守恒：仅精确 "running" 继续；任意非 running 字符串（含含 running 子串的怪值）一律回滚。
        assert_eq!(
            decide_start_failure_action(Some("Running")),
            StartFailureAction::RollbackRelease
        );
        assert_eq!(
            decide_start_failure_action(Some("running ")),
            StartFailureAction::RollbackRelease
        );
        assert_eq!(
            decide_start_failure_action(Some("not-running")),
            StartFailureAction::RollbackRelease
        );
    }
}
