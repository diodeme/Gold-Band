# Scheduled Task Reliability and Direct Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with spec and code-quality reviews.

**Goal:** Replace the JSON/cursor scheduler with a transactional occurrence scheduler and bring Direct scheduled tasks to current AionUi capability parity while keeping Workflow/AUTO on Gold Band's Task/Run/content-fingerprint model.

**Architecture:** A dedicated SQLite scheduler database stores job definitions and mutable occurrences. Each occurrence is atomically created/claimed, leased and completed from runtime lifecycle events. A per-job timer schedules only future points; startup/resume marks missed points without backfill. Tauri commands and the web management page expose run-now, history, diagnostics and attention states.

**Tech Stack:** Rust 2024, `rusqlite`, `chrono`/`chrono-tz`/`cron`, Tauri 2, existing `RuntimeLifecycleBus`, React 19, TypeScript, Vitest, localized prompts under `src/prompts/zh-CN` and `src/prompts/en`.

---

### Task 1: Typed occurrence domain and SQLite repository

**Files:**
- Create: `src/scheduler/occurrence.rs`
- Create: `src/scheduler/db.rs`
- Modify: `src/scheduler/mod.rs`
- Modify: `src/storage/mod.rs`
- Test: `src/scheduler/occurrence.rs`, `src/scheduler/db.rs`

- [ ] **Step 1: Write failing tests for statuses and unique claims**

Add tests named `occurrence_status_round_trips_stable_values`, `scheduled_occurrence_is_unique_by_job_time_and_trigger_kind`, `only_one_owner_can_claim_an_occurrence`, and `expired_lease_can_be_reclaimed`. Use a temporary `GoldBandPaths` and a dedicated SQLite file; assert the second claim returns `ClaimResult::AlreadyOwned` or `ClaimResult::Busy`, never a second running row.

Run: `cargo test -p gold-band scheduler::occurrence scheduler::db`

Expected: FAIL because the typed occurrence and repository APIs do not exist.

- [ ] **Step 2: Define typed domain values**

Implement `OccurrenceStatus`, `OccurrenceTriggerKind`, `ScheduledErrorCode`, `ScheduledOccurrence`, `ClaimResult`, and `LeaseConfig` with `serde(rename_all = "snake_case")`. `ScheduledErrorCode` must include `PermissionRequired`, `UserInputRequired`, `PreviousRunRequiresAttention`, `QueueBusy`, `AgentUnattendedModeUnsupported`, `ExecutionFailed`, and `LeaseLost`. Do not expose customer-facing text from these types.

- [ ] **Step 3: Add the SQLite schema and transactional operations**

Create `ScheduledTaskDatabase::open(path)` with WAL mode, a busy timeout, schema version table, `scheduled_jobs`, and `scheduled_occurrences`. Add the unique constraint `(job_id, scheduled_at, trigger_kind)` and a partial active index. Implement:

```rust
create_or_get_occurrence(job_id, scheduled_at, trigger_kind)
claim_occurrence(id, owner_id, now, lease_until)
renew_lease(id, owner_id, now, lease_until)
finish_occurrence(id, owner_id, status, links, error)
recover_expired(now)
mark_missed(job_id, scheduled_at)
list_occurrences(job_id, limit)
```

Each claim and finish must use a transaction and verify the owner/lease in the `WHERE` clause. Add a `scheduler_db_path()` method to `GoldBandPaths` without changing the existing search database path.

- [ ] **Step 4: Run focused tests and commit**

Run: `cargo test -p gold-band scheduler::occurrence scheduler::db`

Expected: PASS, including the concurrent claim test. Commit: `git add src/scheduler src/storage/mod.rs && git commit -m "feat: add transactional scheduled occurrences"`.

### Task 2: Schedule semantics and legacy migration

**Files:**
- Modify: `src/scheduler/mod.rs`
- Modify: `src/scheduler/db.rs`
- Modify: `src/scheduler/store.rs`
- Modify: `Cargo.toml`, `Cargo.lock`
- Test: `src/scheduler/mod.rs`, `src/scheduler/db.rs`

