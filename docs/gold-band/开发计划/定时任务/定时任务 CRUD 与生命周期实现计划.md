# 定时任务 CRUD 与生命周期实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## 2026-08-06 SQLite application-service replacement

- [ ] Add RED interface tests proving create is definition/input-only and
  explicit run-now is the only immediate manual execution path.
- [ ] Route list/get/create/update/enable/run-now/delete through one shared
  `ScheduledTaskService` and the project-scoped SQLite repository.
- [ ] Remove active `ScheduledTaskStore` reads/writes and JSON fallback; retain
  it only for marker-controlled legacy migration.
- [ ] Add optimistic conflict, project scope, enable/pause, structured error,
  and delete tombstone rollback/history-preservation regression tests.
- [ ] Keep the current coordinator as a narrow replaceable handle; the
  deadline-driven DelayQueue coordinator remains the next implementation task.

**目标：** 完成定时任务的全局 CRUD、真实调度、三种模式的 Task/Run 生命周期，以及管理页和会话侧栏的可追踪展示。

**架构：** 以 `ScheduledTaskDefinition` 作为唯一调度定义，使用结构化 authoring 快照计算内容指纹；调度器只负责生成触发并调用现有 Task/Run/ACP 创建链路。Workflow/AUTO 的 Agent 配置属于 authoring，变化后清空 `taskId`；模型、思考强度和权限属于执行配置，变化后复用 Task。管理页通过显式 CRUD 请求局部更新，只有 App 根层订阅调度事件并刷新左侧会话列表。

**技术栈：** Rust 2024、serde/serde_json、sha2、chrono/chrono-tz/cron、Tauri 2、React 19、TypeScript、Tailwind CSS、shadcn/ui、Vitest。

---

## 文件边界

- `src/scheduler/mod.rs`：定时任务领域类型、内容身份投影和调度约束。
- `src/scheduler/fingerprint.rs`：规范化 authoring 快照、附件哈希和 SHA-256 指纹。
- `src/scheduler/store.rs`：定义文件的保存、更新、删除和不可变触发记录。
- `src-tauri/src/view_models_conversation.rs`：创建/编辑输入、管理列表 VM、会话元数据中的定时来源。
- `src-tauri/src/commands_conversation.rs`：创建、读取、更新、启停、删除 Tauri 命令及结构化错误码。
- `src-tauri/src/scheduled_runtime.rs`：scheduler loop、队列保护、触发物化和三种模式生命周期。
- `src-tauri/src/main.rs`：注册新增 Tauri 命令。
- `src-tauri/gen/schemas/*.json`：由 Tauri 命令注册生成并校验接口 schema。
- `web/src/types.ts`、`web/src/api/client.ts`、`web/src/api.ts`、`web/src/api/desktop.ts`、`web/src/api/browser.ts`：前端类型和双运行时 API。
- `web/src/pages/ScheduledTaskManagementPage.tsx`：全局列表、筛选、手动刷新和行级 CRUD。
- `web/src/pages/ConversationHomePage.tsx`、`web/src/components/conversation/ConversationComposer.tsx`、`web/src/App.tsx`：Composer 创建/编辑回流。
- `web/src/components/conversation/ConversationSidebar.tsx`：定时 Task 的 AlarmClock 标识。
- `web/tests/` 与 Rust `#[cfg(test)]` 模块：接口层回归测试。
- `docs/gold-band/产品设计文档/**`、`docs/gold-band/开发计划/**`：每个实现阶段同步更新设计和进度。

## Task 1：建立结构化内容身份和稳定指纹

**Files:**
- Create: `src/scheduler/fingerprint.rs`
- Modify: `src/scheduler/mod.rs`
- Modify: `Cargo.toml`, `Cargo.lock`
- Test: `src/scheduler/fingerprint.rs` tests and `src/scheduler/mod.rs` tests

- [ ] **Step 1: 添加失败测试，锁定指纹边界**

