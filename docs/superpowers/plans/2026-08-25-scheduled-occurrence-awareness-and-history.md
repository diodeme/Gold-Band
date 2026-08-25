# Scheduled Occurrence Awareness And Execution History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every accepted scheduled occurrence carry an immutable execution snapshot, make every participating Agent recognize the scheduled execution, render one structured trigger row, and replace automatic occurrence retention with user-managed Run history.

**Architecture:** Keep `ScheduledTaskDefinition` as mutable scheduler authoring and use `ScheduledOccurrence` as the accepted execution identity. At the acceptance transaction, bind the latest content snapshot and full Run locator to the occurrence; project that fact into one visible `scheduledTrigger` Timeline item and one hidden provider prompt. User-facing history is a paged grouping of accepted occurrences by Run, while non-accepted scheduler outcomes remain operational state and never become execution history.

**Tech Stack:** Rust 2024, `rusqlite`, Tokio, Tauri 2, ACP Timeline JSONL/index, MiniJinja prompts, React 19, TypeScript, Tailwind CSS, shadcn/ui, prompt-kit, i18next, Vitest.

---

## Confirmed Product Contract

1. A trigger fact exists only after Gold Band reliably accepts an execution. `pending`, pre-acceptance `retrying`, `missed`, `skipped`, and invalidated work are not trigger facts.
2. Automatic and run-now triggers are distinct: `scheduled` has `scheduledAt`; `manual` does not fabricate one.
3. The Agent receives a hidden occurrence protocol plus exactly one unmodified instruction. The protocol is turn-scoped to the occurrence execution chain and does not leak into later user turns.
4. Accepted content is immutable. Instruction/content edits before acceptance win; edits after acceptance affect only later occurrences. Schedule edits/disable/delete invalidate only unaccepted automatic occurrences.
5. The visible Timeline contains one immutable row per accepted occurrence: `定时任务触发 · <summary>` or `手动执行定时任务 · <summary>`.
6. There is no task-title field. `instructionSummary` is presentation-only, deterministic, and frozen at acceptance.
7. Execution history follows the AionUi conversation-history principle: only real execution containers are listed, there is no age-based retention setting, and users delete complete Runs. Direct continuous may group multiple occurrences in one Run.
8. Deleting a scheduled definition stops future scheduling but preserves accepted history. Deleting an active history Run first requests a durable stop and deletes only after terminal settlement.

## Superseded Existing Behavior

This plan explicitly replaces these earlier baselines:

- `ScheduledTaskContextInfo.title/mode/session_policy/triggered_at` and RuntimeManaged-only rendering.
- `occurrenceRetentionDays`, coordinator retention batches, and user-visible skipped/missed occurrence history.
- `ScheduledTaskVm.title` as a domain-like field. A display summary may remain in view models but must be named `instructionSummary`.
- `ON DELETE CASCADE` from scheduled definitions to accepted occurrences.
- The detail page's occurrence-status history as the primary execution-history list.

## File And Ownership Map

- `src/scheduler/execution.rs`: deterministic summary extraction and immutable accepted-execution value objects.
- `src/scheduler/occurrence.rs`: occurrence lifecycle, full Run locator, and optional accepted snapshot.
- `src/scheduler/db.rs`: schema v2 accepted-occurrence migration, schema v3 deletion-operation migration, acceptance CAS, unaccepted invalidation, and accepted-history paging/grouping.
- `src-tauri/src/scheduled_runtime.rs`: reload-before-accept behavior, schedule revision checks, execution-chain context, and lifecycle-driven deletion settlement.
- `src/provider/mod.rs`: one final scheduled-context projection for every prompt envelope.
- `src/prompts/{zh-CN,en}/runtime/scheduled_task_context.md`: bilingual hidden execution protocol.
- `src/acp/events.rs`, `src/acp/client.rs`, `src/acp/timeline.rs`: durable hidden prompt plus one idempotent `scheduledTrigger` event.
- `src/app/history_deletion.rs`: reusable stop-then-delete Run operation and empty-Task cleanup.
- `src-tauri/src/commands_conversation.rs`, `src-tauri/src/view_models_conversation.rs`: history list/delete commands and VMs.
- `web/src/components/conversation/ScheduledTriggerRow.tsx`: AlarmClock divider row.
- `web/src/pages/ScheduledTaskDetailPage.tsx`: Run history, batch deletion, occurrence focus, and deleted-definition read-only state.
- `web/src/lib/scheduled-task-navigation.ts`, `web/src/routes.ts`: complete deep-link locator.
- `web/src/components/scheduled-tasks/ScheduledRuntimeSettings.tsx`: remove retention controls while preserving keep-awake and completion notification settings.
- `docs/gold-band/产品设计文档/runtime/scheduled-task*.md`, `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`: canonical design updates.
- `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`: supersession and implementation progress.

### Task 1: Freeze Accepted-Execution Types And Summary Semantics

**Files:**
- Create: `src/scheduler/execution.rs`
- Modify: `src/scheduler/mod.rs`
- Modify: `src/scheduler/occurrence.rs`
- Modify: `Cargo.toml`
- Test: `src/scheduler/execution.rs`
- Test: `src/scheduler/occurrence.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [x] **Step 1: Write failing domain tests**

Add tests with these exact cases:

```rust
#[test]
fn instruction_summary_uses_first_non_empty_markdown_block() {
    assert_eq!(
        instruction_summary("\n# 每日代码检查\n\n检查主分支测试。", 120),
        "每日代码检查"
    );
}

#[test]
fn instruction_summary_collapses_whitespace_and_has_a_stable_limit() {
    assert_eq!(instruction_summary("- alpha   beta", 8), "alpha be");
}

