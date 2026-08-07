# Scheduled Task Unified Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make SQLite the only scheduled-task authority, replace fixed polling with deadline-driven coordination, preserve explicit run-now behavior, and complete Direct/Workflow/AUTO reliability, keep-awake, notifications, management UI, localized Skill tools, and user-confirmed Skill extraction.

**Architecture:** A workspace-local SQLite repository owns definitions, derived deadlines, migration markers, and occurrences. One process-level `SchedulerCoordinator` maintains a `tokio_util::time::DelayQueue`, while a shared `ScheduledTaskService` is the only command boundary used by Tauri and the internal MCP server. Mode adapters materialize existing Task/Run/ACP lifecycles; lifecycle events, not process start, finish occurrences.

**Tech Stack:** Rust 2024, `rusqlite`, Tokio, `tokio-util::time::DelayQueue`, `keepawake`, official Rust MCP SDK `rmcp`, Tauri 2, React 19, TypeScript, Tailwind CSS, shadcn/ui, prompt-kit, i18next, Vitest.

---

## Non-Negotiable Acceptance Semantics

1. `create_scheduled_task` validates and saves a definition and input snapshot only. It does not create Task/Run/ACP state, does not create an occurrence, and does not call a model.
2. The first scheduled Task is materialized only when the first planned deadline is claimed by the coordinator.
3. `run_scheduled_task_now` remains a separate explicit action. It immediately creates and starts a `trigger_kind = manual` occurrence, but it never changes the planned `next_run_at`.
4. Direct/new, Direct/continuous, Workflow, and AUTO share occurrence/lease/queue infrastructure but retain their existing Task/Run/content-fingerprint semantics.
5. Every code commit also updates at least one file under `docs/gold-band/产品设计文档` and one file under `docs/gold-band/开发计划`.

## File And Ownership Map

- `src/scheduler/occurrence.rs`: typed occurrence states, links, structured scheduler errors, lease configuration.
- `src/scheduler/queue.rs`: the only source of active-state classification, retry count, retry delay, and skip/retry decisions.
- `src/scheduler/db.rs`: schema migrations, one-time legacy import marker, definition CRUD, optimistic updates, occurrence transactions, retention cleanup.
- `src/scheduler/coordinator.rs`: timer keys, deadline registry, deadline selection, and deterministic reconciliation state independent of Tauri UI.
- `src/scheduler/mod.rs`: schedule/domain exports and next-occurrence calculations only.
- `src-tauri/src/scheduled_service.rs`: shared application service used by Tauri commands and MCP tools; no UI strings.
- `src-tauri/src/scheduled_runtime.rs`: runtime facade, start/shutdown, lifecycle subscriber, public coordinator handle.
- `src-tauri/src/scheduled_runtime/execution.rs`: Direct/new, Direct/continuous, Workflow, AUTO adapters and execution bindings.
- `src-tauri/src/scheduled_runtime/lease.rs`: per-occurrence heartbeat guard and lease-loss handling.
- `src-tauri/src/scheduled_runtime/power.rs`: process-level system sleep inhibitor and idempotent state controller.
- `src-tauri/src/scheduled_runtime/notification.rs`: structured scheduled-notification events and deduplication; no localized customer copy.
- `src-tauri/src/scheduled_tools.rs`: in-process `rmcp` Streamable HTTP server and typed scheduling tools.
- `src/skill/builtin.rs`: localized built-in Skill registry and read-only `SkillSource::BuiltIn` support.
- `src/skill/scheduled.rs`: deterministic occurrence provenance and confirmed Skill write request validation.
- `src/prompts/{zh-CN,en}/skills/scheduled-task/`: mirrored built-in Skill and Skill-draft prompts.
- `web/src/components/scheduled-tasks/`: settings, timezone, occurrence navigation, and Skill-preview UI assembled from existing shadcn/ui primitives.
- `web/src/pages/ScheduledTaskManagementPage.tsx`: compact list, filters, quick actions, and effective keep-awake state.
- `web/src/pages/ScheduledTaskDetailPage.tsx`: complete history, diagnostics, links, run-now, and Skill extraction.
- `web/src/i18n.ts`: all new and existing scheduled-task customer copy in zh-CN and en.

### Task 1: Repair The Test Baseline And Freeze Scheduler Contracts

**Files:**
- Modify: `src/app/mod.rs:3558`
- Modify: `src/app/mod.rs:3776`
- Modify: `src/scheduler/occurrence.rs`
- Modify: `src/scheduler/queue.rs`
- Modify: `src/config/mod.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src/scheduler/occurrence.rs`
- Test: `src/scheduler/queue.rs`

- [ ] **Step 1: Remove the two stale test-only lifecycle fields**

Delete only `scheduled_task_context: None` from the `RunPaused` constructor at line 3558 and the `RunCompleted` constructor at line 3776. The lifecycle enum already derives scheduled context from `App`; adding the removed field back would create two authorities.

- [ ] **Step 2: Run the current scheduler suite and confirm the baseline is restored**

Run: `cargo test scheduler:: --lib`

Expected: compilation succeeds; scheduler tests run instead of failing with `E0559`.

- [ ] **Step 3: Write failing serialization tests for the new structured codes and policy**

Add assertions for the exact stable values:

```rust
assert_eq!(
    serde_json::to_string(&ScheduledErrorCode::MigrationConflict).unwrap(),
    "\"SCHEDULED_MIGRATION_CONFLICT\""
);
assert_eq!(
    serde_json::to_string(&ScheduledErrorCode::CoordinatorUnavailable).unwrap(),
    "\"SCHEDULED_COORDINATOR_UNAVAILABLE\""
);
assert_eq!(
    serde_json::to_string(&ScheduledErrorCode::PowerInhibitorFailed).unwrap(),
    "\"SCHEDULED_POWER_INHIBITOR_FAILED\""
);
assert_eq!(
    serde_json::to_string(&ScheduledErrorCode::NotificationFailed).unwrap(),
    "\"SCHEDULED_NOTIFICATION_FAILED\""
);
assert_eq!(
    serde_json::to_string(&ScheduledErrorCode::SkillValidationFailed).unwrap(),
    "\"SCHEDULED_SKILL_VALIDATION_FAILED\""
);
```

Run: `cargo test -p gold-band scheduler::occurrence` and `cargo test -p gold-band scheduler::queue`

Expected: FAIL until the enum and central policy constants exist.

- [ ] **Step 4: Add the stable error variants and central policy values**

Extend `ScheduledErrorCode` and its `Display`/`FromStr` mappings with:

```rust
MigrationConflict,
CoordinatorUnavailable,
PowerInhibitorFailed,
NotificationFailed,
SkillValidationFailed,
```

Keep retry values only in `src/scheduler/queue.rs`:

