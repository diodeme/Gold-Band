# 修复指标上报 Token 数据为 0 的方案

## 问题现象

客户端在进行指标上报时，`NodeStarted` 和 `NodeCompleted` 事件中的 token 数据（`inputTokens`、`outputTokens`、`cacheReadTokens`、`totalTokens`）全部为 0。

## 根因分析

### 问题引入

提交 `2135ee9`（`fix(runtime): keep metrics off workflow control path`，2026-06-11，作者 diodeme）在重构 orchestrator 代码时，将原本正常工作的 token 读取逻辑替换成了硬编码 0。

### 数据流对比

**重构前（token 正常工作）**：

```
ACP adapter session/prompt response
  → acp.snapshot.json (持久化)
  → read_session_tokens() 读取真实 token
  → LastExecutedNode { input_tokens, output_tokens, ... }  ← 真实值
  → MetricsEventContext { input_tokens, output_tokens, ... } ← 真实值
  → 指标上报 ← 真实值
```

**重构后（token 全为 0）**：

```
ACP adapter session/prompt response
  → acp.snapshot.json (持久化) ← 仍然写入成功，但不再被读取
  → completed_node_snapshot(round, &node, 0, 0, 0, 0) ← 硬编码 0
  → LastExecutedNode { input_tokens: 0, ... } ← 全为 0
  → MetricsEventContext { input_tokens: 0, ... } ← 全为 0
  → 指标上报 ← 全为 0
```

### 涉及文件

| 文件 | 问题行 | 说明 |
|---|---|---|
| `src/app/orchestrator.rs` | ~6663 | `completed_node_snapshot(round, &node, 0, 0, 0, 0)` 硬编码 0 |
| `src/app/orchestrator.rs` | ~6322-6325 | `NodeStarted` 的 `MetricsEventContext` 中 token 硬编码 0 |
| `src/app/orchestrator.rs` | ~6709-6712 | `NodeCompleted` 的 `MetricsEventContext` 中 token 硬编码 0 |
| `src-tauri/src/metrics.rs` | ~445-448 | `NodeStarted` handler 中当前节点 token 硬编码 0 |
| `src-tauri/src/metrics.rs` | ~596-599 | `NodeCompleted` handler 中 end sentinel token 硬编码 0（这个是设计如此，sentinel 不需要 token） |

### 注意：acp.snapshot.json 仍然正常写入

Token 数据在 ACP adapter 层仍然被正确采集并持久化到 `acp.snapshot.json`（`src/acp/client.rs:1195-1237`），只是 orchestrator 不再读取它。

## 修复方案

### 总体思路

**不引入新的数据通道**（不改 `ProviderRunResult`、`NodeState` 等数据结构），而是**恢复被删除的 token 读取逻辑**——在 orchestrator 构建 `completed_node_snapshot` 和 `MetricsEventContext` 时，通过 `read_session_tokens()` 从 ACP session 文件读取真实 token 数据。

这是最小化、最安全的修复方式，因为：
1. `read_session_tokens()` 是已验证可用的函数（读取 `acp.snapshot.json` + `acp.timeline.jsonl`）
2. 不改变现有的数据结构定义
3. 不影响其他模块

### 具体修改

#### 1. 修复 `completed_node_snapshot` 调用 — 传入真实 token

**文件**：`src/app/orchestrator.rs`  
**位置**：`drive_from_node_with_initial_session` 函数中，`persist_runtime_state` 之后

**当前代码**：
```rust
persist_runtime_state(app, task_id, run, round, &node)?;

let completed_snapshot = completed_node_snapshot(round, &node, 0, 0, 0, 0);
```

**修改为**：
```rust
persist_runtime_state(app, task_id, run, round, &node)?;

// 从 ACP session 文件读取真实 token 数据
let attempt_dir =
    app.paths
        .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
let session_paths = crate::acp::events::AcpAttemptPaths::from_attempt_dir(attempt_dir);
let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
    crate::acp::events::read_session_tokens(&session_paths.session);

let completed_snapshot = completed_node_snapshot(
    round, &node,
    input_tokens,
    output_tokens,
    cache_read_tokens,
    total_tokens,
);
```

#### 2. 修复 `NodeCompleted` 的 `MetricsEventContext` — 传入真实 token

**文件**：`src/app/orchestrator.rs`  
**位置**：workflow 结束时的 metrics 上报（`apply_control_decision` 返回 `None` 的分支）

**当前代码**：
```rust
input_tokens: 0,
output_tokens: 0,
cache_read_tokens: 0,
total_tokens: 0,
acp_session_path: Some(session_paths.session.to_string()),
```

**修改为**：直接使用步骤 1 已读取的 `input_tokens`、`output_tokens`、`cache_read_tokens`、`total_tokens`：
```rust
input_tokens,
output_tokens,
cache_read_tokens,
total_tokens,
acp_session_path: Some(session_paths.session.to_string()),
```