```rust
#[test]
fn fingerprint_changes_when_workflow_agent_changes() {
    let base = ScheduledTaskContentInput::workflow(
        "日报",
        "workspace-a",
        json!({"nodes":[{"provider":"claude-acp"}]}),
    );
    let changed = base.with_workflow_authoring(json!({
        "nodes":[{"provider":"codex-acp"}]
    }));
    assert_ne!(content_fingerprint(&base).unwrap(), content_fingerprint(&changed).unwrap());
}

#[test]
fn fingerprint_input_excludes_execution_fields_by_construction() {
    let left = ScheduledTaskContentInput::auto(
        "日报", "workspace-a", "fixed", "claude-acp", None, vec![],
    );
    let right = ScheduledTaskContentInput::auto(
        "日报", "workspace-a", "fixed", "claude-acp", None, vec![],
    );
    assert_eq!(content_fingerprint(&left).unwrap(), content_fingerprint(&right).unwrap());
}
```

Run: `cargo test -p gold-band scheduler::fingerprint`

Expected: FAIL because no canonical input or fingerprint function exists.

- [ ] **Step 2: 引入 SHA-256 并定义 canonical 数据结构**

Add `sha2 = "0.10"` to the root dependencies and define a serializable `ScheduledTaskContentInput` containing only:

```rust
pub struct ScheduledTaskContentInput {
    pub mode: ScheduledMode,
    pub instruction: String,
    pub attachment_hashes: Vec<String>,
    pub workspace_id: String,
    pub workflow_authoring: Option<Value>,
    pub auto_authoring: Option<AutoAuthoringIdentity>,
    pub direct_agent_id: Option<String>,
}

pub struct AutoAuthoringIdentity {
    pub agent_strategy: String,
    pub agent_type: String,
    pub bootstrap_agent_type: Option<String>,
    pub available_agent_types: Vec<String>,
    pub global_goal: Option<String>,
    pub allowed_workflow_ids: Vec<String>,
}
```

Implement the test constructors used above as ordinary associated functions:

```rust
impl ScheduledTaskContentInput {
    fn workflow(instruction: &str, workspace_id: &str, authoring: Value) -> Self {
        Self {
            mode: ScheduledMode::Workflow,
            instruction: instruction.to_string(),
            attachment_hashes: Vec::new(),
            workspace_id: workspace_id.to_string(),
            workflow_authoring: Some(authoring),
            auto_authoring: None,
            direct_agent_id: None,
        }
    }
    fn auto(instruction: &str, workspace_id: &str, strategy: &str, agent: &str,
            bootstrap: Option<&str>, available: Vec<String>) -> Self {
        Self {
            mode: ScheduledMode::Auto,
            instruction: instruction.to_string(),
            attachment_hashes: Vec::new(),
            workspace_id: workspace_id.to_string(),
            workflow_authoring: None,
            auto_authoring: Some(AutoAuthoringIdentity {
                agent_strategy: strategy.to_string(),
                agent_type: agent.to_string(),
                bootstrap_agent_type: bootstrap.map(str::to_string),
                available_agent_types: available,
                global_goal: None,
                allowed_workflow_ids: Vec::new(),
            }),
            direct_agent_id: None,
        }
    }
    fn with_workflow_authoring(mut self, authoring: Value) -> Self {
        self.workflow_authoring = Some(authoring);
        self
    }
}
```

`workflow_authoring` retains node/edge structure, provider/Agent identity, profiles, goals and control rules while removing model, permission and ACP execution options. `available_agent_types` is sorted and deduplicated; models inside available-agent refs are excluded. Serialize the normalized value with `serde_json::to_vec`, hash with `Sha256`, and format as `sha256:<lowercase hex>`.

- [ ] **Step 3: 将 canonical 内容快照接入 `ScheduledTaskDefinition`**

Add `content_snapshot` with `#[serde(default)]`, keep `content_fingerprint`, and expose:

