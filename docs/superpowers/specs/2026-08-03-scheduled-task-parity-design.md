# Scheduled Task Reliability and Direct Parity Design

**Date:** 2026-08-03
**Status:** Superseded by `2026-08-05-scheduled-task-unified-runtime-design.md`
**Scope:** Direct parity with the current AionUi scheduled-task capabilities, plus reliability hardening shared by Workflow and AUTO.

## Goal

Gold Band scheduled tasks must provide the current Direct-mode capabilities of AionUi without adopting AionUi's conversation domain model. Workflow and AUTO remain Gold Band-specific execution modes, but use the same scheduling, occurrence, queue, recovery, and notification infrastructure.

The scheduled task does not have an independent name field. Its display title is derived from the first non-empty instruction line and is never used as an identity key.

## Design Decision

Use a dedicated SQLite scheduler database at the application runtime root. Keep copied instruction and attachment snapshots in the existing project runtime directories, but remove the JSON definition and trigger files as the active persistence path after a one-time migration.

SQLite is selected because the scheduler needs an atomic uniqueness constraint, claim transaction, lease recovery, and cross-workspace listing. The existing JSON store cannot provide those guarantees without rebuilding a database protocol on top of file locks.

The AionCore cron crate is not copied. Its occurrence and lease behavior is reproduced behind Gold Band interfaces, while Gold Band's `Task -> Run -> Round -> Attempt`, content fingerprint, Workflow, and AUTO models remain authoritative.

## Domain Model

### Scheduled job

`ScheduledJob` stores the user-authored trigger definition and execution configuration:

- project/workspace identity, mode, Direct session policy, instruction snapshot reference, attachment snapshot reference;
- structured schedule and IANA timezone;
- overlap policy and maximum busy retries;
- enabled state, `next_run_at`, content fingerprint, task association, created/updated timestamps;
- diagnostic counters. Keep-awake is a global application preference, not a per-job field.

There is no persisted `name`. The UI title is derived from instruction text. The job ID is the only stable identity.

### Scheduled occurrence

Every planned or manually requested execution is represented by one mutable occurrence row. The database enforces `UNIQUE(job_id, scheduled_at, trigger_kind)` for scheduled occurrences.

Required fields:

```text
id, job_id, scheduled_at, trigger_kind
status, attempt, owner_id, lease_until, heartbeat_at
task_id, run_id, round_id, attempt_id
error_code, error_params, started_at, finished_at
created_at, updated_at
```

`trigger_kind` is `scheduled` or `manual`. Manual runs do not advance the job's next scheduled time.

Occurrence statuses are typed Rust enums and serialized as stable lowercase values:

```text
pending | running | retrying | succeeded | failed
skipped | missed | attention_required
```

`attention_required` is scheduler-terminal for claiming purposes, but the linked Run remains resumable. After the user answers and the original Run completes, the occurrence may transition to `succeeded` or `failed`.

### Error codes

The backend stores codes and structured parameters, never customer-facing text. The initial codes include:

```text
SCHEDULED_PERMISSION_REQUIRED
SCHEDULED_USER_INPUT_REQUIRED
SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION
SCHEDULED_QUEUE_BUSY
SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED
SCHEDULED_EXECUTION_FAILED
SCHEDULED_LEASE_LOST
```

## Scheduler Lifecycle

1. Load enabled jobs and recover expired leases at application startup.
2. For each job, calculate the next future occurrence using the typed schedule calculator.
3. Use an independent timer per job. A timer callback creates the occurrence with the unique constraint and attempts an atomic claim.
4. If another process owns the occurrence, do nothing. If the job has an active execution, apply `skip_when_running` or `retry_when_busy`.
5. Start the selected execution adapter only after the claim transaction commits.
6. Renew the lease while the execution is active.
7. Finish the occurrence only when the existing Task/Run/ACP layer emits a matching real completion event. Starting an ACP process is not completion.
8. Schedule the next future occurrence after every terminal result.

