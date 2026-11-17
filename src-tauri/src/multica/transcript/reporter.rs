//! 单个远程任务的执行记录上报器：有界缓冲 + 500ms 批 flush + seq 分配 + 终态排空 + 用量上报。
//!
//! 生命周期与 `(local_task_id, local_run_id)` 绑定（= 一个 remote task 的一次执行）。服务端
//! 侧 seq 作用域是 `task_id`，而每次 retry/rerun 服务端都会产生新的 `task_id`，因此**不需要**
//! 任何跨进程 seq 水位文件：新 run 从 1 开始即可。
//!
//! 失败策略：批次失败即丢弃（协议明确批次原子、不重试；重发同 seq 会产生重复行）；
//! 只有 404（任务已不可见/无权）会让本 run 停止上报，避免无意义的持续请求。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use camino::Utf8Path;
use gold_band::acp::usage::read_prompt_usage_segments;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{Notify, oneshot};
use tracing::warn;

use crate::multica::client::MulticaClient;
use crate::multica::error::MulticaError;

use super::model::{FLUSH_INTERVAL, UsageRow};
use super::projector::Projector;

/// 取当前连接对应的 client。进程内连接信息可能被用户重配，所以每次发送时重新读取。
pub(crate) type ClientProvider = Arc<dyn Fn() -> Option<MulticaClient> + Send + Sync>;

/// 在飞远程任务的上报器注册表（Tauri managed state）。
///
/// 两层 map 用 `&str` 直接查找，避免每条 live 事件都为主键分配字符串。
#[derive(Default)]
pub struct TranscriptRegistry {
    runs: Mutex<HashMap<String, HashMap<String, Arc<Reporter>>>>,
}

impl TranscriptRegistry {
    fn insert(&self, local_task_id: &str, local_run_id: &str, reporter: Arc<Reporter>) {
        let mut runs = self.lock();
        if let Some(previous) = runs
            .entry(local_task_id.to_string())
            .or_default()
            .insert(local_run_id.to_string(), reporter)
        {
            // 同一 run 重复注册（极端竞态）：旧 worker 立即收尾，避免两路写同一 task 的 seq。
            previous.request_finish();
        }
    }

    fn find(&self, local_task_id: &str, local_run_id: &str) -> Option<Arc<Reporter>> {
        self.lock()
            .get(local_task_id)
            .and_then(|runs| runs.get(local_run_id))
            .cloned()
    }