```rust
pub fn recompute_content_fingerprint(&mut self) -> anyhow::Result<()> {
    self.content_fingerprint = fingerprint::content_fingerprint(&self.content_snapshot)?;
    Ok(())
}
```

The create/update command must never hash instruction text alone.

- [ ] **Step 4: Run focused tests and commit**

Run: `cargo test -p gold-band scheduler::fingerprint scheduler::tests`

Expected: PASS for attachment hashes, workspace, Direct Agent, Workflow authoring and AUTO Agent authoring changes, while model/thought/permission changes keep the same fingerprint.

Commit: `git add Cargo.toml Cargo.lock src/scheduler/mod.rs src/scheduler/fingerprint.rs && git commit -m "feat: define scheduled task content identity"`

Implementation status (2026-07-31): Steps 2-4 are complete. The canonical
authoring projection and `ScheduledTaskDefinition.content_snapshot` are now
covered by focused Rust tests; the remaining unchecked items are subsequent
CRUD and runtime tasks.

## Task 2：补齐定义存储的 update/delete 和触发记录

**Files:**
- Modify: `src/scheduler/store.rs`
- Modify: `src/storage/mod.rs`
- Test: `src/scheduler/store.rs`

- [ ] **Step 1: 写存储失败测试**

```rust
#[test]
fn update_replaces_one_definition_and_delete_keeps_task_history() {
    let (store, paths) = test_store();
    let mut definition = sample_definition("scheduled-001");
    store.save(&definition).unwrap();
    std::fs::create_dir_all(paths.task_dir("task-001").as_std_path()).unwrap();
    definition.instruction = "updated".to_string();
    store.update(&definition).unwrap();
    assert_eq!(store.load("scheduled-001").unwrap().instruction, "updated");
    store.delete("scheduled-001").unwrap();
    assert!(!paths.scheduled_task_dir("scheduled-001").exists());
    assert!(paths.task_dir("task-001").exists());
}
```

Run: `cargo test -p gold-band scheduler::store`

Expected: FAIL because `update` and `delete` are absent.

- [ ] **Step 2: 实现精确目录操作**

Implement `update` with an ID/path equality check, atomic JSON replacement, and `delete` using only the resolved `scheduled_task_dir(id)`. Add `triggers_dir`, `trigger_file`, and a monotonically increasing `trigger-NNN.json` writer. Trigger records contain scheduled task ID, scheduled time, status, task ID, run ID, attempt count and timestamps; deleting a definition removes only its definition/input/triggers directory.

- [ ] **Step 3: 固化启停语义并测试**

Keep `task_id` unchanged on enable/disable. Reset `anchor_at` only when enabling an `Every` schedule, and update `updated_at` once. Add tests for multiple definitions, enable reset, and trigger record numbering.

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p gold-band scheduler::store`

Expected: PASS with task history untouched after scheduled-definition deletion.

Commit: `git add src/scheduler/store.rs src/storage/mod.rs && git commit -m "feat: add scheduled task storage CRUD"`

Implementation status (2026-07-31): Task 2 is complete. Definition updates and
deletes are path-checked, trigger records are immutable and monotonically
numbered, and enable/disable preserves the materialized Task association.

## Task 3：实现 typed CRUD 命令和后端错误码

**Files:**
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/gen/schemas/desktop-schema.json`, `src-tauri/gen/schemas/windows-schema.json`
- Test: Rust command helper tests

- [ ] **Step 1: 定义编辑 VM 和更新输入**

Add `ScheduledTaskEditVm` and `UpdateScheduledTaskInputVm` with `scheduled_task_id`, `project_id`, `expected_updated_at`, `content`, `run_mode`, `workflow_template_id`, `include_interview`, `direct_config`, `auto_config`, `attachment_paths`, `schedule`, `overlap_policy`, and `session_policy`. Return attachment names and read-only Direct Agent identity in the edit VM.

