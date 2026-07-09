# Gold-Band ObservabilityBus 技术方案

**状态**: 设计阶段  
**日期**: 2026-06-17  
**关联**: `docs/gold-band/fix-token-metrics-reporting.md`

---

## 1. 动机

当前 metrics 上报的实现存在以下问题：

| 问题 | 影响 |
|---|---|
| orchestrator 中嵌入了 ~50 行 `MetricsEventContext` 构造代码（重复两次） | 主循环可读性下降 |
| orchestrator 需要手动调用 `read_session_tokens()` 读取 token 文件 | 编排层关心了不该关心的关注点 |
| `LastExecutedNode` 携带 `input_tokens`/`output_tokens`/`cache_read_tokens`/`total_tokens` | 领域泄漏 — token 是 metrics 概念，不应出现在运行时持久化数据模型里 |
| `MetricsEvent` + `MetricsEventContext` 两套重叠的结构体 | 概念重复 |
| 新增观察者（如 tracing、audit log）需修改 orchestrator | 违反开闭原则 |
| `app.metrics_callback` 是单订阅者设计 | 扩展性受限 |

## 2. 目标架构

引入 `ObservabilityBus` 作为轻量事件总线，orchestrator 只发布通用 `WorkflowEvent`，metrics 等观察者以订阅者身份独立消费事件。

```
                        ┌─────────────────────────────────────┐
                        │          ObservabilityBus            │
                        │   subscribers: RwLock<Vec<Sub>>      │
                        │                                      │
                        │   subscribe(handler)                 │
                        │   emit(event) → for each sub:         │
                        │     catch_unwind(handler(event))     │
                        └──────────────┬──────────────────────┘
                                       │ emit(WorkflowEvent)
              ┌────────────────────────┼────────────────────────┐
              ▼                        ▼                        ▼
   ┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
   │  MetricsReporter │   │  TraceLogger     │   │  AuditLogger     │
   │  (subscribe)     │   │  (未来扩展)       │   │  (未来扩展)       │
   │                  │   │                  │   │                  │
   │  读取 token      │   │                  │   │                  │
   │  构建 HTTP 请求  │   │                  │   │                  │
   └──────────────────┘   └──────────────────┘   └──────────────────┘

        orchestrator 不感知任何订阅者，只 emit 事件
```

## 3. 详细设计

### 3.1 新增：`ObservabilityBus`

**文件**: `src/app/observability.rs`（新文件）

```rust
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, RwLock};

use crate::app::WorkflowEvent;

/// Lightweight in-process event bus for workflow lifecycle events.
///
/// Subscribers register via [`subscribe`] (typically during app setup, before
/// any workflow starts). The orchestrator publishes via [`emit`]. Each
/// subscriber is invoked inside `catch_unwind` — a panic in one subscriber
/// never affects other subscribers or the orchestrator.
///
/// # Constraints
///
/// - `subscribe()` acquires a write lock. Only call it during setup, not from
///   hot paths or inside a subscriber handler.
/// - Subscriber handlers **must not** call `subscribe()` or `emit()` on the
///   same `ObservabilityBus` instance — the read lock held by `emit()` would
///   deadlock with the write lock needed by `subscribe()`.
///
/// # Thread safety
///
/// `ObservabilityBus` is `Send + Sync`. `emit()` takes a read lock — multiple
/// threads can emit concurrently without blocking each other.
#[derive(Clone, Default)]
pub struct ObservabilityBus {
    subscribers: Arc<RwLock<Vec<Arc<dyn Fn(WorkflowEvent) + Send + Sync>>>>,
}

impl ObservabilityBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a subscriber. Takes a write lock — call during app init,
    /// not during workflow execution.
    pub fn subscribe(&self, handler: Arc<dyn Fn(WorkflowEvent) + Send + Sync>) {
        self.subscribers
            .write()
            .expect("ObservabilityBus subscriber list poisoned; this is a bug")
            .push(handler);
    }

    /// Publish an event to all subscribers. Each subscriber is wrapped in
    /// `catch_unwind`. Takes a read lock for the duration of the call.
    ///
    /// The event is cloned once per subscriber. With the current single
    /// subscriber (metrics), this is one clone per emit — negligible. If
    /// subscriber count grows beyond ~10, consider switching the handler
    /// signature to `Fn(Arc<WorkflowEvent>)` to avoid per-subscriber clones.
    pub fn emit(&self, event: WorkflowEvent) {
        let subs = self
            .subscribers
            .read()
            .expect("ObservabilityBus subscriber list poisoned; this is a bug");
        for sub in subs.iter() {
            let sub = Arc::clone(sub);
            let event = event.clone();
            let _ = panic::catch_unwind(AssertUnwindSafe(move || {
                sub(event);
            }));
        }
    }
}
```

