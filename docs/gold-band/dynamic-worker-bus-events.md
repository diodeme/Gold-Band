# Dynamic Worker Bus 事件上报方案

**状态**: 设计阶段  
**日期**: 2026-06-17  
**关联**: 
- `docs/gold-band/observability-bus-design.md` — ObservabilityBus 架构
- `docs/gold-band/fix-token-metrics-reporting.md` — Token 上报修复

---

## 1. 背景

ObservabilityBus 重构后，外层 `Worker` 节点的 token 上报已恢复正常。但会话 auto 模式使用 `AiDynamic` 节点，其内部的 dynamic worker（bootstrap、worker、acceptance）不在 Bus 上发事件，导致：

- **AiDynamic 外层节点** `NodeCompleted` 上报的 token 始终为 0（它自己不跑 ACP）
- **Dynamic worker** 有 token 数据（跑了 ACP）但不上报

本方案让 dynamic worker 也通过 `ObservabilityBus` 发射 `NodeStarted` / `NodeCompleted` 事件。

## 2. 当前 Dynamic Worker 执行流程

```
drive_from_node_with_initial_session()
  └─ execute_ai_dynamic_node()
       └─ drive_dynamic_graph()
            └─ launch_ready_dynamic_nodes()
                 └─ thread::spawn {
                      execute_dynamic_node_job(app, task_id, run_id, round_id, ...)
                        └─ execute_dynamic_worker(ctx, graph, node)
                             └─ ctx.app.provider_for_id(...).run_worker_with_callbacks(invocation)
                                  └─ client::run_prompt()
                                       └─ write_session() → acp.snapshot.json 写入 token
                        → 返回 DynamicExecutionResult { node, proposals }
                        → tx.send(DynamicExecutionMessage { node_id, result })
                      }
            └─ rx.recv() → apply_dynamic_execution_message()
```

关键约束：
- Dynamic worker 在 `thread::spawn` 的独立线程中运行
- 结果通过 `mpsc::channel` 传回主图循环
- `DynamicExecutionContext` 已持有 `app: &App`，其中 `app.observability_bus` 可直接使用
- `execute_dynamic_node_job` 通过 `app.clone_for_background()` 获得 App 引用，Bus 通过 `Arc` 共享

## 3. 目标架构

```
execute_dynamic_node_job(app, task_id, run_id, ...)
  │
  ├── emit NodeStarted (predecessor=None, status=RUNNING)
  │
  ├── execute_dynamic_worker(ctx, graph, node)
  │     └── provider.run_worker_with_callbacks()
  │           └── ACP session → acp.snapshot.json
  │
  └── emit NodeCompleted (outcome, attempt_dir → subscriber 自读 token)
```

每个 dynamic worker 独立上报一条 `NodeStarted` + `NodeCompleted` 批次。

## 4. 详细设计

### 4.1 新增：dynamic worker event 构建函数

**文件**: `src/app/orchestrator.rs`

```rust
/// Emit a NodeStarted event for a dynamic worker.
fn emit_dynamic_worker_started(
    app: &App,
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
) {
    let attempt_dir = app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id, ctx.run_id, ctx.round_id,
            ctx.outer_node_id, ctx.outer_attempt_id,
            &node.id, &dynamic_attempt_id(node),
        )
        .to_string();

    app.observability_bus.emit(WorkflowEvent::NodeStarted {
        task_id: ctx.task_id.to_string(),
        task_uuid: None,           // dynamic workers don't have UUIDs
        run_id: ctx.run_id.to_string(),
        run_uuid: None,
        round_id: ctx.round_id.to_string(),
        round_uuid: None,
        node_id: node.id.clone(),
        node_uuid: None,
        attempt_id: dynamic_attempt_id(node),
        repo_root: app.paths.repo_root.to_string(),
        seq: None,                 // dynamic workers don't have round trace
        node_name: Some(node.title.clone()),
        agent_type: node.provider.clone(),
        started_at: node.started_at.clone().unwrap_or_else(now_rfc3339_like),
        attempt_dir: Some(attempt_dir),
        predecessor: None,         // independent workers within dynamic graph
    });
}

/// Emit a NodeCompleted event for a dynamic worker.
fn emit_dynamic_worker_completed(
    app: &App,
    ctx: &DynamicExecutionContext<'_>,
    node: &DynamicNodeState,
) {
    let attempt_dir = app
        .paths
        .dynamic_node_attempt_dir(
            ctx.task_id, ctx.run_id, ctx.round_id,
            ctx.outer_node_id, ctx.outer_attempt_id,
            &node.id, &dynamic_attempt_id(node),
        )
        .to_string();

    let outcome = match node.outcome {
        Some(NodeOutcome::Success) => "SUCCESS",
        _ => "FAILED",
    };

    app.observability_bus.emit(WorkflowEvent::NodeCompleted {
        task_id: ctx.task_id.to_string(),
        task_uuid: None,
        run_id: ctx.run_id.to_string(),
        run_uuid: None,
        round_id: ctx.round_id.to_string(),
        round_uuid: None,
        node_id: node.id.clone(),
        node_uuid: None,
        attempt_id: dynamic_attempt_id(node),
        repo_root: app.paths.repo_root.to_string(),
        seq: None,
        node_name: node.title.clone(),
        agent_type: node.provider.clone(),
        started_at: node.started_at.clone().unwrap_or_default(),
        finished_at: node.finished_at.clone(),
        outcome: outcome.to_string(),
        attempt_dir,
    });
}
```