- [ ] **Step 2: 抽取创建/更新共用的 snapshot builder，并先写规则测试**

```rust
#[test]
fn update_rejects_direct_agent_change() {
    let old = sample_direct_update("claude-acp");
    let changed = old.with_agent("codex-acp");
    assert_eq!(validate_scheduled_update(&old, &changed).unwrap_err().code,
        "scheduled-task.direct-agent-immutable");
}

#[test]
fn workflow_agent_change_clears_task_but_model_change_does_not() {
    let old = sample_workflow_definition("claude-acp");
    assert!(task_association_after_update(&old, old.with_agent("codex-acp")).is_none());
    assert_eq!(task_association_after_update(&old, old.with_model("opus")), old.task_id);
}
```

Use structured codes only: `scheduled-task.not-found`, `scheduled-task.update-conflict`, `scheduled-task.direct-agent-immutable`, `scheduled-task.workspace-not-found`, `scheduled-task.invalid-schedule`, and `scheduled-task.attachment-copy-failed`.

The test fixtures are concrete pure helpers in the same test module: `sample_direct_update(agent: &str) -> ScheduledTaskUpdateProjection`, `sample_workflow_definition(agent: &str) -> ScheduledTaskDefinition`, `with_agent`/`with_model` return a cloned projection, `validate_scheduled_update(old, new) -> Result<(), ScheduledTaskCommandError>`, and `task_association_after_update(old, new) -> Option<String>` returns `None` only when the canonical fingerprint changes.

- [ ] **Step 3: 实现 `get_scheduled_task`、`update_scheduled_task`、`delete_scheduled_task`**

Resolve the workspace from `project_id`, load the exact store, compare `expected_updated_at`, validate the complete typed input, rebuild the canonical content snapshot, and clear `task_id` only when the fingerprint changes. For Direct, compare the stored Agent before writing and reject changes. Replace attachment snapshots through a temporary sibling directory and rename it into `inputs`; do not touch task history. Emit one updated event after successful update/delete; deletion emits a removal status without reloading every definition.

- [ ] **Step 4: 注册命令并生成 schema**

Register all three commands in `src-tauri/src/main.rs`, run the repository schema generation/build command, and verify both generated schema files contain the same command signatures.

- [ ] **Step 5: Run command tests and commit**

Run: `cargo test -p gold-band-desktop commands_conversation::tests`

Expected: PASS for not-found, optimistic conflict, Direct immutability, Workflow/AUTO Agent fingerprint changes, model/thought/permission preservation, and history-preserving delete.

Commit: `git add src-tauri/src/view_models_conversation.rs src-tauri/src/commands_conversation.rs src-tauri/src/main.rs src-tauri/gen/schemas && git commit -m "feat: add scheduled task CRUD commands"`

Implementation status (2026-07-31): Task 3 is complete. Desktop commands now
expose typed create/read/update/delete/enable operations, use structured
scheduled-task error codes, persist canonical content snapshots, and enforce
Direct Agent immutability with optimistic update timestamps.

## Task 4：修正 scheduler 的 Task/Run/Session 生命周期

**Files:**
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Test: `src-tauri/src/scheduled_runtime.rs` tests and runtime integration tests

- [ ] **Step 1: 先写动作判定测试**

```rust
#[test]
fn direct_new_always_materializes_a_new_task() {
    let definition = definition("direct", SessionPolicy::New, Some("task-001"));
    assert_eq!(scheduled_execution_action(&definition), ScheduledExecutionAction::MaterializeTaskAndRun);
}

#[test]
fn workflow_and_auto_reuse_task_but_create_run() {
    for mode in ["workflow", "auto"] {
        let definition = definition(mode, SessionPolicy::New, Some("task-001"));
        assert_eq!(scheduled_execution_action(&definition),
            ScheduledExecutionAction::StartNewRun { task_id: "task-001".into() });
    }
}
```

Expected: the current implementation fails the Direct case because it starts a new Run on the old Task.