**关键设计决策**：

1. **`RwLock` 而非 channel** — channel 需要后台任务消费，增加复杂度。`RwLock` + 同步调用足够：订阅者内部自行异步化（`tauri::async_runtime::spawn`），`emit()` 立即返回。

2. **同步 emit，订阅者异步化** — metrics subscriber 内部通过 `tauri::async_runtime::spawn` 发送 HTTP 请求，不阻塞 emit 循环。

3. **框架统一 `catch_unwind`** — 消除 orchestrator 中 3 处手工包装，容错逻辑收敛到一处。

4. **cheap clone** — `ObservabilityBus` 内部是 `Arc`，clone 仅增加引用计数。

### 3.2 新增：`WorkflowEvent` 枚举

**文件**: `src/app/mod.rs`（替代现有 `MetricsEvent` + `MetricsEventContext`）

```rust
/// Workflow lifecycle events emitted by the orchestrator.
/// Subscribers (metrics, tracing, audit, etc.) observe these events
/// without the orchestrator knowing who is listening.
///
/// Each event is self-contained — it carries all IDs, metadata, and
/// filesystem paths a subscriber needs, so no separate "context" struct
/// is required.
///
/// Token counts (input/output/cache/total) are NOT included. Subscribers
/// read token data themselves from `attempt_dir/acp.snapshot.json` via
/// `read_session_tokens()`.
#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    /// A node has started executing. The orchestrator is about to invoke
    /// the AI provider. `predecessor` carries the previous node's snapshot
    /// so subscribers can close out prior records.
    NodeStarted {
        // ── IDs (display + UUID) ──
        task_id: String,
        task_uuid: Option<String>,
        run_id: String,
        run_uuid: Option<String>,
        round_id: String,
        round_uuid: Option<String>,
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: Option<String>,
        agent_type: Option<String>,
        started_at: String,
        /// Path to the current node's attempt directory. `None` here
        /// because the attempt dir hasn't been populated yet (node
        /// just started). `NodeCompleted` carries the real path.
        attempt_dir: Option<String>,
        /// The immediately preceding node in this run.
        /// `None` for the first node of a run.
        predecessor: Option<crate::runtime::LastExecutedNode>,
    },

    /// A node has completed execution (the AI provider returned).
    /// The orchestrator has already persisted runtime state.
    NodeCompleted {
        // ── IDs (display + UUID) ──
        task_id: String,
        task_uuid: Option<String>,
        run_id: String,
        run_uuid: Option<String>,
        round_id: String,
        round_uuid: Option<String>,
        node_id: String,
        node_uuid: Option<String>,
        attempt_id: String,
        // ── Metadata ──
        repo_root: String,
        seq: Option<u32>,
        node_name: String,
        agent_type: Option<String>,
        started_at: String,
        finished_at: Option<String>,
        outcome: String, // "SUCCESS" | "FAILED"
        /// Path to this node's attempt directory. Contains
        /// `acp.snapshot.json` with final token counts.
        attempt_dir: String,
    },
}
```

**关键设计决策**：

1. **事件自包含** — 不再需要 `MetricsEventContext`。全部信息在 `WorkflowEvent` 中。

2. **Token 不在此** — 订阅者通过 `attempt_dir` 调用 `read_session_tokens()` 自行读取。

3. **直接用 `LastExecutedNode` 做 predecessor** — 不引入单独的 `PredecessorNode` 结构体。`LastExecutedNode` 去掉 token 字段后本身就是纯粹的 predecessor 描述。两个几乎相同的结构体维护成本高于解耦收益。

### 3.3 修改：`LastExecutedNode`

**文件**: `src/runtime/mod.rs`

