use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Pending,
    Running,
    Retrying,
    Succeeded,
    Failed,
    Skipped,
    Missed,
    AttentionRequired,
}

impl OccurrenceStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Skipped | Self::Missed | Self::AttentionRequired
        )
    }
}

impl fmt::Display for OccurrenceStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Missed => "missed",
            Self::AttentionRequired => "attention_required",
        };
        formatter.write_str(value)
    }
}

impl std::str::FromStr for OccurrenceStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "retrying" => Ok(Self::Retrying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            "missed" => Ok(Self::Missed),
            "attention_required" => Ok(Self::AttentionRequired),
            _ => Err(format!("unsupported occurrence status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceTriggerKind {
    Scheduled,
    Manual,
}

impl fmt::Display for OccurrenceTriggerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Scheduled => "scheduled",
            Self::Manual => "manual",
        })
    }
}

impl std::str::FromStr for OccurrenceTriggerKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "scheduled" => Ok(Self::Scheduled),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("unsupported occurrence trigger kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduledErrorCode {
    #[serde(rename = "SCHEDULED_NOT_FOUND")]
    NotFound,
    #[serde(rename = "SCHEDULED_CONFLICT")]
    Conflict,
    #[serde(rename = "SCHEDULED_VALIDATION_FAILED")]
    ValidationFailed,
    #[serde(rename = "SCHEDULED_STORAGE_FAILED")]
    StorageFailed,
    #[serde(rename = "SCHEDULED_ATTACHMENT_FAILED")]
    AttachmentFailed,
    #[serde(rename = "SCHEDULED_PERMISSION_REQUIRED")]
    PermissionRequired,
    #[serde(rename = "SCHEDULED_USER_INPUT_REQUIRED")]
    UserInputRequired,
    #[serde(rename = "SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION")]
    PreviousRunRequiresAttention,
    #[serde(rename = "SCHEDULED_QUEUE_BUSY")]
    QueueBusy,
    #[serde(rename = "SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED")]
    AgentUnattendedModeUnsupported,
    #[serde(rename = "SCHEDULED_EXECUTION_FAILED")]
    ExecutionFailed,
    #[serde(rename = "SCHEDULED_LEASE_LOST")]
    LeaseLost,
    #[serde(rename = "SCHEDULED_MIGRATION_CONFLICT")]
    MigrationConflict,
    #[serde(rename = "SCHEDULED_COORDINATOR_UNAVAILABLE")]
    CoordinatorUnavailable,
    #[serde(rename = "SCHEDULED_POWER_INHIBITOR_FAILED")]
    PowerInhibitorFailed,
    #[serde(rename = "SCHEDULED_NOTIFICATION_FAILED")]
    NotificationFailed,
    #[serde(rename = "SCHEDULED_SKILL_VALIDATION_FAILED")]
    SkillValidationFailed,
}

impl fmt::Display for ScheduledErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::NotFound => "SCHEDULED_NOT_FOUND",
            Self::Conflict => "SCHEDULED_CONFLICT",
            Self::ValidationFailed => "SCHEDULED_VALIDATION_FAILED",
            Self::StorageFailed => "SCHEDULED_STORAGE_FAILED",
            Self::AttachmentFailed => "SCHEDULED_ATTACHMENT_FAILED",
            Self::PermissionRequired => "SCHEDULED_PERMISSION_REQUIRED",
            Self::UserInputRequired => "SCHEDULED_USER_INPUT_REQUIRED",
            Self::PreviousRunRequiresAttention => "SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION",
            Self::QueueBusy => "SCHEDULED_QUEUE_BUSY",
            Self::AgentUnattendedModeUnsupported => "SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED",
            Self::ExecutionFailed => "SCHEDULED_EXECUTION_FAILED",
            Self::LeaseLost => "SCHEDULED_LEASE_LOST",
            Self::MigrationConflict => "SCHEDULED_MIGRATION_CONFLICT",
            Self::CoordinatorUnavailable => "SCHEDULED_COORDINATOR_UNAVAILABLE",
            Self::PowerInhibitorFailed => "SCHEDULED_POWER_INHIBITOR_FAILED",
            Self::NotificationFailed => "SCHEDULED_NOTIFICATION_FAILED",
            Self::SkillValidationFailed => "SCHEDULED_SKILL_VALIDATION_FAILED",
        };
        formatter.write_str(value)
    }
}