#[test]
fn instruction_summary_keeps_the_complete_first_markdown_block() {
    assert_eq!(
        instruction_summary("first line\nsecond line\n\nignored block", 120),
        "first line second line"
    );
}

#[test]
fn accepted_snapshot_keeps_full_content_and_presentation_summary_separate() {
    let snapshot = accepted_snapshot_fixture();
    assert_eq!(snapshot.instruction_summary, "每日代码检查");
    assert_eq!(snapshot.content.instruction, "# 每日代码检查\n\n检查主分支测试。");
}

#[test]
fn occurrence_links_cover_the_complete_run_locator() {
    let links = OccurrenceLinks {
        task_id: Some("task-1".into()),
        run_id: Some("run-1".into()),
        round_id: Some("round-1".into()),
        node_id: Some("node-1".into()),
        attempt_id: Some("attempt-1".into()),
    };
    assert!(links.is_complete());
}
```

- [x] **Step 2: Run the tests and verify RED**

Run:

```powershell
cargo test -p gold-band scheduler::execution
cargo test -p gold-band scheduler::occurrence
```

Expected: compilation fails because `execution`, `instruction_summary`, `ScheduledExecutionSnapshot`, `node_id`, and `OccurrenceLinks::is_complete` do not exist.

- [x] **Step 3: Add the focused domain module and types**

Add `pulldown-cmark = "0.13"` to root dependencies and export `pub mod execution;` from `src/scheduler/mod.rs`. Define:

```rust
use chrono::{DateTime, Utc};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use super::ScheduledTaskContentSnapshot;

pub const SCHEDULED_INSTRUCTION_SUMMARY_MAX_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledAutomaticTriggerContext {
    pub scheduled_at: DateTime<Utc>,
    pub schedule_summary: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledExecutionSnapshot {
    pub accepted_at: DateTime<Utc>,
    pub definition_revision: i64,
    pub content_fingerprint: String,
    pub content: ScheduledTaskContentSnapshot,
    pub instruction_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic: Option<ScheduledAutomaticTriggerContext>,
}

pub fn instruction_summary(markdown: &str, max_chars: usize) -> String {
    let mut text = String::new();
    let mut in_block = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::CodeBlock(_))
                if !in_block =>
            {
                in_block = true;
            }
            Event::Text(value) | Event::Code(value) if in_block => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak if in_block => text.push(' '),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) if in_block && !text.trim().is_empty() => break,
            _ => {}
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}
```

`ScheduledAutomaticTriggerContext` is `Some` only for `OccurrenceTriggerKind::Scheduled`; manual run-now snapshots must store `None`. This one optional value prevents partial states where a manual execution accidentally carries schedule metadata or an automatic execution carries only some of `scheduledAt/schedule/timezone`.

Extend `OccurrenceLinks` with `node_id`, add `is_complete`, and extend `ScheduledOccurrence` with:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub accepted_execution: Option<ScheduledExecutionSnapshot>,
```

Keep `ScheduledTaskDefinition` mutable; do not add a task-name/title field.

- [x] **Step 4: Run domain tests and verify GREEN**

Run:

```powershell
cargo test -p gold-band scheduler::execution
cargo test -p gold-band scheduler::occurrence
cargo fmt --all -- --check
```

Expected: all targeted tests pass and formatting reports no diff.

- [x] **Step 5: Record the new authoring/snapshot boundary in both required doc trees**

Document `ScheduledTaskDefinition -> acceptance CAS -> ScheduledExecutionSnapshot`, the absence of a title field, and the exact summary rule. Add a dated unchecked implementation section to the development plan.

- [x] **Step 6: Commit the domain contract**

```powershell
git add Cargo.toml Cargo.lock src/scheduler/execution.rs src/scheduler/mod.rs src/scheduler/occurrence.rs docs/gold-band/产品设计文档/runtime/scheduled-task.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: define accepted scheduled execution snapshots"
```

### Task 2: Replace Occurrence Retention With Acceptance And History Transactions

**Files:**
- Modify: `src/scheduler/mod.rs`
- Modify: `src/scheduler/db.rs`
- Modify: `src/scheduler/queue.rs`
- Modify: `src/config/mod.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Test: `src/scheduler/db.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [x] **Step 1: Write failing repository tests for schema v2 and acceptance**

Add tests named exactly:

```rust
#[test] fn schema_v1_upgrades_to_v2_and_removes_definition_cascade()
#[test] fn accept_occurrence_persists_snapshot_links_and_accepted_at_once()
#[test] fn repeated_accept_with_identical_snapshot_is_idempotent()
#[test] fn repeated_accept_with_different_revision_is_a_conflict()
#[test] fn accepted_occurrence_survives_definition_delete()
#[test] fn definition_delete_removes_only_unaccepted_occurrences()
#[test] fn execution_history_returns_only_accepted_occurrences()
#[test] fn execution_history_groups_continuous_occurrences_by_run()
#[test] fn schedule_change_invalidates_unaccepted_automatic_occurrences()
#[test] fn schedule_change_does_not_invalidate_manual_occurrences()
#[test] fn content_only_change_keeps_unaccepted_occurrence_runnable()
#[test] fn disabled_definition_rejects_unaccepted_automatic_acceptance()
```

The v1 migration fixture must create a real v1 database, open it through `ScheduledTaskDatabase::open`, and assert schema version `2`. Because legacy occurrence rows cannot prove an accepted snapshot, the migration deliberately drops v1 occurrence rows while preserving `scheduled_jobs` and all Task/Run files outside SQLite. Do not create a compatibility snapshot from mutable current authoring.

- [x] **Step 2: Run repository tests and verify RED**

```powershell
cargo test -p gold-band scheduler::db
```

Expected: new schema and repository API tests fail.

- [x] **Step 3: Add schedule revision and schema v2**