```diff
 #[derive(Debug, Clone, Serialize, Deserialize, Default)]
 #[serde(rename_all = "camelCase")]
 pub struct LastExecutedNode {
     pub node_id: String,
     pub uuid: String,
     #[serde(default)]
     pub round_uuid: String,
     pub node_name: String,
     #[serde(default)]
     pub seq: Option<u32>,
     #[serde(default)]
     pub agent_type: Option<String>,
     pub status: String,
     pub started_at: String,
     pub finished_at: Option<String>,
-    pub input_tokens: u64,
-    pub output_tokens: u64,
-    pub cache_read_tokens: u64,
-    pub total_tokens: u64,
+    /// Path to this node's attempt directory. Subscribers read
+    /// `acp.snapshot.json` from here for token counts.
+    #[serde(default)]
+    pub attempt_dir: Option<String>,
 }
```

**变更**：4 个 `u64` token 字段 → 1 个 `Option<String>` attempt_dir。

**持久化兼容性**：
- 旧 `run.json` → 新代码：`input_tokens`/`output_tokens` 等未知字段被 serde 默认忽略；`attempt_dir` 为 `None`。**兼容。**
- 新 `run.json` → 旧代码：`attempt_dir` 被忽略；`input_tokens` 等字段缺失，`u64` 默认 0。token 数据丢失但运行正常。**可降级。**

### 3.4 修改：`App` 结构体

**文件**: `src/app/mod.rs`

```diff
 pub struct App {
     pub paths: GoldBandPaths,
     pub config: RuntimeConfig,
     provider_override: Option<Arc<dyn ProviderAdapter>>,
     acp_live_update: Option<…>,
     acp_session_update: Option<…>,
-    metrics_callback: Option<Arc<dyn Fn(MetricsEventContext, MetricsEvent) + Send + Sync>>,
+    pub observability_bus: ObservabilityBus,
 }
```

**变更**：
- 删除 `metrics_callback`
- 新增 `observability_bus: ObservabilityBus`（内部 `Arc`，cheap clone）
- 删除 `with_metrics_callback()` 方法
- `clone_for_background()` 改为 `observability_bus: self.observability_bus.clone()`
- `with_config()` / `with_provider_config()` 改为 `observability_bus: ObservabilityBus::new()`

### 3.5 修改：`completed_node_snapshot`

**文件**: `src/app/orchestrator.rs`

```diff
 fn completed_node_snapshot(
     round: &RoundState,
     node: &NodeState,
-    input_tokens: u64,
-    output_tokens: u64,
-    cache_read_tokens: u64,
-    total_tokens: u64,
+    attempt_dir: Option<String>,
 ) -> crate::runtime::LastExecutedNode {
     let status = match node.outcome {
         Some(NodeOutcome::Success) => "SUCCESS",
         _ => "FAILED",
     };
     let node_name = /* … 不变 … */;
     let seq = /* … 不变 … */;
     let agent_type = /* … 不变 … */;

     crate::runtime::LastExecutedNode {
         node_id: node.node_id.clone(),
         uuid: node.uuid.clone().unwrap_or_default(),
         round_uuid: round.uuid.clone().unwrap_or_default(),
         node_name,
         seq,
         agent_type,
         status: status.to_string(),
         started_at: node.started_at.clone(),
         finished_at: node.finished_at.clone(),
-        input_tokens,
-        output_tokens,
-        cache_read_tokens,
-        total_tokens,
+        attempt_dir,
     }
 }
```

### 3.6 修改：orchestrator 主循环

**文件**: `src/app/orchestrator.rs` — `drive_from_node_with_initial_session`

#### 改动 A：NodeStarted 位置（替代 ~6286-6337 行）

删除 ~50 行 `MetricsEventContext` 构造 + `catch_unwind(callback)`，替换为：

```rust
// ── Observability: notify node started ──
app.observability_bus.emit(WorkflowEvent::NodeStarted {
    task_id: task_id.to_string(),
    task_uuid: run.task_uuid.clone(),
    run_id: run.id.clone(),
    run_uuid: run.uuid.clone(),
    round_id: round.id.clone(),
    round_uuid: round.uuid.clone(),
    node_id: node.node_id.clone(),
    node_uuid: node.uuid.clone(),
    attempt_id: node.attempt_id.clone(),
    repo_root: app.paths.repo_root.to_string(),
    seq: round.trace.iter()
        .filter(|t| t.node_id == node.node_id)
        .map(|t| t.sequence).last(),
    node_name: node.resolved_config.get("profileName")
        .and_then(|v| v.as_str()).filter(|s| !s.is_empty())
        .or_else(|| node.resolved_config.get("profile").and_then(|v| v.as_str()))
        .map(|s| s.to_string()),
    agent_type: node.resolved_config.get("provider")
        .and_then(|v| v.as_str()).map(|s| s.to_string()),
    started_at: node.started_at.clone(),
    attempt_dir: None, // current node hasn't executed yet
    predecessor: run.last_executed_node.clone(),
});
```

