# Scheduled Runtime Config Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure scheduled occurrences use current Agent settings and pre-accept failures appear as the task's latest trigger.

**Architecture:** Refresh each cached `WorkspaceRegistration` through the existing registration candidate boundary whenever settings change. Keep occurrence as canonical lifecycle and advance the existing job runtime projection after a pre-accept execution failure.

**Tech Stack:** Rust, Tokio, Tauri, rusqlite, React/TypeScript view-model consumers.

---

### Task 1: Refresh workspace runtime snapshots

**Files:**
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Write the failing test**

Extend `LoopCoordinatorRuntime` with a mutable config marker and add a coordinator test that registers a workspace, changes the marker, handles `SchedulerCommand::SettingsChanged`, then asserts the cached registration contains the new marker.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p gold-band-desktop scheduled_runtime::tests::settings_changed_refreshes_registered_workspace_runtime_config --offline -- --exact`

Expected: FAIL because `SettingsChanged` only calls `reconcile_all()` and `app_for_workspace()` is not called again.

- [ ] **Step 3: Implement the minimal refresh**

Add a coordinator method that clones the currently registered workspace paths and calls the existing `register_workspace_with_retry(path, ReconcileReason::Explicit)` for each. Route `SettingsChanged` to that method. Preserve the current registration until a fully reconciled candidate succeeds.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run the command from Step 2 and expect one passing test.

### Task 2: Project pre-accept execution failures

**Files:**
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Test: `src-tauri/src/scheduled_runtime.rs`

- [ ] **Step 1: Write the failing persistence test**

Add a focused test around the failure-projection helper using a real temporary `ScheduledTaskDatabase`. Materialize and claim a due occurrence, project `SCHEDULED_EXECUTION_FAILED`, then assert occurrence status, job `last_trigger_at`, `last_trigger_status`, `last_error`, and unchanged advanced `next_run_at`.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p gold-band-desktop scheduled_runtime::tests::pre_accept_execution_failure_updates_latest_trigger_projection --offline -- --exact`

Expected: FAIL because the current failure path only finishes the occurrence.

- [ ] **Step 3: Implement the minimal projection**

After `finish_occurrence(... Failed ...)` succeeds, disarm the guard, call the existing `advance_definition_after_point(..., "failed", now)`, set `last_error` to `SCHEDULED_EXECUTION_FAILED`, and persist through `persist_active_projection()` with the current expected revision. Update scheduled and manual callers to pass their mutable definition/revision.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run the command from Step 2 and expect one passing test.

### Task 3: Synchronize authoritative docs

**Files:**
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] Record that `SettingsChanged` refreshes workspace runtime snapshots before future executions.
- [ ] Record that pre-accept failures remain canonical occurrence failures and also update the existing management-list runtime projection.
- [ ] Record root-cause classification, tests, over-design review, and performance impact.

### Task 4: Verification

**Files:**
- Test: `src-tauri/src/scheduled_runtime.rs`
- Test: `web/tests/*scheduled-task*`

- [ ] Run both new focused tests together.
- [ ] Run the complete desktop scheduled runtime test module offline.
- [ ] Run `cargo fmt --all -- --check` and `cargo check -p gold-band-desktop --tests --offline`.
- [ ] Run scheduled-task Web tests, TypeScript check, and production build.
- [ ] Start the desktop/frontend development target, deep link to scheduled task management, verify the latest failed state and cleanup the launched process.