```rust
pub const QUEUE_RETRY_INTERVAL: Duration = Duration::seconds(30);
pub const QUEUE_MAX_RETRIES: u8 = 3;
pub const LATE_FIRE_GRACE: Duration = Duration::seconds(60);
pub const DEFAULT_OCCURRENCE_RETENTION_DAYS: u16 = 30;
pub const MIN_OCCURRENCE_RETENTION_DAYS: u16 = 1;
pub const MAX_OCCURRENCE_RETENTION_DAYS: u16 = 3650;
pub const RETENTION_DELETE_BATCH_SIZE: usize = 500;
```

- [ ] **Step 5: Pass focused tests, update both document trees, and commit**

Run: `cargo test -p gold-band scheduler::occurrence` and `cargo test -p gold-band scheduler::queue`

Expected: PASS.

Record the baseline repair and policy ownership in both documentation files, then commit:

```bash
git add src/app/mod.rs src/scheduler/occurrence.rs src/scheduler/queue.rs src/config/mod.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "fix: restore scheduled runtime test contracts"
```

### Task 2: Upgrade The SQLite Repository And Import Legacy JSON Exactly Once

**Files:**
- Modify: `src/scheduler/db.rs`
- Modify: `src/scheduler/mod.rs`
- Modify: `src/scheduler/store.rs`
- Modify: `src/storage/mod.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src/scheduler/db.rs`

- [ ] **Step 1: Write failing schema, marker, direct-get, conflict, and retention tests**

Add tests named:

```rust
#[test] fn schema_v1_migrates_to_v2_without_losing_jobs_or_occurrences()
#[test] fn legacy_json_import_writes_marker_only_after_full_transaction_commits()
#[test] fn an_empty_database_with_completed_marker_does_not_reimport_json()
#[test] fn legacy_shared_database_is_copied_once_per_project_and_marked()
#[test] fn get_job_definition_is_scoped_by_project_and_job_id()
#[test] fn optimistic_update_rejects_stale_updated_at()
#[test] fn manual_occurrence_does_not_change_next_run_at()
#[test] fn due_materialization_atomically_creates_occurrence_and_advances_next_run_at()
#[test] fn stale_revision_cannot_materialize_a_due_occurrence()
#[test] fn retention_deletes_only_old_terminal_unlinked_occurrences()
#[test] fn retention_preserves_attention_and_nonterminal_run_links()
```

Use `tempfile::tempdir()` and the fixed timestamp `Utc.with_ymd_and_hms(2026, 8, 5, 10, 0, 0).unwrap()`. For the manual test, read `next_run_at` before and after `create_or_get_occurrence(job_id, scheduled_at, OccurrenceTriggerKind::Manual)` and assert equality.

Run: `cargo test -p gold-band scheduler::db`

Expected: FAIL because schema v2 and the repository methods do not exist.

- [ ] **Step 2: Introduce explicit repository records and update results**

Add these public contracts to `src/scheduler/db.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJobRecord {
    pub definition: ScheduledTaskDefinition,
    pub revision: i64,
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateJobResult {
    Updated(ScheduledJobRecord),
    Conflict(ScheduledJobRecord),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionResult {
    pub deleted: usize,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DueMaterialization {
    Ready { job: ScheduledJobRecord, occurrence: ScheduledOccurrence },
    NotDue,
    Stale,
    Disabled,
}

pub const LEGACY_JSON_MIGRATION: &str = "legacy-json-v1";
pub const LEGACY_SHARED_DB_MIGRATION: &str = "legacy-shared-db-v1";
const SCHEMA_VERSION: i64 = 2;
```

Add repository methods with these signatures:

```rust
pub fn create_job(&self, definition: &ScheduledTaskDefinition, next_run_at: Option<DateTime<Utc>>) -> Result<ScheduledJobRecord>;
pub fn get_job_definition(&self, project_id: &str, job_id: &str) -> Result<Option<ScheduledJobRecord>>;
pub fn update_job(&self, definition: &ScheduledTaskDefinition, expected_updated_at: DateTime<Utc>, next_run_at: Option<DateTime<Utc>>) -> Result<UpdateJobResult>;
pub fn set_job_enabled(&self, project_id: &str, job_id: &str, expected_updated_at: DateTime<Utc>, enabled: bool, next_run_at: Option<DateTime<Utc>>) -> Result<UpdateJobResult>;
pub fn list_enabled_jobs(&self) -> Result<Vec<ScheduledJobRecord>>;
pub fn enabled_job_count(&self) -> Result<usize>;
pub fn delete_job(&self, project_id: &str, job_id: &str) -> Result<bool>;
pub fn materialize_due_occurrence(&self, project_id: &str, job_id: &str, expected_revision: i64, now: DateTime<Utc>) -> Result<DueMaterialization>;
pub fn update_job_runtime_projection(&self, definition: &ScheduledTaskDefinition, expected_revision: i64) -> Result<UpdateJobResult>;
pub fn resume_attention_occurrence(&self, id: &str, owner_id: &str, now: DateTime<Utc>, lease_until: DateTime<Utc>) -> Result<ClaimResult>;
pub fn cleanup_terminal_occurrences(&self, cutoff: DateTime<Utc>, batch_size: usize, protected_run_ids: &HashSet<String>) -> Result<RetentionResult>;
pub fn import_legacy_database_once(&self, source: &ScheduledTaskDatabase, project_id: &str, applied_at: DateTime<Utc>) -> Result<usize>;
```

- [ ] **Step 3: Replace version stamping with ordered migrations**

Schema v2 adds `revision`, `next_run_at`, and a durable migration table:

```sql
CREATE TABLE IF NOT EXISTS scheduler_migrations (
    name TEXT PRIMARY KEY,
    applied_at INTEGER NOT NULL,
    details_json TEXT
);
ALTER TABLE scheduled_jobs ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE scheduled_jobs ADD COLUMN next_run_at INTEGER;
CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_enabled_deadline
    ON scheduled_jobs(enabled, next_run_at)
    WHERE enabled = 1;
```

Implement `migrate_schema_v1_to_v2(transaction)` and update `scheduler_schema.version` only after its transaction commits. Never overwrite a version newer than the binary supports.

- [ ] **Step 4: Make legacy import one transaction with a completion marker**

Read the legacy files into a value object before opening the transaction:

```rust
#[derive(Debug, Clone)]
pub struct LegacySchedulerSnapshot {
    pub definitions: Vec<ScheduledTaskDefinition>,
    pub triggers: BTreeMap<String, Vec<ScheduledTriggerRecord>>,
}
```

Implement:

```rust
pub fn import_legacy_snapshot_once(
    &self,
    snapshot: &LegacySchedulerSnapshot,
    applied_at: DateTime<Utc>,
) -> Result<usize>;
```

The method checks `scheduler_migrations`, imports every definition and trigger in one `TransactionBehavior::Immediate` transaction, writes `LEGACY_JSON_MIGRATION` last, then commits. A conflict rolls back definitions, occurrences, and marker together. Apply the same transaction-and-marker rule to the former shared SQLite database with `LEGACY_SHARED_DB_MIGRATION`, scoped to the current project.

