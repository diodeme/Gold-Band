use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::app::intervention::{
    ElicitationQuestionKind, InterventionCommandService, InterventionError, InterventionLocator,
    permission_action_is_safe_remote_reject, permission_actions_require_desktop,
};
use crate::app::{AcpTurnOutcome, App, RuntimeInterventionKind, RuntimeLifecycleEvent};
use crate::domain::RunOutcome;

use super::{
    IM_LIFECYCLE_QUEUE_CAPACITY, IM_PAYLOAD_VERSION, ImChannelCapabilities, ImChannelKind,
    ImDelivery, ImDeliveryInsertResult, ImDeliveryPayload, ImDestination, ImIntegrationSettings,
    ImNavigationLocator, ImNotificationKind, ImNotificationPreferences, ImRepository,
    ImRepositoryError, InformationalNotification, InterventionPresentation, InterventionQuestion,
    InterventionQuestionKind, InterventionQuestionOption, InterventionRef,
};

const WECOM_MAX_PERMISSION_OPTIONS: usize = 20;
const MAX_IM_PRESENTATION_FIELD_CHARS: usize = 128;

#[derive(Debug, Clone)]
pub struct ImProjectionTarget {
    pub channel: ImChannelKind,
    pub enabled: bool,
    pub credential_available: bool,
    pub capabilities: ImChannelCapabilities,
    pub destination: Option<ImDestination>,
    pub notifications: ImNotificationPreferences,
}