impl std::str::FromStr for ScheduledErrorCode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "SCHEDULED_NOT_FOUND" => Ok(Self::NotFound),
            "SCHEDULED_CONFLICT" => Ok(Self::Conflict),
            "SCHEDULED_VALIDATION_FAILED" => Ok(Self::ValidationFailed),
            "SCHEDULED_STORAGE_FAILED" => Ok(Self::StorageFailed),
            "SCHEDULED_ATTACHMENT_FAILED" => Ok(Self::AttachmentFailed),
            "SCHEDULED_PERMISSION_REQUIRED" => Ok(Self::PermissionRequired),
            "SCHEDULED_USER_INPUT_REQUIRED" => Ok(Self::UserInputRequired),
            "SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION" => Ok(Self::PreviousRunRequiresAttention),
            "SCHEDULED_QUEUE_BUSY" => Ok(Self::QueueBusy),
            "SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED" => {
                Ok(Self::AgentUnattendedModeUnsupported)
            }
            "SCHEDULED_EXECUTION_FAILED" => Ok(Self::ExecutionFailed),
            "SCHEDULED_LEASE_LOST" => Ok(Self::LeaseLost),
            "SCHEDULED_MIGRATION_CONFLICT" => Ok(Self::MigrationConflict),
            "SCHEDULED_COORDINATOR_UNAVAILABLE" => Ok(Self::CoordinatorUnavailable),
            "SCHEDULED_POWER_INHIBITOR_FAILED" => Ok(Self::PowerInhibitorFailed),
            "SCHEDULED_NOTIFICATION_FAILED" => Ok(Self::NotificationFailed),
            "SCHEDULED_SKILL_VALIDATION_FAILED" => Ok(Self::SkillValidationFailed),
            _ => Err(format!("unsupported scheduled error code: {value}")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceLinks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
}