**对比现状**：
- 删除了 `MetricsEventContext`（28 行的 struct literal）
- 删除了 `catch_unwind(AssertUnwindSafe(|| metrics_cb(...)))`（4 行）
- 删除了 `if let Some(metrics_cb) = &app.metrics_callback { ... }` 条件（2 行）
- 替换为直接的 `app.observability_bus.emit(...)`（1 行 + struct 字段）

#### 改动 B：NodeCompleted 构建位置（替代 ~6660-6678 行）

删除 `read_session_tokens` + `catch_unwind` + 重复的 `session_paths` 构建，改为仅构建路径：

```rust
persist_runtime_state(app, task_id, run, round, &node)?;

// Build attempt_dir for both snapshot persistence and observability event
let attempt_dir = app.paths
    .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id)
    .to_string();

let completed_snapshot = completed_node_snapshot(round, &node, Some(attempt_dir.clone()));
let decision = decide_next_step(workflow, run, round, &node);

if let Some(next) = apply_control_decision(/* ... */)? {
    run.last_executed_node = Some(completed_snapshot);
    // … continue loop …
}
```

**关键变化**：不再调用 `read_session_tokens()`。orchestrator 完全脱离 token 文件读取。

#### 改动 C：NodeCompleted 事件（替代 ~6701-6736 行）

删除第二段 `MetricsEventContext` 构造 + `catch_unwind(callback)`，替换为：

```rust
// Workflow ended — emit completed event for observability subscribers
run.last_executed_node = Some(completed_snapshot.clone());
app.observability_bus.emit(WorkflowEvent::NodeCompleted {
    task_id: task_id.to_string(),
    task_uuid: run.task_uuid.clone(),
    run_id: run.id.clone(),
    run_uuid: run.uuid.clone(),
    round_id: round.id.clone(),
    round_uuid: round.uuid.clone(),
    node_id: node.node_id.clone(),
    node_uuid: node.uuid.clone(),
    attempt_id: node.attempt_id.clone(),
    repo_root: app.paths.repo_root.to_string(),
    seq: completed_snapshot.seq,
    node_name: completed_snapshot.node_name.clone(),
    agent_type: completed_snapshot.agent_type.clone(),
    started_at: node.started_at.clone(),
    finished_at: node.finished_at.clone(),
    outcome: completed_snapshot.status.clone(),
    attempt_dir, // owned String from above, no clone needed
});
```

### 3.7 修改：Metrics 订阅者

**文件**: `src-tauri/src/metrics.rs`

删除整个 `create_metrics_callback` 函数（第 371-610 行），新增 `create_metrics_subscriber`：