The test fixture `definition(mode, session_policy, task_id)` constructs a valid `ScheduledTaskDefinition` with `ScheduleSpec::at` and the supplied association; it is defined beside the tests so the action tests do not depend on filesystem state.

- [ ] **Step 2: 分离四种执行动作**

Use `MaterializeTaskAndRun` for Direct new with or without an existing association, `StartNewRun` only for Workflow/AUTO with an existing Task, and `ContinueSession` only for Direct continuous. When a new Task is materialized, set `definition.task_id = Some(new_task_id)` unconditionally so Direct new points to the latest Task.

- [ ] **Step 3: 处理 continuous 会话不可恢复**

Before `send_acp_prompt`, verify the latest run/round/attempt is resumable with the existing continuation predicate. If no valid attempt exists or the prompt call returns a non-resumable error, materialize a new Direct Task and continue from it. A successful continuous trigger preserves Task, Run and Round; it does not create a new Run.

- [ ] **Step 4: 写入 trigger records and preserve queue protection**

Record `scheduled`, `running`, `skipped`, `failed` and `completed` transitions through `ScheduledTaskStore`. `skip_when_running` and `retry_when_busy` inspect the associated Task's active runs; retry count remains on the same trigger. A one-shot `At` definition disables itself after a successful trigger.

- [ ] **Step 5: Run lifecycle tests and commit**

Run: `cargo test -p gold-band-desktop scheduled_runtime`

Expected: PASS for Direct new (`task-001`, `task-002`), Direct continuous (same Task/Run/Round), Workflow/AUTO (same Task with new Run), authoring-change new Task, queue skip/retry, and trigger records.

Commit: `git add src-tauri/src/scheduled_runtime.rs src-tauri/src/view_models_conversation.rs && git commit -m "fix: enforce scheduled task lifecycle semantics"`

## Task 5：补齐前端 API、浏览器 fallback 和 App 事件边界

**Files:**
- Modify: `web/src/types.ts`
- Modify: `web/src/api/client.ts`, `web/src/api.ts`, `web/src/api/desktop.ts`, `web/src/api/browser.ts`
- Modify: `web/src/App.tsx`
- Test: `web/tests/api.test.ts`, `web/tests/browser-scheduled-task-api.test.ts`

- [ ] **Step 1: 为 CRUD API 写失败测试**

```ts
it('forwards scheduled task CRUD through the runtime facade', async () => {
  const api = {
    getScheduledTask: vi.fn().mockResolvedValue({ id: 'scheduled-1' }),
    updateScheduledTask: vi.fn().mockResolvedValue({ id: 'scheduled-1' }),
    deleteScheduledTask: vi.fn().mockResolvedValue(undefined),
  };
  vi.mocked(getRuntimeApi).mockReturnValue(api as never);
  await getScheduledTask('project-a', 'scheduled-1');
  await updateScheduledTask({ projectId: 'project-a', scheduledTaskId: 'scheduled-1' } as never);
  await deleteScheduledTask('project-a', 'scheduled-1');
  expect(api.getScheduledTask).toHaveBeenCalledWith('project-a', 'scheduled-1');
  expect(api.updateScheduledTask).toHaveBeenCalled();
  expect(api.deleteScheduledTask).toHaveBeenCalledWith('project-a', 'scheduled-1');
});
```

- [ ] **Step 2: 实现 desktop/browser API 对称接口**

Add the typed methods to `RuntimeApi`, map desktop calls to `get_scheduled_task`, `update_scheduled_task`, and `delete_scheduled_task`, and make browser preview update/remove only the targeted array entry. Browser CRUD must keep multiple definitions and emit a targeted event.

- [ ] **Step 3: 保留 App 根层事件刷新，移除管理页订阅职责**

Keep the existing `App.tsx` subscription that calls `getConversationSidebar()` so a triggered Task/Run appears in the left sidebar. The management page receives no scheduled-event listener; this prevents background events from resetting filters or putting the table into a full loading state.