pub type ScheduledOccurrenceLinks = OccurrenceLinks;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledError {
    pub code: ScheduledErrorCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl ScheduledError {
    pub fn new(code: ScheduledErrorCode) -> Self {
        Self { code, params: None }
    }

    pub fn with_params(code: ScheduledErrorCode, params: Value) -> Self {
        Self {
            code,
            params: Some(params),
        }
    }
}

pub type OccurrenceError = ScheduledError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledOccurrence {
    pub id: String,
    pub job_id: String,
    pub scheduled_at: DateTime<Utc>,
    pub trigger_kind: OccurrenceTriggerKind,
    pub status: OccurrenceStatus,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ScheduledErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ScheduledOccurrence {
    pub fn links(&self) -> OccurrenceLinks {
        OccurrenceLinks {
            task_id: self.task_id.clone(),
            run_id: self.run_id.clone(),
            round_id: self.round_id.clone(),
            attempt_id: self.attempt_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimResult {
    Claimed(ScheduledOccurrence),
    AlreadyOwned,
    Busy,
    NotFound,
}

impl ClaimResult {
    pub fn is_claimed(&self) -> bool {
        matches!(self, Self::Claimed(_))
    }

    pub fn occurrence(&self) -> Option<&ScheduledOccurrence> {
        match self {
            Self::Claimed(occurrence) => Some(occurrence),
            Self::AlreadyOwned | Self::Busy | Self::NotFound => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LeaseConfig {
    pub lease_seconds: i64,
    pub heartbeat_seconds: i64,
}

const MIN_LEASE_SECONDS: i64 = 2;
const MIN_HEARTBEAT_SECONDS: i64 = 1;

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            lease_seconds: 60,
            heartbeat_seconds: 20,
        }
    }
}

impl LeaseConfig {
    pub fn lease_until(self, now: DateTime<Utc>) -> DateTime<Utc> {
        now + Duration::seconds(self.effective_lease_seconds())
    }

    pub fn heartbeat_interval(self) -> Duration {
        Duration::seconds(self.effective_heartbeat_seconds())
    }

    fn effective_lease_seconds(self) -> i64 {
        self.lease_seconds.max(MIN_LEASE_SECONDS)
    }

    fn effective_heartbeat_seconds(self) -> i64 {
        self.heartbeat_seconds
            .max(MIN_HEARTBEAT_SECONDS)
            .min(self.effective_lease_seconds() - MIN_HEARTBEAT_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimResult, LeaseConfig, OccurrenceStatus, OccurrenceTriggerKind, ScheduledErrorCode,
    };
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn occurrence_status_round_trips_stable_values() {
        let statuses = [
            OccurrenceStatus::Pending,
            OccurrenceStatus::Running,
            OccurrenceStatus::Retrying,
            OccurrenceStatus::Succeeded,
            OccurrenceStatus::Failed,
            OccurrenceStatus::Skipped,
            OccurrenceStatus::Missed,
            OccurrenceStatus::AttentionRequired,
        ];

        for status in statuses {
            let encoded = serde_json::to_string(&status).unwrap();
            let decoded: OccurrenceStatus = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, status);
        }

        assert_eq!(
            serde_json::to_string(&OccurrenceStatus::AttentionRequired).unwrap(),
            "\"attention_required\""
        );
        assert_eq!(
            serde_json::to_string(&OccurrenceTriggerKind::Scheduled).unwrap(),
            "\"scheduled\""
        );
        assert_eq!(
            serde_json::to_string(&ScheduledErrorCode::PermissionRequired).unwrap(),
            "\"SCHEDULED_PERMISSION_REQUIRED\""
        );
    }

    #[test]
    fn scheduled_error_codes_round_trip_stable_wire_values() {
        let cases = [
            (
                ScheduledErrorCode::MigrationConflict,
                "SCHEDULED_MIGRATION_CONFLICT",
            ),
            (
                ScheduledErrorCode::CoordinatorUnavailable,
                "SCHEDULED_COORDINATOR_UNAVAILABLE",
            ),
            (
                ScheduledErrorCode::PowerInhibitorFailed,
                "SCHEDULED_POWER_INHIBITOR_FAILED",
            ),
            (
                ScheduledErrorCode::NotificationFailed,
                "SCHEDULED_NOTIFICATION_FAILED",
            ),
            (
                ScheduledErrorCode::SkillValidationFailed,
                "SCHEDULED_SKILL_VALIDATION_FAILED",
            ),
        ];

        for (code, wire_code) in cases {
            let encoded = serde_json::to_string(&code).unwrap();
            assert_eq!(encoded, format!("\"{wire_code}\""));

            let decoded: ScheduledErrorCode = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, code);
            assert_eq!(code.to_string(), wire_code);
            assert_eq!(wire_code.parse::<ScheduledErrorCode>().unwrap(), code);
        }
    }

    #[test]
    fn claim_result_and_lease_config_have_stable_serialization() {
        let now = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let config = LeaseConfig::default();
        assert!(config.lease_until(now) > now);

        let encoded = serde_json::to_string(&ClaimResult::Busy).unwrap();
        assert_eq!(encoded, "\"busy\"");
    }

    #[test]
    fn lease_config_normalizes_heartbeat_before_effective_lease_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let cases = [
            (
                LeaseConfig {
                    lease_seconds: 0,
                    heartbeat_seconds: 0,
                },
                2,
                1,
            ),
            (
                LeaseConfig {
                    lease_seconds: -10,
                    heartbeat_seconds: -5,
                },
                2,
                1,
            ),
            (
                LeaseConfig {
                    lease_seconds: 5,
                    heartbeat_seconds: 5,
                },
                5,
                4,
            ),
            (
                LeaseConfig {
                    lease_seconds: 5,
                    heartbeat_seconds: 30,
                },
                5,
                4,
            ),
        ];

        for (config, effective_lease_seconds, effective_heartbeat_seconds) in cases {
            let lease_until = config.lease_until(now);
            let heartbeat = config.heartbeat_interval();
            assert_eq!(
                lease_until,
                now + Duration::seconds(effective_lease_seconds)
            );
            assert_eq!(heartbeat, Duration::seconds(effective_heartbeat_seconds));
            assert!(now + heartbeat < lease_until);
        }
    }
}