Add a defaulted `schedule_revision: u64` to `ScheduledTaskDefinition`. Increment it only when `schedule` changes; reject automatic acceptance when the materialized occurrence revision differs or the definition is disabled/deleted. Manual occurrences do not depend on schedule revision.

Upgrade `SCHEMA_VERSION` to `2`. Rebuild `scheduled_occurrences` without a foreign key cascade and add:

```sql
schedule_revision INTEGER,
node_id TEXT,
accepted_at INTEGER,
execution_snapshot_json TEXT,
CHECK (
  (accepted_at IS NULL AND execution_snapshot_json IS NULL)
  OR
  (accepted_at IS NOT NULL AND execution_snapshot_json IS NOT NULL
   AND task_id IS NOT NULL AND run_id IS NOT NULL AND round_id IS NOT NULL
   AND node_id IS NOT NULL AND attempt_id IS NOT NULL)
)
```

Add indexes:

```sql
CREATE INDEX idx_scheduled_execution_history
ON scheduled_occurrences(project_id, job_id, accepted_at DESC, run_id, id)
WHERE accepted_at IS NOT NULL;

CREATE INDEX idx_scheduled_execution_run
ON scheduled_occurrences(project_id, task_id, run_id, accepted_at DESC)
WHERE accepted_at IS NOT NULL;

CREATE INDEX idx_scheduled_unaccepted_terminal
ON scheduled_occurrences(project_id, status, id)
WHERE accepted_at IS NULL
  AND status IN ('missed', 'skipped', 'failed');
```

- [x] **Step 4: Replace link-only acceptance with one CAS API**

Replace `accept_occurrence_links` with:

```rust
pub fn accept_occurrence_execution(
    &self,
    project_id: &str,
    occurrence_id: &str,
    owner_id: &str,
    expected_definition_revision: i64,
    links: &OccurrenceLinks,
    snapshot: &ScheduledExecutionSnapshot,
) -> Result<AcceptExecutionResult>;

pub enum AcceptExecutionResult {
    Accepted(ScheduledOccurrence),
    AlreadyAccepted(ScheduledOccurrence),
    DefinitionChanged,
    NotFound,
    LostClaim,
}
```

Within one immediate transaction, verify the owner/lease, load the current job revision and definition, validate schedule revision for automatic occurrences, and validate the trigger invariant (`Scheduled` requires `snapshot.automatic = Some`, `Manual` requires `None`). Then write complete links plus snapshot JSON and `accepted_at`, and return the reloaded occurrence. Identical retries return `AlreadyAccepted`; different locators or snapshot revisions return `LostClaim`/`DefinitionChanged` rather than overwriting history.

- [x] **Step 5: Remove age-based retention from the backend**

Delete:

- `cleanup_terminal_occurrences` and `RetentionResult`.
- occurrence retention constants from `src/scheduler/queue.rs`.
- `scheduled_occurrence_retention_days` from persisted settings and `RuntimeConfig`.
- coordinator startup/terminal retention calls.

Add one guarded repository operation:

```rust
pub fn delete_unaccepted_terminal_occurrence(
    &self,
    project_id: &str,
    occurrence_id: &str,
) -> Result<bool>;
```

It deletes only rows with `accepted_at IS NULL` whose status is terminal (`missed`, `skipped`, or pre-acceptance `failed`). Invalidated automatic work is deleted directly in the same schedule revision transaction instead of being assigned a fake trigger status. The coordinator first emits the existing structured log/notification, then calls this guard; startup repeats the indexed cleanup for terminal unaccepted rows left by a crash. `pending`, live claims, and `retrying` rows are never deleted by this path. Accepted rows are structurally excluded and are removed only by explicit Run-history deletion. Do not expose a replacement retention setting.

- [x] **Step 6: Run repository/runtime tests and verify GREEN**

```powershell
cargo test -p gold-band scheduler::db
cargo test -p gold-band scheduler::queue
cargo test -p gold-band config::tests
cargo test -p gold-band-desktop scheduled_runtime
```

Expected: schema v2, acceptance, delete, edit-race, and no-retention tests pass.

- [x] **Step 7: Update required docs and commit**

Update schema, migration, history ownership, and performance sections. Explicitly mark prior retention phases as superseded rather than silently rewriting historical progress.

```powershell
git add src/scheduler/mod.rs src/scheduler/db.rs src/scheduler/queue.rs src/config/mod.rs src-tauri/src/scheduled_runtime.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "refactor: make accepted occurrences durable history"
```

### Task 3: Make Runtime Acceptance Deterministic Across Edits And Modes

**Files:**
- Modify: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/node_executor.rs`
- Test: `src-tauri/src/scheduled_service.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`
- Test: `src/app/mod.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Write failing interface-level race tests**

Add these tests:

```rust
#[tokio::test] async fn content_edit_before_acceptance_executes_the_new_snapshot()
#[tokio::test] async fn content_edit_after_acceptance_does_not_mutate_the_running_snapshot()
#[tokio::test] async fn retrying_occurrence_reloads_latest_content_before_acceptance()
#[tokio::test] async fn schedule_edit_cancels_old_unaccepted_automatic_occurrence()
#[tokio::test] async fn disable_rejects_a_concurrent_unaccepted_automatic_claim()
#[tokio::test] async fn manual_occurrence_survives_schedule_edit_and_uses_latest_content()
#[tokio::test] async fn accepted_occurrence_recovery_reuses_snapshot_and_locator()
#[test] fn ordinary_user_turn_clears_scheduled_execution_context()
```

Use barriers around `claim_occurrence` and `accept_occurrence_execution` so the test controls whether authoring or acceptance wins. Assertions must read the persisted snapshot; do not assert only the in-memory definition clone.