```rust
use gold_band::app::WorkflowEvent;
use gold_band::acp::events::read_session_tokens;

/// Create a metrics subscriber for the ObservabilityBus.
/// Replaces the old `create_metrics_callback`.
///
/// The subscriber is called **synchronously** by the bus, so it does the
/// minimum work needed (settings lookup, token read, metric construction)
/// and then delegates the HTTP request to `tauri::async_runtime::spawn`.
pub fn create_metrics_subscriber<R: Runtime>(
    app: AppHandle<R>,
) -> Arc<dyn Fn(WorkflowEvent) + Send + Sync> {
    Arc::new(move |event: WorkflowEvent| {
        // ── Guard: settings check (same as before) ──
        let settings = match app.try_state::<DesktopState>() {
            Some(state) => match state.context() {
                Ok(ctx) => metrics_settings(&ctx.config),
                Err(_) => return,
            },
            None => return,
        };
        if !settings.enabled {
            return;
        }
        let node_metrics_endpoint = match &settings.node_metrics_endpoint {
            Some(ep) => ep.clone(),
            None => return,
        };
        let api_key = match app.try_state::<DesktopState>() {
            Some(state) => match state.context() {
                Ok(ctx) => match get_api_key(&ctx.config) {
                    Some(k) => k,
                    None => return,
                },
                Err(_) => return,
            },
            None => return,
        };

        let user_id = get_system_username();
        let reported_at = chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();

        match event {
            WorkflowEvent::NodeStarted {
                repo_root,
                task_id,
                task_uuid,
                run_id,
                run_uuid,
                round_id,
                round_uuid,
                node_id,
                node_uuid,
                attempt_id,
                seq,
                node_name,
                agent_type,
                started_at,
                predecessor,
                ..
            } => {
                let attempt_count = attempt_id
                    .strip_prefix("attempt-")
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(0)
                    .saturating_sub(1);

                let node_status = if attempt_count > 0 {
                    "Reentrancy".to_string()
                } else {
                    "RUNNING".to_string()
                };

                // ── Build predecessor metric ──
                let predecessor_item = match &predecessor {
                    Some(pred) => {
                        // Read predecessor tokens from ITS attempt_dir
                        let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
                            pred.attempt_dir.as_ref().map(|d| {
                                // Construct path: <attempt_dir>/acp.session.json
                                // read_session_tokens uses .parent() to find acp.snapshot.json
                                let path = std::path::Utf8PathBuf::from(d).join("acp.session.json");
                                gold_band::acp::events::read_session_tokens(&path)
                            }).unwrap_or((0, 0, 0, 0));

                        NodeMetricItem {
                            workspace: repo_root.clone(),
                            user_id: user_id.clone(),
                            task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                            run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                            round_id: pred.round_uuid.clone(),
                            node_id: pred.uuid.clone(),
                            seq: pred.seq,
                            node_name: Some(pred.node_name.clone()),
                            agent_type: pred.agent_type.clone(),
                            attempt_count: 0,
                            started_at: Some(to_iso8601(&pred.started_at)),
                            ended_at: pred.finished_at.as_ref().map(|s| to_iso8601(s)),
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            total_tokens,
                            status: pred.status.clone(),
                            reported_at: Some(reported_at.clone()),
                        }
                    }
                    None => start_sentinel_metric(
                        &repo_root, &user_id,
                        &task_uuid.clone().unwrap_or(task_id.clone()),
                        &run_uuid.clone().unwrap_or(run_id.clone()),
                        &round_uuid.clone().unwrap_or(round_id.clone()),
                        &to_iso8601(&started_at),
                        &reported_at,
                    ),
                };

                // ── Build current node metric (token=0 — hasn't executed yet) ──
                let current = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
                    node_id: node_uuid.clone().unwrap_or(node_id.clone()),
                    seq,
                    node_name: node_name.clone(),
                    agent_type: agent_type.clone(),
                    attempt_count,
                    started_at: Some(to_iso8601(&started_at)),
                    ended_at: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    status: node_status,
                    reported_at: Some(reported_at.clone()),
                };

                let batch = NodeMetricBatch {
                    metrics: vec![predecessor_item, current],
                };

                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                });
            }

            WorkflowEvent::NodeCompleted {
                repo_root,
                task_id,
                task_uuid,
                run_id,
                run_uuid,
                round_id,
                round_uuid,
                node_id,
                node_uuid,
                attempt_id: _,
                seq,
                node_name,
                agent_type,
                started_at,
                finished_at,
                outcome,
                attempt_dir,
            } => {
                // Read tokens from this node's attempt_dir
                let (input_tokens, output_tokens, cache_read_tokens, total_tokens) = {
                    let path = std::path::Utf8PathBuf::from(&attempt_dir)
                        .join("acp.session.json");
                    read_session_tokens(&path)
                };

                let last_node = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
                    node_id: node_uuid.clone().unwrap_or(node_id.clone()),
                    seq,
                    node_name: Some(node_name.clone()),
                    agent_type: agent_type.clone(),
                    attempt_count: 0,
                    started_at: Some(to_iso8601(&started_at)),
                    ended_at: finished_at.as_ref().map(|s| to_iso8601(s)),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    total_tokens,
                    status: outcome.clone(),
                    reported_at: Some(reported_at.clone()),
                };

                // End sentinel: token=0, always
                let end_sentinel = NodeMetricItem {
                    workspace: repo_root.clone(),
                    user_id: user_id.clone(),
                    task_id: task_uuid.clone().unwrap_or(task_id.clone()),
                    run_id: run_uuid.clone().unwrap_or(run_id.clone()),
                    round_id: round_uuid.clone().unwrap_or(round_id.clone()),
                    node_id: uuid::Uuid::new_v4().simple().to_string(),
                    seq: None,
                    node_name: Some("结束".to_string()),
                    agent_type: None,
                    attempt_count: 0,
                    started_at: finished_at
                        .as_ref()
                        .map(|s| to_iso8601(s))
                        .unwrap_or_else(|| reported_at.clone()),
                    ended_at: Some(reported_at.clone()),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    status: outcome,
                    reported_at: Some(reported_at.clone()),
                };

                let batch = NodeMetricBatch {
                    metrics: vec![last_node, end_sentinel],
                };

                tauri::async_runtime::spawn(async move {
                    send_node_metrics_batch(&node_metrics_endpoint, &api_key, batch).await;
                });
            }
        }
    })
}
```

