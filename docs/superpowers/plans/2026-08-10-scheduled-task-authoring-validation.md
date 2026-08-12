# Scheduled Task Authoring Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make scheduled-task authoring use the system timezone, resolve one-time wall-clock input authoritatively in Rust, and reject invalid At, Cron, weekly, and Every schedules before persistence.

**Architecture:** Keep the persisted `ScheduleSpec` unchanged, but introduce a separate authoring DTO that can only become a `ScheduleSpec` through validated domain constructors. The frontend uses Temporal and cron-parser for immediate feedback; Rust remains authoritative and maps semantic failures to the existing structured scheduled-task error envelope.

**Tech Stack:** Rust (`chrono`, `chrono-tz`, `cron`, serde), Tauri 2, React 19, TypeScript, `@js-temporal/polyfill`, `cron-parser`, Vitest, shadcn/ui, Tailwind CSS.

**Workspace constraint:** Several target files already contain uncommitted user changes. Preserve those changes and do not create implementation commits that would accidentally include them; verify and report the scoped diff instead.

---

### Task 1: Add authoritative local-time construction to the scheduler domain

**Files:**
- Modify: `src/scheduler/mod.rs`
- Test: `src/scheduler/mod.rs` unit-test module

- [ ] **Step 1: Write failing domain tests for ordinary, nonexistent, and ambiguous local times**

Add tests that express the public API before implementing it:

```rust
#[test]
fn at_local_resolves_an_ordinary_wall_clock_time() {
    let schedule = ScheduleSpec::at_local(
        "2026-08-10",
        "09:30",
        "Asia/Shanghai",
        LocalTimeDisambiguation::Earlier,
    )
    .unwrap();

    assert_eq!(
        schedule.kind,
        ScheduleKind::At {
            at: Utc.with_ymd_and_hms(2026, 8, 10, 1, 30, 0).unwrap(),
            timezone: "Asia/Shanghai".to_string(),
        }
    );
}

#[test]
fn at_local_rejects_a_nonexistent_dst_time() {
    assert!(matches!(
        ScheduleSpec::at_local(
            "2026-03-08",
            "02:30",
            "America/New_York",
            LocalTimeDisambiguation::Earlier,
        ),
        Err(ScheduleError::NonexistentLocalTime { .. })
    ));
}

#[test]
fn at_local_honors_dst_overlap_disambiguation() {
    let earlier = ScheduleSpec::at_local(
        "2026-11-01",
        "01:30",
        "America/New_York",
        LocalTimeDisambiguation::Earlier,
    )
    .unwrap();
    let later = ScheduleSpec::at_local(
        "2026-11-01",
        "01:30",
        "America/New_York",
        LocalTimeDisambiguation::Later,
    )
    .unwrap();

    let ScheduleKind::At { at: earlier_at, .. } = earlier.kind else {
        panic!("expected At schedule");
    };
    let ScheduleKind::At { at: later_at, .. } = later.kind else {
        panic!("expected At schedule");
    };
    assert_eq!(earlier_at, Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap());
    assert_eq!(later_at, Utc.with_ymd_and_hms(2026, 11, 1, 6, 30, 0).unwrap());
}

#[test]
fn at_local_rejects_invalid_date_time_and_timezone() {
    assert!(matches!(
        ScheduleSpec::at_local(
            "2026-02-30",
            "09:30",
            "UTC",
            LocalTimeDisambiguation::Earlier,
        ),
        Err(ScheduleError::InvalidLocalDate { .. })
    ));
    assert!(matches!(
        ScheduleSpec::at_local(
            "2026-08-10",
            "25:00",
            "UTC",
            LocalTimeDisambiguation::Earlier,
        ),
        Err(ScheduleError::InvalidTime { .. })
    ));
    assert!(matches!(
        ScheduleSpec::at_local(
            "2026-08-10",
            "09:30",
            "Invalid/Zone",
            LocalTimeDisambiguation::Earlier,
        ),
        Err(ScheduleError::InvalidTimezone { .. })
    ));
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```powershell
cargo test -p gold-band scheduler::tests::at_local -- --nocapture
```

Expected: compilation fails because `LocalTimeDisambiguation`, `ScheduleSpec::at_local`, and the new error variants do not exist.

- [ ] **Step 3: Implement the minimal domain API**

Add the enum and error variants:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalTimeDisambiguation {
    Earlier,
    Later,
}

#[error("invalid local date: {date}")]
InvalidLocalDate { date: String },
#[error("nonexistent local time: {local_date} {local_time} {timezone}")]
NonexistentLocalTime {
    local_date: String,
    local_time: String,
    timezone: String,
},
```