- [ ] **Step 2: Run the targeted tests and verify RED**

```powershell
cargo test -p gold-band-desktop scheduled_service
cargo test -p gold-band-desktop scheduled_runtime
cargo test -p gold-band app::tests::ordinary_user_turn_clears_scheduled_execution_context
```

Expected: stale-definition execution and missing accepted snapshot assertions fail.

- [ ] **Step 3: Reload authority immediately before acceptance**

In automatic and manual paths:

1. Claim the provisional occurrence.
2. Apply overlap policy. Busy retry remains unaccepted.
3. Reload the current `ScheduledJobRecord` from SQLite.
4. Reject deleted/disabled or schedule-revision-mismatched automatic work.
5. Build `ScheduledExecutionSnapshot` from the reloaded definition.
6. Prepare Task/Run/Attempt without launching it.
7. Call `accept_occurrence_execution` with the current job revision and complete locator.
8. Launch only after `Accepted` or reconcile the exact same launch after `AlreadyAccepted`.

If `DefinitionChanged` occurs because content authoring won concurrently, discard the prepared unaccepted Run through existing prepared guards, reload once, and rebuild from the new definition. Do not loop without a bound; a second conflict returns structured `SCHEDULED_CONFLICT` and leaves the occurrence unaccepted/retryable.

- [ ] **Step 4: Propagate one immutable context through the occurrence execution chain**

Replace context fields with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTaskContextInfo {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: String,
    pub trigger_kind: OccurrenceTriggerKind,
    pub accepted_at: String,
    pub automatic: Option<ScheduledAutomaticTriggerContext>,
    pub content_fingerprint: String,
    pub instruction_summary: String,
    pub timeline_owner: OccurrenceLinks,
}
```

Build this context only from the accepted snapshot. For automatic occurrences `automatic` carries all three frozen schedule facts; for manual run-now it is `None` and no schedule fact is projected into the Agent prompt.

All automatic Workflow/AUTO child invocations inherit the context. `App::as_turn` and manual user-follow-up builders clear it. A user response that resumes `attention_required` keeps the occurrence association in lifecycle links but does not set the unattended scheduled prompt context.

- [ ] **Step 5: Run mode/race tests and verify GREEN**

```powershell
cargo test -p gold-band-desktop scheduled_runtime
cargo test -p gold-band-desktop scheduled_service
cargo test -p gold-band app::tests
```

Expected: Direct new/continuous, Workflow, AUTO, automatic/manual, edit-before/after, recovery, and ordinary-follow-up cases pass.

- [ ] **Step 6: Update docs and commit**

```powershell
git add src-tauri/src/scheduled_service.rs src-tauri/src/scheduled_runtime.rs src/app/mod.rs src/app/node_executor.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "fix: bind scheduled acceptance to current authoring"
```

### Task 4: Inject One Hidden Scheduled Protocol At The Final Prompt Boundary

**Files:**
- Modify: `src/provider/mod.rs`
- Modify: `src/prompts.rs`
- Modify: `src/prompts/zh-CN/runtime/scheduled_task_context.md`
- Modify: `src/prompts/en/runtime/scheduled_task_context.md`
- Modify: `src/acp/client.rs`
- Test: `src/provider/mod.rs`
- Test: `src/acp/client.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Write the failing prompt matrix**

Create table-driven tests covering:

```rust
#[test] fn scheduled_protocol_is_injected_into_runtime_managed_new_turn()
#[test] fn scheduled_protocol_is_injected_into_raw_agent_new_turn()
#[test] fn scheduled_protocol_is_injected_into_raw_agent_restored_continue_turn()
#[test] fn automatic_protocol_has_scheduled_at_schedule_and_timezone()
#[test] fn manual_protocol_omits_all_automatic_schedule_fields_and_says_manual_run()
#[test] fn provider_prompt_contains_the_original_instruction_exactly_once()
#[test] fn scheduled_prompt_is_hidden_but_preserves_display_text()
#[test] fn ordinary_follow_up_has_no_scheduled_protocol()
#[test] fn runtime_repair_for_the_same_occurrence_keeps_the_protocol_without_a_new_trigger_identity()
```

For the restored ACP test, assert the context is present in `user_prompt`; asserting only `system_prompt` is insufficient.

- [ ] **Step 2: Run prompt tests and verify RED**

```powershell
cargo test -p gold-band provider::tests
cargo test -p gold-band acp::client::tests
```

Expected: RawAgent and restored-continuous assertions fail; the instruction-count test detects current duplication.

- [ ] **Step 3: Replace the templates with the agreed protocol**

The Chinese template must render this structure, with conditional automatic fields and no task title/mode/session-policy fields:

```markdown
# 本次定时任务执行

{% if automatic %}本次调用是 Gold Band 已接受的一次自动定时触发执行。
{% else %}本次调用是用户通过“立即执行”手动触发、且 Gold Band 已接受的一次定时任务执行。
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

这是无人值守执行。默认自主采取合理且可逆的行动；仅当继续执行不安全、不可逆、客观上无法完成或缺少必要信息时请求用户介入。
```

The English template must express the same facts and autonomy boundary. Keep both templates under `src/prompts/`; do not inline either language in Rust.

- [ ] **Step 4: Move injection to one final boundary**

After the existing `PromptEnvelopeMode` match computes the ordinary user prompt, call:

```rust
fn project_scheduled_execution(
    req: &WorkerInvocation,
    base_user_prompt: String,
) -> Result<(String, PromptVisibility, Option<String>)>;
```

The function prepends one trusted Gold Band hidden block and leaves the base instruction unchanged. Remove `render_hidden_context`'s RuntimeManaged-only scheduled append. When context exists, set `PromptVisibility::Hidden` and `hidden_reason = Some("scheduledTaskExecution")`; keep `prompt_display.display_text` untouched for audit/workspace inspection.