#### 3. 修复 `NodeStarted` 的 `MetricsEventContext` — 从 `LastExecutedNode` 传递 token

**文件**：`src/app/orchestrator.rs`  
**位置**：`drive_from_node_with_initial_session` 函数中，首个 node 开始前的 metrics 上报

`NodeStarted` 事件中当前节点 token 应该是 0（还没开始执行），这本身是正确的设计。但 predecessor 的 token 来自 `run.last_executed_node`，在步骤 1 修复后，`LastExecutedNode` 的 token 已经是真实值，因此 predecessor 的 token 会自动修复，**无需额外修改**。

但考虑到 `MetricsEventContext.token` 字段在此场景下实际上未被 `NodeStarted` handler 使用（handler 中 current node token 直接硬编码 0），所以第 6322-6325 行的硬编码 0 也不需要修改。

### 修改范围总结

| 序号 | 文件 | 修改内容 | 影响范围 |
|---|---|---|---|
| 1 | `src/app/orchestrator.rs` | 在调用 `completed_node_snapshot` 前，调用 `read_session_tokens()` 读取真实 token | `LastExecutedNode` token 恢复 → `NodeStarted` predecessor token 恢复 |
| 2 | `src/app/orchestrator.rs` | `NodeCompleted` 的 `MetricsEventContext` 使用已读取的真实 token | `NodeCompleted` handler 的 `MetricsEventContext` 获得真实值（`NodeCompleted` handler 本身还会再次读取，两处数据一致） |

### 不需要修改的部分

| 文件/位置 | 原因 |
|---|---|
| `src/provider/mod.rs` / `ProviderRunResult` | 不是根本原因，且修改影响面大。原有架构就是通过 `read_session_tokens()` 旁路读取 |
| `src-tauri/src/metrics.rs` / `NodeStarted` handler | predecessor 的 token 来自 `LastExecutedNode`，修复后自动恢复 |
| `src-tauri/src/metrics.rs` / `NodeCompleted` handler | 已有 `read_tokens_best_effort()` 补偿读取，修复后数据更一致 |
| `src/app/orchestrator.rs` / `NodeStarted` MetricsEventContext | 当前节点尚未执行，token 为 0 是正确的 |

## 验证方法

1. 启动客户端，执行一个 workflow
2. 查看 `NodeStarted` 事件上报的 predecessor token 字段是否为非 0 值
3. 查看 `NodeCompleted` 事件上报的 last_node token 字段是否为非 0 值
4. 对比 ACP session 目录下的 `acp.snapshot.json` 中的 token 值与上报值是否一致

## 综合评估

### 一、精确改动点

所有改动集中在 **一个文件**：`src/app/orchestrator.rs`。

#### 改动 A（核心）：`completed_node_snapshot` 调用处

**位置**：`drive_from_node_with_initial_session` 函数，第 6661-6663 行附近

```diff
 persist_runtime_state(app, task_id, run, round, &node)?;

-let completed_snapshot = completed_node_snapshot(round, &node, 0, 0, 0, 0);
+let attempt_dir =
+    app.paths
+        .attempt_dir(task_id, &run.id, &round.id, &node.node_id, &node.attempt_id);
+let session_paths = crate::acp::events::AcpAttemptPaths::from_attempt_dir(attempt_dir);
+let (input_tokens, output_tokens, cache_read_tokens, total_tokens) =
+    crate::acp::events::read_session_tokens(&session_paths.session);
+
+let completed_snapshot = completed_node_snapshot(
+    round, &node,
+    input_tokens,
+    output_tokens,
+    cache_read_tokens,
+    total_tokens,
+);
```

新增约 10 行，实质就是**在调 `completed_node_snapshot` 之前加一次 `read_session_tokens()` 调用**。

#### 改动 B（配套）：`NodeCompleted` MetricsEventContext

第 6709-6712 行：将硬编码 0 替换为改动 A 中已读取的变量。

```diff
-            input_tokens: 0,
-            output_tokens: 0,
-            cache_read_tokens: 0,
-            total_tokens: 0,
+            input_tokens,
+            output_tokens,
+            cache_read_tokens,
+            total_tokens,
```

**总计：新增 ~10 行，修改 4 行，零删除**。

### 二、时序正确性分析

关键问题是：在 orchestrator 执行到 `completed_node_snapshot` 调用点时，`acp.snapshot.json` 是否已经包含了 token 数据？

**调用链时序**：