- [x] **Step 1: Add failing time and migration tests**

Add tests named `hourly_schedule_returns_next_wall_clock_hour`, `system_timezone_is_used_when_schedule_timezone_is_omitted`, `dst_gap_returns_next_valid_occurrence`, `dst_overlap_uses_the_first_valid_occurrence`, `every_edit_only_resets_anchor_when_interval_changes`, `legacy_json_definition_import_is_idempotent`, and `legacy_import_rejects_conflicting_ids`.

Run: `cargo test -p gold-band scheduler::tests scheduler::db::migration`

Expected: FAIL for Hourly, system timezone, DST, anchor edit, and migration behavior.

- [x] **Step 2: Fix schedule calculation without changing the public schedule tags**

Implement `Hourly` as the next local `xx:00:00`, independent of the stored `hour/minute` fields. Resolve the default timezone using a system-timezone helper and retain a valid IANA string. Handle `chrono_tz::LocalResult::Single` and `Ambiguous` deterministically; skip `None` local times instead of returning `None` for the whole schedule. Keep `Every` absolute to `anchorAt`, and only reset its anchor when enabling or changing value/unit.

- [x] **Step 3: Implement one-time JSON import**

Read legacy definitions and trigger files through the existing `ScheduledTaskStore`, insert jobs and occurrences in one transaction per definition, preserve Task/Run links and content snapshots, and make a second import a no-op. On ID or timestamp conflict return a typed migration error. Do not make runtime scheduling read the JSON after successful import.

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p gold-band scheduler`

Expected: PASS with the previous 27 baseline tests plus the new time/migration tests. Commit: `git add Cargo.toml Cargo.lock src/scheduler src/storage/mod.rs && git commit -m "fix: align scheduled time semantics and migrate legacy definitions"`.

### Task 3: Runtime lifecycle binding and per-job scheduling

**Files:**
- Modify: `src/app/mod.rs`
- Modify: `src/app/observability.rs`
- Modify: `src-tauri/src/commands.rs`
- Replace: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`, `src/app/mod.rs`

- [ ] **Step 1: Add failing lifecycle binding tests**

Add tests named `scheduled_run_completion_finishes_matching_occurrence`, `scheduled_turn_failure_finishes_occurrence_as_failed`, `scheduled_intervention_releases_lease_as_attention_required`, `startup_marks_past_points_missed_without_backfill`, and `run_now_does_not_advance_next_scheduled_time`.

Run: `cargo test -p gold-band-desktop scheduled_runtime`

Expected: FAIL because lifecycle events have no scheduled origin and the runtime still uses the JSON cursor loop.

- [ ] **Step 2: Carry scheduled origin through lifecycle events**

Add an optional occurrence ID to `RunPaused`, `InterventionRequested`, `RunCompleted`, and `AcpTurnFinished`. `App::clone_for_background` must preserve it. `emit_lifecycle_event` must use the origin when a scheduled execution adapter creates a run or sends a continuous prompt. Existing non-scheduled constructors set `None`.

- [ ] **Step 3: Replace the polling loop with a scheduler service**

Create a `ScheduledRuntime` service that loads all registered workspaces, migrates definitions, recovers expired leases, creates one timer per enabled job, and reschedules after every terminal result. The timer callback creates/claims an occurrence, evaluates the shared active predicate, and applies typed skip/retry policy. A late callback or system-resume notification marks old points `missed` and schedules only the next future point.

- [ ] **Step 4: Finish occurrences from lifecycle events**

Register a named inline lifecycle subscriber. Map `RunCompleted`/`AcpTurnFinished` outcomes to `succeeded` or `failed`; map permission interventions to `failed + PermissionRequired`; map user-question interventions to `attention_required + UserInputRequired`. Release lease before emitting UI/OS notifications. At no point write `completed` merely because `create_conversation_run_vm` or `run_start_background` returned.

- [ ] **Step 5: Add manual run-now service API**

Create a manual occurrence, claim it with the same queue policy, and return its occurrence ID plus linked Task/Run when available. Do not update `next_run_at` or the scheduled occurrence cursor.