Implement `ScheduleSpec::at_local` with `NaiveDate::parse_from_str`, `NaiveTime::parse_from_str`, `chrono_tz::Tz::from_local_datetime`, and explicit UTC ordering for ambiguous candidates. Do not add a second persisted representation or change existing serialization.

- [ ] **Step 4: Verify GREEN and run all scheduler domain tests**

Run:

```powershell
cargo test -p gold-band scheduler::tests -- --nocapture
```

Expected: all scheduler unit tests pass, including both DST candidates.

### Task 2: Separate Tauri authoring input from persisted ScheduleSpec

**Files:**
- Modify: `src-tauri/src/view_models_conversation.rs`
- Modify: `src-tauri/src/scheduled_service.rs`
- Test: `src-tauri/src/scheduled_service.rs` unit-test module

- [ ] **Step 1: Write failing service-boundary tests**

Add test helpers that construct `ScheduledScheduleInputVm`, then add these tests:

```rust
#[test]
fn create_rejects_invalid_cron_before_persisting_or_notifying() {
    let fixture = Fixture::new();
    let mut input = fixture.create_input();
    input.schedule = ScheduledScheduleInputVm::Cron {
        expression: "not a cron".to_string(),
        timezone: "UTC".to_string(),
    };

    let error = fixture.service.create(input).unwrap_err();

    assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
    assert_eq!(error.params["field"], "schedule.cron");
    assert_eq!(error.params["reason"], "invalid-cron");
    assert!(fixture.database.list_job_definitions().unwrap().is_empty());
    assert_eq!(fixture.coordinator.command_count(), 0);
}

#[test]
fn create_rejects_empty_weekly_days() {
    let fixture = Fixture::new();
    let mut input = fixture.create_input();
    input.schedule = ScheduledScheduleInputVm::Repeat {
        preset: RepeatPreset::Weekly { weekdays: Vec::new() },
        hour: 9,
        minute: 0,
        timezone: "UTC".to_string(),
    };

    let error = fixture.service.create(input).unwrap_err();

    assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
    assert_eq!(error.params["field"], "schedule.weekdays");
    assert_eq!(error.params["reason"], "empty-weekdays");
}

#[test]
fn create_rejects_zero_every_value() {
    let fixture = Fixture::new();
    let mut input = fixture.create_input();
    input.schedule = ScheduledScheduleInputVm::Every {
        every: ScheduledEveryInputVm { value: 0, unit: "minutes".to_string() },
        anchor_at: Utc::now(),
        timezone: "UTC".to_string(),
    };

    let error = fixture.service.create(input).unwrap_err();

    assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
    assert_eq!(error.params["field"], "schedule.every");
    assert_eq!(error.params["reason"], "invalid-every-value");
}

#[test]
fn create_normalizes_local_at_to_utc_and_keeps_timezone() {
    let fixture = Fixture::new();
    let mut input = fixture.create_input();
    input.schedule = ScheduledScheduleInputVm::At {
        local_date: "2026-11-01".to_string(),
        local_time: "01:30".to_string(),
        timezone: "America/New_York".to_string(),
        disambiguation: LocalTimeDisambiguation::Later,
    };

    let created = fixture.service.create(input).unwrap();
    let ScheduleKind::At { at, timezone } = created.definition.schedule.kind else {
        panic!("expected At schedule");
    };
    assert_eq!(at, Utc.with_ymd_and_hms(2026, 11, 1, 6, 30, 0).unwrap());
    assert_eq!(timezone, "America/New_York");
}
```