**与当前实现的差异**：

| 维度 | 当前 `create_metrics_callback` | 新 `create_metrics_subscriber` |
|---|---|---|
| 入参 | `(MetricsEventContext, MetricsEvent)` | `(WorkflowEvent)` — 单一事件，自包含 |
| Settings guard | 两个 handler 各自检查 | 函数入口统一检查一次 |
| Token 读取 | `NodeCompleted` 通过 `ctx.acp_session_path`；`NodeStarted` 从 `predecessor.input_tokens`（一直是 0） | 两个分支都通过 `attempt_dir` 读 `read_session_tokens` |
| HTTP 发送 | `tauri::async_runtime::spawn` | 相同 |
| 行数 | ~240 行 | ~180 行（含注释） |
| catch_unwind | `read_tokens_best_effort` 内部 + orchestrator 外部 | bus 框架保证，不再需要 `read_tokens_best_effort`，直接调 `read_session_tokens` |

### 3.8 修改：调用方

**文件**: `src-tauri/src/commands.rs`（3 处）、`commands_conversation.rs`（2 处）、`state.rs`（1 处）

```diff
-let app = base_app
-    .with_metrics_callback(crate::metrics::create_metrics_callback(app_handle.clone()));
+let app = base_app;
+app.observability_bus
+    .subscribe(crate::metrics::create_metrics_subscriber(app_handle.clone()));
```

调用方变化数：6 处，每处 +2/-1 行。

### 3.9 删除项汇总

| 删除项 | 位置 | 说明 |
|---|---|---|
| `MetricsEventContext` 结构体 | `src/app/mod.rs:426-454` | 被 `WorkflowEvent` 取代 |
| `MetricsEvent` 枚举 | `src/app/mod.rs:456-465` | 被 `WorkflowEvent` 取代 |
| `App.metrics_callback` 字段 | `src/app/mod.rs:477` | 被 `observability_bus` 取代 |
| `App::with_metrics_callback()` | `src/app/mod.rs:693-698` | 改为 `bus.subscribe()` |
| `App::clone_for_background()` 中 metrics_callback clone | `src/app/mod.rs:671` | 改为 `self.observability_bus.clone()` |
| `create_metrics_callback` 函数 | `src-tauri/src/metrics.rs:371-610` | 被 `create_metrics_subscriber` 取代 |
| orchestrator 中两段 `MetricsEventContext` 构造 | `src/app/orchestrator.rs:6287-6337, 6701-6736` | 替换为 `bus.emit()` |
| orchestrator 中 `read_session_tokens` + `catch_unwind` | `src/app/orchestrator.rs:6663-6673` | 不必要 |
| `completed_node_snapshot` 的 4 个 token 参数 | `src/app/orchestrator.rs` | 改为 1 个 `attempt_dir` |
| `read_tokens_best_effort` 调用 | `src-tauri/src/metrics.rs` | 改为直接调 `read_session_tokens`（bus 已提供 catch_unwind） |

## 4. 文件变更清单