- [ ] **Step 4: Run web tests and commit**

Run: `npm run web:test -- --run web/tests/api.test.ts web/tests/browser-scheduled-task-api.test.ts`

Expected: PASS, including two browser definitions surviving create/update/delete independently.

Commit: `git add web/src/types.ts web/src/api.ts web/src/api/client.ts web/src/api/desktop.ts web/src/api/browser.ts web/src/App.tsx web/tests/api.test.ts web/tests/browser-scheduled-task-api.test.ts && git commit -m "feat: expose scheduled task CRUD in runtime APIs"`

## Task 6：完成管理页行级 CRUD 和 Composer 编辑回流

**Files:**
- Modify: `web/src/pages/ScheduledTaskManagementPage.tsx`
- Modify: `web/src/pages/ConversationHomePage.tsx`
- Modify: `web/src/components/conversation/ConversationComposer.tsx`
- Modify: `web/src/App.tsx`
- Test: `web/tests/scheduled-task-management-page.test.ts`, new `web/tests/scheduled-task-crud.test.tsx`

- [ ] **Step 1: 写管理页交互失败测试**

Assert that initial load happens once, a scheduled update event does not call `listScheduledTasks` again, manual refresh keeps the existing rows while showing icon progress, and successful enable/edit/delete replaces or removes only the target row.

- [ ] **Step 2: 实现列表局部状态**

Replace `loadTasks` with `refreshTasks({ initial: boolean })`; only initial load clears the loading state. `replaceTask`, `removeTask`, and `setTaskEnabled` update one row by `(projectId, id)`. Keep workspace/status filters untouched. Add a shadcn Button with Lucide `RefreshCw`, `aria-label="刷新定时任务"`, and disable it only while the explicit request is running.

- [ ] **Step 3: 添加管理页创建、编辑和删除入口**

Add a top-right “创建定时任务” command that navigates to the current workspace Composer. The row menu contains edit, enable/disable, and delete; delete uses shadcn `AlertDialog` and states that historical Task/Run records remain. Do not add “立即执行一次” or a task-name field.

- [ ] **Step 4: 将编辑恢复到现有 Composer**

Add `ScheduledTaskEditVm | null` to the App draft state. Selecting edit calls `getScheduledTask`, switches to `conversation-home`, restores content, attachments, run mode and schedule, and renders Direct Agent as disabled/read-only. Composer submit chooses `updateScheduledTask` for edit mode and `createScheduledTask` for create mode; after success it clears the draft and returns to the management page without duplicating model/permission/workspace controls.

- [ ] **Step 5: Run UI tests and commit**

Run: `npm run web:test -- --run web/tests/scheduled-task-management-page.test.ts web/tests/scheduled-task-crud.test.tsx`

Expected: PASS for no auto-refresh, manual refresh, row-local CRUD, edit routing and Direct Agent read-only behavior.

Commit: `git add web/src/pages/ScheduledTaskManagementPage.tsx web/src/pages/ConversationHomePage.tsx web/src/components/conversation/ConversationComposer.tsx web/src/App.tsx web/tests/scheduled-task-management-page.test.ts web/tests/scheduled-task-crud.test.tsx && git commit -m "feat: complete scheduled task management CRUD"`

## Task 7：标注定时来源并完成深链和视觉验收

**Files:**
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `web/src/components/conversation/ConversationSidebar.tsx`
- Modify: `web/src/components/conversation/ConversationRunHeader.tsx`
- Modify: `web/src/i18n.ts`
- Test: sidebar/run-header tests and browser deep-link test

- [ ] **Step 1: 扩展会话 metadata 和 VM**

Persist `scheduled_task_id` and `scheduled_trigger_id` in scheduled-created conversation metadata, expose `scheduledTaskId` on `ConversationTaskRowVm` and `ConversationRunVm`, and keep ordinary user-created conversations null.