Add this update-path regression test:

```rust
#[test]
fn update_rejects_invalid_schedule_without_mutating_the_job() {
    let fixture = Fixture::new();
    let created = fixture.service.create(fixture.create_input()).unwrap();
    let command_count = fixture.coordinator.command_count();
    let mut input = fixture.update_input(&created.definition, "unchanged");
    input.schedule = ScheduledScheduleInputVm::Cron {
        expression: "invalid".to_string(),
        timezone: "UTC".to_string(),
    };

    let error = fixture.service.update(input).unwrap_err();
    let persisted = fixture
        .database
        .get_job_definition(&created.definition.project_id, created.definition.id())
        .unwrap()
        .unwrap();

    assert_eq!(error.code, ScheduledErrorCode::ValidationFailed);
    assert_eq!(persisted.definition.schedule, created.definition.schedule);
    assert_eq!(fixture.coordinator.command_count(), command_count);
}
```

Add `CoordinatorSpy::command_count()` as a test-only accessor returning the locked vector length.

- [ ] **Step 2: Run service tests and verify RED**

Run:

```powershell
cargo test -p gold-band-desktop scheduled_service::tests::create_rejects_invalid_cron_before_persisting_or_notifying -- --nocapture
```

Expected: compilation fails because the input DTO and conversion boundary do not exist.

- [ ] **Step 3: Add the authoring DTO and a single conversion method**