- [ ] **Step 5: Implement optimistic SQL and protected retention cleanup**

The update statement must include the expected timestamp:

```sql
UPDATE scheduled_jobs
SET project_id = ?2,
    enabled = ?3,
    definition_json = ?4,
    next_run_at = ?5,
    revision = revision + 1,
    updated_at = ?6
WHERE id = ?1 AND project_id = ?2 AND updated_at = ?7;
```

`materialize_due_occurrence` must load the matching enabled job/revision, insert or get the scheduled occurrence at the stored `next_run_at`, calculate the following planned point, update `next_run_at` and revision, then commit once. `update_job_runtime_projection` persists Task binding and last-result fields while preserving the existing `next_run_at` column. A crash after due materialization therefore leaves a pending occurrence that startup reconcile can resume.

Retention may delete only terminal rows older than the cutoff, excluding `attention_required` and excluding rows linked to a Run that is still nonterminal according to the caller-provided protected run IDs. Add the protected IDs as a temporary transaction table or bounded SQL parameter set; do not delete Task/Run/ACP files.

- [ ] **Step 6: Pass repository tests, update both document trees, and commit**

Run: `cargo test -p gold-band scheduler::db`

Expected: PASS, including a second legacy import that returns zero without reading JSON again.

```bash
git add src/scheduler src/storage/mod.rs docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: make sqlite authoritative for scheduled tasks"
```

### Task 3: Add One Shared Service And Switch Every Command To SQLite

**Files:**
- Create: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-crud-design.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务 CRUD 与生命周期实现计划.md`
- Test: `src-tauri/src/commands_conversation.rs`
- Test: `src-tauri/src/scheduled_service.rs`

- [ ] **Step 1: Write failing interface tests for create and run-now separation**

Add tests with explicit spies/counters:

```rust
#[test]
fn create_only_persists_definition_and_inputs() {
    let result = service.create(create_input()).unwrap();
    assert!(result.definition.task_id.is_none());
    assert_eq!(repository.list_occurrences(result.definition.id(), 10).unwrap(), Vec::new());
    assert_eq!(execution_spy.start_count(), 0);
}

#[tokio::test]
async fn run_now_creates_manual_occurrence_without_advancing_planned_deadline() {
    let created = service.create(create_input()).unwrap();
    let before = repository.get_job_definition(PROJECT_ID, created.definition.id()).unwrap().unwrap().next_run_at;
    let run = service.run_now(PROJECT_ID, created.definition.id()).await.unwrap();
    let after = repository.get_job_definition(PROJECT_ID, created.definition.id()).unwrap().unwrap().next_run_at;
    assert_eq!(run.occurrence.trigger_kind, OccurrenceTriggerKind::Manual);
    assert_eq!(before, after);
    assert_eq!(execution_spy.start_count(), 1);
}
```

Run: `cargo test -p gold-band-desktop scheduled_service` and `cargo test -p gold-band-desktop commands_conversation::tests`

Expected: FAIL because CRUD is still split between JSON and SQLite and run-now bypasses a shared service.

- [ ] **Step 2: Define a single application-service boundary**

Create `ScheduledTaskService` with these operations:

```rust
pub struct ScheduledTaskService {
    app_handle: AppHandle,
    coordinator: SchedulerCoordinatorHandle,
}

impl ScheduledTaskService {
    pub fn list(&self, project_id: Option<&str>) -> ScheduledServiceResult<Vec<ScheduledTaskDefinition>>;
    pub fn get(&self, project_id: &str, job_id: &str) -> ScheduledServiceResult<ScheduledJobRecord>;
    pub fn create(&self, input: CreateScheduledTaskInputVm) -> ScheduledServiceResult<ScheduledJobRecord>;
    pub fn update(&self, input: UpdateScheduledTaskInputVm) -> ScheduledServiceResult<ScheduledJobRecord>;
    pub fn set_enabled(&self, project_id: &str, job_id: &str, enabled: bool) -> ScheduledServiceResult<ScheduledJobRecord>;
    pub async fn run_now(&self, project_id: &str, job_id: &str) -> ScheduledServiceResult<ManualRunResult>;
    pub fn delete(&self, project_id: &str, job_id: &str) -> ScheduledServiceResult<()>;
}
```

Use a structured service error:

```rust
pub struct ScheduledServiceError {
    pub code: ScheduledErrorCode,
    pub params: serde_json::Value,
    pub trace_id: Option<String>,
}

pub type ScheduledServiceResult<T> = Result<T, ScheduledServiceError>;
```

Tauri maps it to the existing `{ code, params }` `CommandErrorVm`; no method returns localized copy.

- [ ] **Step 3: Make create a definition-only transaction**

The create sequence is fixed:

1. Resolve workspace and validate mode/unattended capability.
2. Build `ScheduledTaskContentSnapshot` and content fingerprint.
3. Copy attachments into a uniquely named staging directory.
4. Atomically rename the staging directory to the new, unique job input directory.
5. Call `repository.create_job(definition, next_run_at)`.
6. If the database transaction fails, remove only the newly created job input directory.
7. Notify the coordinator with `JobCreated` after commit.
8. Return the definition VM.

Do not call `create_conversation_run_vm`, `execute_definition`, an ACP client, or a model provider from this method.

- [ ] **Step 4: Remove active JSON reads, writes, dual writes, and fallback**

Replace every `ScheduledTaskStore::load/save/update/delete/list` call in `commands_conversation.rs` and scheduled ViewModel aggregation with the repository/service. Keep `ScheduledTaskStore` only inside legacy snapshot loading. Replace definition scans with `get_job_definition(project_id, job_id)`.

For delete, atomically rename only that job's input snapshot directory to a job-specific tombstone, delete the SQLite definition/occurrences, restore the directory if the database transaction fails, and remove the tombstone after success. Never delete the linked Task/Run/Round/ACP history.

- [ ] **Step 5: Route run-now through the coordinator and preserve its existing UI action**

`run_now` sends `SchedulerCommand::RunNow` with a one-shot reply. The coordinator creates the manual occurrence and starts it immediately. The service returns `RunScheduledTaskResultVm`; it never calls the create method and never modifies `next_run_at`.

- [ ] **Step 6: Pass command tests, update both document trees, and commit**

Run: `cargo test -p gold-band-desktop scheduled_service`, `cargo test -p gold-band-desktop commands_conversation::tests`, and `cargo test -p gold-band-desktop view_models_conversation::tests`

Expected: PASS. A source scan must show no `ScheduledTaskStore` use outside the migration loader and migration tests:

Run: `rg -n "ScheduledTaskStore" src src-tauri/src`

Expected: matches only `src/scheduler/store.rs`, legacy migration code, and migration tests.

```bash
git add src-tauri/src src/scheduler docs/gold-band/产品设计文档/runtime/scheduled-task-crud-design.md docs/gold-band/产品设计文档/runtime/scheduled-task.md "docs/gold-band/开发计划/定时任务/定时任务 CRUD 与生命周期实现计划.md"
git commit -m "refactor: route scheduled task commands through sqlite service"
```

### Task 4: Replace Fixed Polling With A Deadline-Driven Coordinator

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src-tauri/Cargo.toml`
- Create: `src/scheduler/coordinator.rs`
- Modify: `src/scheduler/mod.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务运行时实现补充.md`
- Test: `src/scheduler/coordinator.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Add Tokio timer dependencies and failing paused-time tests**

Add:

```toml
# Cargo.toml
tokio = { version = "1", features = ["macros", "process", "rt-multi-thread", "fs", "io-util", "sync", "time", "test-util"] }
tokio-util = { version = "0.7", features = ["time"] }