- [ ] **Step 2: 使用 AlarmClock 作为低噪声身份标记**

In `TaskRow`, render `AlarmClock` in the existing fixed identity slot when `task.scheduledTaskId` is present; keep Direct Agent icons and Workflow/AUTO status dots unchanged for ordinary tasks. Add the same icon beside the run header title and a compact separator for Direct continuous scheduled turns. Do not prepend `[定时]` or expose `scheduled-UUID`.

- [ ] **Step 3: Verify routes and deep links**

Add route assertions for `/chat/scheduled-tasks` and a scheduled run deep link. Use the existing Vite server on an available port, open the management route directly, and verify table loading, filters, edit dialog, delete confirmation, refresh icon, sidebar AlarmClock and run header without overlap at desktop and mobile widths.

- [ ] **Step 4: Run web build and commit**

Run: `npm run web:build && npm run web:test`

Expected: TypeScript build and all web tests pass; the deep-link page renders without a blank state or console error.

Commit: `git add src-tauri/src/view_models_conversation.rs web/src/components/conversation/ConversationSidebar.tsx web/src/components/conversation/ConversationRunHeader.tsx web/src/i18n.ts web/tests && git commit -m "feat: mark scheduled conversations in sidebar"`

## Task 8：端到端回归、文档同步和交付门槛

**Files:**
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-crud-design.md`
- Modify: `docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: Rust and web full suites

- [ ] **Step 1: 执行接口级验收矩阵**

Run:

```powershell
cargo test --workspace
npm run web:test
npm run web:build
```

Verify these exact cases: two definitions coexist; At defaults to the current date in the Composer; timezone labels are country names; queue skip/retry works; Direct new produces a new Task on every trigger; Direct continuous preserves Task/Run/Round; Workflow/AUTO unchanged authoring creates a new Run; Workflow/AUTO Agent/authoring changes create a new Task; model/thought/permission changes preserve Task; delete preserves generated history; management does not auto-refresh; App sidebar does refresh after a trigger.

- [ ] **Step 2: 更新文档和进度**

Record the implemented command names, fingerprint exclusions, error codes, trigger record format and completed tests. Remove any statement that says Workflow/AUTO Agent is excluded from the fingerprint or that management auto-refreshes from scheduler events.

- [ ] **Step 3: 清理验证资源并提交**

Stop only the dev server started for this verification, remove temporary test schedule directories created under the test repository, run `git diff --check`, and commit the documentation/progress update:

```powershell
git add "docs/gold-band/产品设计文档/runtime/scheduled-task.md" "docs/gold-band/产品设计文档/runtime/scheduled-task-crud-design.md" "docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md" "docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md" "docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md" "docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md" "docs/gold-band/开发计划/定时任务/定时任务全局管理与会话刷新实现计划.md" "docs/gold-band/开发计划/定时任务/定时任务 CRUD 与生命周期实现计划.md"
git commit -m "docs: record scheduled task CRUD implementation"
```

## 完成状态（2026-07-31）

- Task 1-3：结构化 authoring 指纹、定义存储、触发记录、Tauri CRUD 和结构化错误码已落地。
- Task 4：Direct 新会话每次物化新 Task；Direct 持续会话复用同一 Task/Run/Round/ACP attempt；Workflow/AUTO 内容未变时复用 Task 创建新 Run，authoring 变化时创建新 Task。
- Task 5-7：桌面端和浏览器 fallback API、App 根层会话刷新、管理页手动 CRUD、Composer 定时入口、AlarmClock 标识和深链已落地；管理页不监听后台调度事件。
- Task 8：`npm run web:test`（85 个文件、562 个测试）、`npm run web:build`、`cargo test -p gold-band scheduler::tests`、`cargo test -p gold-band-desktop scheduled_runtime` 和 `cargo fmt --all -- --check` 已通过。
- 后续项：错过时间点的 `missed` 触发记录和补跑策略暂不实现。
