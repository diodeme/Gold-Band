use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::app::intervention::{
    ElicitationQuestionKind, InterventionAllowedAction, InterventionCommandService,
    InterventionError, InterventionLocator, RemoteElicitationForm,
    permission_action_is_safe_remote_reject, permission_actions_require_desktop,
};
use crate::app::{AcpTurnOutcome, App, RuntimeInterventionKind, RuntimeLifecycleEvent};
use crate::domain::RunOutcome;

use super::{
    IM_LIFECYCLE_QUEUE_CAPACITY, IM_PAYLOAD_VERSION, INTERVENTION_NODE_FIELD,
    INTERVENTION_TASK_FIELD, INTERVENTION_WORKSPACE_FIELD, ImChannelCapabilities, ImChannelKind,
    ImCustomAnswerSupport, ImDelivery, ImDeliveryInsertResult, ImDeliveryPayload, ImDestination,
    ImIntegrationSettings, ImInteractionCapabilities, ImNavigationLocator, ImNotificationKind,
    ImNotificationPreferences, ImRepository, ImRepositoryError, InformationalNotification,
    InterventionPresentation, InterventionQuestion, InterventionQuestionKind,
    InterventionQuestionOption, InterventionRef, is_intervention_context_field,
};

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
    pub workspace_label: Option<String>,
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
        let workspace_label = app
            .paths
            .repo_root
            .file_name()
            .unwrap_or(app.paths.project_id.as_str());
        self.project_event_with_workspace_label(app, event, Some(workspace_label), targets, now_ms)
    }

    pub fn project_event_with_workspace_label(
        &self,
        app: &App,
        event: &RuntimeLifecycleEvent,
        workspace_label: Option<&str>,
        targets: &[ImProjectionTarget],
        now_ms: i64,
    ) -> Result<ImProjectionResult, ImProjectionError> {
        let Some(draft) = self.draft_for_event(app, event, workspace_label)? else {
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
                workspace_label,
            } = job;
            let canonical_event_id = event_canonical_id(&event).to_string();
            let result = tokio::task::spawn_blocking(move || {
                ImLifecycleProjector::new(repository).project_event_with_workspace_label(
                    &app,
                    &event,
                    workspace_label.as_deref(),
                    &targets,
                    now_ms,
                )
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
        workspace_label: Option<&str>,
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
                let workspace_label = workspace_label
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        app.paths
                            .repo_root
                            .file_name()
                            .map(str::to_string)
                            .filter(|value| !value.trim().is_empty())
                    })
                    .unwrap_or_else(|| app.paths.project_id.clone());
                let task_label = task_title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        app.task_show(task_id)
                            .ok()
                            .and_then(|task| task.title)
                            .map(|title| title.trim().to_string())
                            .filter(|title| !title.is_empty())
                    })
                    .unwrap_or_else(|| task_id.clone());
                let node_label = {
                    let label = node_label.trim();
                    if label.is_empty() {
                        node_id.as_str()
                    } else {
                        label
                    }
                };
                let mut presentation_fields = BTreeMap::from([
                    (
                        INTERVENTION_WORKSPACE_FIELD.into(),
                        bounded_presentation_field(&workspace_label),
                    ),
                    (
                        INTERVENTION_TASK_FIELD.into(),
                        bounded_presentation_field(&task_label),
                    ),
                    (
                        INTERVENTION_NODE_FIELD.into(),
                        bounded_presentation_field(node_label),
                    ),
                ]);
                if let Some(prompt) = snapshot.prompt.as_ref() {
                    presentation_fields.extend(
                        prompt
                            .fields
                            .iter()
                            .filter(|(key, _)| !is_intervention_context_field(key))
                            .map(|(key, value)| (key.clone(), value.clone())),
                    );
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
                    // Decline stays in the draft; each channel decides whether
                    // it can publish it (`project_for_channel`).
                    remote_allowed_actions.retain(|action| {
                        matches!(
                            action,
                            InterventionAllowedAction::ElicitationFixedForm { .. }
                                | InterventionAllowedAction::ElicitationDecline
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
            let Some(payload) = project_payload_for_channel(
                draft.notification_kind,
                &draft.payload,
                &target.capabilities.interaction,
            ) else {
                result.skipped += 1;
                continue;
            };
            let destination = target
                .destination
                .clone()
                .expect("eligible target has destination");
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

/// Writes the actions one channel actually publishes into its own payload;
/// `None` means this channel receives no delivery (§7.2).
fn project_payload_for_channel(
    kind: ImNotificationKind,
    payload: &ImDeliveryPayload,
    capabilities: &ImInteractionCapabilities,
) -> Option<ImDeliveryPayload> {
    let ImDeliveryPayload::Intervention {
        version,
        reference,
        presentation,
    } = payload
    else {
        return Some(payload.clone());
    };
    let mut reference = reference.clone();
    reference.allowed_actions =
        project_for_channel(kind, &reference.allowed_actions, capabilities)?;
    Some(ImDeliveryPayload::Intervention {
        version: *version,
        reference,
        presentation: presentation.clone(),
    })
}

/// Pure per-channel projection of the channel-independent allowed actions.
pub(crate) fn project_for_channel(
    kind: ImNotificationKind,
    actions: &[InterventionAllowedAction],
    capabilities: &ImInteractionCapabilities,
) -> Option<Vec<InterventionAllowedAction>> {
    match kind {
        ImNotificationKind::Permission => Some(remote_permission_actions(
            actions,
            usize::from(capabilities.permission_max_options),
        )),
        ImNotificationKind::Elicitation => {
            let form = InterventionAllowedAction::fixed_form(actions)?;
            let mut form = fit_form(form, capabilities)?;
            form.decline_allowed &= actions
                .iter()
                .any(|action| matches!(action, InterventionAllowedAction::ElicitationDecline));
            let decline_allowed = form.decline_allowed;
            let mut fitted = vec![InterventionAllowedAction::ElicitationFixedForm { form }];
            if decline_allowed {
                fitted.push(InterventionAllowedAction::ElicitationDecline);
            }
            Some(fitted)
        }
        _ => Some(actions.to_vec()),
    }
}

/// Fits a remote form to one channel's interaction capabilities. Returns
/// `None` when the channel cannot carry the form faithfully; the desktop
/// still handles it.
pub(crate) fn fit_form(
    form: &RemoteElicitationForm,
    capabilities: &ImInteractionCapabilities,
) -> Option<RemoteElicitationForm> {
    if form.questions.is_empty() || form.questions.len() > usize::from(capabilities.max_questions) {
        return None;
    }
    let multi_question = form.questions.len() > 1;
    let single_choice_limit = if multi_question {
        capabilities.single_choice_max_options.multi_question
    } else {
        capabilities.single_choice_max_options.single_question
    };
    let mut fitted = form.clone();
    for question in &mut fitted.questions {
        let option_count = question.question.options.len();
        match question.question.question_kind {
            ElicitationQuestionKind::SingleSelect => {
                if option_count > usize::from(single_choice_limit) {
                    return None;
                }
            }
            ElicitationQuestionKind::MultiSelect => {
                let multi_choice = capabilities.multi_choice?;
                if option_count > usize::from(multi_choice.max_options)
                    || (multi_question && !multi_choice.within_multi_question)
                {
                    return None;
                }
            }
            ElicitationQuestionKind::FreeText => {
                let free_text = capabilities.free_text?;
                question.constraints.max_chars =
                    question.constraints.max_chars.min(free_text.max_chars);
                question.constraints.multiline &= free_text.multiline;
                continue;
            }
        }
        if question.custom_answer_key.is_none() {
            continue;
        }
        match capabilities.custom_answer {
            ImCustomAnswerSupport::Supported { max_chars } => {
                question.constraints.max_chars = question.constraints.max_chars.min(max_chars);
            }
            ImCustomAnswerSupport::Unsupported => {
                if question.question.question_kind == ElicitationQuestionKind::MultiSelect {
                    return None;
                }
                // Single select keeps its listed options only.
                question.custom_answer_key = None;
            }
        }
    }
    fitted.decline_allowed &= capabilities.decline;
    Some(fitted)
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
    actions: &[InterventionAllowedAction],
    capacity: usize,
) -> Vec<InterventionAllowedAction> {
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
            capabilities: match channel {
                ImChannelKind::WeCom => crate::im::connectors::wecom::WECOM_CHANNEL_CAPABILITIES,
                ImChannelKind::Feishu => crate::im::connectors::feishu::FEISHU_CHANNEL_CAPABILITIES,
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
            task_uuid: None,
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
            task_uuid: None,
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
            task_uuid: None,
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
            task_uuid: None,
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
            task_uuid: None,
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
            "turn-1",
            "prompt-event-1",
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
        write_elicitation_projection_fixture_with_required(
            app,
            properties,
            serde_json::json!(["features"]),
        );
    }

    fn write_elicitation_projection_fixture_with_required(
        app: &App,
        properties: serde_json::Value,
        required: serde_json::Value,
    ) {
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
                    "required": required
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
            &crate::acp::elicitation::pending_elicitation_state(
                "elicit-001",
                "turn-1",
                "prompt-event-1",
                serde_json::json!(1),
                request,
                "2026-09-03T00:00:01Z".into(),
            ),
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
                .project_event_with_workspace_label(
                    &app,
                    &event,
                    Some("Workspace A"),
                    &[target(ImChannelKind::WeCom)],
                    100,
                )
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
        assert_eq!(
            presentation
                .fields
                .get("workspaceLabel")
                .map(String::as_str),
            Some("Workspace A")
        );
        assert_eq!(
            presentation.fields.get("nodeLabel").map(String::as_str),
            Some("Direct agent")
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
    fn intervention_context_falls_back_to_canonical_task_identity() {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_permission_projection_fixture(&app);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        let mut event = permission_event(&app.paths.project_id, "permission-task-fallback");
        if let RuntimeLifecycleEvent::InterventionRequested { task_title, .. } = &mut event {
            *task_title = None;
        }

        projector
            .project_event(&app, &event, &[target(ImChannelKind::WeCom)], 100)
            .unwrap();
        let claimed = repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap();
        let ImDeliveryPayload::Intervention { presentation, .. } = &claimed[0].delivery.payload
        else {
            panic!("permission delivery must be intervention payload");
        };
        assert_eq!(
            presentation.fields.get("taskTitle").map(String::as_str),
            Some("task-1")
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
            presentation
                .fields
                .get("workspaceLabel")
                .map(String::as_str),
            Some("repo")
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
        let ImDeliveryPayload::Intervention {
            reference,
            presentation,
            ..
        } = &deliveries[0].delivery.payload
        else {
            panic!("expected intervention delivery");
        };
        assert_eq!(
            presentation
                .fields
                .get("workspaceLabel")
                .map(String::as_str),
            Some("repo")
        );
        assert_eq!(
            presentation.fields.get("taskTitle").map(String::as_str),
            Some("Feature survey")
        );
        assert_eq!(
            presentation.fields.get("nodeLabel").map(String::as_str),
            Some("Direct agent")
        );
        assert_eq!(reference.allowed_actions.len(), 1);
        assert!(matches!(
            &reference.allowed_actions[0],
            crate::app::intervention::InterventionAllowedAction::ElicitationFixedForm { .. }
        ));
    }

    fn single_select(title: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "string",
            "title": title,
            "oneOf": [
                {"const": "a", "title": "A"},
                {"const": "b", "title": "B"}
            ]
        })
    }

    fn multi_select(title: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "array",
            "title": title,
            "items": {"anyOf": [
                {"const": "a", "title": "A"},
                {"const": "b", "title": "B"}
            ]}
        })
    }

    /// Projects one elicitation schema to a WeCom target and returns the
    /// persisted actionable delivery, or `None` when WeCom received none.
    fn project_wecom_elicitation(
        properties: serde_json::Value,
        required: serde_json::Value,
    ) -> Option<ImDelivery> {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
        write_elicitation_projection_fixture_with_required(&app, properties, required);
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        let projector = ImLifecycleProjector::new(Arc::clone(&repository));
        projector
            .project_event(
                &app,
                &elicitation_event(&app.paths.project_id, "elicitation-boundary"),
                &[target(ImChannelKind::WeCom)],
                100,
            )
            .unwrap();
        repository
            .claim_due(
                ImChannelKind::WeCom,
                100,
                std::time::Duration::from_secs(60),
                1,
            )
            .unwrap()
            .into_iter()
            .map(|claimed| claimed.delivery)
            .find(|delivery| matches!(delivery.payload, ImDeliveryPayload::Intervention { .. }))
    }

    fn has_fixed_form(delivery: &ImDelivery) -> bool {
        let ImDeliveryPayload::Intervention { reference, .. } = &delivery.payload else {
            return false;
        };
        reference.allowed_actions.iter().any(|action| {
            matches!(
                action,
                crate::app::intervention::InterventionAllowedAction::ElicitationFixedForm { .. }
            )
        })
    }

    fn remote_question(
        kind: ElicitationQuestionKind,
        options: usize,
        custom_answer: bool,
    ) -> crate::app::intervention::RemoteQuestion {
        crate::app::intervention::RemoteQuestion {
            selector_key: "q0".into(),
            question: crate::app::intervention::ElicitationQuestion {
                field_name: "field".into(),
                title: "Question".into(),
                description: None,
                question_kind: kind,
                options: (0..options)
                    .map(
                        |index| crate::app::intervention::ElicitationQuestionOption {
                            value: serde_json::json!(index),
                            label: format!("Option {index}"),
                            description: None,
                        },
                    )
                    .collect(),
                allows_custom_answer: custom_answer,
            },
            constraints: crate::app::intervention::RemoteQuestionConstraints {
                required: true,
                allows_empty: false,
                max_chars: 4000,
                multiline: true,
            },
            custom_answer_key: custom_answer.then(|| "field_custom".into()),
        }
    }

    fn remote_form(
        questions: Vec<crate::app::intervention::RemoteQuestion>,
    ) -> RemoteElicitationForm {
        RemoteElicitationForm {
            questions,
            decline_allowed: true,
        }
    }

    #[test]
    fn fit_form_follows_channel_capabilities() {
        use ElicitationQuestionKind::{FreeText, MultiSelect, SingleSelect};
        let wecom = crate::im::connectors::wecom::WECOM_CHANNEL_CAPABILITIES.interaction;
        let rich = crate::im::connector::test_support::capabilities(
            crate::im::ImMessageUpdateMode::Anytime,
        )
        .interaction;
        let cases = [
            (
                "four questions",
                vec![remote_question(SingleSelect, 2, false); 4],
                false,
                true,
            ),
            (
                "five questions",
                vec![remote_question(SingleSelect, 2, false); 5],
                false,
                true,
            ),
            (
                "six questions",
                vec![remote_question(SingleSelect, 2, false); 6],
                false,
                false,
            ),
            (
                "single select 20",
                vec![remote_question(SingleSelect, 20, false)],
                true,
                true,
            ),
            (
                "single select 21",
                vec![remote_question(SingleSelect, 21, false)],
                false,
                false,
            ),
            (
                "multi-question options 11",
                vec![
                    remote_question(SingleSelect, 11, false),
                    remote_question(SingleSelect, 2, false),
                ],
                false,
                true,
            ),
            (
                "multi select in multi-question form",
                vec![
                    remote_question(SingleSelect, 2, false),
                    remote_question(MultiSelect, 2, false),
                ],
                false,
                true,
            ),
            (
                "multi select custom",
                vec![remote_question(MultiSelect, 2, true)],
                false,
                true,
            ),
            (
                "single select custom",
                vec![remote_question(SingleSelect, 2, true)],
                true,
                true,
            ),
            (
                "free text",
                vec![remote_question(FreeText, 0, false)],
                false,
                true,
            ),
        ];
        for (name, questions, wecom_fits, rich_fits) in cases {
            let form = remote_form(questions);
            assert_eq!(
                fit_form(&form, &wecom).is_some(),
                wecom_fits,
                "WeCom: {name}"
            );
            assert_eq!(fit_form(&form, &rich).is_some(), rich_fits, "rich: {name}");
        }
    }

    #[test]
    fn fit_form_keeps_feishu_select_capacity_in_multi_question_forms() {
        use ElicitationQuestionKind::{MultiSelect, SingleSelect};
        let feishu = crate::im::connectors::feishu::FEISHU_CHANNEL_CAPABILITIES.interaction;
        let form = remote_form(vec![
            remote_question(SingleSelect, 4, false),
            remote_question(SingleSelect, 4, false),
            remote_question(MultiSelect, 4, true),
            remote_question(SingleSelect, 3, true),
        ]);

        let fitted = fit_form(&form, &feishu).expect("actual Feishu follow-up shape");
        assert_eq!(fitted.questions.len(), 4);
        assert!(fitted.questions[2].custom_answer_key.is_some());
        assert!(fitted.questions[3].custom_answer_key.is_some());
    }

    #[test]
    fn fit_form_trims_unsupported_details_without_touching_supported_ones() {
        let wecom = crate::im::connectors::wecom::WECOM_CHANNEL_CAPABILITIES.interaction;
        let rich = crate::im::connector::test_support::capabilities(
            crate::im::ImMessageUpdateMode::Anytime,
        )
        .interaction;
        let single_custom = remote_form(vec![remote_question(
            ElicitationQuestionKind::SingleSelect,
            2,
            true,
        )]);
        let on_wecom = fit_form(&single_custom, &wecom).unwrap();
        assert_eq!(on_wecom.questions[0].custom_answer_key, None);
        assert!(!on_wecom.decline_allowed);
        let on_rich = fit_form(&single_custom, &rich).unwrap();
        assert_eq!(
            on_rich.questions[0].custom_answer_key.as_deref(),
            Some("field_custom")
        );
        assert_eq!(on_rich.questions[0].constraints.max_chars, 500);
        assert!(on_rich.decline_allowed);

        let free_text = remote_form(vec![remote_question(
            ElicitationQuestionKind::FreeText,
            0,
            false,
        )]);
        assert_eq!(
            fit_form(&free_text, &rich).unwrap().questions[0]
                .constraints
                .max_chars,
            2000
        );
    }

    #[test]
    fn decline_is_published_only_where_the_channel_supports_it() {
        let form = remote_form(vec![remote_question(
            ElicitationQuestionKind::SingleSelect,
            2,
            false,
        )]);
        let actions = [
            InterventionAllowedAction::ElicitationFixedForm { form },
            InterventionAllowedAction::ElicitationDecline,
        ];
        let wecom = crate::im::connectors::wecom::WECOM_CHANNEL_CAPABILITIES.interaction;
        let rich = crate::im::connector::test_support::capabilities(
            crate::im::ImMessageUpdateMode::Anytime,
        )
        .interaction;
        let on_wecom =
            project_for_channel(ImNotificationKind::Elicitation, &actions, &wecom).unwrap();
        assert_eq!(on_wecom.len(), 1);
        let on_rich =
            project_for_channel(ImNotificationKind::Elicitation, &actions, &rich).unwrap();
        assert_eq!(on_rich.len(), 2);
        assert!(matches!(
            on_rich[1],
            InterventionAllowedAction::ElicitationDecline
        ));
    }

    // WeCom golden boundary (§11.1): these cases freeze which elicitation
    // shapes reach WeCom as an actionable card across the capability refactor.
    #[test]
    fn wecom_four_question_form_is_not_projected_before_sending_details() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "q1": single_select("Q1"),
                "q2": single_select("Q2"),
                "q3": single_select("Q3"),
                "q4": single_select("Q4")
            }),
            serde_json::json!(["q1"]),
        );
        assert!(
            delivery.is_none(),
            "WeCom rejects four selectors with 42047; skip the whole delivery"
        );
    }

    #[test]
    fn wecom_golden_six_questions_are_not_projected() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "q1": single_select("Q1"),
                "q2": single_select("Q2"),
                "q3": single_select("Q3"),
                "q4": single_select("Q4"),
                "q5": single_select("Q5"),
                "q6": single_select("Q6")
            }),
            serde_json::json!(["q1"]),
        );
        assert!(delivery.is_none());
    }

    #[test]
    fn wecom_five_single_select_questions_exceed_the_verified_platform_limit() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "q1": single_select("Q1"),
                "q2": single_select("Q2"),
                "q3": single_select("Q3"),
                "q4": single_select("Q4"),
                "q5": single_select("Q5")
            }),
            serde_json::json!(["q1"]),
        );
        assert!(delivery.is_none());
    }

    #[test]
    fn wecom_golden_three_single_select_questions_are_projected() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "q1": single_select("Q1"),
                "q2": single_select("Q2"),
                "q3": single_select("Q3")
            }),
            serde_json::json!(["q1"]),
        )
        .expect("three single-select questions stay actionable on WeCom");
        assert!(has_fixed_form(&delivery));
    }

    #[test]
    fn wecom_golden_multi_question_form_with_multi_select_is_not_projected() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "q1": single_select("Q1"),
                "q2": multi_select("Q2")
            }),
            serde_json::json!(["q1"]),
        );
        assert!(delivery.is_none());
    }

    #[test]
    fn wecom_golden_multi_select_with_custom_answer_is_not_projected() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "features": multi_select("Features"),
                "features_custom": {"type": "string", "title": "Other"}
            }),
            serde_json::json!(["features"]),
        );
        assert!(delivery.is_none());
    }

    #[test]
    fn wecom_golden_single_select_with_custom_answer_keeps_listed_options_only() {
        let delivery = project_wecom_elicitation(
            serde_json::json!({
                "database": single_select("Database"),
                "database_custom": {"type": "string", "title": "Other"}
            }),
            serde_json::json!(["database"]),
        )
        .expect("single select with a custom answer stays actionable on WeCom");
        assert!(has_fixed_form(&delivery));
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
            "turn-first",
            "prompt-event-first",
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
            "turn-second",
            "prompt-event-second",
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
            "turn-1",
            "prompt-event-1",
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
            "turn-1",
            "prompt-event-1",
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
                        workspace_label: None,
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
                    workspace_label: None,
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
            workspace_label: None,
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
