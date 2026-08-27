# Scheduled History Occurrence Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make scheduled-history removal delete only terminal accepted occurrences while preserving the Run as a usable conversation.

**Architecture:** Keep accepted Run aggregation as the read model, but move deletion ownership back to the scheduler occurrence repository. A deletion command carries the selected history row's latest occurrence as a watermark, validates that the canonical Run is completed, and atomically deletes only occurrences visible through that watermark. Remove the durable stop-and-delete Run workflow because the command no longer owns Run, Task, Timeline, or SearchIndex lifecycles.

**Tech Stack:** Rust, rusqlite, Tauri commands, React, TypeScript, Vitest, shadcn/ui.

---

### Task 1: Freeze the repository contract with failing tests

**Files:**
- Modify: `src/scheduler/db.rs`

- [ ] Add a test where two accepted occurrences share one Run, delete through the first occurrence, and assert only the first row is removed.
- [ ] Add tests for a foreign watermark and an already-removed watermark retry; assert no unrelated row changes and the retry reaches the same desired state.
- [ ] Run `cargo test -p gold-band scheduler::db::tests::remove_execution_history --jobs 1 -- --test-threads=1` and confirm the new assertions fail because the current repository deletes the whole Run group without a watermark contract.

### Task 2: Replace durable Run deletion with occurrence removal

**Files:**
- Modify: `src/scheduler/db.rs`
- Delete: `src/app/history_deletion.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/storage/sqlite.rs`

- [ ] Bump the scheduler component schema and remove `scheduled_history_deletions`, its index, mapping types, and repository methods.
- [ ] Implement the immediate-transaction occurrence removal command scoped by project, scheduled task, Task, Run, and watermark.
- [ ] Remove Run trash, empty Task cleanup, definition binding cleanup, and SearchIndex deletion from scheduled-history removal.
- [ ] Run the focused scheduler and storage tests; confirm occurrence removal is green and no scheduled-history path mutates Run-owned storage.

### Task 3: Enforce terminal-only deletion at the service boundary

**Files:**
- Modify: `src-tauri/src/scheduled_service.rs`
- Modify: `src-tauri/src/commands_conversation.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/scheduled_runtime.rs`
- Modify: `src-tauri/src/view_models_conversation.rs`

- [ ] Add failing interface tests for Completed, Running, Paused, and waiting-for-user-input Runs; assert only Completed reaches repository deletion.
- [ ] Change the request DTO to include `throughOccurrenceId` and reduce item results to `completed | failed` with stable structured errors.
- [ ] Delete stop reconciliation, lifecycle finalization, and startup replay wiring for history deletion.
- [ ] Verify a completed deletion preserves `get_conversation_run` and the definition's continuous-session Task binding.

### Task 4: Update the Web contract and interaction

**Files:**
- Modify: `web/src/api/client.ts`
- Modify: `web/src/api/browser.ts`
- Modify: `web/src/pages/ScheduledTaskDetailPage.tsx`
- Modify: `web/src/i18n.ts`
- Modify: `web/tests/scheduled-task-detail-page.test.tsx`
- Modify: `web/tests/app-scheduled-task-detail.test.ts`

- [ ] Add failing DOM tests that only Completed rows are selectable, the command carries the latest occurrence watermark, and successful removal does not navigate to or refresh a deleted Run.
- [ ] Replace delete-Run wording and phase handling with remove-history wording and target-level pending state using existing shadcn controls.
- [ ] Update browser parity to preserve Run data and delete only accepted occurrences through the watermark.
- [ ] Run `npm run web:test -- web/tests/scheduled-task-detail-page.test.tsx web/tests/app-scheduled-task-detail.test.ts` and confirm the tests pass after the minimal implementation.

### Task 5: Verify the complete behavior

**Files:**
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task-runtime-implementation.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`

- [ ] Run focused Rust tests with `--jobs 1 -- --test-threads=1`, then run the relevant desktop command and view-model suites.
- [ ] Run the focused Web tests, `npm run web:build`, `cargo fmt --all -- --check`, and `git diff --check`.
- [ ] Start the frontend and use the in-app browser with a scheduled-task detail deep link to verify terminal removal, active-row disabled state, narrow layout, and continued interaction from ordinary conversation history.
- [ ] Stop the test server and close test pages, then record exact fresh results in the Phase 23 development-plan section.

## Plan Self-Review

- Spec coverage: terminal-only removal, Run preservation, watermark concurrency, structured partial failure, UI convergence, documentation, and visual verification are all mapped to tasks.
- Placeholder scan: no deferred implementation or unspecified error-handling steps remain.
- Type consistency: `throughOccurrenceId` is the single watermark name across repository, service, Web, and browser parity.