- [ ] **Step 5: Run prompt/ACP tests and verify GREEN**

```powershell
cargo test -p gold-band provider::tests
cargo test -p gold-band acp::client::tests
```

Expected: every envelope contains one protocol and one instruction; ordinary turns contain neither scheduled metadata nor unattended posture.

- [ ] **Step 6: Update prompt design docs and commit**

```powershell
git add src/provider/mod.rs src/prompts.rs src/prompts/zh-CN/runtime/scheduled_task_context.md src/prompts/en/runtime/scheduled_task_context.md src/acp/client.rs docs/gold-band/产品设计文档/runtime/scheduled-task.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: project scheduled context into every agent turn"
```

### Task 5: Persist One Structured Trigger Row Beside The Hidden Prompt

**Files:**
- Modify: `src/acp/events.rs`
- Modify: `src/acp/client.rs`
- Modify: `src/acp/timeline.rs`
- Modify: `src-tauri/src/view_models.rs`
- Test: `src/acp/events.rs`
- Test: `src/acp/client.rs`
- Test: `src/acp/timeline.rs`
- Test: `src-tauri/src/view_models.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Write failing event and restore tests**

Add tests:

```rust
#[test] fn scheduled_trigger_event_has_deterministic_occurrence_identity()
#[test] fn automatic_and_manual_trigger_events_have_distinct_kinds()
#[test] fn scheduled_trigger_is_visible_while_provider_prompt_is_hidden()
#[test] fn prompt_retry_does_not_duplicate_the_scheduled_trigger()
#[test] fn workflow_child_invocation_gets_context_but_not_a_second_trigger_row()
#[test] fn timeline_index_restores_scheduled_trigger_after_restart()
#[test] fn trigger_event_snapshot_is_immutable_under_later_definition_edits()
```

- [ ] **Step 2: Run Timeline tests and verify RED**

```powershell
cargo test -p gold-band acp::events::tests
cargo test -p gold-band acp::timeline::tests
cargo test -p gold-band acp::client::tests
cargo test -p gold-band-desktop view_models::tests
```

Expected: `scheduledTrigger` is unknown and retry emits no deterministic visible projection.

- [ ] **Step 3: Add a typed trigger payload and event builder**

Define:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTriggerPayload {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub occurrence_id: String,
    pub trigger_kind: OccurrenceTriggerKind,
    pub scheduled_at: Option<String>,
    pub accepted_at: String,
    pub instruction_summary: String,
    pub content_fingerprint: String,
    pub links: OccurrenceLinks,
}

pub fn scheduled_trigger_event(seq: u64, payload: &ScheduledTriggerPayload) -> AcpUiEvent;
```

Use deterministic ID `scheduled-trigger:{occurrenceId}` and `kind = "scheduledTrigger"`. Store the typed payload in `raw.scheduledTrigger`; do not put the full instruction in visible `content`.

- [ ] **Step 4: Emit at the accepted prompt boundary and make it idempotent**

When ACP accepts the logical prompt for the Timeline-owner attempt:

1. Upsert `scheduledTrigger` using its deterministic ID.
2. Persist the hidden `userTextDelta` prompt with the same logical prompt ID and `hiddenFromChat=true`.
3. Continue provider execution.

On retry/recovery, Timeline upsert must converge on the same event. Non-owner Workflow/AUTO invocations receive the hidden protocol but skip trigger-row emission by comparing the current complete attempt locator to `ScheduledTaskContextInfo.timeline_owner`.

- [ ] **Step 5: Run Timeline tests and verify GREEN**

```powershell
cargo test -p gold-band acp::events::tests
cargo test -p gold-band acp::timeline::tests
cargo test -p gold-band acp::client::tests
cargo test -p gold-band-desktop view_models::tests
```

Expected: one visible trigger and one hidden prompt survive retry and restart with unchanged payload.

- [ ] **Step 6: Update docs and commit**

```powershell
git add src/acp/events.rs src/acp/client.rs src/acp/timeline.rs src-tauri/src/view_models.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: persist scheduled trigger timeline facts"
```

### Task 6: Add Run-Based History Queries And Durable Stop-Then-Delete

**Files:**
- Create: `src/app/history_deletion.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/storage/sqlite.rs`
- Modify: `src/scheduler/db.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Test: `src/app/history_deletion.rs`
- Test: `src/scheduler/db.rs`
- Test: `src-tauri/src/commands_conversation.rs`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Write failing history and deletion interface tests**

Add tests:

```rust
#[test] fn schema_v2_upgrades_to_v3_without_changing_accepted_history()
#[test] fn history_page_groups_multiple_continuous_occurrences_into_one_run()
#[test] fn history_page_excludes_unaccepted_scheduler_outcomes()
#[test] fn history_page_works_after_the_definition_is_deleted()
#[tokio::test] async fn terminal_history_delete_moves_only_the_target_run_to_trash()
#[tokio::test] async fn deleting_the_last_run_removes_the_empty_task_shell()
#[tokio::test] async fn deleting_the_bound_task_clears_the_definition_task_binding()
#[tokio::test] async fn active_history_delete_persists_intent_then_requests_stop()
#[tokio::test] async fn terminal_lifecycle_event_finishes_a_pending_history_delete()
#[tokio::test] async fn startup_reconciles_a_stop_accepted_history_delete()
#[tokio::test] async fn repeated_history_delete_is_idempotent()
#[tokio::test] async fn failed_history_delete_retries_the_same_operation_from_its_current_phase()
#[tokio::test] async fn batch_delete_returns_one_structured_result_per_locator()
```

- [ ] **Step 2: Run tests and verify RED**

```powershell
cargo test -p gold-band app::history_deletion
cargo test -p gold-band scheduler::db
cargo test -p gold-band-desktop commands_conversation
cargo test -p gold-band-desktop scheduled_runtime
```

Expected: history grouping, Run deletion, and durable operation APIs do not exist.

- [ ] **Step 3: Add accepted-history paging and deletion-operation persistence**

Add repository types:

```rust
pub struct ScheduledExecutionHistoryRecord {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: String,
    pub run_id: String,
    pub first_accepted_at: DateTime<Utc>,
    pub last_accepted_at: DateTime<Utc>,
    pub occurrence_count: u32,
    pub latest_occurrence_id: String,
    pub latest_summary: String,
    pub latest_content_fingerprint: String,
}