```
drive_from_node_with_initial_session()
  └→ execute_ai_node()                          // 同步阻塞
       └→ provider.run_worker_with_callbacks()   // 同步阻塞
            └→ client::run_prompt()              // 同步阻塞
                 ├─ session/prompt 完成
                 ├─ 解析 usage.inputTokens 等    // client.rs:878-884
                 └─ write_session()              // client.rs:259 → 写入 acp.snapshot.json
                     └─ session_metadata()       // client.rs:1195 → 包含 input_tokens 等
  └→ persist_runtime_state()                    // 状态持久化
  └→ ★ completed_node_snapshot() 调用点         // 此时 snapshot 已就绪 ← 我们在这里读取
```

由于 `execute_ai_node` 是**同步阻塞**调用，当它返回时，ACP adapter 已经：
1. 收到 `session/prompt` 的完整响应
2. 解析了 `usage` 中的 token 字段
3. 写入了 `acp.snapshot.json`

**时序结论：`read_session_tokens()` 在 `completed_node_snapshot` 调用点一定能读到最新数据，不存在竞态条件。**

### 三、数据源可靠性分析

`read_session_tokens()` (`src/acp/events.rs:152-241`) 采用**双重读取策略**：

| 来源 | 优先级 | 说明 |
|---|---|---|
| `acp.snapshot.json` | 第一来源 | `write_session()` 在 prompt 完成后写入，包含完整 token 字段 |
| `acp.timeline.jsonl` | 补充来源 | 扫描 `usageUpdate` 事件，用 `max()` 取值作为补充 |

**容错机制**：
- `AcpSessionMetadata` 中 token 字段是 `Option<u64>`，`read_session_tokens` 使用 `unwrap_or(0)` 兜底 —— 如果 adapter 未返回 token（如某些 adapter 不支持），不会 panic，返回 0
- `read_tokens_best_effort` 外层包了 `catch_unwind`，极端情况下也不会导致 orchestrator 崩溃
- timeline 扫描使用 `max()` 防止覆盖已有值

**数据完整性**：`acp.snapshot.json` 写入在 `client.rs:259` 的 `write_session` 中，此时 `self.input_tokens` 等字段已于 `client.rs:878-884` 从 adapter 响应中解析完成。只要 adapter 的 `session/prompt` 响应包含 `usage` 对象，数据就是完整的。

### 四、对各上报链路的影响

#### 4.1 NodeStarted 事件

`NodeStarted` handler (`src-tauri/src/metrics.rs:376-510`) 每次上报包含两条 metric：

| metric | token 来源 | 当前值 | 修复后 |
|---|---|---|---|
| **predecessor（前序节点）** | `LastExecutedNode.input_tokens` 等 | 0（因为 `completed_node_snapshot` 传了 0） | ✅ **修复为真实值** |
| **current（当前节点）** | handler 中硬编码 0 | 0 | 0（保持不变，当前节点尚未执行，token 为 0 正确） |

**对 NodeStarted 的影响：只有 predecessor 的 token 从 0 恢复为真实值。** 第一个节点（无 predecessor）时，handler 会生成 `start_sentinel_metric`，token 为 0，不受影响。

#### 4.2 NodeCompleted 事件

`NodeCompleted` handler (`src-tauri/src/metrics.rs:511-608`) 每次上报包含两条 metric：

| metric | token 来源 | 当前值 | 修复后 |
|---|---|---|---|
| **last_node（最后完成的节点）** | `read_tokens_best_effort(ctx.acp_session_path)` | 取决于 snapshot 内容 | 不变（handler 自身读取，不受本次改动影响） |
| **end_sentinel（结束标记）** | handler 中硬编码 0 | 0 | 0（保持不变，sentinel 不携带 token 是设计意图） |

**对 NodeCompleted 的影响：** 改动 B 修复了 `MetricsEventContext` 中的 token 字段，但 `NodeCompleted` handler **并不使用** `ctx.input_tokens` 等字段——它独立调用 `read_tokens_best_effort` 读取。改动 B 的意义在于**保持数据一致性**，避免未来维护者困惑"为什么 Context 里 token 全是 0 但 handler 里又能读到真实值"。

#### 4.3 改动后的数据流总览

```
                     ┌──────────────────────────────────────────────┐
                     │        acp.snapshot.json (持久化)             │
                     │  input_tokens, output_tokens, total_tokens   │
                     └────────────┬─────────────────────────────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    │  read_session_tokens()     │
                    │  (orchestrator 调用)       │  ← 改动 A：新增
                    └─────────────┬─────────────┘
                                  │
              ┌───────────────────┼───────────────────┐
              ▼                   ▼                    ▼
   completed_node_snapshot   NodeCompleted ctx     (数据一致性)
   → LastExecutedNode        token 字段
   → NodeStarted predecessor
        ↑ 修复 ✅               ↑ 修复 ✅
```

### 五、边界情况分析