    fn remove(&self, local_task_id: &str, local_run_id: &str) -> Option<Arc<Reporter>> {
        let mut runs = self.lock();
        let removed = runs
            .get_mut(local_task_id)
            .and_then(|by_run| by_run.remove(local_run_id));
        if runs.get(local_task_id).is_some_and(HashMap::is_empty) {
            runs.remove(local_task_id);
        }
        removed
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, HashMap<String, Arc<Reporter>>>> {
        self.runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(crate) struct Reporter {
    remote_task_id: String,
    client: ClientProvider,
    state: Mutex<ReporterState>,
    notify: Notify,
}

struct ReporterState {
    project: Projector,
    next_seq: u64,
    usage: UsageAccumulator,
    /// 待刷新用量的 attempt 目录（节点/turn 边界登记，由 worker 统一做文件读取）。
    usage_pending: BTreeSet<String>,
    usage_dirty: bool,
    /// 收到终态信号：做完最后一次 flush + 用量上报后退出。
    closing: bool,
    /// 服务端已不认识该任务（404）：停止一切上报。
    stopped: bool,
    done: Option<oneshot::Sender<()>>,
}

impl Reporter {
    fn new(
        remote_task_id: String,
        client: ClientProvider,
        initial_user_text: Option<&str>,
    ) -> Self {
        Self {
            remote_task_id,
            client,
            state: Mutex::new(ReporterState {
                // 初始需求原话在构造期即占据 outbox 最前部 → 首批 drain 分到 seq=1，
                // 保证「任务开始执行前，初始输入是第一条消息」（multica 回放时序要求）。
                project: Projector::new(initial_user_text),
                // seq 作用域 = task_id；每个 task 的第一次上报从 1 开始，严格递增。
                next_seq: 1,
                usage: UsageAccumulator::default(),
                usage_pending: BTreeSet::new(),
                usage_dirty: false,
                closing: false,
                stopped: false,
                done: None,
            }),
            notify: Notify::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ReporterState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 读取待刷新的 attempt 用量日志（worker 独占，不在 lifecycle 热路径上）。
    ///
    /// 每个 attempt 在每次边界只登记一次，因此这里是「每边界一次小文件读取」，不是轮询式重读。
    /// jsonl 是同步 I/O，放 blocking 线程池执行，不占住 async worker 线程。
    async fn refresh_usage(self: &Arc<Self>, attempt_dirs: &[String]) {
        let dirs = attempt_dirs.to_vec();
        let read = tauri::async_runtime::handle()
            .inner()
            .spawn_blocking(move || {
                let mut accumulated = Vec::with_capacity(dirs.len());
                for dir in &dirs {
                    let segments = read_prompt_usage_segments(Utf8Path::new(dir));
                    if !segments.is_empty() {
                        accumulated.push((dir.clone(), segments));
                    }
                }
                accumulated
            });
        let accumulated = match read.await {
            Ok(accumulated) => accumulated,
            Err(error) => {
                warn!(task = %self.remote_task_id, %error, "multica usage refresh failed");
                Vec::new()
            }
        };
        if accumulated.is_empty() {
            return;
        }
        let mut state = self.lock();
        // closing 也要合并：终态那一拍要带上最后一个边界的用量。
        if state.stopped {
            return;
        }
        let mut merged = false;
        for (dir, segments) in accumulated {
            merged |= state.usage.merge(&dir, segments);
        }
        if merged {
            state.usage_dirty = true;
        }
    }

    fn request_finish(&self) {
        {
            let mut state = self.lock();
            if state.closing {
                return;
            }
            state.closing = true;
        }
        self.notify.notify_one();
    }
}

/// 注册一个新的上报器并启动 flush worker。
///
/// `initial_user_text`：任务初始需求原话（用户的话，非组装后 prompt），作为 seq=1 的
/// 第一条 `user` 消息上报。fresh 传 composer 提交文本；resume 传被续任务 requirement 文件。
pub(crate) fn start_run(
    app: &AppHandle,
    remote_task_id: &str,
    local_task_id: &str,
    local_run_id: &str,
    initial_user_text: Option<&str>,
) {
    let Some(registry) = app.try_state::<TranscriptRegistry>() else {
        return;
    };
    let provider_app = app.clone();
    let client: ClientProvider = Arc::new(move || {
        let context = crate::multica::bridge::desktop_context(&provider_app)?;
        crate::multica::bridge::multica_client(&context)
    });
    let reporter = Arc::new(Reporter::new(
        remote_task_id.to_string(),
        client,
        initial_user_text,
    ));
    registry.insert(local_task_id, local_run_id, reporter.clone());
    tauri::async_runtime::spawn(run_worker(reporter));
}

/// 观测一个 live 事件（root 分支）。非 multica 任务 / 非上报类型直接返回。
pub(crate) fn observe(
    app: &AppHandle,
    context: &gold_band::app::AcpLiveEventContext,
    event: &gold_band::acp::events::AcpUiEvent,
) {
    if !is_reported_event(event) {
        return;
    }
    // 子 agent 分支不属于本次执行的对外时间线（Gold Band 自己也是折叠展示）；
    // 先于 registry 查做过滤，子分支的高频流式事件不必抢注册表锁。
    if gold_band::acp::branches::event_branch_id(event) != gold_band::acp::branches::ROOT_BRANCH_ID
    {
        return;
    }
    let Some(registry) = app.try_state::<TranscriptRegistry>() else {
        return;
    };
    let Some(reporter) = registry.find(&context.task_id, &context.run_id) else {
        return;
    };
    reporter.lock().project.observe(event);
}

fn is_reported_event(event: &gold_band::acp::events::AcpUiEvent) -> bool {
    match event.kind.as_str() {
        "textDelta" | "toolCall" | "toolCallUpdate" | "userTextDelta" => true,
        "thoughtDelta" => super::model::REPORT_THINKING,
        _ => false,
    }
}

/// 上报一条运行期错误（失败/取消）。只在 error 行进缓冲后立即唤醒 worker，不等 500ms 节拍。
pub(crate) fn report_error<R: Runtime>(
    app: &AppHandle<R>,
    local_task_id: &str,
    local_run_id: &str,
    message: &str,
) {
    let Some(registry) = app.try_state::<TranscriptRegistry>() else {
        return;
    };
    let Some(reporter) = registry.find(local_task_id, local_run_id) else {
        return;
    };
    {
        let mut state = reporter.lock();
        if state.closing {
            return;
        }
        state.project.push_error(message);
    }
    reporter.notify.notify_one();
}

/// 节点/turn 边界到达：登记该 attempt 的用量待刷新。
///
/// **不做文件读取**——lifecycle 订阅器回调在热路径上，这里只做一次内存登记；jsonl 由 worker
/// 在下一个 500ms 节拍统一读取并累计（同一 attempt 的重复登记被集合去重）。
/// 幂等：同一 `(attempt_dir, turn_seq)` 只累计一次；上报的是**累计总量**（服务端按
/// `(task_id, provider, model)` 整体替换，不是累加）。
pub(crate) fn accumulate_usage<R: Runtime>(
    app: &AppHandle<R>,
    local_task_id: &str,
    local_run_id: &str,
    attempt_dir: &str,
) {
    let Some(registry) = app.try_state::<TranscriptRegistry>() else {
        return;
    };
    let Some(reporter) = registry.find(local_task_id, local_run_id) else {
        return;
    };
    let mut state = reporter.lock();
    if state.closing {
        return;
    }
    state.usage_pending.insert(attempt_dir.to_string());
}

/// 终态排空：把剩余消息与最终用量送出去，最多等 `timeout`。
///
/// 幂等：同 run 重复调用第二次拿不到 reporter，直接返回。
pub(crate) async fn finish_run<R: Runtime>(
    app: &AppHandle<R>,
    local_task_id: &str,
    local_run_id: &str,
    timeout: Duration,
) {
    let Some(reporter) = app
        .try_state::<TranscriptRegistry>()
        .and_then(|registry| registry.remove(local_task_id, local_run_id))
    else {
        return;
    };
    let (done_tx, done_rx) = oneshot::channel();
    {
        let mut state = reporter.lock();
        if state.closing {
            return;
        }
        state.done = Some(done_tx);
        state.closing = true;
    }
    reporter.notify.notify_one();
    if tokio::time::timeout(timeout, done_rx).await.is_err() {
        warn!(
            task = %reporter.remote_task_id,
            "multica transcript drain timed out; remaining messages dropped"
        );
        return;
    }
    let dropped = reporter.lock().project.dropped_messages();
    if dropped > 0 {
        // 背压上限生效（服务端写入跟不上或任务产出行数过大）——只记录，不重发。
        warn!(task = %reporter.remote_task_id, dropped, "multica transcript dropped messages");
    }
}

async fn run_worker(reporter: Arc<Reporter>) {
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = reporter.notify.notified() => {}
        }

        let (batch, pending_usage, closing, stopped) = {
            let mut guard = reporter.lock();
            let state = &mut *guard;
            // 文本按 flush 节奏成段：不跨批次累积成一个巨块。
            state.project.close_segment();
            let batch = state.project.drain(&mut state.next_seq);
            let pending_usage: Vec<String> = state.usage_pending.iter().cloned().collect();
            state.usage_pending.clear();
            (batch, pending_usage, state.closing, state.stopped)
        };

        if !pending_usage.is_empty() && !stopped {
            reporter.refresh_usage(&pending_usage).await;
        }

        if !batch.is_empty() && !stopped {
            match (reporter.client)() {
                None => {
                    warn!(task = %reporter.remote_task_id, "multica transcript batch dropped: no client");
                }
                Some(client) => {
                    match client
                        .post_task_messages(&reporter.remote_task_id, &batch)
                        .await
                    {
                        Ok(()) => {}
                        Err(MulticaError::TaskNotFound) => {
                            warn!(
                                task = %reporter.remote_task_id,
                                "multica transcript stopped: task not found"
                            );
                            reporter.lock().stopped = true;
                        }
                        Err(error) => {
                            // 单批失败即丢（不重试：重发同 seq 会产生重复行）。
                            warn!(
                                task = %reporter.remote_task_id,
                                count = batch.len(),
                                %error,
                                "multica transcript batch dropped"
                            );
                        }
                    }
                }
            }
        }

        if !stopped {
            report_usage_if_dirty(&reporter).await;
        }

        if closing {
            report_usage(&reporter).await;
            let done = reporter.lock().done.take();
            if let Some(done) = done {
                let _ = done.send(());
            }
            return;
        }
    }
}

async fn report_usage_if_dirty(reporter: &Reporter) {
    let dirty = {
        let mut state = reporter.lock();
        std::mem::take(&mut state.usage_dirty)
    };
    if dirty {
        report_usage(reporter).await;
    }
}

async fn report_usage(reporter: &Reporter) {
    let rows = {
        let state = reporter.lock();
        if state.stopped {
            return;
        }
        state.usage.rows()
    };
    // 无用量行（例如 agent 不输出 usage）就没必要打请求；有行时重报累计值幂等。
    if rows.is_empty() {
        return;
    }
    let Some(client) = (reporter.client)() else {
        return;
    };
    if let Err(error) = client
        .post_task_usage(&reporter.remote_task_id, &rows)
        .await
    {
        warn!(task = %reporter.remote_task_id, %error, "multica usage report dropped");
    }
}

/// token 用量累计器：按 `(provider, model)` 汇总，按 `(attempt_dir, turn_seq)` 去重。
#[derive(Default)]
struct UsageAccumulator {
    seen: HashSet<(String, u64)>,
    totals: BTreeMap<(String, String), TokenSums>,
}

#[derive(Default, Clone, Copy)]
struct TokenSums {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl UsageAccumulator {
    /// 返回是否有新片段被累计。
    fn merge(
        &mut self,
        attempt_dir: &str,
        segments: Vec<gold_band::acp::usage::AcpPromptUsageSegment>,
    ) -> bool {
        let mut merged = false;
        for segment in segments {
            if !self
                .seen
                .insert((attempt_dir.to_string(), segment.turn_seq))
            {
                continue;
            }
            // provider 归一化为 trim+小写：服务端按 (task_id, provider, model) UPSERT 且自动小写
            // provider，若本端按原始大小写分桶，"Anthropic"/"anthropic" 会在服务端落到同一行
            // 先后覆盖 → 少算 token。聚合键必须与服务端归一化一致；model 服务端不归一化，保持原样。
            // provider 为空串 → 服务端从任务 runtime 回填（协议约定），无需本端兜底。
            let key = (
                segment
                    .provider
                    .map(|provider| provider.trim().to_ascii_lowercase())
                    .unwrap_or_default(),
                segment.model.unwrap_or_default(),
            );
            let totals = self.totals.entry(key).or_default();
            totals.input += segment.usage.input_tokens.unwrap_or(0);
            totals.output += segment.usage.output_tokens.unwrap_or(0);
            totals.cache_read += segment.usage.cached_read_tokens.unwrap_or(0);
            totals.cache_write += segment.usage.cached_write_tokens.unwrap_or(0);
            merged = true;
        }
        merged
    }

    fn rows(&self) -> Vec<UsageRow> {
        self.totals
            .iter()
            .filter(|(_, totals)| {
                totals.input + totals.output + totals.cache_read + totals.cache_write > 0
            })
            .map(|((provider, model), totals)| UsageRow {
                provider: provider.clone(),
                model: model.clone(),
                input_tokens: totals.input,
                output_tokens: totals.output,
                cache_read_tokens: totals.cache_read,
                cache_write_tokens: totals.cache_write,
                // 无权威费用：0 让服务端按费率估算（协议约定）。
                cost_usd_ticks: 0,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reporter() -> Arc<Reporter> {
        Arc::new(Reporter::new("task-1".to_string(), Arc::new(|| None), None))
    }

    #[test]
    fn registry_scopes_runs_by_local_task_and_run() {
        let registry = TranscriptRegistry::default();
        let reporter = reporter();
        registry.insert("task-a", "run-1", reporter.clone());
        assert!(registry.find("task-a", "run-1").is_some());
        assert!(registry.find("task-a", "run-2").is_none());
        assert!(registry.find("task-b", "run-1").is_none());
        assert!(registry.remove("task-a", "run-1").is_some());
        assert!(registry.find("task-a", "run-1").is_none());
        assert!(registry.lock().is_empty());
    }

    #[test]
    fn finish_is_idempotent_for_missing_run() {
        let registry = TranscriptRegistry::default();
        assert!(registry.remove("task-a", "run-1").is_none());
    }

    #[test]
    fn usage_accumulates_and_dedupes_by_attempt_turn() {
        let mut accumulator = UsageAccumulator::default();
        let segment =
            |turn_seq: u64, input: u64, output: u64| gold_band::acp::usage::AcpPromptUsageSegment {
                turn_id: format!("turn-{turn_seq}"),
                turn_seq,
                elapsed_ms: None,
                provider: Some("anthropic".to_string()),
                model: Some("claude-sonnet-5-5".to_string()),
                usage: gold_band::acp::usage::AcpPromptTokenUsage {
                    input_tokens: Some(input),
                    output_tokens: Some(output),
                    cached_read_tokens: Some(0),
                    cached_write_tokens: Some(0),
                    total_tokens: None,
                },
            };
        assert!(accumulator.merge("attempt-1", vec![segment(1, 10, 5)]));
        assert!(!accumulator.merge("attempt-1", vec![segment(1, 10, 5)]));
        assert!(accumulator.merge("attempt-1", vec![segment(2, 20, 7)]));
        let rows = accumulator.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, "anthropic");
        assert_eq!(rows[0].input_tokens, 30);
        assert_eq!(rows[0].output_tokens, 12);
        assert_eq!(rows[0].cost_usd_ticks, 0);
    }

    #[test]
    fn usage_rows_split_by_provider_and_model() {
        let mut accumulator = UsageAccumulator::default();
        let segment = |provider: &str, model: &str, input: u64| {
            gold_band::acp::usage::AcpPromptUsageSegment {
                turn_id: "turn".to_string(),
                turn_seq: input,
                elapsed_ms: None,
                provider: Some(provider.to_string()),
                model: Some(model.to_string()),
                usage: gold_band::acp::usage::AcpPromptTokenUsage {
                    input_tokens: Some(input),
                    output_tokens: None,
                    cached_read_tokens: None,
                    cached_write_tokens: None,
                    total_tokens: None,
                },
            }
        };
        accumulator.merge("attempt-1", vec![segment("a", "m1", 1)]);
        accumulator.merge("attempt-2", vec![segment("b", "m2", 2)]);
        let rows = accumulator.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].provider, "a");
        assert_eq!(rows[1].provider, "b");
    }

    /// 服务端按 `(task_id, provider, model)` UPSERT 且自动小写 provider：本端聚合键必须先
    /// 归一化，否则大小写/空白漂移会被拆成多条 entry，服务端同 key 后写胜 → 少算 token。
    #[test]
    fn usage_provider_case_and_space_drift_collapses_into_one_row() {
        let mut accumulator = UsageAccumulator::default();
        let segment = |turn_seq: u64, provider: &str, input: u64| {
            gold_band::acp::usage::AcpPromptUsageSegment {
                turn_id: format!("turn-{turn_seq}"),
                turn_seq,
                elapsed_ms: None,
                provider: Some(provider.to_string()),
                model: Some("claude-sonnet-5-5".to_string()),
                usage: gold_band::acp::usage::AcpPromptTokenUsage {
                    input_tokens: Some(input),
                    output_tokens: None,
                    cached_read_tokens: None,
                    cached_write_tokens: None,
                    total_tokens: None,
                },
            }
        };
        accumulator.merge("attempt-1", vec![segment(1, "Anthropic ", 10)]);
        accumulator.merge("attempt-2", vec![segment(2, "ANTHROPIC", 20)]);
        accumulator.merge("attempt-3", vec![segment(3, "anthropic", 30)]);
        let rows = accumulator.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, "anthropic");
        assert_eq!(rows[0].input_tokens, 60);
    }

    /// provider 缺失 → 空串上报，服务端从任务 runtime 回填（协议约定，本端不做兜底猜测）。
    #[test]
    fn usage_without_provider_is_reported_empty_for_server_backfill() {
        let mut accumulator = UsageAccumulator::default();
        let segment = gold_band::acp::usage::AcpPromptUsageSegment {
            turn_id: "turn".to_string(),
            turn_seq: 1,
            elapsed_ms: None,
            provider: None,
            model: Some("claude-sonnet-5-5".to_string()),
            usage: gold_band::acp::usage::AcpPromptTokenUsage {
                input_tokens: Some(5),
                output_tokens: None,
                cached_read_tokens: None,
                cached_write_tokens: None,
                total_tokens: None,
            },
        };
        assert!(accumulator.merge("attempt-1", vec![segment]));
        let rows = accumulator.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, "");
        assert_eq!(rows[0].input_tokens, 5);
    }

    /// finish 协议：closing 后 worker 必须排空退出并回 done 信号（客户端不可用时也不许卡住）。
    #[tokio::test]
    async fn worker_signals_done_after_closing_tick() {
        let reporter = reporter();
        reporter.lock().project.push_error("boom");
        reporter
            .lock()
            .usage_pending
            .insert("attempt-1".to_string());
        let worker = tokio::spawn(run_worker(reporter.clone()));

        let (done_tx, done_rx) = oneshot::channel();
        {
            let mut state = reporter.lock();
            state.done = Some(done_tx);
            state.closing = true;
        }
        reporter.notify.notify_one();

        tokio::time::timeout(Duration::from_secs(2), done_rx)
            .await
            .expect("worker must signal done")
            .expect("done sender must fire");
        worker.await.expect("worker task must not panic");
        // 错误消息占用了 seq 1，排空后水位前进。
        assert_eq!(reporter.lock().next_seq, 2);
    }
}