pub struct ScheduledHistoryDeletionOperation {
    pub operation_id: String,
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: String,
    pub run_id: String,
    pub status: HistoryDeletionStatus,
    pub revision: u64,
    pub attempt: u32,
    pub last_error: Option<ScheduledError>,
}
```

Upgrade scheduler storage from schema v2 to schema v3 and add deletion-operation persistence with unique `(project_id, task_id, run_id)`. The migration test must first open a v1 fixture through Task 2 to produce a real v2 database, reopen that database with the v3 code, and assert accepted occurrences remain unchanged while the new table and indexes exist.

Allowed monotonic phases are `accepted -> stopping -> deleting -> completed`. A failed attempt does not move the durable operation backward or into an unretryable terminal state: persist `last_error`, keep the current phase, and increment `attempt` when the user or startup reconciliation retries the same `operation_id`. Per-item command results may report `failed` for that attempt while the canonical operation remains resumable. Repeated requests for a completed operation return `completed` without touching the filesystem again.

- [ ] **Step 4: Implement one Run deletion owner**

`src/app/history_deletion.rs` owns:

```rust
pub fn request_run_history_deletion(
    app: &App,
    operation: &ScheduledHistoryDeletionOperation,
) -> Result<HistoryDeletionAction>;

pub fn finalize_terminal_run_history_deletion(
    app: &App,
    task_id: &str,
    run_id: &str,
) -> Result<()>;
```

For a terminal Run, move only `runs/<runId>` to trash, delete matching search-index sessions through a new `SearchIndex::delete_run(task_id, run_id)`, and remove accepted occurrence rows for that Run. If no Run remains, move the empty Task directory to trash and clear the scheduled definition's `task_id` binding with revision/CAS. For an active Run, persist the operation first, call the existing canonical stop path, and finish deletion from the scheduled runtime lifecycle subscriber after terminal settlement. Startup scans only pending deletion-operation rows, not every Run directory.

- [ ] **Step 5: Expose paged list and batch delete commands**

Define VMs and commands:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledExecutionHistoryVm {
    pub project_id: String,
    pub scheduled_task_id: String,
    pub task_id: String,
    pub run_id: String,
    pub first_accepted_at: String,
    pub last_accepted_at: String,
    pub occurrence_count: u32,
    pub latest_occurrence_id: String,
    pub latest_summary: String,
    pub latest_content_fingerprint: String,
    pub run: ConversationRunSummaryVm,
}

#[tauri::command]
pub fn list_scheduled_execution_history(
    state: State<'_, DesktopState>,
    project_id: String,
    scheduled_task_id: String,
    cursor: Option<String>,
) -> CommandResult<ScheduledExecutionHistoryPageVm>;

#[tauri::command]
pub async fn delete_scheduled_execution_history(
    state: State<'_, DesktopState>,
    items: Vec<ScheduledExecutionHistoryDeleteInputVm>,
) -> CommandResult<Vec<ScheduledExecutionHistoryDeleteResultVm>>;
```

Each batch result contains the complete locator, `operationId`, and typed status/code/params. Backend code returns no localized message.

- [ ] **Step 6: Run backend tests and verify GREEN**

```powershell
cargo test -p gold-band app::history_deletion
cargo test -p gold-band scheduler::db
cargo test -p gold-band storage::sqlite
cargo test -p gold-band-desktop commands_conversation
cargo test -p gold-band-desktop scheduled_runtime
cargo test -p gold-band-desktop scheduled_service
```

Expected: history lists only accepted grouped Runs; active delete survives restart; batch partial failure is isolated.

- [ ] **Step 7: Update docs and commit**

```powershell
git add src/app/history_deletion.rs src/app/mod.rs src/storage/sqlite.rs src/scheduler/db.rs src-tauri/src/scheduled_runtime.rs src-tauri/src/scheduled_service.rs src-tauri/src/commands_conversation.rs src-tauri/src/main.rs src-tauri/src/view_models_conversation.rs docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: manage scheduled execution history by run"
```

### Task 7: Replace The Web History And Render Structured Trigger Rows

**Files:**
- Create: `web/src/components/conversation/ScheduledTriggerRow.tsx`
- Modify: `web/src/components/acp/ACPChatDialog.tsx`
- Modify: `web/src/pages/ScheduledTaskDetailPage.tsx`
- Modify: `web/src/lib/scheduled-task-navigation.ts`
- Modify: `web/src/routes.ts`
- Modify: `web/src/types.ts`
- Modify: `web/src/api.ts`
- Modify: `web/src/api/client.ts`
- Modify: `web/src/api/desktop.ts`
- Modify: `web/src/api/browser.ts`
- Modify: `web/src/components/scheduled-tasks/ScheduledRuntimeSettings.tsx`
- Modify: `web/src/i18n.ts`
- Test: `web/tests/scheduled-trigger-row.test.tsx`
- Test: `web/tests/scheduled-task-navigation.test.ts`
- Test: `web/tests/scheduled-task-management-page.test.ts`
- Test: `web/tests/scheduled-task-settings.test.ts`
- Test: `web/tests/browser-scheduled-task-api.test.ts`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Write failing component, navigation, and API tests**