| 文件 | 操作 | 估计行数 |
|---|---|---|
| `src/app/observability.rs` | **新增** | +50 |
| `src/app/mod.rs` | 修改 | +50 / -45 |
| `src/app/orchestrator.rs` | 修改 | +50 / -75（净 -25） |
| `src/runtime/mod.rs` | 修改 | +3 / -6 |
| `src-tauri/src/metrics.rs` | 修改 | +180 / -250（净 -70） |
| `src-tauri/src/commands.rs` | 修改 | +12 / -6 |
| `src-tauri/src/commands_conversation.rs` | 修改 | +6 / -3 |
| `src-tauri/src/state.rs` | 修改 | +3 / -3 |
| **总计** | | **+354 / -388（净 -34 行）** |

## 5. 迁移步骤

按依赖顺序执行，每步 `cargo check` 编译验证：

| 步骤 | 内容 | 影响范围 | 验证 |
|---|---|---|---|
| 1 | 新建 `src/app/observability.rs` | 无依赖 | `cargo check` |
| 2 | `src/app/mod.rs`：定义 `WorkflowEvent`；`App` 用 `observability_bus` 替换 `metrics_callback`；删除 `MetricsEvent`/`MetricsEventContext` | 步骤 1 | `cargo check`（此时 metrics.rs 引用旧类型会报错，预期行为） |
| 3 | `src/runtime/mod.rs`：`LastExecutedNode` token → `attempt_dir`；`src/app/orchestrator.rs`：`completed_node_snapshot` 参数修改 + orchestrator 用 `bus.emit()` 替换旧回调 | 步骤 2 | `cargo check` |
| 4 | `src-tauri/src/metrics.rs`：`create_metrics_subscriber` 替换 `create_metrics_callback` | 步骤 2, 3 | `cargo check` |
| 5 | `src-tauri/src/commands.rs`、`commands_conversation.rs`、`state.rs`：`subscribe()` 替换 `with_metrics_callback()` | 步骤 4 | `cargo check` |
| 6 | 端到端验证：执行 workflow，检查 `NodeStarted` predecessor token、`NodeCompleted` last_node token 为非 0 | 全部 | 手动测试 |

## 6. 风险评估

| 风险 | 等级 | 缓解措施 |
|---|---|---|
| `LastExecutedNode` 序列化兼容旧 `run.json` | **低** | serde 默认忽略未知字段；`attempt_dir: Option<String>` 有 `#[serde(default)]` → `None` |
| `RwLock` 死锁（subscriber 内调 subscribe/emit） | **极低** | 文档化约束；当前 subscriber 不调这些方法 |
| subscriber 丢失 settings guard 导致指标禁用时仍发请求 | **已消除** | subscriber 入口统一检查 settings（见修正 2） |
| `read_tokens_best_effort` 改为 `read_session_tokens` 后丢失 `catch_unwind` | **已消除** | bus 框架在 emit 循环中统一 `catch_unwind` |
| 事件 clone 开销 | **低** | 单 subscriber（当前）1 次 clone/emit；工作流级别事件（每节点 1-2 次），非热路径 |
| 迁移中遗漏某个调用方 | **低** | 步骤 2 删除 `with_metrics_callback` 后，遗漏的调用方立即编译报错 |

## 7. 对工作流执行的影响

**无影响。** 执行模型完全相同：

```
当前:  orchestrator → catch_unwind → callback(ctx, event)
                           └→ try_state → enabled? → spawn HTTP

Bus:   orchestrator → bus.emit(event)
                           └→ RwLock::read()
                               └→ catch_unwind → subscriber(event)
                                     └→ try_state → enabled? → spawn HTTP
```

- 同步调用 → 同步调用（无新增 async 边界）
- `catch_unwind` 存在 → `catch_unwind` 存在（位置从 orchestrator 移到 bus 框架）
- `spawn` HTTP → `spawn` HTTP（不变）
- 额外开销：一次 `RwLock::read()` + 一次闭包间接调用（纳秒级）

## 8. 与当前实现的对比

| 维度 | 当前（token fix 之后） | Bus 模式 |
|---|---|---|
| orchestrator 中 metrics 相关代码 | ~50 行 `MetricsEventContext` 构造 | **0 行** |
| Token 读取位置 | orchestrator（`read_session_tokens` + `catch_unwind`） | **metrics subscriber 内部** |
| `LastExecutedNode` token 字段 | 4 个 `u64`（领域泄漏） | **0 个**，改为 `attempt_dir: Option<String>` |
| 容错机制 | 3 处手工 `catch_unwind` | **框架统一保证** |
| 新增观察者代价 | 修改 `App` 结构体 + orchestrator 主循环 | **`bus.subscribe()` 一行** |
| 结构体数量 | 5 个 | 2 个（`WorkflowEvent` + `NodeMetricItem`） |
| 测试性 | callback 耦合 `AppHandle` | subscriber 依赖 `WorkflowEvent` + `AppHandle`（不变） |
| 净代码量 | — | **-34 行** |