impl ImProjectionTarget {
    fn eligible(&self, kind: ImNotificationKind) -> bool {
        self.enabled
            && self.credential_available
            && self.capabilities.proactive_delivery
            && self.capabilities.private_chat
            && (!kind.is_intervention() || self.capabilities.card_actions)
            && self.destination.is_some()
            && self.notifications.enabled(kind)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImProjectionResult {
    pub inserted: usize,
    pub duplicates: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImProjectionCompletion {
    pub canonical_event_id: String,
    pub result: Result<ImProjectionResult, &'static str>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImProjectionError {
    #[error("IM projection repository operation failed")]
    Repository(#[from] ImRepositoryError),
    #[error("IM projection intervention inspection failed")]
    Intervention(#[from] InterventionError),
}

impl ImProjectionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Repository(error) => error.code(),
            Self::Intervention(error) => error.code.as_str(),
        }
    }
}

pub struct ImLifecycleProjectionJob {
    pub app: App,
    pub event: RuntimeLifecycleEvent,
    pub targets: Vec<ImProjectionTarget>,
    pub now_ms: i64,
}

pub enum ImLifecycleProjectionQueueItem {
    Event(ImLifecycleProjectionJob),
    Barrier(oneshot::Sender<()>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImProjectionEnqueueOutcome {
    Enqueued,
    Saturated,
    Closed,
}

pub fn try_enqueue_projection(
    sender: &mpsc::Sender<ImLifecycleProjectionQueueItem>,
    job: ImLifecycleProjectionJob,
) -> ImProjectionEnqueueOutcome {
    match sender.try_send(ImLifecycleProjectionQueueItem::Event(job)) {
        Ok(()) => ImProjectionEnqueueOutcome::Enqueued,
        Err(mpsc::error::TrySendError::Full(_)) => ImProjectionEnqueueOutcome::Saturated,
        Err(mpsc::error::TrySendError::Closed(_)) => ImProjectionEnqueueOutcome::Closed,
    }
}

pub async fn drain_projection_queue(
    sender: &mpsc::Sender<ImLifecycleProjectionQueueItem>,
) -> Result<(), &'static str> {
    let (barrier_sender, barrier_receiver) = oneshot::channel();
    sender
        .send(ImLifecycleProjectionQueueItem::Barrier(barrier_sender))
        .await
        .map_err(|_| "IM_PROJECTION_QUEUE_CLOSED")?;
    barrier_receiver
        .await
        .map_err(|_| "IM_PROJECTION_QUEUE_CLOSED")
}

pub fn im_lifecycle_projection_channel() -> (
    mpsc::Sender<ImLifecycleProjectionQueueItem>,
    mpsc::Receiver<ImLifecycleProjectionQueueItem>,
) {
    mpsc::channel(IM_LIFECYCLE_QUEUE_CAPACITY)
}

pub struct ImLifecycleProjector {
    repository: Arc<ImRepository>,
}

impl ImLifecycleProjector {
    pub fn new(repository: Arc<ImRepository>) -> Self {
        Self { repository }
    }

    pub fn project_event(
        &self,
        app: &App,
        event: &RuntimeLifecycleEvent,
        targets: &[ImProjectionTarget],
        now_ms: i64,
    ) -> Result<ImProjectionResult, ImProjectionError> {
        let Some(draft) = self.draft_for_event(app, event)? else {
            return Ok(ImProjectionResult {
                skipped: targets.len(),
                ..ImProjectionResult::default()
            });
        };
        Ok(self.enqueue_draft(draft, targets, now_ms)?)
    }

    pub async fn run<F>(
        self,
        mut receiver: mpsc::Receiver<ImLifecycleProjectionQueueItem>,
        on_complete: F,
    ) where
        F: Fn(ImProjectionCompletion) + Send + Sync + 'static,
    {
        while let Some(item) = receiver.recv().await {
            let job = match item {
                ImLifecycleProjectionQueueItem::Event(job) => job,
                ImLifecycleProjectionQueueItem::Barrier(completion) => {
                    let _ = completion.send(());
                    continue;
                }
            };
            let repository = Arc::clone(&self.repository);
            let ImLifecycleProjectionJob {
                app,
                event,
                targets,
                now_ms,
            } = job;
            let canonical_event_id = event_canonical_id(&event).to_string();
            let result = tokio::task::spawn_blocking(move || {
                ImLifecycleProjector::new(repository).project_event(&app, &event, &targets, now_ms)
            })
            .await
            .map_err(|_| "IM_PROJECTION_TASK_FAILED")
            .and_then(|result| result.map_err(|error| error.code()));
            on_complete(ImProjectionCompletion {
                canonical_event_id,
                result,
            });
        }
    }

    fn draft_for_event(
        &self,
        app: &App,
        event: &RuntimeLifecycleEvent,
    ) -> Result<Option<DeliveryDraft>, ImProjectionError> {
        Ok(match event {
            RuntimeLifecycleEvent::InterventionRequested {
                event_id,
                project_id,
                task_id,
                run_id,
                round_id,
                node_id,
                attempt_id,
                outer_node_id,
                outer_attempt_id,
                request,
                node_label,
                kind,
                task_title,
                ..
            } => {
                let Some(notification_kind) = intervention_notification_kind(*kind) else {
                    return Ok(None);
                };
                let locator = InterventionLocator {
                    project_id: project_id.clone(),
                    task_id: task_id.clone(),
                    run_id: run_id.clone(),
                    round_id: round_id.clone(),
                    node_id: node_id.clone(),
                    attempt_id: attempt_id.clone(),
                    outer_node_id: outer_node_id.clone(),
                    outer_attempt_id: outer_attempt_id.clone(),
                };
                let snapshot =
                    InterventionCommandService::new(app).inspect(locator, request.clone())?;
                if notification_kind == ImNotificationKind::Elicitation
                    && !snapshot.allowed_actions.iter().any(|action| {
                        matches!(
                            action,
                            crate::app::intervention::InterventionAllowedAction::ElicitationFixedForm { .. }
                        )
                    })
                {
                    return Ok(None);
                }
                let mut presentation_fields = BTreeMap::from([
                    ("nodeLabel".into(), bounded_presentation_field(node_label)),
                    (
                        "taskTitle".into(),
                        bounded_presentation_field(&task_title.clone().unwrap_or_default()),
                    ),
                ]);
                if let Some(prompt) = snapshot.prompt.as_ref() {
                    presentation_fields.extend(prompt.fields.clone());
                }
                let presentation_body = snapshot
                    .prompt
                    .as_ref()
                    .map(|prompt| prompt.message.clone());
                let presentation_context = snapshot
                    .prompt
                    .as_ref()
                    .and_then(|prompt| prompt.context.clone());
                let permission_presentation_title =
                    if notification_kind == ImNotificationKind::Permission {
                        snapshot
                            .prompt
                            .as_ref()
                            .and_then(|prompt| prompt.title.clone())
                    } else {
                        None
                    };
                let permission_presentation_summary =
                    if notification_kind == ImNotificationKind::Permission {
                        presentation_body
                            .clone()
                            .filter(|body| !body.trim().is_empty())
                    } else {
                        None
                    };
                let presentation_questions = snapshot
                    .prompt
                    .as_ref()
                    .map(|prompt| {
                        prompt
                            .questions
                            .iter()
                            .map(|question| InterventionQuestion {
                                field_name: bounded_presentation_field(&question.field_name),
                                title: question.title.clone(),
                                description: question.description.clone(),
                                question_kind: match question.question_kind {
                                    ElicitationQuestionKind::SingleSelect => {
                                        InterventionQuestionKind::SingleSelect
                                    }
                                    ElicitationQuestionKind::MultiSelect => {
                                        InterventionQuestionKind::MultiSelect
                                    }
                                    ElicitationQuestionKind::FreeText => {
                                        InterventionQuestionKind::FreeText
                                    }
                                },
                                options: question
                                    .options
                                    .iter()
                                    .map(|option| InterventionQuestionOption {
                                        value: option.value.clone(),
                                        label: option.label.clone(),
                                        description: option.description.clone(),
                                    })
                                    .collect(),
                                allows_custom_answer: question.allows_custom_answer,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let is_permission = notification_kind == ImNotificationKind::Permission;
                let mut remote_allowed_actions = snapshot.allowed_actions.clone();
                let prompt_requires_desktop = snapshot
                    .prompt
                    .as_ref()
                    .is_some_and(|prompt| prompt.requires_desktop);
                if !is_permission && prompt_requires_desktop {
                    remote_allowed_actions.retain(|action| {
                        matches!(
                            action,
                            crate::app::intervention::InterventionAllowedAction::PermissionOption { .. }
                        ) && permission_action_is_safe_remote_reject(action)
                    });
                } else if notification_kind == ImNotificationKind::Elicitation {
                    remote_allowed_actions.retain(|action| {
                        matches!(
                            action,
                            crate::app::intervention::InterventionAllowedAction::ElicitationFixedForm { .. }
                        )
                    });
                }
                let expires_at_ms = snapshot.expires_at_ms.unwrap_or_else(|| {
                    chrono::Utc::now().timestamp_millis() + 24 * 60 * 60 * 1_000
                });
                Some(DeliveryDraft {
                    notification_kind,
                    canonical_event_id: event_id.clone(),
                    payload: ImDeliveryPayload::Intervention {
                        version: IM_PAYLOAD_VERSION,
                        reference: InterventionRef {
                            locator: snapshot.locator,
                            request: snapshot.request,
                            expected_state: snapshot.expected_state,
                            allowed_actions: remote_allowed_actions,
                            expires_at_ms: snapshot.expires_at_ms,
                        },
                        presentation: InterventionPresentation {
                            title_key: intervention_title_key(notification_kind).into(),
                            summary_key: intervention_summary_key(notification_kind).into(),
                            title: permission_presentation_title,
                            summary: permission_presentation_summary,
                            body: presentation_body,
                            context: presentation_context,
                            fields: presentation_fields,
                            questions: presentation_questions,
                        },
                    },
                    expires_at_ms,
                })
            }
            RuntimeLifecycleEvent::RunCompleted {
                event_id,
                scheduled_occurrence_id: None,
                project_id,
                task_id,
                run_id,
                round_id,
                node_id,
                attempt_id,
                outcome,
                task_title,
                ..
            } => {
                let notification_kind = match outcome {
                    RunOutcome::Success => ImNotificationKind::RunSuccess,
                    _ => ImNotificationKind::RunFailure,
                };
                Some(information_draft(
                    event_id,
                    notification_kind,
                    project_id,
                    task_id,
                    run_id,
                    round_id,
                    node_id,
                    attempt_id,
                    None,
                    format!("{outcome:?}").to_lowercase(),
                    task_title.as_deref(),
                ))
            }
            RuntimeLifecycleEvent::AcpTurnFinished {
                event_id,
                scheduled_occurrence_id: None,
                project_id,
                task_id,
                run_id,
                round_id,
                node_id,
                attempt_id,
                outcome,
                task_title,
                ..
            } => Some(information_draft(
                event_id,
                ImNotificationKind::AcpTurnFinished,
                project_id,
                task_id,
                run_id,
                round_id,
                node_id,
                attempt_id,
                None,
                acp_outcome(*outcome).into(),
                task_title.as_deref(),
            )),
            _ => None,
        })
    }

    fn enqueue_draft(
        &self,
        draft: DeliveryDraft,
        targets: &[ImProjectionTarget],
        now_ms: i64,
    ) -> Result<ImProjectionResult, ImRepositoryError> {
        let mut result = ImProjectionResult::default();
        for target in targets {
            if !target.eligible(draft.notification_kind) {
                result.skipped += 1;
                continue;
            }
            if draft.notification_kind == ImNotificationKind::Elicitation
                && target.channel != ImChannelKind::WeCom
            {
                result.skipped += 1;
                continue;
            }
            let destination = target
                .destination
                .clone()
                .expect("eligible target has destination");
            let payload = if draft.notification_kind == ImNotificationKind::Permission {
                project_permission_payload(&draft.payload)
            } else {
                draft.payload.clone()
            };
            let delivery = ImDelivery {
                delivery_id: ImDelivery::deterministic_id(
                    target.channel,
                    &destination.destination_id,
                    draft.notification_kind,
                    &draft.canonical_event_id,
                ),
                channel: target.channel,
                destination,
                notification_kind: draft.notification_kind,
                canonical_event_id: draft.canonical_event_id.clone(),
                payload,
                expires_at_ms: draft.expires_at_ms,
                display_ref: None,
            };
            match self.repository.enqueue(&delivery, now_ms)? {
                ImDeliveryInsertResult::Inserted => result.inserted += 1,
                ImDeliveryInsertResult::Duplicate => result.duplicates += 1,
            }
        }
        Ok(result)
    }
}

fn project_permission_payload(payload: &ImDeliveryPayload) -> ImDeliveryPayload {
    let ImDeliveryPayload::Intervention {
        version,
        reference,
        presentation,
    } = payload
    else {
        return payload.clone();
    };
    let mut reference = reference.clone();
    reference.allowed_actions = remote_permission_actions(&reference.allowed_actions);
    ImDeliveryPayload::Intervention {
        version: *version,
        reference,
        presentation: presentation.clone(),
    }
}

fn bounded_presentation_field(value: &str) -> String {
    let mut chars = value.chars();
    let prefix = chars
        .by_ref()
        .take(MAX_IM_PRESENTATION_FIELD_CHARS)
        .collect::<String>();
    if chars.next().is_none() {
        prefix
    } else {
        format!(
            "{}...",
            value
                .chars()
                .take(MAX_IM_PRESENTATION_FIELD_CHARS - 3)
                .collect::<String>()
        )
    }
}

fn remote_permission_actions(
    actions: &[crate::app::intervention::InterventionAllowedAction],
) -> Vec<crate::app::intervention::InterventionAllowedAction> {
    let capacity = WECOM_MAX_PERMISSION_OPTIONS;
    if actions.len() <= capacity && !permission_actions_require_desktop(actions) {
        return actions.to_vec();
    }
    actions
        .iter()
        .filter(|action| permission_action_is_safe_remote_reject(action))
        .take(capacity)
        .cloned()
        .collect()
}

#[derive(Clone)]
struct DeliveryDraft {
    notification_kind: ImNotificationKind,
    canonical_event_id: String,
    payload: ImDeliveryPayload,
    expires_at_ms: i64,
}

fn event_canonical_id(event: &RuntimeLifecycleEvent) -> &str {
    match event {
        RuntimeLifecycleEvent::RunPaused { event_id, .. }
        | RuntimeLifecycleEvent::InterventionRequested { event_id, .. }
        | RuntimeLifecycleEvent::RunCompleted { event_id, .. }
        | RuntimeLifecycleEvent::AcpTurnFinished { event_id, .. } => event_id,
        _ => "unsupported-lifecycle-event",
    }
}

fn intervention_notification_kind(kind: RuntimeInterventionKind) -> Option<ImNotificationKind> {
    match kind {
        RuntimeInterventionKind::PermissionRequested => Some(ImNotificationKind::Permission),
        RuntimeInterventionKind::ElicitationRequested => Some(ImNotificationKind::Elicitation),
        RuntimeInterventionKind::ManualDecisionRequired => Some(ImNotificationKind::ManualCheck),
        RuntimeInterventionKind::RuntimeAbnormal
        | RuntimeInterventionKind::ErrorBlocked
        | RuntimeInterventionKind::ProcessInterrupted => None,
    }
}

fn intervention_title_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.notification.permission.title",
        ImNotificationKind::Elicitation => "im.notification.elicitation.title",
        ImNotificationKind::ManualCheck => "im.notification.manualCheck.title",
        _ => unreachable!("only intervention kinds have action titles"),
    }
}

fn intervention_summary_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.notification.permission.summary",
        ImNotificationKind::Elicitation => "im.notification.elicitation.summary",
        ImNotificationKind::ManualCheck => "im.notification.manualCheck.summary",
        _ => unreachable!("only intervention kinds have action summaries"),
    }
}

#[allow(clippy::too_many_arguments)]
fn information_draft(
    event_id: &str,
    notification_kind: ImNotificationKind,
    project_id: &str,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    scheduled_occurrence_id: Option<&str>,
    outcome: String,
    task_title: Option<&str>,
) -> DeliveryDraft {
    DeliveryDraft {
        notification_kind,
        canonical_event_id: event_id.into(),
        payload: ImDeliveryPayload::Information {
            version: IM_PAYLOAD_VERSION,
            notification: InformationalNotification {
                canonical_event_id: event_id.into(),
                notification_kind,
                locator: ImNavigationLocator {
                    project_id: project_id.into(),
                    task_id: Some(task_id.into()),
                    run_id: Some(run_id.into()),
                    round_id: Some(round_id.into()),
                    node_id: Some(node_id.into()),
                    attempt_id: Some(attempt_id.into()),
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: scheduled_occurrence_id.map(str::to_string),
                },
                outcome,
                summary_key: format!("im.notification.{}.summary", notification_kind.as_str()),
                parameters: BTreeMap::from([(
                    "taskTitle".into(),
                    task_title.unwrap_or_default().into(),
                )]),
                missed_count: None,
            },
        },
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 7 * 24 * 60 * 60 * 1_000,
    }
}

fn acp_outcome(outcome: AcpTurnOutcome) -> &'static str {
    match outcome {
        AcpTurnOutcome::Completed => "completed",
        AcpTurnOutcome::Failed => "failed",
        AcpTurnOutcome::Cancelled => "cancelled",
    }
}

pub fn targets_from_settings(
    settings: &ImIntegrationSettings,
    capabilities: impl Fn(ImChannelKind) -> ImChannelCapabilities,
) -> Vec<ImProjectionTarget> {
    settings
        .channels
        .iter()
        .map(|channel| ImProjectionTarget {
            channel: channel.kind,
            enabled: channel.enabled,
            credential_available: channel.credential_ref.is_some(),
            capabilities: capabilities(channel.kind),
            destination: channel.binding.as_ref().map(|binding| ImDestination {
                destination_id: binding.destination_id.clone(),
                conversation_id: binding.conversation_id.clone(),
                authorized_actor_id: binding.authorized_actor_id.clone(),
            }),
            notifications: channel.notifications.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    fn target(channel: ImChannelKind) -> ImProjectionTarget {
        ImProjectionTarget {
            channel,
            enabled: true,
            credential_available: true,
            capabilities: ImChannelCapabilities {
                proactive_delivery: true,
                card_actions: true,
                message_update: true,
                private_chat: true,
            },
            destination: Some(ImDestination {
                destination_id: format!("{channel:?}-user"),
                conversation_id: format!("{channel:?}-chat"),
                authorized_actor_id: format!("{channel:?}-actor"),
            }),
            notifications: ImNotificationPreferences {
                run_success: true,
                ..ImNotificationPreferences::default()
            },
        }
    }

    fn run_completed(scheduled: bool, event_id: &str) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::RunCompleted {
            event_id: event_id.into(),
            occurred_at: "2026-08-30T00:00:00Z".into(),
            scheduled_occurrence_id: scheduled.then(|| "occurrence-1".into()),
            project_id: "project-1".into(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            node_label: "Node".into(),
            outcome: RunOutcome::Success,
            task_title: Some("Task".into()),
            completion_agent_label: None,
        }
    }

    fn missing_permission(project_id: &str, event_id: &str) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::InterventionRequested {
            event_id: event_id.into(),
            occurred_at: "2026-08-30T00:00:00Z".into(),
            scheduled_occurrence_id: None,
            project_id: project_id.into(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
            request: crate::app::intervention::InterventionRequestIdentity::Permission {
                request_id: "permission-missing".into(),
            },
            node_label: "Node".into(),
            kind: RuntimeInterventionKind::PermissionRequested,
            task_title: Some("Task".into()),
        }
    }

    fn permission_event(project_id: &str, event_id: &str) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::InterventionRequested {
            event_id: event_id.into(),
            occurred_at: "2026-08-30T00:00:00Z".into(),
            scheduled_occurrence_id: None,
            project_id: project_id.into(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
            request: crate::app::intervention::InterventionRequestIdentity::Permission {
                request_id: "permission-1".into(),
            },
            node_label: "Direct agent".into(),
            kind: RuntimeInterventionKind::PermissionRequested,
            task_title: Some("Repair download".into()),
        }
    }

    fn manual_check_event(project_id: &str, event_id: &str) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::InterventionRequested {
            event_id: event_id.into(),
            occurred_at: "2026-08-30T00:00:00Z".into(),
            scheduled_occurrence_id: None,
            project_id: project_id.into(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
            request: crate::app::intervention::InterventionRequestIdentity::ManualCheck,
            node_label: "Interview".into(),
            kind: RuntimeInterventionKind::ManualDecisionRequired,
            task_title: Some("Prepare release".into()),
        }
    }

    fn elicitation_event(project_id: &str, event_id: &str) -> RuntimeLifecycleEvent {
        RuntimeLifecycleEvent::InterventionRequested {
            event_id: event_id.into(),
            occurred_at: "2026-09-03T00:00:00Z".into(),
            scheduled_occurrence_id: None,
            project_id: project_id.into(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
            request: crate::app::intervention::InterventionRequestIdentity::Elicitation {
                elicitation_id: "elicit-001".into(),
            },
            node_label: "Direct agent".into(),
            kind: RuntimeInterventionKind::ElicitationRequested,
            task_title: Some("Feature survey".into()),
        }
    }

    fn permission_option_id(action: &crate::app::intervention::InterventionAllowedAction) -> &str {
        let crate::app::intervention::InterventionAllowedAction::PermissionOption {
            option_id, ..
        } = action
        else {
            unreachable!("permission fixtures contain permission options");
        };
        option_id
    }

    fn write_permission_projection_fixture(app: &App) {
        let locator = InterventionLocator {
            project_id: app.paths.project_id.clone(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let run = crate::runtime::RunState {
            version: crate::domain::VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: crate::domain::RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-30T00:00:00Z".into(),
            updated_at: "2026-08-30T00:00:01Z".into(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(locator.node_id.clone()),
            current_attempt: Some(locator.attempt_id.clone()),
            new_rounds_opened: 0,
            pause_reason: Some(crate::domain::PauseReason::PermissionRequested),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        crate::storage::write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run)
            .unwrap();
        crate::storage::write_json(
            &app.paths.node_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &crate::runtime::NodeState {
                version: crate::domain::VERSION.to_string(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: locator.node_id.clone(),
                node_type: crate::domain::NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: crate::domain::RunStatus::Paused,
                outcome: None,
                started_at: "2026-08-30T00:00:00Z".into(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        let attempt_dir = app.paths.attempt_dir(
            &locator.task_id,
            &locator.run_id,
            &locator.round_id,
            &locator.node_id,
            &locator.attempt_id,
        );
        crate::acp::permission::write_pending_permission(
            &attempt_dir,
            "permission-1",
            serde_json::json!({
                "toolCall": {
                    "title": "Edit files",
                    "locations": [{ "path": "D:\\Downloads\\new2.txt" }],
                    "rawInput": { "command": "git status" }
                },
                "options": [
                    { "optionId": "allow_once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "allow_for_session", "name": "Allow for session", "kind": "allow_for_session" },
                    { "optionId": "reject", "name": "Reject", "kind": "reject" },
                    { "optionId": "reject_always", "name": "Reject always", "kind": "reject_always" },
                    { "optionId": "auto", "name": "Use auto mode", "kind": "allow_always" },
                    { "optionId": "acceptEdits", "name": "Auto-accept edits", "kind": "allow_always" },
                    { "optionId": "bypassPermissions", "name": "Bypass permissions", "kind": "allow_always" },
                    { "optionId": "plan", "name": "No, keep planning", "kind": "allow_always" },
                    { "optionId": "cancel", "name": "Cancel", "kind": "reject_once" }
                ],
                "_meta": {
                    "permission": {
                        "title": "Make edits?",
                        "description": "command failed; retry without sandbox?"
                    }
                }
            }),
            "2026-08-30T00:00:01Z".into(),
        )
        .unwrap();
    }

    fn write_manual_check_projection_fixture(app: &App) {
        let locator = InterventionLocator {
            project_id: app.paths.project_id.clone(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let run = crate::runtime::RunState {
            version: crate::domain::VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: crate::domain::RunStatus::Paused,
            outcome: None,
            started_at: "2026-08-30T00:00:00Z".into(),
            updated_at: "2026-08-30T00:00:01Z".into(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(locator.node_id.clone()),
            current_attempt: Some(locator.attempt_id.clone()),
            new_rounds_opened: 0,
            pause_reason: Some(crate::domain::PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: crate::runtime::RuntimeExecutionState {
                phase: crate::runtime::RuntimeExecutionPhase::AwaitingManualCheck,
                ..Default::default()
            },
        };
        crate::storage::write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run)
            .unwrap();
        crate::storage::write_json(
            &app.paths
                .round_file(&locator.task_id, &locator.run_id, &locator.round_id),
            &crate::runtime::RoundState {
                version: crate::domain::VERSION.to_string(),
                id: locator.round_id.clone(),
                run_id: locator.run_id.clone(),
                index: 1,
                status: crate::domain::RunStatus::Paused,
                outcome: None,
                trigger: crate::domain::RoundTrigger::Initial,
                started_at: "2026-08-30T00:00:00Z".into(),
                trace: Vec::new(),
                uuid: None,
            },
        )
        .unwrap();
        crate::storage::write_json(
            &app.paths.node_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &crate::runtime::NodeState {
                version: crate::domain::VERSION.to_string(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: locator.node_id.clone(),
                node_type: crate::domain::NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: crate::domain::RunStatus::Paused,
                outcome: None,
                started_at: "2026-08-30T00:00:00Z".into(),
                finished_at: Some("2026-08-30T00:00:01Z".into()),
                manual_check_pending: true,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        crate::acp::events::write_timeline_items(
            &app.paths.acp_timeline_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &[
                crate::acp::events::AcpUiEvent {
                    id: "old".into(),
                    seq: 1,
                    timestamp: "1Z".into(),
                    kind: "textDelta".into(),
                    session_id: Some("session-1".into()),
                    content: Some("old output".into()),
                    title: None,
                    tool_call_id: None,
                    status: Some("completed".into()),
                    started_seq: Some(1),
                    ended_seq: Some(1),
                    started_at: Some("1Z".into()),
                    ended_at: Some("1Z".into()),
                    timing: None,
                    raw: None,
                },
                crate::acp::events::AcpUiEvent {
                    id: "new".into(),
                    seq: 2,
                    timestamp: "2Z".into(),
                    kind: "textDelta".into(),
                    session_id: Some("session-1".into()),
                    content: Some("latest model output".into()),
                    title: None,
                    tool_call_id: None,
                    status: Some("completed".into()),
                    started_seq: Some(2),
                    ended_seq: Some(2),
                    started_at: Some("2Z".into()),
                    ended_at: Some("2Z".into()),
                    timing: None,
                    raw: None,
                },
            ],
        )
        .unwrap();
    }

    fn write_elicitation_projection_fixture(app: &App, properties: serde_json::Value) {
        let locator = InterventionLocator {
            project_id: app.paths.project_id.clone(),
            task_id: "task-1".into(),
            run_id: "run-1".into(),
            round_id: "round-1".into(),
            node_id: "node-1".into(),
            attempt_id: "attempt-1".into(),
            outer_node_id: None,
            outer_attempt_id: None,
        };
        let run = crate::runtime::RunState {
            version: crate::domain::VERSION.to_string(),
            id: locator.run_id.clone(),
            task_id: locator.task_id.clone(),
            task_uuid: None,
            status: crate::domain::RunStatus::Paused,
            outcome: None,
            started_at: "2026-09-03T00:00:00Z".into(),
            updated_at: "2026-09-03T00:00:01Z".into(),
            workflow_snapshot: "workflow.snapshot.json".into(),
            current_round: Some(locator.round_id.clone()),
            current_node: Some(locator.node_id.clone()),
            current_attempt: Some(locator.attempt_id.clone()),
            new_rounds_opened: 0,
            pause_reason: Some(crate::domain::PauseReason::WaitingForUserInput),
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
        };
        crate::storage::write_json(&app.paths.run_file(&locator.task_id, &locator.run_id), &run)
            .unwrap();
        crate::storage::write_json(
            &app.paths.node_file(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &crate::runtime::NodeState {
                version: crate::domain::VERSION.to_string(),
                acp_storage_schema_version: crate::runtime::CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: locator.node_id.clone(),
                node_type: crate::domain::NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: crate::domain::RunStatus::Paused,
                outcome: None,
                started_at: "2026-09-03T00:00:00Z".into(),
                finished_at: None,
                manual_check_pending: false,
                runtime_execution_id: None,
                resolved_config: Default::default(),
                uuid: None,
            },
        )
        .unwrap();
        let request: agent_client_protocol_schema::v1::CreateElicitationRequest =
            serde_json::from_value(serde_json::json!({
                "mode": "form",
                "sessionId": "session-1",
                "message": "请选择功能",
                "requestedSchema": {
                    "type": "object",
                    "properties": properties,
                    "required": ["features"]
                }
            }))
            .unwrap();
        crate::acp::elicitation::write_pending_elicitation(
            &app.paths.attempt_dir(
                &locator.task_id,
                &locator.run_id,
                &locator.round_id,
                &locator.node_id,
                &locator.attempt_id,
            ),
            &crate::acp::elicitation::PendingElicitationState {
                elicitation_id: "elicit-001".into(),
                jsonrpc_id: serde_json::json!(1),
                request,
                created_at: "2026-09-03T00:00:01Z".into(),
                timeline_identity: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn scheduled_information_events_are_not_projected_to_im() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(repository);
        let targets = [target(ImChannelKind::WeCom)];
        let event = run_completed(true, "event-1");
        let result = projector
            .project_event(&app, &event, &targets, 100)
            .unwrap();
        assert_eq!(result.skipped, 1);
        assert_eq!(result.inserted, 0);
        assert_eq!(result.duplicates, 0);
    }

    #[test]
    fn scheduled_interventions_still_enter_the_im_outbox() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(repository);
        let targets = [target(ImChannelKind::WeCom)];
        let mut event = permission_event(&app.paths.project_id, "scheduled-permission-1");
        if let RuntimeLifecycleEvent::InterventionRequested {
            scheduled_occurrence_id,
            ..
        } = &mut event
        {
            *scheduled_occurrence_id = Some("occurrence-1".into());
        }

        let result = projector
            .project_event(&app, &event, &targets, 100)
            .unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn distinct_event_ids_are_never_time_window_aggregated() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let projector = ImLifecycleProjector::new(Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        )));
        let targets = [target(ImChannelKind::WeCom)];
        for event_id in ["event-1", "event-2", "event-3"] {
            assert_eq!(
                projector
                    .project_event(&app, &run_completed(false, event_id), &targets, 100,)
                    .unwrap()
                    .inserted,
                1
            );
        }
    }

    #[test]
    fn reused_local_run_ids_in_different_tasks_create_distinct_deliveries() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(repository);
        let target = [target(ImChannelKind::WeCom)];

        for task_id in ["task-1", "task-2"] {
            let event_id = crate::app::make_completion_dedup_key(
                "project-1",
                task_id,
                "run-1",
                "round-1",
                "node-1",
                "attempt-1",
            );
            let mut event = run_completed(false, &event_id);
            if let RuntimeLifecycleEvent::RunCompleted {
                task_id: event_task_id,
                ..
            } = &mut event
            {
                *event_task_id = task_id.into();
            }
            assert_eq!(
                projector
                    .project_event(&app, &event, &target, 100)
                    .unwrap()
                    .inserted,
                1
            );
        }
    }

    #[test]
    fn permission_projection_keeps_structured_details_and_all_valid_wecom_actions() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let event = permission_event(&app.paths.project_id, "permission-event-1");
        assert_eq!(
            projector
                .project_event(&app, &event, &[target(ImChannelKind::WeCom)], 100)
                .unwrap()
                .inserted,
            1
        );
        let claimed = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention {
            presentation,
            reference,
            ..
        } = &claimed[0].delivery.payload
        else {
            panic!("permission delivery must be intervention payload");
        };
        assert_eq!(
            presentation.body.as_deref(),
            Some("command failed; retry without sandbox?")
        );
        assert_eq!(presentation.title.as_deref(), Some("Make edits?"));
        assert_eq!(
            presentation.summary.as_deref(),
            Some("command failed; retry without sandbox?")
        );
        assert!(!presentation.fields.contains_key("permissionTitle"));
        assert_eq!(
            presentation
                .fields
                .get("permissionTool")
                .map(String::as_str),
            Some("Edit files")
        );
        assert_eq!(
            presentation
                .fields
                .get("permissionPath")
                .map(String::as_str),
            Some("D:\\Downloads\\new2.txt")
        );
        assert_eq!(
            presentation.fields.get("taskTitle").map(String::as_str),
            Some("Repair download")
        );
        assert_eq!(reference.allowed_actions.len(), 9);
        assert_eq!(
            permission_option_id(&reference.allowed_actions[0]),
            "allow_once"
        );
        assert_eq!(
            permission_option_id(&reference.allowed_actions[8]),
            "cancel"
        );
    }

    #[test]
    fn manual_check_projection_snapshots_latest_model_output() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_manual_check_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let event = manual_check_event(&app.paths.project_id, "manual-check-event-1");

        assert_eq!(
            projector
                .project_event(&app, &event, &[target(ImChannelKind::WeCom)], 100)
                .unwrap()
                .inserted,
            1
        );

        let claimed = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention {
            presentation,
            reference,
            ..
        } = &claimed[0].delivery.payload
        else {
            panic!("manual check delivery must be intervention payload");
        };
        assert_eq!(presentation.body.as_deref(), Some("latest model output"));
        assert_eq!(
            presentation.fields.get("nodeLabel").map(String::as_str),
            Some("Interview")
        );
        assert_eq!(
            presentation.fields.get("taskTitle").map(String::as_str),
            Some("Prepare release")
        );
        assert_eq!(
            reference.allowed_actions,
            vec![
                crate::app::intervention::InterventionAllowedAction::ManualSuccess,
                crate::app::intervention::InterventionAllowedAction::ManualFailure,
            ]
        );
    }

    #[test]
    fn supported_elicitation_forms_only_enter_the_wecom_outbox() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_elicitation_projection_fixture(
            &app,
            serde_json::json!({
                "features": {
                    "type": "array",
                    "title": "功能模块",
                    "items": {"anyOf": [
                        {"const": "auth", "title": "认证"},
                        {"const": "logging", "title": "日志"}
                    ]}
                }
            }),
        );
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let targets = [target(ImChannelKind::WeCom)];
        let result = projector
            .project_event(
                &app,
                &elicitation_event(&app.paths.project_id, "elicitation-supported"),
                &targets,
                100,
            )
            .unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(result.skipped, 0);

        let deliveries = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention { reference, .. } = &deliveries[0].delivery.payload
        else {
            panic!("expected intervention delivery");
        };
        assert_eq!(reference.allowed_actions.len(), 1);
        assert!(matches!(
            &reference.allowed_actions[0],
            crate::app::intervention::InterventionAllowedAction::ElicitationFixedForm { .. }
        ));
    }

    #[test]
    fn unsupported_elicitation_is_skipped_before_an_outbox_row_exists() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_elicitation_projection_fixture(
            &app,
            serde_json::json!({
                "answer": {
                    "type": "string",
                    "title": "自定义答案",
                    "description": "请输入"
                }
            }),
        );
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let result = projector
            .project_event(
                &app,
                &elicitation_event(&app.paths.project_id, "elicitation-unsupported"),
                &[target(ImChannelKind::WeCom)],
                100,
            )
            .unwrap();
        assert_eq!(result.inserted, 0);
        assert_eq!(result.skipped, 1);
        assert!(
            repository
                .claim_due(
                    ImChannelKind::WeCom,
                    100,
                    std::time::Duration::from_secs(60),
                    1,
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reused_permission_request_identity_does_not_suppress_new_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let attempt_dir =
            app.paths
                .attempt_dir("task-1", "run-1", "round-1", "node-1", "attempt-1");
        crate::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            serde_json::json!({
                "toolCall": { "toolCallId": "call-first", "title": "Write first.txt" },
                "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }]
            }),
            "2026-08-30T00:00:01Z".into(),
        )
        .unwrap();
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let mut first = permission_event(&app.paths.project_id, "permission-first");
        if let RuntimeLifecycleEvent::InterventionRequested { request, .. } = &mut first {
            *request = crate::app::intervention::InterventionRequestIdentity::Permission {
                request_id: "0".into(),
            };
        }

        assert_eq!(
            projector
                .project_event(&app, &first, &[target(ImChannelKind::WeCom)], 100)
                .unwrap()
                .inserted,
            1
        );

        crate::acp::permission::write_pending_permission(
            &attempt_dir,
            "0",
            serde_json::json!({
                "toolCall": { "toolCallId": "call-second", "title": "Write second.txt" },
                "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }]
            }),
            "2026-08-30T00:00:02Z".into(),
        )
        .unwrap();
        let mut second = permission_event(&app.paths.project_id, "permission-second");
        if let RuntimeLifecycleEvent::InterventionRequested { request, .. } = &mut second {
            *request = crate::app::intervention::InterventionRequestIdentity::Permission {
                request_id: "0".into(),
            };
        }
        let result = projector
            .project_event(&app, &second, &[target(ImChannelKind::WeCom)], 101)
            .unwrap();
        assert_eq!(result.inserted, 1);
        assert_eq!(
            projector
                .project_event(&app, &second, &[target(ImChannelKind::WeCom)], 102,)
                .unwrap()
                .duplicates,
            1
        );
    }

    #[test]
    fn invalid_wecom_permissions_keep_only_safe_reject_actions() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let ambiguous = permission_event(&app.paths.project_id, "permission-ambiguous");
        let attempt_dir =
            app.paths
                .attempt_dir("task-1", "run-1", "round-1", "node-1", "attempt-1");
        crate::acp::permission::write_pending_permission(
            &attempt_dir,
            "permission-1",
            serde_json::json!({
                "options": [
                    { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "allow_duplicate", "name": "Allow again", "kind": "allow_once" },
                    { "optionId": "cancel", "name": "Cancel", "kind": "cancel" }
                ]
            }),
            "2026-08-30T00:00:01Z".into(),
        )
        .unwrap();
        projector
            .project_event(&app, &ambiguous, &[target(ImChannelKind::WeCom)], 100)
            .unwrap();
        let wecom = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention { reference, .. } = &wecom[0].delivery.payload else {
            unreachable!();
        };
        assert_eq!(reference.allowed_actions.len(), 1);
        assert_eq!(
            permission_option_id(&reference.allowed_actions[0]),
            "cancel"
        );
    }

    #[test]
    fn oversized_wecom_permission_keeps_at_most_twenty_safe_rejects() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let attempt_dir =
            app.paths
                .attempt_dir("task-1", "run-1", "round-1", "node-1", "attempt-1");
        let options = (0..21)
            .map(|index| {
                serde_json::json!({
                    "optionId": format!("reject_{index}"),
                    "name": format!("Reject {index}"),
                    "kind": "reject_once"
                })
            })
            .collect::<Vec<_>>();
        crate::acp::permission::write_pending_permission(
            &attempt_dir,
            "permission-1",
            serde_json::json!({ "options": options }),
            "2026-08-30T00:00:01Z".into(),
        )
        .unwrap();
        projector
            .project_event(
                &app,
                &permission_event(&app.paths.project_id, "permission-oversized"),
                &[target(ImChannelKind::WeCom)],
                100,
            )
            .unwrap();
        let deliveries = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention { reference, .. } = &deliveries[0].delivery.payload
        else {
            unreachable!("permission delivery must be intervention payload");
        };
        assert_eq!(reference.allowed_actions.len(), 20);
        assert!(
            reference
                .allowed_actions
                .iter()
                .all(crate::app::intervention::permission_action_is_safe_remote_reject)
        );
    }

    #[tokio::test]
    async fn async_projection_reports_failure_then_continues_after_delivery_is_durable() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let (sender, receiver) = im_lifecycle_projection_channel();
        let (completion_sender, completion_receiver) = std::sync::mpsc::sync_channel(3);
        let target = target(ImChannelKind::WeCom);

        let mut oversized = run_completed(false, "event-too-large");
        if let RuntimeLifecycleEvent::RunCompleted { task_title, .. } = &mut oversized {
            *task_title = Some("x".repeat(crate::im::MAX_IM_PRESENTATION_BYTES + 1));
        }
        for event in [
            missing_permission(&app.paths.project_id, "event-missing-request"),
            oversized,
            run_completed(false, "event-durable"),
        ] {
            sender
                .send(ImLifecycleProjectionQueueItem::Event(
                    ImLifecycleProjectionJob {
                        app: app.clone_for_background(),
                        event,
                        targets: vec![target.clone()],
                        now_ms: 100,
                    },
                ))
                .await
                .unwrap();
        }
        drop(sender);

        projector
            .run(receiver, move |completion| {
                completion_sender.send(completion).unwrap();
            })
            .await;

        let missing = completion_receiver.recv().unwrap();
        assert_eq!(missing.canonical_event_id, "event-missing-request");
        assert_eq!(missing.result, Err("INTERVENTION_REQUEST_NOT_FOUND"));

        let oversized = completion_receiver.recv().unwrap();
        assert_eq!(oversized.canonical_event_id, "event-too-large");
        assert_eq!(oversized.result, Err("IM_PAYLOAD_TOO_LARGE"));

        let completed = completion_receiver.recv().unwrap();
        assert_eq!(completed.canonical_event_id, "event-durable");
        assert_eq!(completed.result.unwrap().inserted, 1);
        let claimed = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].delivery.canonical_event_id, "event-durable");
    }

    #[tokio::test]
    async fn projection_barrier_waits_for_prior_events_to_be_durable() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let (sender, receiver) = im_lifecycle_projection_channel();
        let run = tokio::spawn(projector.run(receiver, |_| {}));

        sender
            .send(ImLifecycleProjectionQueueItem::Event(
                ImLifecycleProjectionJob {
                    app: app.clone_for_background(),
                    event: run_completed(false, "event-before-barrier"),
                    targets: vec![target(ImChannelKind::WeCom)],
                    now_ms: 100,
                },
            ))
            .await
            .unwrap();
        drain_projection_queue(&sender).await.unwrap();

        let claimed = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(
            claimed[0].delivery.canonical_event_id,
            "event-before-barrier"
        );
        drop(sender);
        run.await.unwrap();
    }

    #[test]
    fn saturated_projection_enqueue_is_bounded_and_does_not_project() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        let (sender, _receiver) = mpsc::channel(1);
        let job = || ImLifecycleProjectionJob {
            app: app.clone_for_background(),
            event: run_completed(false, "event-1"),
            targets: vec![target(ImChannelKind::WeCom)],
            now_ms: 100,
        };
        assert_eq!(
            try_enqueue_projection(&sender, job()),
            ImProjectionEnqueueOutcome::Enqueued
        );
        assert_eq!(
            try_enqueue_projection(&sender, job()),
            ImProjectionEnqueueOutcome::Saturated
        );
    }
}