Cover these exact behaviors:

```tsx
it('renders automatic and manual trigger labels from structured payloads')
it('does not render the hidden scheduled provider prompt as a user bubble')
it('keeps one trigger row when the prompt event is retried')
it('truncates long summaries without widening the timeline')
it('navigates with project task run and occurrence ids')
it('restores occurrence focus from a deep link after reload')
it('groups continuous occurrences under one history run')
it('shows the accepted snapshot when the current fingerprint differs')
it('shows a read-only deleted-task state while preserving the run link')
it('batch deletion keeps per-run pending and error state')
it('removes occurrence retention from runtime settings contracts and UI')
```

- [ ] **Step 2: Run Web tests and verify RED**

```powershell
npm run web:test -- --run web/tests/scheduled-trigger-row.test.tsx web/tests/scheduled-task-navigation.test.ts web/tests/scheduled-task-management-page.test.ts web/tests/scheduled-task-settings.test.ts web/tests/browser-scheduled-task-api.test.ts
```

Expected: the new event kind, history APIs, route locator, and component are missing; old retention assertions fail after contract edits begin.

- [ ] **Step 3: Add typed Web contracts and browser parity**

Add:

```ts
export interface ScheduledTriggerPayloadVm {
  projectId: string;
  scheduledTaskId: string;
  occurrenceId: string;
  triggerKind: 'scheduled' | 'manual';
  scheduledAt?: string | null;
  acceptedAt: string;
  instructionSummary: string;
  contentFingerprint: string;
  links: ScheduledOccurrenceLinksVm;
}

export interface ScheduledExecutionHistoryVm {
  projectId: string;
  scheduledTaskId: string;
  taskId: string;
  runId: string;
  occurrenceCount: number;
  latestOccurrenceId: string;
  latestSummary: string;
  latestContentFingerprint: string;
  firstAcceptedAt: string;
  lastAcceptedAt: string;
  run: ConversationRunSummaryVm;
}
```

Remove `occurrenceRetentionDays` from settings inputs, outputs, cache fixtures, browser defaults, and validation. Browser history fixtures must contain only accepted Runs; do not keep the old skipped/missed fake history.

- [ ] **Step 4: Render the lightweight AlarmClock divider**

`ScheduledTriggerRow` uses Lucide `AlarmClock`, existing Tooltip, theme `foreground/muted-foreground`, one stable line, and no card container. It receives typed payload and `onOpen`; automatic/manual labels come from i18n. Use `min-w-0`, `truncate`, and a full-row button with visible focus state. Do not render status, outcome, retry count, or implementation text.

Add `scheduledTrigger` to `isRenderableEvent` and `ACPTimelineItemRenderer`; hidden `userTextDelta` remains filtered by `hiddenFromChat`.

- [ ] **Step 5: Replace occurrence history with Run history and deep-link focus**

Extend `ConversationPage`:

```ts
| {
    kind: 'scheduled-task-detail';
    projectId: string;
    scheduledTaskId: string;
    taskId?: string;
    runId?: string;
    occurrenceId?: string;
  }
```

Encode the complete locator in `/chat/scheduled-tasks/:scheduledTaskId/history/:taskId/:runId/occurrences/:occurrenceId`. The detail page activates immediately, renders a target-specific skeleton, loads the requested history page, highlights the Run, and expands the occurrence snapshot. If the definition is missing, render the same history area read-only with “原定时任务已删除”; do not turn it into a generic not-found page.

History rows support AionUi-style single/multi selection and deletion of complete Runs. Active deletions display the backend operation's stopping state in the row; completed deletion removes only that row. A failed item remains selected with localized structured recovery text while successful siblings disappear.

- [ ] **Step 6: Run Web tests and verify GREEN**

```powershell
npm run web:test -- --run web/tests/scheduled-trigger-row.test.tsx web/tests/scheduled-task-navigation.test.ts web/tests/scheduled-task-management-page.test.ts web/tests/scheduled-task-settings.test.ts web/tests/browser-scheduled-task-api.test.ts
npm run web:build
```

Expected: targeted tests pass, TypeScript passes, and production build succeeds with no new warning category.

- [ ] **Step 7: Update interaction docs and commit**

```powershell
git add web/src/components/conversation/ScheduledTriggerRow.tsx web/src/components/acp/ACPChatDialog.tsx web/src/pages/ScheduledTaskDetailPage.tsx web/src/lib/scheduled-task-navigation.ts web/src/routes.ts web/src/types.ts web/src/api.ts web/src/api/client.ts web/src/api/desktop.ts web/src/api/browser.ts web/src/components/scheduled-tasks/ScheduledRuntimeSettings.tsx web/src/i18n.ts web/tests/scheduled-trigger-row.test.tsx web/tests/scheduled-task-navigation.test.ts web/tests/scheduled-task-management-page.test.ts web/tests/scheduled-task-settings.test.ts web/tests/browser-scheduled-task-api.test.ts docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md
git commit -m "feat: show scheduled triggers and run history"
```

### Task 8: End-To-End Recovery, Visual Verification, And Documentation Closure