# src-tauri/Cargo.toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time", "test-util"] }
```

Write tests named:

```rust
#[tokio::test(start_paused = true)] async fn create_registers_exactly_one_future_deadline()
#[tokio::test(start_paused = true)] async fn update_replaces_stale_deadline()
#[tokio::test(start_paused = true)] async fn disable_and_delete_cancel_deadline()
#[tokio::test(start_paused = true)] async fn stale_timer_is_a_no_op_after_revision_change()
#[tokio::test(start_paused = true)] async fn manual_run_does_not_replace_scheduled_deadline()
#[tokio::test(start_paused = true)] async fn reconcile_marks_points_beyond_grace_missed_and_keeps_near_late_point()
```

Run: `cargo test -p gold-band scheduler::coordinator` and `cargo test -p gold-band-desktop scheduled_runtime::tests`

Expected: FAIL because `SchedulerCoordinator` and `DelayQueue` do not exist.

- [ ] **Step 2: Define core deadline keys and process-level coordinator commands**

Create the Tauri-independent values in `src/scheduler/coordinator.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScheduledJobKey {
    pub workspace_path: Utf8PathBuf,
    pub project_id: String,
    pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileReason {
    Startup,
    SystemResume,
    TimerDrift,
    Explicit,
}
```

Define the process command in `src-tauri/src/scheduled_runtime.rs`, where Tauri service result types are available:

```rust
#[derive(Debug)]
pub enum SchedulerCommand {
    RegisterWorkspace { workspace_path: Utf8PathBuf },
    UnregisterWorkspace { workspace_path: Utf8PathBuf },
    JobCreated { key: ScheduledJobKey },
    JobUpdated { key: ScheduledJobKey },
    JobEnabled { key: ScheduledJobKey },
    JobDisabled { key: ScheduledJobKey },
    JobDeleted { key: ScheduledJobKey },
    RunNow { key: ScheduledJobKey, reply: oneshot::Sender<ScheduledServiceResult<ManualRunResult>> },
    SettingsChanged,
    Reconcile { reason: ReconcileReason },
    Shutdown,
}
```

- [ ] **Step 3: Implement one DelayQueue entry per enabled job**

Maintain:

```rust
DelayQueue<ScheduledJobKey>
HashMap<ScheduledJobKey, delay_queue::Key>
HashMap<Utf8PathBuf, WorkspaceRegistration>
```

When a command changes a job, re-read its SQLite record and either replace its deadline or remove it. A deadline item carries only the key; processing re-reads `revision`, `enabled`, and `next_run_at`, so a stale wakeup becomes a no-op.

- [ ] **Step 4: Reconcile startup, sleep/resume, expired leases, pending work, and missed points**

For every registered workspace:

1. Run schema migration, `import_legacy_database_once`, and `import_legacy_snapshot_once`.
2. Recover expired leases.
3. Resume pending/retrying occurrences before creating later scheduled occurrences.
4. For points older than `LATE_FIRE_GRACE`, insert `missed` rows and advance in `RETENTION_DELETE_BATCH_SIZE` batches, yielding between batches.
5. Preserve a point within grace as runnable.
6. Schedule the first future `next_run_at` for each enabled job.

- [ ] **Step 5: Start and stop through Tauri async runtime**

Replace the named `std::thread` and `thread::sleep` loop with `tauri::async_runtime::spawn`. Store `SchedulerCoordinatorHandle` in `DesktopState`; on `RunEvent::ExitRequested` send `Shutdown` and release power state. Remove `POLL_CAP`, the one-second scan, and heartbeat work from `tick()`.

- [ ] **Step 6: Pass paused-time tests, prove polling is gone, update docs, and commit**

Run: `cargo test -p gold-band scheduler::coordinator` and `cargo test -p gold-band-desktop scheduled_runtime::tests`

Expected: PASS.

Run: `rg -n "POLL_CAP|thread::sleep|list_job_definitions_for_project" src-tauri/src/scheduled_runtime.rs`

Expected: no matches.

```bash
git add Cargo.toml Cargo.lock src-tauri/Cargo.toml src/scheduler src-tauri/src/scheduled_runtime.rs src-tauri/src/state.rs src-tauri/src/commands_conversation.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务运行时实现补充.md
git commit -m "feat: coordinate scheduled jobs by deadline"
```

### Task 5: Centralize Queue Decisions, Leases, And Four Execution Adapters

**Files:**
- Create: `src-tauri/src/scheduled_runtime/execution.rs`
- Create: `src-tauri/src/scheduled_runtime/lease.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src/scheduler/queue.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/node_executor.rs`
- Modify: `src/app/orchestrator.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src-tauri/src/scheduled_runtime/execution.rs`
- Test: `src-tauri/src/scheduled_runtime/lease.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Write failing adapter and lifecycle tests**

Add tests named:

```rust
#[test] fn direct_new_materializes_a_new_task_run_and_session_per_occurrence()
#[test] fn direct_continuous_reuses_the_resumable_task_and_session_chain()
#[test] fn workflow_reuses_task_for_same_fingerprint_and_creates_a_new_run()
#[test] fn workflow_authoring_change_materializes_a_new_task()
#[test] fn auto_preserves_goal_allowed_workflows_and_authoring_identity()
#[test] fn start_success_keeps_occurrence_running_until_lifecycle_completion()
#[test] fn every_active_state_is_classified_by_decide_queue()
#[tokio::test] async fn heartbeat_runs_independently_of_coordinator_deadlines()
#[tokio::test] async fn lost_lease_cancels_guard_and_records_lease_lost()
```

Run: `cargo test -p gold-band-desktop scheduled_runtime`

Expected: FAIL while execution and heartbeat remain embedded in the polling runtime.

- [ ] **Step 2: Define adapter inputs and outputs**

Use one trait and one binding type:

```rust
pub struct ScheduledExecutionContext<'a> {
    pub app_handle: &'a AppHandle,
    pub app: &'a App,
    pub definition: &'a mut ScheduledTaskDefinition,
    pub occurrence: &'a ScheduledOccurrence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionBinding {
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub round_id: Option<String>,
    pub attempt_id: Option<String>,
    pub session_id: Option<String>,
}

pub trait ScheduledExecutionAdapter: Send + Sync {
    fn start(&self, context: ScheduledExecutionContext<'_>) -> anyhow::Result<ExecutionBinding>;
}
```

`start` means the existing runtime accepted and started work. It never maps to `OccurrenceStatus::Succeeded`.

- [ ] **Step 3: Replace the boolean active probe with the shared queue domain**

Map runtime state to `ActiveExecution::{Idle, Running, PermissionWaiting, WaitingForUserInput, ResumablePaused}`. Call only:

```rust
decide_queue(definition.overlap_policy, active, occurrence.attempt as u8, now)
```

Handle `QueueDecision::RetryAt` by setting the occurrence to `retrying` and scheduling its retry deadline; handle `Skipped` as a terminal occurrence. Delete handwritten `3`, `30 seconds`, and overlap branches from Tauri runtime code. Scheduled and manual occurrences use this same path.

- [ ] **Step 4: Add a per-occurrence lease guard**

`OccurrenceExecutionGuard` owns an interval task based on `LeaseConfig::heartbeat_interval()`. It renews only its occurrence, stops on terminal lifecycle completion, and reports `LeaseLost` when renewal fails. The coordinator never scans active executions for heartbeat work.

- [ ] **Step 5: Preserve real lifecycle completion and interventions**

Continue carrying `scheduled_occurrence_id` through background App clones and lifecycle events. Map:

```text
RunCompleted(Success) / AcpTurnFinished(Success) -> succeeded
RunCompleted(Failure) / AcpTurnFinished(Failure) -> failed
Permission request -> failed + SCHEDULED_PERMISSION_REQUIRED
AskUserQuestion -> attention_required + SCHEDULED_USER_INPUT_REQUIRED
```

An `attention_required` Run remains resumable. The elicitation-response path calls `resume_attention_occurrence` for the same occurrence and starts a new lease guard before resuming its Run; its later completion updates that occurrence rather than creating a new one.

- [ ] **Step 6: Pass runtime tests, update both document trees, and commit**

Run: `cargo test -p gold-band scheduler::queue`, `cargo test -p gold-band app::tests`, and `cargo test -p gold-band-desktop scheduled_runtime`

Expected: PASS.

```bash
git add src/scheduler/queue.rs src/app src-tauri/src/commands.rs src-tauri/src/scheduled_runtime.rs src-tauri/src/scheduled_runtime docs/gold-band/产品设计文档/runtime/scheduled-task.md docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: bind scheduled occurrences to mode adapters"
```

### Task 6: Add Global Keep-Awake And Retention Settings

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src/config/mod.rs`
- Create: `src-tauri/src/scheduled_runtime/power.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `docs/gold-band/产品设计文档/interaction/app/settings.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src/config/mod.rs`
- Test: `src-tauri/src/scheduled_runtime/power.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Add the mature platform dependency and failing state-machine tests**

Add:

```toml
keepawake = "0.6.0"
```

Write tests for this exact activation condition:

```rust
keep_awake_enabled && enabled_job_count > 0 && app_is_running
```

Cover repeated enable, repeated disable, last-job-disabled, settings-off, shutdown, and acquire failure. Assert acquire/release are idempotent and a platform failure does not stop scheduling.

Run: `cargo test -p gold-band-desktop scheduled_runtime::power`

Expected: FAIL until the controller exists.

- [ ] **Step 2: Add persisted settings and validation**

Add to `SettingsConfig`:

```rust
pub scheduled_keep_awake_enabled: Option<bool>,
pub scheduled_completion_notifications_enabled: Option<bool>,
pub scheduled_occurrence_retention_days: Option<u16>,
```

Add to `RuntimeConfig`:

```rust
pub scheduled_keep_awake_enabled: bool,
pub scheduled_completion_notifications_enabled: bool,
pub scheduled_occurrence_retention_days: u16,
```

Defaults are `false` for keep-awake, `true` for completion notifications, and `DEFAULT_OCCURRENCE_RETENTION_DAYS` for retention. Reject retention values outside `1..=3650` with a structured code and params. Add `get_scheduled_runtime_settings` and `save_scheduled_runtime_settings` commands returning:

Bump `CURRENT_SETTINGS_SCHEMA_VERSION` from 2 to 3. The version-3 migration writes the three defaults when absent, preserves explicit values, and is covered by a load-migrate-save-reload test.

```rust
pub struct ScheduledRuntimeSettingsVm {
    pub keep_awake_enabled: bool,
    pub keep_awake_effective: bool,
    pub completion_notifications_enabled: bool,
    pub enabled_job_count: usize,
    pub occurrence_retention_days: u16,
    pub power_error_code: Option<String>,
}
```

- [ ] **Step 3: Implement the process-level sleep inhibitor**

Define a narrow interface:

```rust
pub trait SystemSleepInhibitor: Send {
    fn acquire(&mut self, reason: &str) -> Result<(), ScheduledError>;
    fn release(&mut self);
}
```

The production implementation retains the guard returned by:

```rust
keepawake::Builder::default()
    .display(false)
    .idle(true)
    .sleep(false)
    .create()
```

This prevents automatic system sleep while allowing the display to turn off. It starts no external command and therefore does not create a console window.

- [ ] **Step 4: Run bounded retention from coordinator maintenance**

After startup reconcile and once after a terminal occurrence, request one cleanup batch per workspace. Continue batches by yielding back to Tokio. Keep cleanup failures in structured diagnostics and leave occurrence results unchanged.

- [ ] **Step 5: Pass tests, update both document trees, and commit**

Run: `cargo test -p gold-band config::tests`, `cargo test -p gold-band-desktop scheduled_runtime::power`, and `cargo test -p gold-band-desktop scheduled_runtime::tests`

Expected: PASS.

```bash
git add src/config/mod.rs src-tauri/Cargo.toml Cargo.lock src-tauri/src/scheduled_runtime.rs src-tauri/src/scheduled_runtime src-tauri/src/scheduled_service.rs src-tauri/src/commands_conversation.rs src-tauri/src/view_models_conversation.rs src-tauri/src/main.rs docs/gold-band/产品设计文档/interaction/app/settings.md docs/gold-band/产品设计文档/runtime/scheduled-task.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: add scheduled keep awake and retention settings"
```

### Task 7: Emit Structured Scheduled Notifications And Deep Links

**Files:**
- Create: `src-tauri/src/scheduled_runtime/notification.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/notifications.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `web/src/api/client.ts`
- Modify: `web/src/api/desktop.ts`
- Modify: `web/src/api/browser.ts`
- Modify: `web/src/api.ts`
- Create: `web/src/lib/use-scheduled-notifications.ts`
- Modify: `web/src/App.tsx`
- Modify: `web/src/i18n.ts`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src-tauri/src/notifications.rs`
- Test: `web/tests/scheduled-task-notifications.test.ts`

- [ ] **Step 1: Write failing event-mapping and dedup tests**

Cover:

```text
succeeded -> completion event only when scheduled_completion_notifications_enabled is true
failed -> immediate failure event
attention_required -> immediate attention event
missed -> one aggregate event per reconcile batch
skipped/retrying -> history only, no OS notification
same occurrence/kind -> one notification
```

Run: `cargo test -p gold-band-desktop notifications` and `npm run web:test -- scheduled-task-notifications`

Expected: FAIL because scheduled outcomes are not mapped into the notification pipeline.

- [ ] **Step 2: Define a backend event without customer copy**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledNotificationEventVm {
    pub event_id: String,
    pub kind: String,
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: Option<String>,
    pub error_code: Option<String>,
    pub error_params: Option<serde_json::Value>,
    pub links: OccurrenceLinks,
    pub missed_count: Option<u32>,
}
```

Emit `gold-band://scheduled-notification`. The frontend hook localizes title/body with `i18next`, then calls a narrow native notification command. Reuse `NotificationDedup` and the existing toast action path.

- [ ] **Step 3: Extend navigation payloads for scheduled details and resumable attempts**

The toast action payload carries `projectId`, `scheduledTaskId`, and optional Task/Run/Round/Attempt links. Failed notifications open the occurrence detail; attention notifications open the linked run and attempt; completion opens the linked run when present and otherwise the scheduled detail.

- [ ] **Step 4: Pass Rust/Web tests, update both document trees, and commit**

Run: `cargo test -p gold-band-desktop notifications`, `cargo test -p gold-band-desktop scheduled_runtime`, and `npm run web:test -- scheduled-task-notifications`

Expected: PASS.

```bash
git add src-tauri/src web/src web/tests/scheduled-task-notifications.test.ts docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: notify scheduled occurrence outcomes"
```

### Task 8: Complete The Management UI, History, IANA Timezones, Links, And i18n

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `web/src/types.ts`
- Modify: `web/src/api/client.ts`
- Modify: `web/src/api/desktop.ts`
- Modify: `web/src/api/browser.ts`
- Modify: `web/src/api.ts`
- Modify: `web/src/components/conversation/ScheduledTaskDialog.tsx`
- Create: `web/src/components/scheduled-tasks/TimezoneCombobox.tsx`
- Create: `web/src/components/scheduled-tasks/ScheduledRuntimeSettings.tsx`
- Create: `web/src/lib/scheduled-task-navigation.ts`
- Modify: `web/src/pages/ScheduledTaskManagementPage.tsx`
- Modify: `web/src/pages/ScheduledTaskDetailPage.tsx`
- Modify: `web/src/pages/SettingsPage.tsx`
- Modify: `web/src/routes.ts`
- Modify: `web/src/App.tsx`
- Modify: `web/src/i18n.ts`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/settings.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务全局管理与会话刷新实现计划.md`
- Test: `web/tests/scheduled-task-management-page.test.ts`
- Test: `web/tests/scheduled-task-detail-page.test.ts`
- Test: `web/tests/scheduled-task-settings.test.ts`
- Test: `web/tests/scheduled-task-timezones.test.ts`
- Test: `web/tests/browser-scheduled-task-api.test.ts`
- Test: `web/tests/api/desktop.test.ts`

- [ ] **Step 1: Add a mature IANA fallback and write failing Web tests**

Install the maintained timezone data package:

Run: `npm install @vvo/tzdb`

Tests must assert:

```ts
expect(getScheduledTimezones().length).toBeGreaterThan(300);
expect(getScheduledTimezones()).toContain('UTC');
expect(getScheduledTimezones()).toContain(Intl.DateTimeFormat().resolvedOptions().timeZone);
expect(historyStatuses).toContain('skipped');
expect(historyStatuses).toContain('missed');
expect(runNowAfterCreate).not.toHaveBeenCalledByCreate();
```

Also cover keep-awake effective state, retention range, status filtering, Task/Run/attempt link targets, desktop command arguments, browser parity, and both language trees.

Run: `npm run web:test -- scheduled-task`

Expected: FAIL until the new helpers and UI exist.

- [ ] **Step 2: Build the complete timezone combobox from shadcn/ui copy-in components**

`getScheduledTimezones()` uses `Intl.supportedValuesOf('timeZone')` when available and `@vvo/tzdb` otherwise, deduplicates, sorts, prepends `UTC`, and includes the resolved system zone. `TimezoneCombobox` uses the existing shadcn Popover/Command/Button primitives and Lucide `ChevronsUpDown`/`Check`; it does not hand-roll listbox behavior.

- [ ] **Step 3: Return all occurrence statuses and make links actionable**

Delete the backend `Skipped | Missed` filter. The detail page defaults to all statuses and offers a status menu. Implement:

```ts
export function scheduledOccurrenceTarget(
  projectId: string,
  occurrence: ScheduledOccurrenceVm,
): ConversationPage | null {
  if (!occurrence.taskId || !occurrence.runId) return null;
  return {
    kind: 'conversation-run',
    projectId,
    taskId: occurrence.taskId,
    runId: occurrence.runId,
    roundId: occurrence.roundId ?? undefined,
    attemptId: occurrence.attemptId ?? undefined,
  };
}
```

Extend `ConversationPage`, route parsing, and run selection so linked attempts open directly. Use icon buttons with tooltips for navigation.

- [ ] **Step 4: Expose the same runtime settings in management and Settings pages**

Render `ScheduledRuntimeSettings` in both places. Use shadcn `Switch` controls for keep-awake and completion notifications, plus a numeric input constrained to `1..3650` for retention. Show whether keep-awake is currently effective and a localized structured error when acquisition failed. Do not describe internal APIs or timer implementation in the UI.

- [ ] **Step 5: Move every scheduled-task customer string into i18n**

Replace hardcoded Chinese in:

```text
ScheduledTaskDialog.tsx
ConversationComposer.tsx scheduled-task controls
ScheduledTaskManagementPage.tsx
ScheduledTaskDetailPage.tsx
```

Add matching `zh-CN` and `en` keys for statuses, trigger kinds, errors, settings, confirmation dialogs, tooltips, empty states, and notifications. Rust ViewModels return raw timestamps/enums instead of Chinese schedule labels; format them in the web presentation layer.

Make this a development-stage contract replacement: change `ScheduledTaskVm.schedule` to the typed `ScheduleSpec`, and remove `scheduleLabel`, `timezoneLabel`, and `lastTriggerLabel` from Rust and TypeScript. The frontend derives all three localized labels from typed values. Empty instruction titles return an empty value from Rust and use the localized unnamed label in the frontend.

- [ ] **Step 6: Pass Web tests and production build, update both document trees, and commit**

Run: `npm run web:test -- scheduled-task api/desktop` and `npm run web:build`

Expected: PASS.

```bash
git add package.json package-lock.json web docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md docs/gold-band/产品设计文档/interaction/app/settings.md docs/gold-band/开发计划/定时任务/定时任务全局管理与会话刷新实现计划.md
git commit -m "feat: complete scheduled task management experience"
```

### Task 9: Publish A Localized Built-In Skill And Typed Internal MCP Tools

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `src-tauri/Cargo.toml`
- Create: `src/skill/builtin.rs`
- Modify: `src/skill/mod.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/node_executor.rs`
- Modify: `src/prompts.rs`
- Create: `src/prompts/zh-CN/skills/scheduled-task/SKILL.md`
- Create: `src/prompts/en/skills/scheduled-task/SKILL.md`
- Create: `src-tauri/src/scheduled_tools.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/view_models.rs`
- Modify: `web/src/types.ts`
- Modify: `web/src/pages/ContextManagementPage.tsx`
- Modify: `docs/gold-band/产品设计文档/MCP-SKILL管理-完整设计方案.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src/skill/builtin.rs`
- Test: `src-tauri/src/scheduled_tools.rs`
- Test: `web/tests/skill-agent-display.test.ts`

- [ ] **Step 1: Add official MCP dependencies and failing registry/schema tests**

Add compatible current versions:

```toml
rmcp = { version = "3.1.0", features = ["server", "macros", "transport-streamable-http-server"] }
axum = "0.8"
schemars = { version = "1.0", features = ["derive"] }
```

Tests must prove:

```text
zh-CN and en have the same relative built-in Skill paths
SkillSource::BuiltIn lists and reads scheduled-task
built-in Skills are read-only
every MCP tool has a stable JSON schema
create validates Direct/Workflow/AUTO required fields
MCP create delegates to ScheduledTaskService and starts no execution
pause/resume map to set_enabled
run-now produces a manual occurrence and leaves next_run_at unchanged
```

Run: `cargo test -p gold-band skill::builtin` and `cargo test -p gold-band-desktop scheduled_tools`

Expected: FAIL until registry and tool server exist.

- [ ] **Step 2: Add a read-only localized built-in Skill registry**

Add `built_in: Vec<SkillMeta>` to `SkillListResult`. Add `desktop_language: DesktopLanguage` to `SkillManager`, pass `App.config.desktop_language` from `App::skill_manager()`, and use it when `SkillManager::read(name, BuiltIn)` selects the localized resource. `SkillManager::list()` merges registry metadata separately from Global/Project precedence. Write/delete/sync requests for `BuiltIn` return `SCHEDULED_SKILL_VALIDATION_FAILED` or the existing read-only Skill error, never a filesystem path fallback.

The two `SKILL.md` files use matching frontmatter and section structure. They teach the Agent to confirm ambiguous schedule/timezone/mode inputs, use typed tools, distinguish create from run-now, inspect structured results, and never claim completion before a terminal occurrence.

- [ ] **Step 3: Define typed MCP input models**

Keep all MCP schema types in the Tauri crate so they derive `schemars 1.x` without requiring the core crate's existing `schemars 0.8` domain types to implement the other version's trait. Use tagged mode-specific content rather than free-form JSON:

```rust
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledToolSessionPolicy {
    New,
    Continuous,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledDirectToolConfig {
    pub agent_type: String,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    pub config_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ScheduledToolContent {
    Direct {
        instruction: String,
        direct_config: ScheduledDirectToolConfig,
        session_policy: ScheduledToolSessionPolicy,
    },
    Workflow {
        instruction: String,
        workflow_template_id: String,
        include_interview: Option<bool>,
    },
    Auto {
        instruction: String,
        auto_config: ConversationAutoConfigVm,
    },
}
```

Define Tauri-local `ScheduledToolSchedule`, `ScheduledWorkflowToolConfig`, and `ScheduledAutoToolConfig` with the same approach and explicit `TryFrom` conversions into `ScheduleSpec` and the existing conversation ViewModels. Do not use `serde_json::Value` for mode, schedule, Agent, workflow, or AUTO authoring inputs.

Expose `scheduled_create`, `scheduled_list`, `scheduled_get`, `scheduled_update`, `scheduled_pause`, `scheduled_resume`, `scheduled_run_now`, and `scheduled_delete`. Every tool returns `{ data }` or `{ error: { code, params, traceId } }`.

- [ ] **Step 4: Run one loopback Streamable HTTP server in-process**

Bind `127.0.0.1:0`, retain its cancellation token and endpoint in `DesktopState`, and shut it down with the app. It must not persist a user-editable MCP entry, launch a CLI, use environment-variable protocols, or invoke `std::process::Command`.

Merge its ephemeral endpoint into `App::acp_mcp_servers()`. Change Workflow node execution to call `app.acp_mcp_servers()` instead of constructing `McpManager` directly, so Direct, Workflow, and AUTO receive the same built-in tools.

- [ ] **Step 5: Inject the built-in Skill as fixed runtime system context**

Render through `src/prompts.rs` and add it to `WorkerInvocation.extra_system_sections` when the scheduled tool endpoint is available. Keep the long bilingual content out of Rust. The current task/goal remains user prompt content; tool rules and capability boundaries remain system context.

- [ ] **Step 6: Pass Skill/tool/Web tests, update both document trees, and commit**

Run: `cargo test -p gold-band skill` and `cargo test -p gold-band-desktop scheduled_tools` and `npm run web:test -- skill-agent-display`

Expected: PASS.

```bash
git add Cargo.toml Cargo.lock src-tauri/Cargo.toml src/skill src/app src/prompts.rs src/prompts/zh-CN/skills src/prompts/en/skills src-tauri/src/scheduled_tools.rs src-tauri/src/commands.rs src-tauri/src/state.rs src-tauri/src/main.rs src-tauri/src/scheduled_service.rs src-tauri/src/view_models.rs web/src/types.ts web/src/pages/ContextManagementPage.tsx web/tests/skill-agent-display.test.ts docs/gold-band/产品设计文档/MCP-SKILL管理-完整设计方案.md docs/gold-band/产品设计文档/runtime/scheduled-task.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: add typed scheduled task agent tools"
```

### Task 10: Add User-Confirmed “Save As Skill” From Successful Occurrences

**Files:**
- Create: `src/skill/scheduled.rs`
- Modify: `src/skill/mod.rs`
- Modify: `src/scheduler/mod.rs`
- Create: `src/prompts/zh-CN/skills/scheduled-task/save-as-skill-draft.md`
- Create: `src/prompts/en/skills/scheduled-task/save-as-skill-draft.md`
- Modify: `src/prompts.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `web/src/types.ts`
- Modify: `web/src/api/client.ts`
- Modify: `web/src/api/desktop.ts`
- Modify: `web/src/api/browser.ts`
- Modify: `web/src/api.ts`
- Create: `web/src/components/ui/checkbox.tsx`
- Create: `web/src/components/scheduled-tasks/SaveScheduledTaskAsSkillDialog.tsx`
- Modify: `web/src/pages/ScheduledTaskDetailPage.tsx`
- Modify: `web/src/i18n.ts`
- Modify: `docs/gold-band/产品设计文档/MCP-SKILL管理-完整设计方案.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Test: `src/skill/scheduled.rs`
- Test: `src-tauri/src/commands_conversation.rs`
- Test: `web/tests/scheduled-task-save-as-skill.test.ts`

- [ ] **Step 1: Write failing preview/confirm/conflict tests**

Cover these rules:

```text
only succeeded occurrences can generate a draft
preview reads instruction, mode, stable steps, artifacts, and execution provenance
preview does not write a Skill
confirm writes only Project or Global source selected by the user
confirm never overwrites an existing Skill
confirm does not modify the scheduled job unless linkSkill is explicitly true
Direct/Workflow/AUTO provenance is retained in the draft request
```

Run: `cargo test -p gold-band skill::scheduled` and `cargo test -p gold-band-desktop scheduled_skill` and `npm run web:test -- scheduled-task-save-as-skill`

Expected: FAIL until preview and confirm APIs exist.

- [ ] **Step 2: Define preview and confirmed-write contracts**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledSkillDraftVm {
    pub draft_id: String,
    pub suggested_name: String,
    pub description: String,
    pub content: String,
    pub source_options: Vec<String>,
    pub occurrence_id: String,
    pub content_fingerprint: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmScheduledSkillInputVm {
    pub draft_id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub source: SkillSource,
    pub project_id: String,
    pub link_skill: bool,
}
```

`preview_scheduled_occurrence_skill` creates an expiring in-memory draft tied to occurrence ID and content fingerprint. `confirm_scheduled_occurrence_skill` revalidates the draft, checks name conflict, then delegates to the existing atomic `SkillManager` write flow.

Add the traceable reference to the scheduled definition:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledSkillReference {
    pub name: String,
    pub source: SkillSource,
    pub content_sha256: String,
    pub linked_at: DateTime<Utc>,
}
```

`link_skill = true` saves this reference through the optimistic SQLite update after the Skill write succeeds. It does not overwrite the Skill, automatically update the reference when Skill content later changes, or change the planned `next_run_at`. `link_skill = false` leaves the definition unchanged. Use a named `SCHEDULED_SKILL_DRAFT_TTL` duration for draft expiry rather than an inline numeric timeout.

- [ ] **Step 3: Render a deterministic draft from mirrored prompt resources**

Render `save-as-skill-draft.md` with Minijinja using the occurrence goal, instruction, mode, captured tool/step provenance, artifacts, and output locations. This preview action does not call a model, so it does not need to choose or mutate an Agent session. Validate the rendered frontmatter and body through the existing Skill parser; invalid provenance or output returns `SCHEDULED_SKILL_VALIDATION_FAILED` with params.

- [ ] **Step 4: Build the explicit preview and confirmation dialog**

Show editable name, description, content, Project/Global source, conflict status, and optional “link this scheduled task to the Skill” checkbox. The primary confirm button is the only write action. Closing/canceling discards the draft. Generate the missing Checkbox through the shadcn/ui copy-in flow, then use Dialog/Input/Textarea/Select/Checkbox/Button components and localized copy.

- [ ] **Step 5: Pass tests, update both document trees, and commit**

Run: `cargo test -p gold-band skill::scheduled` and `cargo test -p gold-band-desktop scheduled_skill` and `npm run web:test -- scheduled-task-save-as-skill`

Expected: PASS.

```bash
git add src/skill src/scheduler/mod.rs src/prompts.rs src/prompts/zh-CN/skills/scheduled-task src/prompts/en/skills/scheduled-task src-tauri/src web/src web/tests/scheduled-task-save-as-skill.test.ts docs/gold-band/产品设计文档/MCP-SKILL管理-完整设计方案.md docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: save successful scheduled runs as skills"
```

### Task 11: Full Regression, Desktop Deep-Link Verification, And Documentation Closure

**Files:**
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/settings.md`
- Modify: `docs/gold-band/产品设计文档/MCP-SKILL管理-完整设计方案.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Modify: `docs/gold-band/开发计划/功能点todo列表.md`

- [ ] **Step 1: Run formatting, all interface tests, builds, and diff checks**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
npm run web:test
npm run web:build
git diff --check
```

Expected: every command exits 0. No test may rely on a one-second sleep to observe scheduled state.

- [ ] **Step 2: Verify the user-visible flow in a running frontend**

Start: `npm run web:dev`

Use the browser facade to create one Direct scheduled definition without running it. Record its ID, then deep-link directly to:

```text
http://127.0.0.1:1420/chat/scheduled-tasks/{scheduledTaskId}
```

Verify desktop and mobile-width screenshots for:

```text
creation leaves history empty and does not navigate to a Run
run-now creates a manual row and keeps the planned next time unchanged
skipped and missed rows remain visible
Task/Run/attempt links navigate correctly
IANA timezone search works
keep-awake switch shows enabled versus effective state
Skill preview writes nothing before confirmation
zh-CN and en text fits without overlap
```

Use the in-memory browser facade or a disposable temporary workspace so verification does not touch user projects. Stop the dev server afterward; discard the facade state or remove the entire validated temporary workspace so no scheduled definition, Skill, Task, or Run test resource remains.

- [ ] **Step 3: Record only verified completion in both document trees**

Update architecture, schema, UI, Skill/MCP, migration, power, notification, retention, and testing sections. Change the feature todo row from the old “missed not implemented” statement only after the corresponding tests and visual checks pass.

- [ ] **Step 4: Review the final scope and commit documentation**

Run: `git status --short`, `git diff --stat`, and `git log -12 --oneline`.

Expected: no unrelated user files are staged; every implementation commit contains product-design and development-plan updates.

```bash
git add docs/gold-band/产品设计文档 docs/gold-band/开发计划
git commit -m "docs: close scheduled runtime verification"
```

## Final Acceptance Matrix

| Contract | Required evidence |
|---|---|
| Create saves only definition | Tauri service test: no occurrence, no Task ID, execution spy count 0 |
| Planned first run materializes at deadline | paused-time coordinator integration test |
| Run-now remains immediate | manual occurrence integration test and UI action test |
| Run-now preserves schedule | repository `next_run_at` before/after equality |
| SQLite is sole authority | source scan plus CRUD/migration tests |
| No one-second global polling | source scan plus paused-time DelayQueue tests |
| Four mode semantics | adapter interface tests for Direct/new, Direct/continuous, Workflow, AUTO |
| Lease/queue correctness | concurrent claim, heartbeat, lease-loss, and centralized queue tests |
| Keep-awake behavior | fake inhibitor state-machine tests and narrow platform smoke test |
| Full history and retention | repository cleanup tests and detail-page rendering tests |
| Localized notifications | event mapping/dedup tests and zh-CN/en Web tests |
| Built-in Skill tools | mirrored resource test, MCP schema test, service-delegation test |
| Save as Skill confirmation | preview-no-write, conflict, confirm-write, explicit-link tests |
| Desktop UX | deep-link screenshots at desktop and mobile widths |