---


---

## 4. 补充：UUID 字段对齐与哨兵策略澄清（2026-06-29）

### 4.1 问题

指标统计实践中发现的数据缺陷，根因来自「隐藏 UUID 字段」在设计覆盖上的遗漏：

| 现象 | 根因 |
|---|---|
| `taskId` 上报为 `task-001` 等业务编号 | `TaskState::new()` 不生成 uuid；旧 task.json 无 uuid 字段，上报层回退到业务 ID |
| AI-DYNAMIC 子节点 `nodeId` 为 `bootstrap`/`morning-class` 等 slug | `DynamicNodeState` 结构本身缺少 `uuid` 字段，`emit_dynamic_worker_completed` 硬编码 `node_uuid: None`，回退到业务 slug |

注：方案节点（外层 AiDynamic 节点）`NodeStarted` 行的 token 为 0 是正确语义——节点刚开始时尚未产生 token，并非缺陷。

### 4.2 数据结构修复

**`DynamicNodeState`（src/dynamic.rs）新增 `uuid` 字段**：

```rust
#[serde(default)]
pub uuid: Option<String>,
```

所有 `DynamicNodeState` 生产构造点（bootstrap / 动态 worker / merge / acceptance）统一赋值 `uuid: Some(generate_uuid())`；测试 helper 赋 `None` 以保持确定性。`emit_dynamic_worker_completed` 的 `node_uuid` 由硬编码 `None` 改为 `node.uuid.clone()`。

**`TaskState::new()`（src/runtime/mod.rs）从源头生成 uuid**：

```rust
impl TaskState {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            // ...
            uuid: Some(Uuid::new_v4().simple().to_string()),
        }
    }
}
```

`runtime` 为底层模块，不能反向依赖 `app::ids::generate_uuid`，故直接在 runtime 内使用 `uuid` crate（与 `app::ids::generate_uuid` 保持 `simple()` 格式一致）。新创建的 task 不再产生 `task-001` 兜底；旧 task.json 仍由 `#[serde(default)]` 容错为 `None`。

### 4.3 哨兵策略澄清（一个 AUTO 流程只有一对开始/结束）

**设计原则**：一个 AUTO 流程（即一个 AiDynamic 节点及其内部所有子节点）整体只产生**一对**「开始/结束」哨兵行，由外层 AiDynamic 节点唯一负责。内部子节点（bootstrap / worker / merge / acceptance）**不各自**上报开始/结束哨兵。

实现机制（原有设计，本次未改动该部分）：

- **外层 AiDynamic 节点**的 `NodeStarted`：其 `predecessor` 为 None（或为上一个工作流节点），走正常的开始哨兵 / 前序节点逻辑。
- **外层 AiDynamic 节点**的 `NodeCompleted`：`suppress_sentinel = false`，产出「结束」哨兵。
- **内部子节点**的 `NodeCompleted`：`suppress_sentinel = true`，只产出该节点自身的一条完成态业务记录（SUCCESS/FAILED，携带 token），**不**产出「结束」哨兵。
- **内部子节点不发布 `NodeStarted` 事件**，因此**不**产出 RUNNING 行，也**不**产出「开始」哨兵。

结果：一个 AUTO 流程的指标数据形态为——1 条开始哨兵 + 1 条外层节点 RUNNING + 外层节点完成态 + 各子节点完成态（每子节点 1 条）+ 1 条结束哨兵。子节点各自只有一条完成态记录，不会因为节点数量增加而放大开始/结束哨兵。

### 4.4 不做的事

- 不迁移旧数据（符合开发阶段破坏式更新原则），`#[serde(default)]` 仅用于避免读取旧文件崩溃。
- 不给 AI-DYNAMIC 子节点发布 `NodeStarted`（曾尝试引入，会破坏上述哨兵策略，已回退）。