**Files:**
- Modify: `tests/` interface fixtures as required by actual changed contracts
- Modify: `docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] **Step 1: Add cross-layer acceptance tests**

Add interface-level scenarios that execute through the real service/runtime boundary:

```text
Direct/new automatic -> one hidden prompt + one automatic trigger + one history Run
Direct/continuous automatic twice -> two trigger rows + one history Run + two occurrence anchors
Direct/continuous user follow-up -> visible ordinary user prompt without scheduled protocol
Workflow automatic -> every worker gets protocol, only owner attempt gets trigger row
AUTO manual run-now -> manual trigger, no scheduledAt
content edit while retrying -> accepted snapshot contains new content
schedule edit while retrying -> old automatic occurrence never reaches history
definition delete after acceptance -> history deep link remains readable
provider retry/restart -> no duplicate trigger row
active history delete -> stop intent survives restart and eventually removes Run
```

- [ ] **Step 2: Run the complete automated verification**

```powershell
cargo test -p gold-band
cargo test -p gold-band-desktop
npm run web:test
npm run web:build
cargo fmt --all -- --check
git diff --check
```

Expected: all commands exit `0`; no new failing test, TypeScript error, formatting diff, or whitespace error.

- [ ] **Step 3: Start the frontend for required UI verification**

Run:

```powershell
npm run web:dev
```

Use the in-app browser and deep link directly to a seeded scheduled-task detail and conversation Run. Verify:

- 1440x900 and 390x844 viewports.
- Resize normal width to narrow and back again, preserving the selected Run and occurrence anchor.
- Light and dark themes.
- Automatic and manual trigger text.
- Long CJK and long unbroken Latin summary truncation.
- Keyboard focus/activation and Tooltip.
- Current-definition-changed comparison.
- Deleted-definition read-only state.
- Continuous Run occurrence focus after reload.
- Batch deletion partial failure and active stopping state.
- No overlap, horizontal scroll, React warning, or console error.

- [ ] **Step 4: Stop the dev server and clean seeded resources**

Stop only the process started in Step 3. Delete the seeded test scheduled definition and test Runs through product APIs so deletion/recovery paths are exercised; do not remove unrelated workspace data.

- [ ] **Step 5: Close the canonical docs**

Update all four design documents and mark the dated development-plan section complete only after Step 2 and Step 3 evidence exists. Record:

- Root cause classification: correct design, incomplete prompt projection contract.
- History correction: only accepted Runs are history; no retention-days setting.
- AionUi influence: conversation-managed history and user deletion, without title/artifact copying.
- Data authority, CAS order, recovery, deletion operation, and exact locator.
- Performance conclusion: indexed O(log n) lookup, page size 20, O(1) trigger upsert, bounded batch deletion, no AI summary, no full filesystem scan.
- Overdesign conclusion: one snapshot value object and one durable deletion operation are required by proven invariants; no task-version aggregate, artifact subsystem, cache, or new queue.

- [ ] **Step 6: Review the final diff and commit verification/docs**

```powershell
git status --short
git diff --stat
git diff --check
git add tests docs/gold-band/产品设计文档 docs/gold-band/开发计划
git commit -m "test: verify scheduled occurrence awareness"
```

## Final Acceptance Matrix

| Requirement | Durable proof |
| --- | --- |
| Direct RawAgent knows it is scheduled | provider matrix test inspects restored/new `user_prompt` |
| Instruction appears once | exact occurrence count assertion in rendered prompt |
| User does not see repeated instruction bubble | hidden prompt + visible trigger renderer test |
| Manual is distinct from automatic | typed trigger/prompt tests; manual omits `scheduledAt`, schedule summary, and timezone |
| Every mode is covered | Direct new/continuous, Workflow, AUTO integration matrix |
| Only real triggers create history | accepted-history query excludes all unaccepted outcomes |
| Edit-before/after boundary is deterministic | barrier-controlled revision/CAS tests |
| Historical content is immutable | persisted snapshot and later-edit test |
| Trigger row is one per occurrence | deterministic Timeline ID retry/restart test |
| User follow-up is interactive | `App::as_turn` and provider prompt regression |
| Deleted definition preserves history | schema/delete/deep-link tests |
| History has no automatic retention | settings/schema/coordinator removal contract tests |
| Continuous history deletion is whole-Run | grouped history and Run deletion tests |
| Active deletion is stop-then-delete | durable operation restart test |
| Exact occurrence navigation survives reload | route round-trip and page focus tests |
| UI remains responsive and accessible | desktop/mobile, theme, keyboard, long-text browser verification |

## Self-Review Results

- **Spec coverage:** Every confirmed product contract maps to at least one task and one acceptance row. Notification wording is unchanged except that non-accepted outcomes no longer appear in execution history; existing structured missed/failure notification behavior remains owned by the scheduler.
- **Placeholder scan:** The plan contains no deferred implementation placeholders. Every new type, interface, state sequence, test name, command, and verification command is specified.
- **Type consistency:** `projectId + scheduledTaskId + taskId + runId + occurrenceId` is the cross-layer locator. `OccurrenceLinks` adds `nodeId` for an exact Timeline owner. `ScheduledExecutionSnapshot` is the only accepted content snapshot and is used by repository, prompt context, Timeline payload, history detail, and comparison UI. Automatic-only schedule facts are one optional `ScheduledAutomaticTriggerContext`, so manual run-now cannot serialize a partial automatic context.
- **Migration consistency:** Task 2 owns v1 -> v2 accepted-occurrence migration; Task 6 owns v2 -> v3 deletion-operation migration. Each migration has a fixture that opens the prior version through the public database entry point.
- **Overdesign:** The schema snapshot is required to prevent edits from rewriting history. The correlated automatic-trigger value prevents invalid mixed trigger states. The durable deletion operation is required because stopping and deleting span an asynchronous Run lifecycle and must survive restart. No full task-version system, AionUi artifact subsystem, model-generated summary, new cache, or general-purpose workflow engine is introduced.
- **Performance:** Acceptance and trigger writes are O(1); history uses the new partial index and fixed-size pages; continuous grouping occurs in SQL; batch deletion returns bounded per-item results; startup reads only pending deletion-operation rows. Removing age-retention scans reduces coordinator background I/O.