- [ ] **Step 6: Run focused runtime tests and commit**

Run: `cargo test -p gold-band-desktop scheduled_runtime commands::tests` and `cargo test -p gold-band app::observability`

Expected: PASS with no new warnings in touched modules. Commit: `git add src/app src-tauri/src/commands.rs src-tauri/src/main.rs src-tauri/src/scheduled_runtime.rs && git commit -m "feat: schedule occurrences through runtime lifecycle"`.

### Task 4: Direct unattended execution and typed Tauri API

**Files:**
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/config/mod.rs` if a provider full-auto resolver is needed
- Test: `src-tauri/src/commands_conversation.rs`, `src-tauri/src/view_models_conversation.rs`

- [ ] **Step 1: Add failing command tests**

Add tests named `scheduled_direct_rejects_agent_without_full_auto_mode`, `scheduled_direct_accepts_supported_full_auto_mode`, `run_now_returns_occurrence_reference`, `list_scheduled_occurrence_history_is_ordered`, `permission_request_maps_to_typed_error_code`, and `user_question_maps_to_attention_state`.

Run: `cargo test -p gold-band-desktop commands_conversation::tests view_models_conversation::tests`

Expected: FAIL because the commands and ViewModels do not expose full-auto validation, run-now, history, diagnostics, or attention state.

- [ ] **Step 2: Implement full-auto preflight and typed errors**

Resolve the selected Direct Agent's supported permission modes through the existing config/diagnostics APIs. Require a resolved full-access/bypass mode for scheduled execution. Return `scheduled-task.agent-unattended-mode-unsupported` with provider and available-mode parameters; do not save an invalid job.

- [ ] **Step 3: Add scheduler commands and ViewModels**

Add `run_scheduled_task_now`, `list_scheduled_task_occurrences`, and `get_scheduled_task_diagnostics`. Return occurrence status, error code, next run, last error, run count, retry count, and Task/Run/session links. Add `attention_required` and `failed` as typed status values; frontend copy remains outside Rust.

- [ ] **Step 4: Register commands and update generated schemas**

Register the commands in `src-tauri/src/main.rs`, regenerate Tauri schemas using the repository's existing build command, and verify Windows and default schema files agree.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p gold-band-desktop commands_conversation::tests view_models_conversation::tests`

Expected: PASS. Commit: `git add src-tauri/src/commands_conversation.rs src-tauri/src/view_models_conversation.rs src-tauri/src/main.rs src-tauri/gen/schemas && git commit -m "feat: expose direct scheduled task execution diagnostics"`.

### Task 5: Management UI, detail/history, notifications and keep-awake

**Files:**
- Modify: `web/src/types.ts`
- Modify: `web/src/api/client.ts`, `web/src/api.ts`, `web/src/api/desktop.ts`, `web/src/api/browser.ts`
- Modify: `web/src/pages/ScheduledTaskManagementPage.tsx`
- Modify: `web/src/App.tsx`
- Modify: `src-tauri/src/scheduled_runtime.rs`, `src-tauri/src/notifications.rs`, `src-tauri/src/state.rs`
- Modify: `web/src/i18n.ts` for `zh-CN` and `en` scheduled-task copy
- Test: `web/tests/*scheduled*`, `src-tauri/src/notifications.rs`

- [ ] **Step 1: Add failing web tests**

Cover instruction-derived titles with no name field, run-now action, detail/history rendering, diagnostics, paused/failed/attention statuses, notification deep link, and browser preview parity.

Run: `npm run web:test -- ScheduledTask`

Expected: FAIL because the API and page have no occurrence detail/run-now state.

- [ ] **Step 2: Add typed frontend API models and desktop/browser facades**

Add `ScheduledOccurrenceVm`, `ScheduledTaskDiagnosticsVm`, `RunScheduledTaskResultVm`, and event payloads. Keep browser preview behavior in-memory and make desktop call Tauri directly.

- [ ] **Step 3: Build the detail/history interaction with existing shadcn/ui primitives**