At startup or system resume, past schedule points are materialized as `missed` without automatically executing them. A configurable late-fire grace is used only to distinguish ordinary timer jitter from a real suspend/resume gap.

## Queue and Unattended Interaction

The active-execution predicate is shared by Direct, Workflow, and AUTO and includes running, permission waiting, user-question waiting, waiting-for-user, and resumable paused states.

Scheduled tasks are validated against the selected Agent's full-auto capability before creation or update. A task that cannot run unattended is rejected with `SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED` instead of being saved in a state that will predictably hang.

If a permission request still appears at runtime, the occurrence ends as `failed` with `SCHEDULED_PERMISSION_REQUIRED`. The linked session and request remain available for inspection, and a notification points to the detail view.

If `AskUserQuestion` appears, the occurrence ends its scheduler lease as `attention_required` with `SCHEDULED_USER_INPUT_REQUIRED`. The Run stays paused and resumable. The user notification links directly to the question. No future occurrence may create a second simultaneous unanswered question for the same job; the configured overlap policy applies and records `skipped` or bounded `retrying`.

## Execution Adapters

All adapters receive an occurrence ID and must emit a completion event carrying that ID.

- **Direct/new:** materialize a new Task, Run, and ACP session.
- **Direct/continuous:** reuse the associated Task/session chain and send a scheduled prompt; each prompt still has its own occurrence ID.
- **Workflow:** reuse the Task when the content fingerprint is unchanged and create a new Run.
- **AUTO:** use the existing AUTO authoring/content snapshot rules and create a new Run or Task as currently defined.

The scheduler never writes canonical Run status directly. It only maps executor lifecycle events to occurrence status and calls existing Task/Run APIs.

## Direct Capability Parity

Direct must expose:

- creation from the composer and from the built-in scheduled-task skill;
- one-shot, every, hourly, daily, weekdays, weekly, and custom Cron schedules;
- full IANA timezone support with system timezone as the default;
- new-session and continuous-session execution;
- enable, pause, edit, delete, and immediate manual execution;
- per-occurrence history with Task/Run/session navigation;
- busy retry, bounded retry count, missed detection, crash recovery, and duplicate prevention;
- `next_run_at`, `last_error`, `run_count`, and `retry_count` diagnostics;
- completion and attention notifications plus an explicit keep-awake setting;
- Agent creation/query/update helpers through localized built-in skills.

The explicit no-name decision is a Gold Band product choice. Instruction-derived titles replace AionUi's free-form name while all execution and history capabilities remain available.

## API Boundaries

The scheduler repository exposes transactional operations:

```text
create_job / update_job / delete_job / set_enabled
create_or_get_occurrence / claim_occurrence / renew_lease
finish_occurrence / mark_missed / recover_expired
get_job_history / run_now
```

The Tauri layer exposes typed command errors and ViewModels for job diagnostics, occurrence history, run-now responses, and attention states. The web layer only renders these types and maps error codes to localized text.

## Migration

On first scheduler database initialization, import each existing `scheduled-task.json` and its trigger records into the new tables. The migration is idempotent and rejects conflicting IDs rather than silently overwriting data. After successful import, the JSON files are retained only as migration evidence and are no longer read or written by runtime code.

Deleting a scheduled job deletes its definition, copied inputs, and occurrence history, while materialized Gold Band Task/Run/ACP history remains intact.

## Verification Requirements

Rust interface tests must cover:

- unique occurrence creation under concurrent claims;
- expired lease recovery and heartbeat ownership;
- real completion after delayed ACP/Run termination;
- missed detection after restart and system resume without backfill;
- busy skip/retry behavior;
- permission failure and `attention_required` user-question behavior;
- run-now not advancing the schedule;
- Direct new/continuous behavior and content identity boundaries;
- Workflow/AUTO adapter behavior and task reuse;
- migration and deletion history preservation.

Web tests must cover the management list, detail/history view, run-now action, pause/resume, diagnostics, attention notification deep link, and no-name instruction-derived title.