In `view_models_conversation.rs`, define:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
#[serde(rename_all_fields = "camelCase")]
pub enum ScheduledScheduleInputVm {
    At {
        local_date: String,
        local_time: String,
        timezone: String,
        disambiguation: LocalTimeDisambiguation,
    },
    Repeat {
        preset: RepeatPreset,
        hour: u32,
        minute: u32,
        timezone: String,
    },
    Every {
        every: ScheduledEveryInputVm,
        anchor_at: DateTime<Utc>,
        timezone: String,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduledEveryInputVm {
    pub value: u64,
    pub unit: String,
}
```

Implement `try_into_schedule_spec(self) -> Result<ScheduleSpec, ScheduleError>` by calling only the four validated domain constructors. Change `CreateScheduledTaskInputVm.schedule` and `UpdateScheduledTaskInputVm.schedule` to this DTO. Keep `ScheduledTaskEditVm.schedule` as canonical `ScheduleSpec`.

Update the service test fixture to use one stable future authoring input for create and update tests:

```rust
fn valid_schedule_input() -> ScheduledScheduleInputVm {
    ScheduledScheduleInputVm::At {
        local_date: "2099-01-01".to_string(),
        local_time: "09:00".to_string(),
        timezone: "UTC".to_string(),
        disambiguation: LocalTimeDisambiguation::Earlier,
    }
}
```

Both `Fixture::create_input()` and `Fixture::update_input()` must call this helper instead of assigning a canonical `ScheduleSpec` to a command DTO.

- [ ] **Step 4: Map ScheduleError to the stable structured envelope**

In `scheduled_service.rs`, add one exhaustive mapper:

```rust
fn schedule_input_error(error: ScheduleError) -> ScheduledServiceError {
    let params = match error {
        ScheduleError::InvalidCron { expression } =>
            json!({ "field": "schedule.cron", "reason": "invalid-cron", "expression": expression }),
        ScheduleError::EmptyWeekdays =>
            json!({ "field": "schedule.weekdays", "reason": "empty-weekdays" }),
        ScheduleError::InvalidEveryValue =>
            json!({ "field": "schedule.every", "reason": "invalid-every-value" }),
        ScheduleError::UnsupportedEveryUnit { unit } =>
            json!({ "field": "schedule.every", "reason": "unsupported-every-unit", "unit": unit }),
        ScheduleError::InvalidTimezone { timezone } =>
            json!({ "field": "schedule.timezone", "reason": "invalid-timezone", "timezone": timezone }),
        ScheduleError::InvalidLocalDate { date } =>
            json!({ "field": "schedule.at", "reason": "invalid-date", "date": date }),
        ScheduleError::InvalidTime { time } =>
            json!({ "field": "schedule.at", "reason": "invalid-time", "time": time }),
        ScheduleError::NonexistentLocalTime { local_date, local_time, timezone } => json!({
            "field": "schedule.at",
            "reason": "nonexistent-local-time",
            "localDate": local_date,
            "localTime": local_time,
            "timezone": timezone,
        }),
        ScheduleError::EmptyScheduledTaskId =>
            json!({ "field": "scheduledTaskId", "reason": "empty-scheduled-task-id" }),
        ScheduleError::EmptyProjectId =>
            json!({ "field": "projectId", "reason": "empty-project-id" }),
        ScheduleError::UnsupportedMode { mode } =>
            json!({ "field": "runMode", "reason": "unsupported-mode", "mode": mode }),
    };
    ScheduledServiceError::new(ScheduledErrorCode::ValidationFailed, params)
}
```

Keep this match exhaustive so no backend error prose enters the UI contract. Convert the schedule at the beginning of both `create` and `update`, then pass only canonical `ScheduleSpec` to definitions and persistence.

- [ ] **Step 5: Verify service tests and the desktop crate**

Run:

```powershell
cargo test -p gold-band-desktop scheduled_service::tests -- --nocapture
cargo check -p gold-band-desktop
```

Expected: service tests pass; create/update fixtures use authoring DTOs; desktop crate compiles.

### Task 3: Add pure frontend authoring validation with mature parsers

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Create: `web/src/lib/scheduled-task-authoring.ts`
- Create: `web/tests/scheduled-task-authoring.test.ts`
- Modify: `web/src/lib/scheduled-task-timezones.ts`
- Test: `web/tests/scheduled-task-timezones.test.ts`

- [ ] **Step 1: Add parser dependencies**

Run:

```powershell
npm install @js-temporal/polyfill@0.5.1 cron-parser@5.8.1
```

Expected: both packages appear in `dependencies`; only npm dependency metadata changes.

- [ ] **Step 2: Write failing pure-function tests**

Define the expected public API in tests:

```ts
expect(analyzeScheduledLocalTime('2026-08-10', '09:30', 'Asia/Shanghai')).toMatchObject({
  kind: 'valid',
  earlierInstant: '2026-08-10T01:30:00Z',
});
expect(analyzeScheduledLocalTime('2026-03-08', '02:30', 'America/New_York').kind)
  .toBe('nonexistent');
expect(analyzeScheduledLocalTime('2026-11-01', '01:30', 'America/New_York'))
  .toMatchObject({ kind: 'ambiguous' });

expect(validateScheduledCron('0 0 9 * * MON-FRI')).toBeNull();
expect(validateScheduledCron('not a cron')).toBe('invalid-cron');
expect(validateScheduledWeeklyDays([])).toBe('empty-weekdays');
expect(validateScheduledEvery('0')).toBe('invalid-every-value');
expect(validateScheduledEvery('1.5')).toBe('invalid-every-value');
```

Extend timezone tests by injecting a resolver and asserting valid system zone, invalid zone, thrown resolver, and empty resolver all return the expected system zone or `UTC`.

- [ ] **Step 3: Run focused web tests and verify RED**

Run:

```powershell
npm run web:test -- web/tests/scheduled-task-authoring.test.ts web/tests/scheduled-task-timezones.test.ts
```

Expected: tests fail because the new authoring helpers and injectable system-zone resolver do not exist.

- [ ] **Step 4: Implement the pure helper module**

Use `Temporal.PlainDateTime` and `Temporal.ZonedDateTime.from(..., { disambiguation })` to compare earlier/later candidates with the requested local time. Return a discriminated result:

```ts
export type ScheduledLocalTimeAnalysis =
  | { kind: 'invalid' }
  | { kind: 'nonexistent' }
  | { kind: 'valid'; earlierInstant: string; earlierOffset: string }
  | {
      kind: 'ambiguous';
      earlierInstant: string;
      laterInstant: string;
      earlierOffset: string;
      laterOffset: string;
    };
```

Use `CronExpressionParser.parse(expression, { strict: true })` for the six-field UI contract. Validate Every using the original string and `Number.isSafeInteger`; never coerce invalid values to `1`.

Add `getScheduledSystemTimezone(resolver = defaultResolver)` beside the timezone catalog. Validate the resolver result against the catalog and return `UTC` on empty, invalid, or thrown values.

- [ ] **Step 5: Verify GREEN**

Run:

```powershell
npm run web:test -- web/tests/scheduled-task-authoring.test.ts web/tests/scheduled-task-timezones.test.ts
```

Expected: all authoring and timezone cases pass.

### Task 4: Integrate validated input into types, formatting, and ScheduledTaskDialog

**Files:**
- Modify: `web/src/types.ts`
- Modify: `web/src/components/conversation/ScheduledTaskDialog.tsx`
- Modify: `web/src/components/conversation/ConversationComposer.tsx`
- Modify: `web/src/pages/ScheduledTaskDetailPage.tsx`
- Modify: `web/src/pages/ScheduledTaskManagementPage.tsx`
- Modify: `web/src/lib/scheduled-task-formatting.ts`
- Modify: `web/src/i18n.ts`
- Modify: `web/tests/scheduled-task-composer.test.ts`
- Create: `web/tests/scheduled-task-dialog-validation.test.ts`

- [ ] **Step 1: Write failing type/interaction contract tests**

Replace the old source assertion for `zonedDateTimeToUtcIso` with assertions that the dialog emits local authoring fields and has no custom offset conversion. Add source-level UI contract tests proving:

```ts
expect(dialogSource).not.toContain('function zonedDateTimeToUtcIso');
expect(dialogSource).toContain('getScheduledSystemTimezone');
expect(dialogSource).toContain('analyzeScheduledLocalTime');
expect(dialogSource).toContain("disambiguation: atDisambiguation");
expect(dialogSource).toContain('validationIssue');
expect(dialogSource).toContain('disabled={!canSave || saving}');
```

Add pure payload tests for normal At, ambiguous Earlier/Later, valid Cron, weekly, and Every schedules through an exported `buildScheduledScheduleInput` helper.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```powershell
npm run web:test -- web/tests/scheduled-task-composer.test.ts web/tests/scheduled-task-dialog-validation.test.ts
```

Expected: tests fail on the old UTC payload and missing validation controls.

- [ ] **Step 3: Split persisted and authoring TypeScript types**

Keep `ScheduledScheduleSpec` unchanged and add:

```ts
export type ScheduledAtDisambiguation = 'earlier' | 'later';
export type ScheduledOverlapPolicy = 'skip_when_running' | 'retry_when_busy';
export type ScheduledSessionPolicy = 'new' | 'continuous';
export type ScheduledRepeatPreset =
  | 'Hourly'
  | 'Daily'
  | 'Weekdays'
  | { Weekly: { weekdays: string[] } };
export type ScheduledScheduleInput =
  | {
      kind: 'At';
      localDate: string;
      localTime: string;
      timezone: string;
      disambiguation: ScheduledAtDisambiguation;
    }
  | { kind: 'Every'; every: { value: number; unit: ScheduledEveryUnit }; anchorAt: string; timezone: string }
  | { kind: 'Repeat'; preset: ScheduledRepeatPreset; hour: number; minute: number; timezone: string }
  | { kind: 'Cron'; expression: string; timezone: string };
```

Change only Create/Update command inputs to `ScheduledScheduleInput`. Query VMs continue using `ScheduledScheduleSpec`.

- [ ] **Step 4: Refactor the dialog around a single validation result**

Split output and initial config types:

```ts
export type ScheduledTaskConfig = {
  schedule: ScheduledScheduleInput;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
};

export type ScheduledTaskInitialConfig = {
  schedule: ScheduledScheduleSpec;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
};
```

On a new dialog open, reset timezone to `getScheduledSystemTimezone()` and reset disambiguation to `earlier`. On edit, restore local At fields from canonical UTC and the stored timezone.

Compute `validationIssue` with `useMemo`; render the localized error immediately below its field. For an ambiguous At time, render a compact shadcn segmented control for Earlier/Later and include both offsets in the labels. `canSave` is exactly `validationIssue === null`.

Build the schedule without correction:

```ts
const schedule = buildScheduledScheduleInput({
  tab,
  atDate,
  atTime,
  atDisambiguation,
  frequency,
  selectedWeekdays,
  repeatTime,
  everyValue,
  everyUnit,
  cron,
  timezone,
});
```

Update composer summary formatting with a dedicated `formatScheduledScheduleInput`; do not weaken the existing persisted schedule formatter with a union that obscures the two models. Change management/detail edit helpers to return `ScheduledTaskInitialConfig`.

- [ ] **Step 5: Add localized validation and ambiguity labels**

Add matching zh-CN/en keys under `scheduled.dialog.validation` and `scheduled.dialog.disambiguation` for invalid date/time/timezone, nonexistent local time, invalid Cron, empty weekdays, invalid Every, Earlier, and Later. Do not add backend customer prose.

- [ ] **Step 6: Verify the focused web tests and production type-check**

Run:

```powershell
npm run web:test -- web/tests/scheduled-task-authoring.test.ts web/tests/scheduled-task-dialog-validation.test.ts web/tests/scheduled-task-composer.test.ts web/tests/scheduled-task-timezones.test.ts
npm run web:build
```

Expected: tests and TypeScript/Vite production build pass without warnings introduced by this change.

### Task 5: Synchronize product documentation and lock regression coverage

**Files:**
- Modify: `docs/gold-band/产品设计文档/runtime/scheduled-task.md`
- Modify: `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`
- Modify: `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`
- Modify: `docs/superpowers/specs/2026-08-10-scheduled-task-authoring-validation-design.md`

- [ ] **Step 1: Update the authoritative documents**

Document these final contracts in all required locations:

```text
- command authoring input is distinct from persisted ScheduleSpec
- At is submitted as localDate/localTime/timezone/disambiguation
- nonexistent DST time is invalid
- ambiguous DST time requires Earlier/Later, default Earlier
- frontend validation is immediate; Rust is authoritative
- Cron uses the six-field product syntax
- weekly requires at least one day; Every requires a positive integer
```

Mark the corresponding development-plan items complete only after tests and UI verification succeed. Update the design status to `已实现并验证` only at final completion.

- [ ] **Step 2: Run the complete relevant automated suites**

Run:

```powershell
cargo test -p gold-band scheduler::tests -- --nocapture
cargo test -p gold-band-desktop scheduled_service::tests -- --nocapture
npm run web:test
npm run web:build
```

Expected: every command exits 0 with zero failed tests.

- [ ] **Step 3: Start the frontend and perform UI verification**

Run the frontend on a dedicated port:

```powershell
npm run web:dev -- --port 1421
```

Open `http://127.0.0.1:1421/chat` for composer creation and `http://127.0.0.1:1421/chat/scheduled-tasks` for management/editing. If port 1421 is already occupied, use the next free port and record it in the verification result. Verify at desktop and mobile viewport widths:

```text
1. New task defaults to the OS IANA timezone.
2. Invalid Cron disables Done and shows localized feedback.
3. Weekly with zero selected days disables Done.
4. Empty, zero, negative, and decimal Every values disable Done.
5. America/New_York 2026-03-08 02:30 is rejected.
6. America/New_York 2026-11-01 01:30 shows Earlier/Later and submits distinct choices.
7. Editing an existing At task restores its local wall-clock value and timezone.
8. No text or controls overlap at desktop or mobile width.
```

Stop only the development server and test resources started for this verification.

- [ ] **Step 4: Inspect the final scoped diff**

Run:

```powershell
git diff --check
git status --short
```

Expected: no whitespace errors; only intended files are reported as changed in addition to the user's pre-existing worktree changes.