Add a compact detail view reachable from each row, with run-now, task/run navigation, occurrence status, error code mapping, next run, and counters. Keep the existing no-name instruction summary. Use familiar icons and tooltips; do not add a command bar or explanatory implementation text.

- [ ] **Step 4: Wire notifications and keep-awake state**

Reuse `RuntimeLifecycleBus` and the existing OS notification subscriber for scheduled completion/attention events. Add a persisted keep-awake preference and acquire/release the platform wake lock only while an occurrence is actively executing and the preference is enabled. Use `process::background_command()` for any Windows helper process.

- [ ] **Step 5: Run UI tests and commit**

Run: `npm run web:test -- ScheduledTask` and `npm run web:build`

Expected: PASS. Commit: `git add web src-tauri/src/notifications.rs src-tauri/src/state.rs src-tauri/src/scheduled_runtime.rs && git commit -m "feat: add scheduled task history and unattended feedback"`.

### Task 6: Localized scheduled-task skill and Workflow/AUTO adapter hardening

**Files:**
- Create: `src/prompts/zh-CN/runtime/scheduled-task/skill.md`
- Create: `src/prompts/en/runtime/scheduled-task/skill.md`
- Modify: `src/prompts.rs` to register the bilingual scheduled-task skill assets
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src/scheduler/queue.rs`
- Test: prompt loader tests, `src/scheduler/queue.rs`, `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Add failing skill and adapter tests**

Test that zh-CN/en skill files have the same relative structure and require Agent calls to use typed create/list/update/run-now operations. Add Workflow/AUTO tests for unchanged fingerprint reuse, authoring-change new Task, busy attention blocking, and real completion.

Run: `cargo test -p gold-band prompt scheduler::queue` and `cargo test -p gold-band-desktop scheduled_runtime`

Expected: FAIL until localized skills and shared adapter paths exist.

- [ ] **Step 2: Add bilingual skill content under `src/prompts`**

Use the same headings and variable placeholders in both languages. The skill must teach the Agent to omit a name, use instruction as title source, call typed scheduling APIs, inspect history, and never claim a task succeeded before the returned occurrence is terminal.

- [ ] **Step 3: Route Workflow/AUTO through the same occurrence service**

Keep their current content fingerprint and Task/Run rules, but pass occurrence IDs into run creation and completion events. Reuse the same active predicate and attention/error handling.

- [ ] **Step 4: Run all Rust and Web tests and commit**

Run: `cargo test --workspace` and `npm run web:test && npm run web:build`

Expected: PASS. Commit: `git add src/prompts src/scheduler src-tauri/src/scheduled_runtime.rs web && git commit -m "feat: complete scheduled task parity across execution modes"`.

### Task 7: Documentation and final verification

**Files:**
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/runtime/state/scheduled-task.json.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务运行时实现补充.md`

- [ ] **Step 1: Update completion checkboxes from verified evidence**

Record only behaviors proven by Rust interface tests, Tauri tests, Web tests, and a desktop deep-link verification. Keep no stale statement that JSON cursor polling or process start represents completion.

- [ ] **Step 2: Run final verification**

Run: `cargo fmt --all -- --check`, `cargo test --workspace`, `npm run web:test`, `npm run web:build`, and `git diff --check`.

Expected: all commands exit 0; any pre-existing warning is reported separately from failures.

- [ ] **Step 3: Review the final diff and commit documentation**

Run: `git status --short`, `git diff --stat`, and `git log -1 --oneline`. Commit: `git add docs && git commit -m "docs: record verified scheduled task parity"`.

### Task 3 implementation checkpoint (2026-08-03)

- [x] Added scheduled origin propagation to lifecycle events and background App clones.
- [x] Replaced JSON cursor polling with SQLite definitions, transactional occurrence claim/lease/finish, startup missed-point handling, and bounded queue retry/skip.
- [x] Added lifecycle completion mapping for RunCompleted, ACP turn failure, permission request, and user input intervention.
- [x] Synchronized scheduled task CRUD commands with SQLite definitions and added regression tests for runtime lifecycle behavior.