### 4.2 修改：在 execute_dynamic_node_job 中嵌入事件发射

**文件**: `src/app/orchestrator.rs`

```diff
 fn execute_dynamic_node_job(
     app: &App,
     task_id: &str,
     run_id: &str,
     round_id: &str,
     outer_node_id: &str,
     outer_attempt_id: &str,
     dynamic: &AiDynamicNode,
     node: DynamicNodeState,
 ) -> Result<DynamicExecutionResult> {
     // … load run/graph, build ctx …

+    // ── Observability: notify dynamic worker started ──
+    emit_dynamic_worker_started(app, &ctx, &node);

     match node.kind {
         DynamicNodeKind::Worker => {
-            execute_dynamic_worker(&ctx, &graph, node)
+            let result = execute_dynamic_worker(&ctx, &graph, node)?;
+            // ── Observability: notify dynamic worker completed ──
+            emit_dynamic_worker_completed(app, &ctx, &result.node);
+            Ok(result)
         }
         DynamicNodeKind::WorkflowInvocation => {
-            execute_dynamic_workflow_invocation(&ctx, &graph, node)
+            let result = execute_dynamic_workflow_invocation(&ctx, &graph, node)?;
+            emit_dynamic_worker_completed(app, &ctx, &result.node);
+            Ok(result)
         }
         DynamicNodeKind::Merge | DynamicNodeKind::Acceptance => {
-            execute_dynamic_agent_stage(&ctx, &graph, node)
+            let result = execute_dynamic_agent_stage(&ctx, &graph, node)?;
+            emit_dynamic_worker_completed(app, &ctx, &result.node);
+            Ok(result)
         }
     }
 }
```

**关键点**：
- `NodeStarted` 在 worker 开始前发射（无论类型）
- `NodeCompleted` 在 worker 完成后发射（无论成功/失败）
- 失败路径：如果 `execute_dynamic_worker` 返回 `Err`，`emit_dynamic_worker_completed` 不会被调用。这是正确的——失败的 worker 不应该报告为 COMPLETED。可以在错误路径中单独发射一个 FAILED 事件，但当前版本跳过（失败的 worker 会通过 `pause` 路径重试）

### 4.3 处理错误路径

如果 worker 执行失败（`execute_dynamic_worker` 返回 `Err`），我们应该报告 FAILED 事件以保持一致性：

```rust
match node.kind {
    DynamicNodeKind::Worker => {
        match execute_dynamic_worker(&ctx, &graph, node) {
            Ok(result) => {
                emit_dynamic_worker_completed(app, &ctx, &result.node);
                Ok(result)
            }
            Err(e) => {
                // Emit FAILED event — node didn't complete but we still record it
                let failed_node = DynamicNodeState {
                    outcome: Some(NodeOutcome::Failure),
                    finished_at: Some(now_rfc3339_like()),
                    ..node.clone()
                };
                emit_dynamic_worker_completed(app, &ctx, &failed_node);
                Err(e)
            }
        }
    }
    // … same pattern for other kinds …
}
```

### 4.4 调用方无需修改

`ObservabilityBus` 已通过 `App` 传递到 `execute_dynamic_node_job`。无需修改调用链签名。

### 4.5 Settings check 在 subscriber 中

`create_metrics_subscriber` 已有统一的 settings guard（enabled → endpoint → api_key）。Dynamic worker 的 events 也会经过同样的 check——如果指标被禁用，event 到达 subscriber 后立即 return，零开销。

