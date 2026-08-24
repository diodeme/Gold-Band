use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Timelike, Utc, Weekday,
};
use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

pub mod coordinator;
pub mod db;
pub mod fingerprint;
pub mod occurrence;
pub mod queue;

pub use fingerprint::{
    AutoAuthoringIdentity, ScheduledTaskContentInput, ScheduledTaskContentSnapshot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EveryUnit {
    Minutes,
    Hours,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalTimeDisambiguation {
    Earlier,
    Later,
}

impl EveryUnit {
    fn parse(value: &str) -> Result<Self, ScheduleError> {
        match value {
            "minutes" => Ok(Self::Minutes),
            "hours" => Ok(Self::Hours),
            other => Err(ScheduleError::UnsupportedEveryUnit {
                unit: other.to_string(),
            }),
        }
    }

    fn duration(self, value: u64) -> Duration {
        match self {
            Self::Minutes => Duration::minutes(value as i64),
            Self::Hours => Duration::hours(value as i64),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    #[error("every interval must be a positive integer")]
    InvalidEveryValue,
    #[error("unsupported every unit: {unit}")]
    UnsupportedEveryUnit { unit: String },
    #[error("invalid timezone: {timezone}")]
    InvalidTimezone { timezone: String },
    #[error("invalid local date: {date}")]
    InvalidLocalDate { date: String },
    #[error("invalid local time: {time}")]
    InvalidTime { time: String },
    #[error("nonexistent local time: {local_date} {local_time} {timezone}")]
    NonexistentLocalTime {
        local_date: String,
        local_time: String,
        timezone: String,
    },
    #[error("weekly schedule requires at least one weekday")]
    EmptyWeekdays,
    #[error("invalid cron expression: {expression}")]
    InvalidCron { expression: String },
    #[error("scheduled task id cannot be empty")]
    EmptyScheduledTaskId,
    #[error("scheduled task project id cannot be empty")]
    EmptyProjectId,
    #[error("unsupported scheduled task mode: {mode}")]
    UnsupportedMode { mode: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EverySpec {
    pub value: u64,
    pub unit: EveryUnit,
}

impl EverySpec {
    pub fn new(value: u64, unit: &str) -> Result<Self, ScheduleError> {
        if value == 0 {
            return Err(ScheduleError::InvalidEveryValue);
        }
        Ok(Self {
            value,
            unit: EveryUnit::parse(unit)?,
        })
    }

    fn duration(&self) -> Duration {
        self.unit.duration(self.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatPreset {
    Hourly,
    Daily,
    Weekdays,
    Weekly { weekdays: Vec<Weekday> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSpec {
    #[serde(flatten)]
    pub kind: ScheduleKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduledMode {
    Direct,
    Workflow,
    Auto,
}

impl ScheduledMode {
    fn parse(value: &str) -> Result<Self, ScheduleError> {
        match value {
            "direct" => Ok(Self::Direct),
            "workflow" => Ok(Self::Workflow),
            "auto" => Ok(Self::Auto),
            other => Err(ScheduleError::UnsupportedMode {
                mode: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy {
    SkipWhenRunning,
    RetryWhenBusy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskDefinition {
    pub version: String,
    pub id: String,
    pub project_id: String,
    pub enabled: bool,
    pub mode: ScheduledMode,
    pub session_policy: SessionPolicy,
    pub task_id: Option<String>,
    pub content_fingerprint: String,
    #[serde(default)]
    pub content_snapshot: ScheduledTaskContentSnapshot,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub execution_config: serde_json::Value,
    #[serde(default)]
    pub attachment_names: Vec<String>,
    pub schedule: ScheduleSpec,
    pub overlap_policy: OverlapPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_trigger_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_trigger_status: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub retry_count: u8,
    #[serde(default)]
    pub retry_at: Option<DateTime<Utc>>,
}

impl ScheduledTaskDefinition {
    pub fn display_schedule(&self) -> String {
        match &self.schedule.kind {
            ScheduleKind::At { at, .. } => format!("At {}", at.to_rfc3339()),
            ScheduleKind::Every { every, .. } => format!(
                "Every {} {}",
                every.value,
                match every.unit {
                    EveryUnit::Minutes => "minutes",
                    EveryUnit::Hours => "hours",
                }
            ),
            ScheduleKind::Repeat {
                preset,
                hour,
                minute,
                ..
            } => format!(
                "{} {:02}:{:02}",
                match preset {
                    RepeatPreset::Hourly => "hourly",
                    RepeatPreset::Daily => "daily",
                    RepeatPreset::Weekdays => "weekdays",
                    RepeatPreset::Weekly { .. } => "weekly",
                },
                hour,
                minute
            ),
            ScheduleKind::Cron { expression, .. } => format!("Cron {expression}"),
        }
    }

    pub fn next_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let baseline = self.last_trigger_at.unwrap_or_else(|| {
            self.created_at
                .checked_sub_signed(Duration::seconds(1))
                .unwrap_or(self.created_at)
        });
        self.schedule
            .next_occurrence_after(baseline)
            .filter(|value| *value <= now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionPolicy {
    New,
    Continuous,
}

impl ScheduledTaskDefinition {
    pub fn new(
        project_id: &str,
        id: &str,
        mode: &str,
        schedule: ScheduleSpec,
        overlap_policy: OverlapPolicy,
    ) -> Result<Self, ScheduleError> {
        if project_id.trim().is_empty() {
            return Err(ScheduleError::EmptyProjectId);
        }
        if id.trim().is_empty() {
            return Err(ScheduleError::EmptyScheduledTaskId);
        }
        let now = Utc::now();
        let scheduled_mode = ScheduledMode::parse(mode)?;
        Ok(Self {
            version: "0.1".to_string(),
            id: id.to_string(),
            project_id: project_id.to_string(),
            enabled: true,
            mode: scheduled_mode,
            session_policy: SessionPolicy::New,
            task_id: None,
            content_fingerprint: String::new(),
            content_snapshot: ScheduledTaskContentInput::new(
                scheduled_mode,
                String::new(),
                std::iter::empty::<String>(),
                project_id.to_string(),
            ),
            instruction: String::new(),
            execution_config: serde_json::json!({}),
            attachment_names: Vec::new(),
            schedule,
            overlap_policy,
            created_at: now,
            updated_at: now,
            last_trigger_at: None,
            last_trigger_status: None,
            last_error: None,
            retry_count: 0,
            retry_at: None,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn recompute_content_fingerprint(&mut self) -> anyhow::Result<()> {
        self.content_fingerprint = fingerprint::content_fingerprint(&self.content_snapshot)?;
        Ok(())
    }

    pub fn with_session_policy(mut self, policy: SessionPolicy) -> Result<Self, ScheduleError> {
        if !matches!(self.mode, ScheduledMode::Direct) && policy == SessionPolicy::Continuous {
            return Err(ScheduleError::UnsupportedMode {
                mode: "continuous-session-requires-direct".to_string(),
            });
        }
        self.session_policy = policy;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[serde(rename_all_fields = "camelCase")]
pub enum ScheduleKind {
    At {
        at: DateTime<Utc>,
        #[serde(default = "default_timezone")]
        timezone: String,
    },
    Repeat {
        preset: RepeatPreset,
        hour: u32,
        minute: u32,
        #[serde(default = "default_timezone")]
        timezone: String,
    },
    Every {
        every: EverySpec,
        anchor_at: DateTime<Utc>,
        #[serde(default = "default_timezone")]
        timezone: String,
    },
    Cron {
        expression: String,
        #[serde(default = "default_timezone")]
        timezone: String,
    },
}

impl ScheduleSpec {
    pub fn every(value: u64, unit: &str, anchor_at: DateTime<Utc>) -> Result<Self, ScheduleError> {
        Self::every_in_timezone(value, unit, anchor_at, &default_timezone())
    }

    pub fn every_in_timezone(
        value: u64,
        unit: &str,
        anchor_at: DateTime<Utc>,
        timezone: &str,
    ) -> Result<Self, ScheduleError> {
        parse_timezone(timezone)?;
        Ok(Self {
            kind: ScheduleKind::Every {
                every: EverySpec::new(value, unit)?,
                anchor_at,
                timezone: timezone.to_string(),
            },
        })
    }

    pub fn at(at: DateTime<Utc>) -> Self {
        Self::at_in_timezone(at, &default_timezone()).expect("default timezone is valid")
    }

    pub fn at_in_timezone(at: DateTime<Utc>, timezone: &str) -> Result<Self, ScheduleError> {
        parse_timezone(timezone)?;
        Ok(Self {
            kind: ScheduleKind::At {
                at,
                timezone: timezone.to_string(),
            },
        })
    }

    pub fn at_local(
        local_date: &str,
        local_time: &str,
        timezone: &str,
        disambiguation: LocalTimeDisambiguation,
    ) -> Result<Self, ScheduleError> {
        let date = NaiveDate::parse_from_str(local_date, "%Y-%m-%d").map_err(|_| {
            ScheduleError::InvalidLocalDate {
                date: local_date.to_string(),
            }
        })?;
        let time = NaiveTime::parse_from_str(local_time, "%H:%M").map_err(|_| {
            ScheduleError::InvalidTime {
                time: local_time.to_string(),
            }
        })?;
        let timezone_value = parse_timezone(timezone)?;
        let local = NaiveDateTime::new(date, time);
        let at = match timezone_value.from_local_datetime(&local) {
            LocalResult::None => {
                return Err(ScheduleError::NonexistentLocalTime {
                    local_date: local_date.to_string(),
                    local_time: local_time.to_string(),
                    timezone: timezone.to_string(),
                });
            }
            LocalResult::Single(candidate) => candidate.with_timezone(&Utc),
            LocalResult::Ambiguous(first, second) => {
                let first = first.with_timezone(&Utc);
                let second = second.with_timezone(&Utc);
                match disambiguation {
                    LocalTimeDisambiguation::Earlier => first.min(second),
                    LocalTimeDisambiguation::Later => first.max(second),
                }
            }
        };
        Self::at_in_timezone(at, timezone)
    }

    pub fn repeat(
        preset: RepeatPreset,
        hour: u32,
        minute: u32,
        timezone: &str,
    ) -> Result<Self, ScheduleError> {
        validate_time(hour, minute)?;
        parse_timezone(timezone)?;
        if matches!(preset, RepeatPreset::Weekly { ref weekdays } if weekdays.is_empty()) {
            return Err(ScheduleError::EmptyWeekdays);
        }
        Ok(Self {
            kind: ScheduleKind::Repeat {
                preset,
                hour,
                minute,
                timezone: timezone.to_string(),
            },
        })
    }

    pub fn cron(expression: &str, timezone: &str) -> Result<Self, ScheduleError> {
        parse_timezone(timezone)?;
        Schedule::from_str(expression).map_err(|_| ScheduleError::InvalidCron {
            expression: expression.to_string(),
        })?;
        Ok(Self {
            kind: ScheduleKind::Cron {
                expression: expression.to_string(),
                timezone: timezone.to_string(),
            },
        })
    }

    pub fn anchor_at(&self) -> DateTime<Utc> {
        match self.kind {
            ScheduleKind::Every { anchor_at, .. } => anchor_at,
            _ => panic!("anchor_at is only available for every schedules"),
        }
    }

    pub fn reset_anchor(mut self, anchor_at: DateTime<Utc>) -> Self {
        if let ScheduleKind::Every {
            anchor_at: current, ..
        } = &mut self.kind
        {
            *current = anchor_at;
        }
        self
    }

    pub fn with_every(
        mut self,
        value: u64,
        unit: &str,
        anchor_at: DateTime<Utc>,
    ) -> Result<Self, ScheduleError> {
        let every = EverySpec::new(value, unit)?;
        if matches!(
            &self.kind,
            ScheduleKind::Every { every: current, .. } if current == &every
        ) {
            return Ok(self);
        }
        let timezone = self
            .timezone()
            .map(str::to_string)
            .unwrap_or_else(default_timezone);
        self.kind = ScheduleKind::Every {
            every,
            anchor_at,
            timezone,
        };
        Ok(self)
    }

    pub fn next_occurrence_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match &self.kind {
            ScheduleKind::At { at, .. } => (*at > after).then_some(*at),
            ScheduleKind::Every {
                every, anchor_at, ..
            } => {
                let anchor_millis = i128::from(anchor_at.timestamp_millis());
                let after_millis = i128::from(after.timestamp_millis());
                let interval_millis = i128::from(every.duration().num_milliseconds());
                let elapsed_millis = after_millis - anchor_millis;
                let periods = if elapsed_millis < 0 {
                    1
                } else {
                    elapsed_millis.div_euclid(interval_millis) + 1
                };
                let next_millis =
                    anchor_millis.checked_add(interval_millis.checked_mul(periods)?)?;
                DateTime::from_timestamp_millis(i64::try_from(next_millis).ok()?)
            }
            ScheduleKind::Repeat {
                preset,
                hour,
                minute,
                timezone,
            } => next_repeat_occurrence(preset, *hour, *minute, timezone, after),
            ScheduleKind::Cron {
                expression,
                timezone,
            } => next_cron_occurrence(expression, timezone, after),
        }
    }

    pub fn timezone(&self) -> Option<&str> {
        match &self.kind {
            ScheduleKind::Repeat { timezone, .. } | ScheduleKind::Cron { timezone, .. } => {
                Some(timezone.as_str())
            }
            ScheduleKind::At { timezone, .. } | ScheduleKind::Every { timezone, .. } => {
                Some(timezone.as_str())
            }
        }
    }
}

pub fn system_timezone() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .filter(|timezone| parse_timezone(timezone).is_ok())
        .unwrap_or_else(|| "UTC".to_string())
}

fn default_timezone() -> String {
    system_timezone()
}

fn validate_time(hour: u32, minute: u32) -> Result<(), ScheduleError> {
    NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(|| ScheduleError::InvalidTime {
        time: format!("{hour:02}:{minute:02}"),
    })?;
    Ok(())
}

fn parse_timezone(timezone: &str) -> Result<Tz, ScheduleError> {
    timezone
        .parse()
        .map_err(|_| ScheduleError::InvalidTimezone {
            timezone: timezone.to_string(),
        })
}

fn next_repeat_occurrence(
    preset: &RepeatPreset,
    hour: u32,
    minute: u32,
    timezone: &str,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let tz = parse_timezone(timezone).ok()?;
    let local_after = after.with_timezone(&tz);

    if matches!(preset, RepeatPreset::Hourly) {
        let mut candidate =
            local_after
                .naive_local()
                .date()
                .and_hms_opt(local_after.hour(), 0, 0)?;
        if candidate <= local_after.naive_local() {
            candidate = candidate.checked_add_signed(Duration::hours(1))?;
        }
        for _ in 0..(366 * 24 + 1) {
            if let Some(next) = next_resolved_local_datetime(&tz, candidate, local_after) {
                return Some(next.with_timezone(&Utc));
            }
            candidate = candidate.checked_add_signed(Duration::hours(1))?;
        }
        return None;
    }

    let mut date = local_after.date_naive();
    for _ in 0..8 {
        let weekday = date.weekday();
        let allowed = match preset {
            RepeatPreset::Hourly => true,
            RepeatPreset::Daily => true,
            RepeatPreset::Weekdays => matches!(
                weekday,
                Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri
            ),
            RepeatPreset::Weekly { weekdays } => weekdays.contains(&weekday),
        };
        if allowed {
            let local = NaiveDateTime::new(date, NaiveTime::from_hms_opt(hour, minute, 0)?);
            if let Some(next) = next_resolved_local_datetime(&tz, local, local_after) {
                return Some(next.with_timezone(&Utc));
            }
        }
        date = date.succ_opt()?;
    }
    None
}

fn next_resolved_local_datetime(
    timezone: &Tz,
    local: NaiveDateTime,
    after: DateTime<Tz>,
) -> Option<DateTime<Tz>> {
    match timezone.from_local_datetime(&local) {
        LocalResult::None => None,
        LocalResult::Single(candidate) => (candidate > after).then_some(candidate),
        LocalResult::Ambiguous(first, second) => [first, second]
            .into_iter()
            .filter(|candidate| *candidate > after)
            .min_by_key(|candidate| candidate.with_timezone(&Utc)),
    }
}

fn next_cron_occurrence(
    expression: &str,
    timezone: &str,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let tz = parse_timezone(timezone).ok()?;
    let schedule = Schedule::from_str(expression).ok()?;
    schedule
        .after(&after.with_timezone(&tz))
        .next()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::{
        EverySpec, OverlapPolicy, ScheduleError, ScheduleSpec, ScheduledTaskContentSnapshot,
        ScheduledTaskDefinition, system_timezone,
    };
    use super::{LocalTimeDisambiguation, RepeatPreset, ScheduleKind};
    use chrono::{Duration, TimeZone, Utc, Weekday};

    #[test]
    fn every_accepts_only_positive_minutes_and_hours() {
        assert!(EverySpec::new(15, "minutes").is_ok());
        assert!(EverySpec::new(2, "hours").is_ok());
        assert!(matches!(
            EverySpec::new(1, "days"),
            Err(ScheduleError::UnsupportedEveryUnit { .. })
        ));
        assert!(EverySpec::new(0, "minutes").is_err());
    }

    #[test]
    fn every_sequence_uses_anchor_and_does_not_align_to_wall_clock() {
        let anchor = Utc.with_ymd_and_hms(2026, 7, 30, 10, 10, 0).unwrap();
        let schedule = ScheduleSpec::every(1, "hours", anchor).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(anchor),
            Some(anchor + Duration::hours(1))
        );
        assert_eq!(
            schedule.next_occurrence_after(anchor + Duration::minutes(50)),
            Some(anchor + Duration::hours(1))
        );
    }

    #[test]
    fn every_sequence_advances_after_a_persisted_millisecond_deadline() {
        let anchor = chrono::DateTime::parse_from_rfc3339("2026-08-12T07:16:49.249706300Z")
            .unwrap()
            .with_timezone(&Utc);
        let persisted_deadline = chrono::DateTime::parse_from_rfc3339("2026-08-12T08:22:49.249Z")
            .unwrap()
            .with_timezone(&Utc);
        let schedule = ScheduleSpec::every(3, "minutes", anchor).unwrap();

        let next = schedule.next_occurrence_after(persisted_deadline).unwrap();

        assert_eq!(
            next,
            chrono::DateTime::parse_from_rfc3339("2026-08-12T08:25:49.249Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        assert!(next.timestamp_millis() > persisted_deadline.timestamp_millis());
    }

    #[test]
    fn every_reenable_and_interval_edit_reset_anchor() {
        let original = Utc.with_ymd_and_hms(2026, 7, 30, 10, 10, 0).unwrap();
        let resumed = Utc.with_ymd_and_hms(2026, 7, 30, 14, 0, 0).unwrap();
        let schedule = ScheduleSpec::every(1, "hours", original)
            .unwrap()
            .reset_anchor(resumed)
            .with_every(30, "minutes", resumed)
            .unwrap();

        assert_eq!(schedule.anchor_at(), resumed);
        assert_eq!(
            schedule.next_occurrence_after(resumed),
            Some(resumed + Duration::minutes(30))
        );
    }

    #[test]
    fn every_edit_only_resets_anchor_when_interval_changes() {
        let original = Utc.with_ymd_and_hms(2026, 7, 30, 10, 10, 0).unwrap();
        let edited_at = Utc.with_ymd_and_hms(2026, 7, 30, 14, 0, 0).unwrap();
        let unchanged = ScheduleSpec::every(1, "hours", original)
            .unwrap()
            .with_every(1, "hours", edited_at)
            .unwrap();
        assert_eq!(unchanged.anchor_at(), original);

        let changed = unchanged.with_every(30, "minutes", edited_at).unwrap();
        assert_eq!(changed.anchor_at(), edited_at);
    }

    #[test]
    fn hourly_schedule_returns_next_wall_clock_hour() {
        let schedule =
            ScheduleSpec::repeat(RepeatPreset::Hourly, 7, 15, "America/New_York").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 8, 3, 14, 37, 0).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(after),
            Some(Utc.with_ymd_and_hms(2026, 8, 3, 15, 0, 0).unwrap())
        );
    }

    #[test]
    fn system_timezone_is_used_when_schedule_timezone_is_omitted() {
        let schedule: ScheduleSpec = serde_json::from_value(serde_json::json!({
            "kind": "At",
            "at": "2026-08-03T12:00:00Z"
        }))
        .unwrap();
        let timezone = system_timezone();

        assert_eq!(schedule.timezone(), Some(timezone.as_str()));
        assert!(timezone.parse::<chrono_tz::Tz>().is_ok());
    }

    #[test]
    fn dst_gap_returns_next_valid_occurrence() {
        let schedule =
            ScheduleSpec::repeat(RepeatPreset::Daily, 2, 30, "America/New_York").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 3, 8, 5, 0, 0).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(after),
            Some(Utc.with_ymd_and_hms(2026, 3, 9, 6, 30, 0).unwrap())
        );
    }

    #[test]
    fn dst_overlap_uses_the_first_valid_occurrence() {
        let schedule =
            ScheduleSpec::repeat(RepeatPreset::Daily, 1, 30, "America/New_York").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 11, 1, 4, 0, 0).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(after),
            Some(Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap())
        );
    }

    #[test]
    fn at_schedule_runs_once_and_does_not_backfill_after_execution() {
        let at = Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap();
        let schedule = ScheduleSpec::at(at);

        assert_eq!(
            schedule.next_occurrence_after(at - Duration::minutes(1)),
            Some(at)
        );
        assert_eq!(schedule.next_occurrence_after(at), None);
    }

    #[test]
    fn at_schedule_keeps_the_declared_timezone_for_display_and_round_trip() {
        let at = Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap();
        let schedule = ScheduleSpec::at_in_timezone(at, "America/New_York").unwrap();

        assert_eq!(schedule.timezone(), Some("America/New_York"));
        let encoded = serde_json::to_value(&schedule).unwrap();
        let decoded: ScheduleSpec = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.timezone(), Some("America/New_York"));
    }

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
        assert_eq!(
            earlier_at,
            Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap()
        );
        assert_eq!(
            later_at,
            Utc.with_ymd_and_hms(2026, 11, 1, 6, 30, 0).unwrap()
        );
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

    #[test]
    fn weekly_repeat_supports_multiple_weekdays_in_a_timezone() {
        let schedule = ScheduleSpec::repeat(
            RepeatPreset::Weekly {
                weekdays: vec![Weekday::Mon, Weekday::Wed, Weekday::Fri],
            },
            9,
            0,
            "Asia/Shanghai",
        )
        .unwrap();
        let after = Utc.with_ymd_and_hms(2026, 7, 30, 0, 0, 0).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(after),
            Some(Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap())
        );
    }

    #[test]
    fn cron_schedule_uses_the_declared_timezone() {
        let schedule = ScheduleSpec::cron("0 0 9 * * MON,WED,FRI", "Asia/Shanghai").unwrap();
        let after = Utc.with_ymd_and_hms(2026, 7, 30, 0, 0, 0).unwrap();

        assert_eq!(
            schedule.next_occurrence_after(after),
            Some(Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap())
        );
    }

    #[test]
    fn deserializes_frontend_every_schedule_with_camel_case_anchor() {
        let value = serde_json::json!({
            "kind": "Every",
            "every": { "value": 6, "unit": "hours" },
            "anchorAt": "2026-07-30T10:10:00.000Z"
        });

        let parsed: ScheduleSpec = serde_json::from_value(value).unwrap();

        assert_eq!(
            parsed.anchor_at(),
            Utc.with_ymd_and_hms(2026, 7, 30, 10, 10, 0).unwrap()
        );
    }

    #[test]
    fn due_occurrence_is_returned_once_and_advances_after_trigger() {
        let at = Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-1",
            "direct",
            ScheduleSpec::at(at),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.created_at = at - Duration::minutes(1);
        assert_eq!(definition.next_due(at), Some(at));
        definition.last_trigger_at = Some(at);
        assert_eq!(definition.next_due(at + Duration::hours(1)), None);
    }

    #[test]
    fn new_scheduled_definition_has_no_materialized_task() {
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();

        assert!(definition.task_id.is_none());
    }

    #[test]
    fn content_snapshot_recomputes_and_legacy_json_defaults_it() {
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-1",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        definition.content_snapshot.direct_agent_id = Some("claude-acp".to_string());
        definition.content_snapshot.instruction = "inspect".to_string();
        definition.recompute_content_fingerprint().unwrap();
        assert!(definition.content_fingerprint.starts_with("sha256:"));

        let mut persisted = serde_json::to_value(&definition).unwrap();
        persisted
            .as_object_mut()
            .expect("definition serializes as object")
            .remove("contentSnapshot");
        let restored: ScheduledTaskDefinition = serde_json::from_value(persisted).unwrap();
        assert_eq!(
            restored.content_snapshot,
            ScheduledTaskContentSnapshot::default()
        );
    }
}
