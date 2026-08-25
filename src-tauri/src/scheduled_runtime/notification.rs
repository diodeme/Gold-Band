use gold_band::scheduler::occurrence::{OccurrenceLinks, OccurrenceStatus, ScheduledOccurrence};
use serde::Serialize;
use uuid::Uuid;

pub const SCHEDULED_NOTIFICATION_EVENT: &str = "gold-band://scheduled-notification";

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

pub fn notification_event_for_occurrence(
    project_id: &str,
    completion_notifications_enabled: bool,
    occurrence: &ScheduledOccurrence,
) -> Option<ScheduledNotificationEventVm> {
    let kind = match occurrence.status {
        OccurrenceStatus::Succeeded if completion_notifications_enabled => "completion",
        OccurrenceStatus::Failed => "failed",
        OccurrenceStatus::AttentionRequired => "attentionRequired",
        OccurrenceStatus::Pending
        | OccurrenceStatus::Running
        | OccurrenceStatus::Retrying
        | OccurrenceStatus::Succeeded
        | OccurrenceStatus::Skipped
        | OccurrenceStatus::Missed => return None,
    };
    Some(ScheduledNotificationEventVm {
        event_id: format!("scheduled:{kind}:{}", occurrence.id),
        kind: kind.to_string(),
        project_id: project_id.to_string(),
        scheduled_task_id: occurrence.job_id.clone(),
        occurrence_id: Some(occurrence.id.clone()),
        error_code: occurrence.error_code.map(|code| code.to_string()),
        error_params: occurrence.error_params.clone(),
        links: occurrence.links(),
        missed_count: None,
    })
}

pub fn missed_notification_event(
    project_id: &str,
    scheduled_task_id: &str,
    missed_count: u32,
) -> ScheduledNotificationEventVm {
    ScheduledNotificationEventVm {
        event_id: format!("scheduled:missed:{}", Uuid::new_v4().simple()),
        kind: "missed".to_string(),
        project_id: project_id.to_string(),
        scheduled_task_id: scheduled_task_id.to_string(),
        occurrence_id: None,
        error_code: None,
        error_params: None,
        links: OccurrenceLinks::default(),
        missed_count: Some(missed_count),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use gold_band::scheduler::occurrence::{
        OccurrenceStatus, OccurrenceTriggerKind, ScheduledErrorCode, ScheduledOccurrence,
    };

    use super::{missed_notification_event, notification_event_for_occurrence};

    fn occurrence(status: OccurrenceStatus) -> ScheduledOccurrence {
        let now = Utc::now();
        ScheduledOccurrence {
            id: "occurrence-a".to_string(),
            job_id: "scheduled-a".to_string(),
            scheduled_at: now,
            trigger_kind: OccurrenceTriggerKind::Scheduled,
            status,
            attempt: 1,
            owner_id: None,
            lease_until: None,
            heartbeat_at: None,
            task_id: Some("task-a".to_string()),
            run_id: Some("run-a".to_string()),
            round_id: Some("round-a".to_string()),
            attempt_id: Some("attempt-a".to_string()),
            error_code: Some(ScheduledErrorCode::ExecutionFailed),
            error_params: Some(serde_json::json!({ "reason": "test" })),
            started_at: Some(now),
            finished_at: Some(now),
            accepted_execution: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn succeeded_requires_completion_notification_setting() {
        let succeeded = occurrence(OccurrenceStatus::Succeeded);

        assert!(notification_event_for_occurrence("project-a", false, &succeeded).is_none());
        assert_eq!(
            notification_event_for_occurrence("project-a", true, &succeeded)
                .unwrap()
                .kind,
            "completion"
        );
    }

    #[test]
    fn failed_and_attention_map_to_immediate_events() {
        let failed = notification_event_for_occurrence(
            "project-a",
            false,
            &occurrence(OccurrenceStatus::Failed),
        )
        .unwrap();
        let attention = notification_event_for_occurrence(
            "project-a",
            false,
            &occurrence(OccurrenceStatus::AttentionRequired),
        )
        .unwrap();

        assert_eq!(failed.kind, "failed");
        assert_eq!(
            failed.error_code.as_deref(),
            Some("SCHEDULED_EXECUTION_FAILED")
        );
        assert_eq!(attention.kind, "attentionRequired");
    }

    #[test]
    fn skipped_retrying_and_missed_are_not_individual_notifications() {
        for status in [
            OccurrenceStatus::Skipped,
            OccurrenceStatus::Retrying,
            OccurrenceStatus::Missed,
        ] {
            assert!(
                notification_event_for_occurrence("project-a", true, &occurrence(status)).is_none()
            );
        }
    }

    #[test]
    fn missed_points_are_aggregated_once_per_reconcile_batch() {
        let event = missed_notification_event("project-a", "scheduled-a", 3);

        assert_eq!(event.kind, "missed");
        assert_eq!(event.missed_count, Some(3));
        assert_eq!(event.occurrence_id, None);
    }
}