## 5. 数据流总览

```
drive_from_node_with_initial_session (AiDynamic outer node)
  ├── emit NodeStarted (outer)
  │     predecessor = previous node's LastExecutedNode
  │     attempt_dir = outer node's attempt dir (no ACP data)
  │
  ├── execute_ai_dynamic_node()
  │     │
  │     ├── dynamic_worker_1 (bootstrap)
  │     │     ├── emit NodeStarted (inner) ← NEW
  │     │     ├── execute_dynamic_worker → ACP session → acp.snapshot.json
  │     │     └── emit NodeCompleted (inner) ← NEW
  │     │           attempt_dir = dynamic_node_attempt_dir(..., "bootstrap", "attempt-001")
  │     │           subscriber → read_session_tokens() → real tokens!
  │     │
  │     ├── dynamic_worker_2
  │     │     ├── emit NodeStarted (inner) ← NEW
  │     │     ├── execute_dynamic_worker → ACP session → acp.snapshot.json
  │     │     └── emit NodeCompleted (inner) ← NEW
  │     │
  │     └── dynamic_worker_N (acceptance)
  │           └── ...
  │
  └── emit NodeCompleted (outer)
        attempt_dir = outer node's attempt dir (no ACP data → token=0, correct)
```

### 指标上报效果

**修复前**（当前）：
```
batch[0] = AiDynamic 外层节点 started (token=0, RUNNING)
batch[1] = AiDynamic 外层节点 completed (token=0, SUCCESS)  ← 假数据
```

**修复后**：
```
batch[0] = AiDynamic 外层节点 started    (token=0, RUNNING)
batch[1] = AiDynamic bootstrap started    (token=0, RUNNING)      ← NEW
batch[2] = AiDynamic bootstrap completed  (token=39781, SUCCESS)  ← NEW, 真实 token
batch[3] = AiDynamic worker-1 started     (token=0, RUNNING)      ← NEW
batch[4] = AiDynamic worker-1 completed   (token=12653, SUCCESS)  ← NEW, 真实 token
batch[5] = AiDynamic acceptance started   (token=0, RUNNING)      ← NEW
batch[6] = AiDynamic acceptance completed (token=5192,  SUCCESS)  ← NEW, 真实 token
batch[7] = AiDynamic 外层节点 completed   (token=0, SUCCESS)
```

## 6. 文件变更清单

| 文件 | 操作 | 行数 |
|---|---|---|
| `src/app/orchestrator.rs` | 新增 `emit_dynamic_worker_started` / `emit_dynamic_worker_completed` 两个函数 + 修改 `execute_dynamic_node_job` | +80 |
| **总计** | | **+80** |

仅一个文件！不需要改任何调用方或数据结构。

## 7. 风险评估

| 风险 | 等级 | 缓解 |
|---|---|---|
| Dynamic worker 产生大量事件 | **低** | 每个 AiDynamic graph 只有 3-10 个 worker，事件量与人工 workflow 的节点数相当 |
| `NodeStarted`/`NodeCompleted` 成对不匹配（错误路径漏发） | **中** | 在 `Err` 路径中也发射 FAILED 事件（见 4.3）。worker 重试会重新发 NodeStarted |
| `attempt_dir` 路径构建错误 | **低** | 使用已有的 `dynamic_node_attempt_dir` 函数（已验证可用），`read_session_tokens` 通过 `.parent().join("acp.snapshot.json")` 解析 |
| `dynamic_attempt_id` 硬编码 "attempt-001" | **低** | 当前只有一次尝试；如果未来支持重试，`dynamic_attempt_id` 会更新，emit 自动跟随 |
| Bus 跨线程 emit（dynamic worker 在 `thread::spawn` 中） | **极低** | `ObservabilityBus` 是 `Send + Sync`，`emit()` 使用 `RwLock::read()` |

## 8. 验证方法

1. 编译通过：`cargo check --manifest-path src-tauri/Cargo.toml`
2. 从会话页面发起一个 auto 模式任务
3. 检查指标日志 `[node-metrics]` 中是否有 dynamic worker 的 `NodeStarted` / `NodeCompleted` 事件
4. 检查 `NodeCompleted` 事件的 `inputTokens`/`outputTokens`/`totalTokens` 是否为非 0
5. 对比 dynamic worker 的 `acp.snapshot.json` 中的值与上报值