| 场景 | 行为 | 是否正确 |
|---|---|---|
| **第一个节点（无 predecessor）** | `NodeStarted` handler 生成 `start_sentinel_metric`，token 硬编码 0 | ✅ 正确 |
| **第二个及后续节点** | predecessor 来自 `LastExecutedNode`，修复后 token 为真实值 | ✅ 修复后正确 |
| **节点重试（attempt > 1）** | 每次 attempt 有独立目录，`read_session_tokens` 读取当前 attempt 的 snapshot | ✅ 正确 |
| **adapter 未返回 usage** | `read_session_tokens` 返回 0（`unwrap_or(0)` 兜底） | ✅ 正确（没有数据就是 0） |
| **snapshot 文件不存在** | `read_session_tokens` 中 `read_to_string` 返回 `Err`，跳过 snapshot；timeline 扫描同样无数据；返回全 0 | ✅ 正确 |
| **workflow 中途暂停/异常退出** | 不经过 `NodeCompleted` 路径，`LastExecutedNode` 不更新 | ✅ 正确 |
| **非 Worker 节点（如 ManualCheck）** | 不经过 ACP adapter，没有 token 数据，snapshot 也不存在 | ✅ 返回 0 正确 |

### 六、与原实现（pre-2135ee9）的差异

| 维度 | 原实现（pre-2135ee9） | 本次修复 | 差异 |
|---|---|---|---|
| `LastExecutedNode` 构建 | 手动构造，调用 `read_session_tokens` | 通过 `completed_node_snapshot` 函数，传入 `read_session_tokens` 结果 | 等价。`completed_node_snapshot` 是纯数据映射函数 |
| `NodeCompleted` ctx token | 用 `read_session_tokens` 结果直接赋值 | 复用改动 A 的变量 | 等价。读取的是同一份数据 |
| `run.last_executed_node` 赋值时机 | 在 workflow 继续时赋值 | 在 `apply_control_decision` 返回 `Some` 时赋值 | 等价。同一位置，同一个值 |
| 代码结构 | 内联（inline） | 通过 `completed_node_snapshot` 函数 | 新结构更好（提取了重复逻辑），修复后行为等价 |

**结论：本次修复在行为上等价于 revert 到 pre-2135ee9 的 token 处理逻辑，同时保留了 `completed_node_snapshot` 函数（减少重复代码）。**

### 七、NodeCompleted handler 冗余读取的考量

当前 `NodeCompleted` handler 通过 `read_tokens_best_effort(ctx.acp_session_path)` 独立读取 token，与 orchestrator 的 `read_session_tokens` 形成双重读取。考虑过是否应该让 handler 直接使用 `ctx.input_tokens` 等字段（从 orchestrator 传入），但这有额外好处：

- **解耦**：handler 不依赖 orchestrator 是否传了正确的 token 值
- **容错**：如果 orchestrator 某次忘记读取，handler 仍有兜底
- **统一路径**：`NodeStarted` handler 无法从 context 获取 token（当前节点尚未执行），`NodeCompleted` handler 独立读取保持了实现模式的一致性

**结论：双重读取是合理的防御性设计，不需要简化。**

### 八、回归风险

| 风险维度 | 评估 |
|---|---|
| **编译风险** | 极低。使用的 `AcpAttemptPaths`、`read_session_tokens` 是项目中已存在的公开 API |
| **运行时风险** | 低。`read_session_tokens` 纯读取，内部有多层 fallback |
| **性能影响** | 可忽略。一次文件读取（几 KB） + 一次 JSON 解析，在节点完成后执行，不在热路径上 |
| **与其他模块交互** | 无。改动局限于 `orchestrator.rs` 内部 |
| **对现有 workflow 行为的影响** | 无。只是恢复了之前就有的 token 上报功能 |

### 九、验证清单

- [ ] 编译通过：`cargo build`
- [ ] 执行一个多节点 workflow，检查 `NodeStarted` 事件中 predecessor 的 `inputTokens`/`outputTokens`/`totalTokens` 是否为非 0 值
- [ ] 检查 `NodeCompleted` 事件中 `last_node` 的 token 字段是否为非 0 值
- [ ] 对比 `acp.snapshot.json` 中的值与上报值是否一致
- [ ] 检查第一个节点的 `NodeStarted` 事件中 predecessor 是否为 sentinel（token 为 0 且 name 为 "开始"）——不应报错
- [ ] 回归测试：workflow 正常执行到结束，状态持久化正常，UI 无异常

## 风险评估

- **风险等级**：低
- **影响范围**：仅 orchestrator 中 token 数据读取逻辑（`src/app/orchestrator.rs`，约 14 行变更）
- **回滚方式**：如果出现问题，回滚 `src/app/orchestrator.rs` 即可
- **副作用**：无。`read_session_tokens()` 是纯读取操作，不修改任何文件
