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
}

impl fmt::Display for ScheduledErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::PermissionRequired => "SCHEDULED_PERMISSION_REQUIRED",
            Self::UserInputRequired => "SCHEDULED_USER_INPUT_REQUIRED",
            Self::PreviousRunRequiresAttention => "SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION",
            Self::QueueBusy => "SCHEDULED_QUEUE_BUSY",
            Self::AgentUnattendedModeUnsupported => "SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED",
            Self::ExecutionFailed => "SCHEDULED_EXECUTION_FAILED",
            Self::LeaseLost => "SCHEDULED_LEASE_LOST",
        };
        formatter.write_str(value)
    }
}

impl std::str::FromStr for ScheduledErrorCode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "SCHEDULED_PERMISSION_REQUIRED" => Ok(Self::PermissionRequired),
            "SCHEDULED_USER_INPUT_REQUIRED" => Ok(Self::UserInputRequired),
            "SCHEDULED_PREVIOUS_RUN_REQUIRES_ATTENTION" => Ok(Self::PreviousRunRequiresAttention),
            "SCHEDULED_QUEUE_BUSY" => Ok(Self::QueueBusy),
            "SCHEDULED_AGENT_UNATTENDED_MODE_UNSUPPORTED" => {
                Ok(Self::AgentUnattendedModeUnsupported)
            }
            "SCHEDULED_EXECUTION_FAILED" => Ok(Self::ExecutionFailed),
            "SCHEDULED_LEASE_LOST" => Ok(Self::LeaseLost),
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
        now + Duration::seconds(self.lease_seconds.max(1))
    }

    pub fn heartbeat_interval(self) -> Duration {
        Duration::seconds(self.heartbeat_seconds.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ClaimResult, LeaseConfig, OccurrenceStatus, OccurrenceTriggerKind, ScheduledErrorCode,
    };
    use chrono::{TimeZone, Utc};

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
    fn claim_result_and_lease_config_have_stable_serialization() {
        let now = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        let config = LeaseConfig::default();
        assert!(config.lease_until(now) > now);

        let encoded = serde_json::to_string(&ClaimResult::Busy).unwrap();
        assert_eq!(encoded, "\"busy\"");
    }
}
