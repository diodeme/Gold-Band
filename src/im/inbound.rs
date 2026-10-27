use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::acp::elicitation::ElicitationAction;
use crate::app::intervention::{
    InterventionAction, InterventionAllowedAction, InterventionCommand, InterventionCommandResult,
    InterventionCommandStatus, InterventionError, InterventionErrorCode,
    REMOTE_CUSTOM_ANSWER_OPTION, RemoteElicitationAnswer, RemoteFormViolation,
    intervention_action_for_allowed, intervention_action_for_elicitation_form,
    intervention_action_is_allowed,
};

use super::{
    ImChannelKind, ImDelivery, ImDeliveryPayload, ImErrorCode, ImInboundActionResult,
    ImInboundEnvelope, ImInboundOutcome, ImRepository, ImRepositoryError, ImResolutionOutcome,
    ImSubmittedAction, InterventionRef,
};

const ACTION_TOKEN_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImActionTokenClaims {
    pub version: u16,
    pub delivery_id: String,
    pub canonical_event_id: String,
    pub channel: ImChannelKind,
    pub destination_id: String,
    pub expires_at_ms: i64,
}

#[derive(Clone)]
pub struct ImActionTokenCodec {
    signing_key: Arc<[u8]>,
}

impl ImActionTokenCodec {
    pub fn new(signing_key: Vec<u8>) -> Result<Self, ImInboundError> {
        if signing_key.len() < 32 {
            return Err(ImInboundError::TokenInvalid);
        }
        Ok(Self {
            signing_key: signing_key.into(),
        })
    }

    pub fn encode(&self, claims: &ImActionTokenClaims) -> Result<String, ImInboundError> {
        if claims.version != ACTION_TOKEN_VERSION {
            return Err(ImInboundError::TokenInvalid);
        }
        let payload = serde_json::to_vec(claims).map_err(|_| ImInboundError::TokenInvalid)?;
        let signature = self.sign(&payload)?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    pub fn decode(&self, token: &str) -> Result<ImActionTokenClaims, ImInboundError> {
        let (payload, signature) = token.split_once('.').ok_or(ImInboundError::TokenInvalid)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| ImInboundError::TokenInvalid)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| ImInboundError::TokenInvalid)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key)
            .map_err(|_| ImInboundError::TokenInvalid)?;
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| ImInboundError::TokenInvalid)?;
        let claims: ImActionTokenClaims =
            serde_json::from_slice(&payload).map_err(|_| ImInboundError::TokenInvalid)?;
        if claims.version != ACTION_TOKEN_VERSION {
            return Err(ImInboundError::TokenInvalid);
        }
        Ok(claims)
    }

    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, ImInboundError> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing_key)
            .map_err(|_| ImInboundError::TokenInvalid)?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImInboundError {
    #[error("action token is invalid")]
    TokenInvalid,
    #[error("action token expired")]
    Expired,
    #[error("delivery is not actionable")]
    InformationNotActionable,
    #[error("private conversation does not match binding")]
    ConversationMismatch,
    #[error("actor does not match binding")]
    ActorMismatch,
    #[error("current channel binding no longer authorizes this delivery")]
    BindingRevoked,
    #[error("current channel binding could not be loaded")]
    BindingUnavailable,
    #[error("action identity is invalid")]
    ActionIdentityInvalid,
    #[error("action was not published for this delivery")]
    ActionNotAllowed,
    #[error("delivery does not exist")]
    DeliveryNotFound,
    #[error("submitted form answers are invalid")]
    Form(RemoteFormViolation),
    #[error("IM repository failed")]
    Repository(#[from] ImRepositoryError),
    #[error("Runtime intervention failed")]
    Intervention(#[from] InterventionError),
}

impl ImInboundError {
    pub const fn code(&self) -> ImErrorCode {
        match self {
            Self::TokenInvalid => ImErrorCode::ActionTokenInvalid,
            Self::Expired => ImErrorCode::ActionExpired,
            Self::InformationNotActionable => ImErrorCode::InformationNotActionable,
            Self::ConversationMismatch => ImErrorCode::ConversationMismatch,
            Self::ActorMismatch => ImErrorCode::ActorMismatch,
            Self::BindingRevoked => ImErrorCode::BindingRevoked,
            Self::BindingUnavailable | Self::Repository(_) => ImErrorCode::StorageUnavailable,
            Self::ActionIdentityInvalid => ImErrorCode::ActionIdInvalid,
            Self::ActionNotAllowed => ImErrorCode::ActionInvalid,
            Self::DeliveryNotFound => ImErrorCode::DeliveryNotFound,
            Self::Form(violation) => match violation {
                RemoteFormViolation::AnswerMissing => ImErrorCode::FormAnswerMissing,
                RemoteFormViolation::OptionInvalid => ImErrorCode::FormOptionInvalid,
                RemoteFormViolation::CustomAnswerMissing => ImErrorCode::FormCustomAnswerMissing,
                RemoteFormViolation::TextTooLong => ImErrorCode::FormTextTooLong,
                RemoteFormViolation::AnswerInvalid => ImErrorCode::ActionInvalid,
            },
            Self::Intervention(error) => match error.code {
                InterventionErrorCode::InterventionExpired => ImErrorCode::ActionExpired,
                InterventionErrorCode::InterventionStorageUnavailable => {
                    ImErrorCode::ActionProcessingFailed
                }
                _ => ImErrorCode::ActionInvalid,
            },
        }
    }
}

#[derive(Default)]
struct ActionLocks {
    entries: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl ActionLocks {
    fn for_action(&self, action_id: &str) -> Arc<Mutex<()>> {
        self.entries
            .lock()
            .expect("IM action lock registry poisoned")
            .entry(action_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn release(&self, action_id: &str, lock: &Arc<Mutex<()>>) {
        let mut entries = self
            .entries
            .lock()
            .expect("IM action lock registry poisoned");
        if Arc::strong_count(lock) == 2 {
            entries.remove(action_id);
        }
    }
}

/// Result of one inbound call. `resolution` is present exactly when the recorded
/// outcome is `Accepted`; a replay of the same platform event re-derives it from
/// the identical submission, so the first call and its replays respond the same.
#[derive(Debug, Clone, PartialEq)]
pub struct ImInboundHandling {
    pub result: ImInboundActionResult,
    pub resolution: Option<ImResolutionOutcome>,
}

pub struct ImInboundActionService {
    repository: Arc<ImRepository>,
    token_codec: ImActionTokenCodec,
    locks: ActionLocks,
}

impl ImInboundActionService {
    pub fn new(repository: Arc<ImRepository>, token_codec: ImActionTokenCodec) -> Self {
        Self {
            repository,
            token_codec,
            locks: ActionLocks::default(),
        }
    }

    pub fn handle(
        &self,
        envelope: &ImInboundEnvelope,
        now_ms: i64,
        execute: impl FnOnce(
            InterventionCommand,
        ) -> Result<InterventionCommandResult, InterventionError>,
    ) -> Result<ImInboundHandling, ImInboundError> {
        let delivery = self
            .repository
            .delivery(&envelope.delivery_id)?
            .ok_or(ImInboundError::DeliveryNotFound)?;
        self.handle_with_delivery(envelope, &delivery, now_ms, execute)
    }

    pub fn handle_with_delivery(
        &self,
        envelope: &ImInboundEnvelope,
        delivery: &ImDelivery,
        now_ms: i64,
        execute: impl FnOnce(
            InterventionCommand,
        ) -> Result<InterventionCommandResult, InterventionError>,
    ) -> Result<ImInboundHandling, ImInboundError> {
        let expected_action_id =
            deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        if envelope.action_id != expected_action_id || envelope.platform_event_id.trim().is_empty()
        {
            return Err(ImInboundError::ActionIdentityInvalid);
        }
        let lock = self.locks.for_action(&envelope.action_id);
        let guard = lock.lock().expect("IM action mutex poisoned");
        let result = self.handle_locked(envelope, delivery, now_ms, execute);
        drop(guard);
        self.locks.release(&envelope.action_id, &lock);
        result
    }

    fn handle_locked(
        &self,
        envelope: &ImInboundEnvelope,
        delivery: &ImDelivery,
        now_ms: i64,
        execute: impl FnOnce(
            InterventionCommand,
        ) -> Result<InterventionCommandResult, InterventionError>,
    ) -> Result<ImInboundHandling, ImInboundError> {
        if let Some(existing) = self
            .repository
            .inbound_result(envelope.channel, &envelope.platform_event_id)?
        {
            let replay = match existing.outcome {
                ImInboundOutcome::Accepted => {
                    let ImDeliveryPayload::Intervention { ref reference, .. } = delivery.payload
                    else {
                        return Err(ImInboundError::InformationNotActionable);
                    };
                    let action = submitted_intervention_action(envelope, reference)?;
                    Some(ImResolutionOutcome::from(&action))
                }
                _ => None,
            };
            return Ok(ImInboundHandling {
                result: existing,
                resolution: replay,
            });
        }
        let ImDeliveryPayload::Intervention { ref reference, .. } = delivery.payload else {
            return Err(ImInboundError::InformationNotActionable);
        };
        if envelope.conversation_id != delivery.destination.conversation_id {
            return Err(ImInboundError::ConversationMismatch);
        }
        if envelope.actor_id != delivery.destination.authorized_actor_id {
            return Err(ImInboundError::ActorMismatch);
        }
        if let Some(existing) = self
            .repository
            .successful_inbound_result_for_canonical_event(
                envelope.channel,
                &delivery.canonical_event_id,
            )?
        {
            let result = ImInboundActionResult {
                action_id: envelope.action_id.clone(),
                channel: envelope.channel,
                canonical_event_id: Some(delivery.canonical_event_id.clone()),
                actor_id_hash: blake3::hash(envelope.actor_id.as_bytes())
                    .to_hex()
                    .to_string(),
                platform_event_id: envelope.platform_event_id.clone(),
                outcome: ImInboundOutcome::AlreadyApplied,
                result_revision: existing.result_revision,
                received_at_ms: now_ms,
                completed_at_ms: now_ms,
            };
            self.repository.record_inbound_result(&result)?;
            return Ok(ImInboundHandling {
                result,
                resolution: None,
            });
        }
        if let Some(action_token) = &envelope.action_token {
            let claims = self.token_codec.decode(action_token)?;
            if claims.expires_at_ms <= now_ms {
                return Err(ImInboundError::Expired);
            }
            if claims.delivery_id != delivery.delivery_id
                || claims.canonical_event_id != delivery.canonical_event_id
                || claims.channel != delivery.channel
                || claims.destination_id != delivery.destination.destination_id
            {
                return Err(ImInboundError::TokenInvalid);
            }
        }
        if delivery.expires_at_ms <= now_ms {
            return Err(ImInboundError::Expired);
        }
        let action = submitted_intervention_action(envelope, reference)?;
        let resolution = ImResolutionOutcome::from(&action);
        let command = InterventionCommand {
            locator: reference.locator.clone(),
            request: reference.request.clone(),
            expected_state: reference.expected_state.clone(),
            action,
            expires_at_ms: reference.expires_at_ms,
        };
        let command_result = match execute(command) {
            Ok(result) => result,
            Err(error)
                if matches!(
                    error.code,
                    InterventionErrorCode::InterventionAlreadyHandled
                        | InterventionErrorCode::InterventionRevisionConflict
                ) =>
            {
                InterventionCommandResult {
                    status: InterventionCommandStatus::AlreadyApplied,
                    canonical_state: error.code.as_str().to_owned(),
                }
            }
            Err(error) => return Err(error.into()),
        };
        let result = ImInboundActionResult {
            action_id: envelope.action_id.clone(),
            channel: envelope.channel,
            canonical_event_id: Some(delivery.canonical_event_id.clone()),
            actor_id_hash: blake3::hash(envelope.actor_id.as_bytes())
                .to_hex()
                .to_string(),
            platform_event_id: envelope.platform_event_id.clone(),
            outcome: match command_result.status {
                InterventionCommandStatus::Accepted => ImInboundOutcome::Accepted,
                InterventionCommandStatus::AlreadyApplied => ImInboundOutcome::AlreadyApplied,
            },
            result_revision: None,
            received_at_ms: now_ms,
            completed_at_ms: now_ms,
        };
        self.repository.record_inbound_result(&result)?;
        let accepted = result.outcome == ImInboundOutcome::Accepted;
        Ok(ImInboundHandling {
            result,
            resolution: accepted.then_some(resolution),
        })
    }
}

/// Maps a decoded submission to the domain action published by this delivery.
/// Pure: the same envelope and delivery always yield the same action.
fn submitted_intervention_action(
    envelope: &ImInboundEnvelope,
    reference: &InterventionRef,
) -> Result<InterventionAction, ImInboundError> {
    let action = match &envelope.action {
        ImSubmittedAction::PublishedAction { index } => {
            if reference.allowed_actions.iter().any(|action| {
                matches!(
                    action,
                    InterventionAllowedAction::ElicitationFixedForm { .. }
                )
            }) {
                return Err(ImInboundError::ActionNotAllowed);
            }
            let allowed = reference
                .allowed_actions
                .get(*index)
                .ok_or(ImInboundError::ActionNotAllowed)?;
            intervention_action_for_allowed(allowed)
        }
        ImSubmittedAction::PermissionOption { option_id } => InterventionAction::PermissionOption {
            option_id: option_id.clone(),
        },
        ImSubmittedAction::ManualCheck { outcome } => {
            InterventionAction::ManualCheck { outcome: *outcome }
        }
        ImSubmittedAction::ElicitationDecline => InterventionAction::Elicitation {
            action: ElicitationAction::Decline,
            content: None,
        },
        ImSubmittedAction::ElicitationForm { answers } => {
            let mut forms = reference
                .allowed_actions
                .iter()
                .filter_map(|action| match action {
                    InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
                    _ => None,
                });
            let Some(form) = forms.next() else {
                return Err(ImInboundError::ActionNotAllowed);
            };
            if forms.next().is_some() {
                return Err(ImInboundError::ActionIdentityInvalid);
            }
            let answers = normalized_form_answers(form, answers)?;
            intervention_action_for_elicitation_form(form, &answers)
                .map_err(ImInboundError::Form)?
        }
    };
    if !intervention_action_is_allowed(&reference.allowed_actions, &action) {
        return Err(ImInboundError::ActionNotAllowed);
    }
    Ok(action)
}

/// Normalizes transport form values against the fitted form persisted in the
/// delivery. String selections, omitted empty multi-selects, and the separate
/// custom-answer input are converted to the domain answer contract.
fn normalized_form_answers(
    form: &crate::app::intervention::RemoteElicitationForm,
    answers: &BTreeMap<String, RemoteElicitationAnswer>,
) -> Result<BTreeMap<String, RemoteElicitationAnswer>, ImInboundError> {
    let allowed_keys = form
        .questions
        .iter()
        .flat_map(|question| {
            let selector = question.selector_key.as_str();
            let custom = question
                .custom_answer_key
                .is_some()
                .then(|| format!("{selector}_other"));
            [Some(selector.to_owned()), custom]
        })
        .collect::<Vec<_>>();
    if answers.keys().any(|key| {
        !allowed_keys
            .iter()
            .any(|allowed| allowed.as_deref() == Some(key.as_str()))
    }) {
        return Err(ImInboundError::Form(RemoteFormViolation::AnswerInvalid));
    }

    let mut normalized = BTreeMap::new();
    for question in &form.questions {
        let selector = question.selector_key.as_str();
        match question.question.question_kind {
            crate::app::intervention::ElicitationQuestionKind::FreeText => {
                if let Some(answer) = answers.get(selector) {
                    let RemoteElicitationAnswer::Text { .. } = answer else {
                        return Err(ImInboundError::Form(RemoteFormViolation::AnswerInvalid));
                    };
                    normalized.insert(selector.to_owned(), answer.clone());
                }
            }
            crate::app::intervention::ElicitationQuestionKind::SingleSelect
            | crate::app::intervention::ElicitationQuestionKind::MultiSelect => {
                let selected = match answers.get(selector) {
                    None => Vec::new(),
                    Some(RemoteElicitationAnswer::Text { text }) => vec![text.clone()],
                    Some(RemoteElicitationAnswer::Choice {
                        option_ids,
                        custom_text: None,
                    }) => option_ids.clone(),
                    Some(_) => {
                        return Err(ImInboundError::Form(RemoteFormViolation::AnswerInvalid));
                    }
                };
                let custom_answer = if question.custom_answer_key.is_some() {
                    match answers.get(&format!("{selector}_other")) {
                        None => None,
                        Some(RemoteElicitationAnswer::Text { text: custom }) => {
                            Some(custom.clone())
                        }
                        Some(_) => {
                            return Err(ImInboundError::Form(RemoteFormViolation::AnswerInvalid));
                        }
                    }
                } else {
                    None
                };
                if selected
                    .iter()
                    .any(|option| option == REMOTE_CUSTOM_ANSWER_OPTION)
                {
                    if selected.len() != 1 {
                        return Err(ImInboundError::Form(RemoteFormViolation::OptionInvalid));
                    }
                    let text = custom_answer.filter(|text| !text.trim().is_empty()).ok_or(
                        ImInboundError::Form(RemoteFormViolation::CustomAnswerMissing),
                    )?;
                    normalized.insert(
                        selector.to_owned(),
                        RemoteElicitationAnswer::Choice {
                            option_ids: Vec::new(),
                            custom_text: Some(text),
                        },
                    );
                } else {
                    normalized.insert(
                        selector.to_owned(),
                        RemoteElicitationAnswer::Choice {
                            option_ids: selected,
                            custom_text: None,
                        },
                    );
                }
            }
        }
    }
    Ok(normalized)
}

pub fn deterministic_action_id(channel: ImChannelKind, platform_event_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(channel.as_str().as_bytes());
    hasher.update(&(platform_event_id.len() as u64).to_be_bytes());
    hasher.update(platform_event_id.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    use super::*;
    use crate::acp::permission::write_pending_permission;
    use crate::app::App;
    use crate::app::intervention::{
        ElicitationQuestion, ElicitationQuestionKind, ElicitationQuestionOption,
        InterventionCommandService, InterventionLocator, InterventionRequestIdentity,
        MAX_REMOTE_TEXT_CHARS, RemoteElicitationAnswer, RemoteElicitationForm, RemoteQuestion,
        RemoteQuestionConstraints,
    };
    use crate::domain::{NodeType, PauseReason, RunStatus, VERSION};
    use crate::im::{
        IM_PAYLOAD_VERSION, ImDelivery, ImDestination, ImNavigationLocator, ImNotificationKind,
        ImResolutionOutcome, ImResolvedBy, InformationalNotification, InterventionPresentation,
        InterventionRef, InterventionResolutionNotification, resolution_event_id,
    };
    use crate::runtime::{CURRENT_ACP_STORAGE_SCHEMA_VERSION, NodeState, RunState};
    use crate::storage::write_json;

    struct Fixture {
        _temp: TempDir,
        app: App,
        repository: Arc<ImRepository>,
        service: ImInboundActionService,
        delivery: ImDelivery,
        envelope: ImInboundEnvelope,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.app.paths.runtime_root.as_std_path());
        }
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Utf8PathBuf::from_path_buf(temp.path().join("repo")).unwrap());
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
        write_json(
            &app.paths.run_file(&locator.task_id, &locator.run_id),
            &RunState {
                version: VERSION.into(),
                id: locator.run_id.clone(),
                task_id: locator.task_id.clone(),
                task_uuid: None,
                status: RunStatus::Paused,
                outcome: None,
                started_at: "2026-08-30T00:00:00Z".into(),
                updated_at: "2026-08-30T00:00:01Z".into(),
                workflow_snapshot: "workflow.snapshot.json".into(),
                current_round: Some(locator.round_id.clone()),
                current_node: Some(locator.node_id.clone()),
                current_attempt: Some(locator.attempt_id.clone()),
                new_rounds_opened: 0,
                pause_reason: Some(PauseReason::PermissionRequested),
                uuid: None,
                last_executed_node: None,
                worktree: None,
                execution: Default::default(),
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
        write_json(
            &attempt_dir.join("node.json"),
            &NodeState {
                version: VERSION.into(),
                acp_storage_schema_version: CURRENT_ACP_STORAGE_SCHEMA_VERSION,
                node_id: locator.node_id.clone(),
                node_type: NodeType::Worker,
                run_id: locator.run_id.clone(),
                round_id: locator.round_id.clone(),
                attempt_id: locator.attempt_id.clone(),
                status: RunStatus::Paused,
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
        write_pending_permission(
            &attempt_dir,
            "request-1",
            "turn-1",
            "prompt-event-1",
            serde_json::json!({
                "options": [
                    {
                        "optionId": "allow",
                        "name": "Allow",
                        "kind": "allow_once"
                    }
                ]
            }),
            "2026-08-30T00:00:01Z".into(),
        )
        .unwrap();
        let request = InterventionRequestIdentity::Permission {
            request_id: "request-1".into(),
        };
        let snapshot = InterventionCommandService::new(&app)
            .inspect(locator.clone(), request.clone())
            .unwrap();
        let destination = ImDestination {
            destination_id: "user-1".into(),
            conversation_id: "private-chat-1".into(),
            authorized_actor_id: "actor-1".into(),
        };
        let channel = ImChannelKind::WeCom;
        let kind = ImNotificationKind::Permission;
        let delivery = ImDelivery {
            delivery_id: ImDelivery::deterministic_id(channel, "user-1", kind, "event-1"),
            channel,
            destination,
            notification_kind: kind,
            canonical_event_id: "event-1".into(),
            payload: ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: InterventionRef {
                    locator,
                    request,
                    expected_state: snapshot.expected_state,
                    allowed_actions: snapshot.allowed_actions,
                    expires_at_ms: None,
                },
                presentation: InterventionPresentation {
                    title_key: "im.notification.permission.title".into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: None,
                    summary: None,
                    body: None,
                    context: None,
                    fields: Default::default(),
                    questions: Vec::new(),
                },
            },
            expires_at_ms: 10_000,
            display_ref: None,
        };
        let repository = Arc::new(ImRepository::new(
            Utf8PathBuf::from_path_buf(temp.path().join("core.db")).unwrap(),
        ));
        repository.enqueue(&delivery, 100).unwrap();
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let token = codec
            .encode(&ImActionTokenClaims {
                version: ACTION_TOKEN_VERSION,
                delivery_id: delivery.delivery_id.clone(),
                canonical_event_id: delivery.canonical_event_id.clone(),
                channel,
                destination_id: delivery.destination.destination_id.clone(),
                expires_at_ms: 10_000,
            })
            .unwrap();
        let platform_event_id = "callback-1".to_string();
        let envelope = ImInboundEnvelope {
            action_id: deterministic_action_id(channel, &platform_event_id),
            channel,
            platform_event_id,
            conversation_id: "private-chat-1".into(),
            actor_id: "actor-1".into(),
            delivery_id: delivery.delivery_id.clone(),
            action_token: Some(token),
            action: ImSubmittedAction::PermissionOption {
                option_id: "allow".into(),
            },
        };
        let service = ImInboundActionService::new(Arc::clone(&repository), codec);
        Fixture {
            _temp: temp,
            app,
            repository,
            service,
            delivery,
            envelope,
        }
    }

    #[test]
    fn duplicate_platform_callback_and_action_id_execute_runtime_once() {
        let fixture = fixture();
        let executions = AtomicUsize::new(0);
        let first = fixture
            .service
            .handle(&fixture.envelope, 100, |command| {
                executions.fetch_add(1, Ordering::SeqCst);
                InterventionCommandService::new(&fixture.app).execute(command)
            })
            .unwrap();
        let second = fixture
            .service
            .handle(&fixture.envelope, 101, |_| {
                panic!("duplicate callback must use the inbound audit result")
            })
            .unwrap();
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(first, second);
        assert_eq!(
            first.resolution,
            Some(ImResolutionOutcome::PermissionOption {
                option_id: "allow".into()
            })
        );

        let mut late_click = fixture.envelope.clone();
        late_click.platform_event_id = "callback-2".into();
        late_click.action_id =
            deterministic_action_id(late_click.channel, &late_click.platform_event_id);
        let late_result = fixture
            .service
            .handle(&late_click, 102, |_| {
                panic!("a new msgid for the same intervention must not execute Runtime")
            })
            .unwrap();
        assert_eq!(late_result.result.outcome, ImInboundOutcome::AlreadyApplied);
        assert_eq!(late_result.resolution, None);
        assert_eq!(
            fixture
                .repository
                .inbound_result(ImChannelKind::WeCom, "callback-2")
                .unwrap()
                .map(|result| result.outcome),
            Some(ImInboundOutcome::AlreadyApplied)
        );
    }

    #[test]
    fn concurrent_redelivery_through_a_shared_service_executes_once() {
        let fixture = fixture();
        let executions = AtomicUsize::new(0);
        let barrier = std::sync::Barrier::new(2);
        let handle = || {
            barrier.wait();
            fixture
                .service
                .handle(&fixture.envelope, 100, |command| {
                    executions.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    InterventionCommandService::new(&fixture.app).execute(command)
                })
                .unwrap()
        };
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(handle);
            let second = scope.spawn(handle);
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(first, second);
        assert_eq!(first.result.outcome, ImInboundOutcome::Accepted);
    }

    #[test]
    fn published_action_index_resolves_from_authoritative_outbox() {
        let fixture = fixture();
        let mut envelope = fixture.envelope.clone();
        envelope.platform_event_id = "local-index-callback".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        envelope.action_token = None;
        envelope.action = ImSubmittedAction::PublishedAction { index: 0 };

        let result = fixture
            .service
            .handle(&envelope, 100, |command| {
                InterventionCommandService::new(&fixture.app).execute(command)
            })
            .unwrap();
        assert_eq!(result.result.outcome, ImInboundOutcome::Accepted);

        let mut invalid = envelope.clone();
        invalid.platform_event_id = "local-index-invalid-callback".into();
        invalid.action_id = deterministic_action_id(invalid.channel, &invalid.platform_event_id);
        invalid.action = ImSubmittedAction::PublishedAction { index: 1 };
        let late_invalid = fixture
            .service
            .handle(&invalid, 100, |_| unreachable!())
            .unwrap();
        assert_eq!(
            late_invalid.result.outcome,
            ImInboundOutcome::AlreadyApplied
        );
    }

    #[test]
    fn unpublished_actions_and_tampered_tokens_are_rejected_before_runtime() {
        let fixture = fixture();
        let mut unpublished = fixture.envelope.clone();
        unpublished.action = ImSubmittedAction::PermissionOption {
            option_id: "reject".into(),
        };
        assert!(matches!(
            fixture
                .service
                .handle(&unpublished, 100, |_| unreachable!()),
            Err(ImInboundError::ActionNotAllowed)
        ));
        let mut out_of_range = fixture.envelope.clone();
        out_of_range.action = ImSubmittedAction::PublishedAction { index: 9 };
        assert!(matches!(
            fixture
                .service
                .handle(&out_of_range, 100, |_| unreachable!()),
            Err(ImInboundError::ActionNotAllowed)
        ));
        let mut tampered = fixture.envelope.clone();
        tampered.action_token = Some("forged.token".into());
        let error = fixture
            .service
            .handle(&tampered, 100, |_| unreachable!())
            .unwrap_err();
        assert_eq!(error.code(), ImErrorCode::ActionTokenInvalid);
        let mut missing = fixture.envelope.clone();
        missing.delivery_id = "missing-delivery".into();
        let error = fixture
            .service
            .handle(&missing, 100, |_| unreachable!())
            .unwrap_err();
        assert_eq!(error.code(), ImErrorCode::DeliveryNotFound);
        let error = fixture
            .service
            .handle(&fixture.envelope, 10_000, |_| unreachable!())
            .unwrap_err();
        assert_eq!(error.code(), ImErrorCode::ActionExpired);
    }

    #[test]
    fn form_selection_restores_typed_content_from_the_outbox() {
        let fixture = fixture();
        let mut delivery = fixture.delivery.clone();
        delivery.notification_kind = ImNotificationKind::Elicitation;
        delivery.canonical_event_id = "elicitation-event-1".into();
        delivery.refresh_id();
        let ImDeliveryPayload::Intervention {
            reference,
            presentation,
            ..
        } = &mut delivery.payload
        else {
            unreachable!();
        };
        reference.request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-1".into(),
        };
        reference.allowed_actions = vec![InterventionAllowedAction::ElicitationFixedForm {
            form: RemoteElicitationForm {
                questions: vec![RemoteQuestion {
                    selector_key: "q0".into(),
                    question: ElicitationQuestion {
                        field_name: "features".into(),
                        title: "功能".into(),
                        description: None,
                        question_kind: ElicitationQuestionKind::MultiSelect,
                        options: vec![
                            ElicitationQuestionOption {
                                value: serde_json::json!("auth"),
                                label: "认证".into(),
                                description: None,
                            },
                            ElicitationQuestionOption {
                                value: serde_json::json!("logging"),
                                label: "日志".into(),
                                description: None,
                            },
                        ],
                        allows_custom_answer: false,
                    },
                    constraints: RemoteQuestionConstraints {
                        required: true,
                        allows_empty: false,
                        max_chars: MAX_REMOTE_TEXT_CHARS,
                        multiline: false,
                    },
                    custom_answer_key: None,
                }],
                decline_allowed: false,
            },
        }];
        presentation.questions = Vec::new();
        fixture.repository.enqueue(&delivery, 100).unwrap();

        let mut envelope = fixture.envelope.clone();
        envelope.delivery_id = delivery.delivery_id.clone();
        envelope.platform_event_id = "form-callback-1".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        envelope.action_token = None;
        envelope.action = ImSubmittedAction::ElicitationForm {
            answers: BTreeMap::from([(
                "q0".to_string(),
                RemoteElicitationAnswer::Choice {
                    option_ids: vec!["1".into(), "0".into()],
                    custom_text: None,
                },
            )]),
        };
        let mut seen_content = None;
        let result = fixture
            .service
            .handle_with_delivery(&envelope, &delivery, 100, |command| {
                let InterventionAction::Elicitation { content, .. } = &command.action else {
                    panic!("expected elicitation action");
                };
                seen_content = content.clone();
                Ok(InterventionCommandResult {
                    status: InterventionCommandStatus::Accepted,
                    canonical_state: "accepted".into(),
                })
            })
            .unwrap();
        assert_eq!(result.result.outcome, ImInboundOutcome::Accepted);
        assert_eq!(
            result.resolution,
            Some(ImResolutionOutcome::ElicitationSubmitted {
                content: serde_json::json!({"features": ["logging", "auth"]})
            })
        );
        assert_eq!(
            seen_content,
            Some(serde_json::json!({"features": ["logging", "auth"]}))
        );

        let mut duplicate = envelope.clone();
        duplicate.platform_event_id = "form-callback-duplicate".into();
        duplicate.action_id =
            deterministic_action_id(duplicate.channel, &duplicate.platform_event_id);
        duplicate.action = ImSubmittedAction::ElicitationForm {
            answers: BTreeMap::from([(
                "q0".to_string(),
                RemoteElicitationAnswer::Choice {
                    option_ids: vec!["0".into()],
                    custom_text: None,
                },
            )]),
        };
        let duplicate_result = fixture
            .service
            .handle_with_delivery(&duplicate, &delivery, 100, |_| unreachable!())
            .unwrap();
        assert_eq!(
            duplicate_result.result.outcome,
            ImInboundOutcome::AlreadyApplied
        );
    }

    #[test]
    fn feishu_rendered_decline_reaches_the_domain_once_and_requires_permission() {
        use crate::im::connectors::feishu::card::{decode_card_action, delivery_card_content};
        let fixture = fixture();
        let mut delivery = fixture.delivery.clone();
        delivery.channel = ImChannelKind::Feishu;
        delivery.notification_kind = ImNotificationKind::Elicitation;
        delivery.canonical_event_id = "feishu-decline-event".into();
        delivery.refresh_id();
        let ImDeliveryPayload::Intervention { reference, .. } = &mut delivery.payload else {
            unreachable!()
        };
        reference.request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-1".into(),
        };
        reference.allowed_actions = vec![
            InterventionAllowedAction::ElicitationFixedForm {
                form: RemoteElicitationForm {
                    questions: vec![],
                    decline_allowed: true,
                },
            },
            InterventionAllowedAction::ElicitationDecline,
        ];
        fixture.repository.enqueue(&delivery, 100).unwrap();
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let card: serde_json::Value = serde_json::from_str(
            &delivery_card_content(&delivery, &codec, crate::im::ImLocale::ZhCn).unwrap(),
        )
        .unwrap();
        let value = card["body"]["elements"][2]["behaviors"][0]["value"].clone();
        let callback = serde_json::json!({
            "header": { "event_type": "card.action.trigger", "event_id": "decline-callback", "app_id": "cli_test" },
            "event": {
                "operator": { "open_id": delivery.destination.authorized_actor_id },
                "context": { "open_chat_id": delivery.destination.conversation_id },
                "action": { "value": value }
            }
        });
        let envelope = decode_card_action(&serde_json::to_vec(&callback).unwrap())
            .unwrap()
            .envelope;
        let mut denied = delivery.clone();
        let ImDeliveryPayload::Intervention { reference, .. } = &mut denied.payload else {
            unreachable!()
        };
        reference.allowed_actions.pop();
        assert!(matches!(
            submitted_intervention_action(&envelope, reference),
            Err(ImInboundError::ActionNotAllowed)
        ));
        let executions = AtomicUsize::new(0);
        let result = fixture
            .service
            .handle(&envelope, 100, |command| {
                assert_eq!(
                    command.action,
                    InterventionAction::Elicitation {
                        action: ElicitationAction::Decline,
                        content: None
                    }
                );
                executions.fetch_add(1, Ordering::SeqCst);
                Ok(InterventionCommandResult {
                    status: InterventionCommandStatus::Accepted,
                    canonical_state: "declined".into(),
                })
            })
            .unwrap();
        assert_eq!(result.result.outcome, ImInboundOutcome::Accepted);
        assert_eq!(
            result.resolution,
            Some(ImResolutionOutcome::ElicitationDeclined)
        );
        let mut duplicate = envelope;
        duplicate.platform_event_id = "second-decline-callback".into();
        duplicate.action_id =
            deterministic_action_id(duplicate.channel, &duplicate.platform_event_id);
        let result = fixture
            .service
            .handle(&duplicate, 100, |_| panic!("late decline must not execute"))
            .unwrap();
        assert_eq!(result.result.outcome, ImInboundOutcome::AlreadyApplied);
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn remote_submissions_can_never_resolve_an_elicitation_as_cancelled() {
        // `Cancel` is reserved for desktop/runtime stop and expiry paths.
        for kind in ["elicitationCancel", "elicitationCancelled", "cancel"] {
            assert!(
                serde_json::from_value::<ImSubmittedAction>(serde_json::json!({ "kind": kind }))
                    .is_err(),
                "{kind} must not be a remote submission"
            );
        }
        let cancel = InterventionAction::Elicitation {
            action: ElicitationAction::Cancel,
            content: None,
        };
        let allowed_actions = vec![
            InterventionAllowedAction::ElicitationFixedForm {
                form: RemoteElicitationForm {
                    questions: vec![],
                    decline_allowed: true,
                },
            },
            InterventionAllowedAction::ElicitationAccept,
            InterventionAllowedAction::ElicitationDecline,
            InterventionAllowedAction::ManualSuccess,
            InterventionAllowedAction::ManualFailure,
        ];
        assert!(!intervention_action_is_allowed(&allowed_actions, &cancel));
        for allowed in &allowed_actions {
            assert_ne!(intervention_action_for_allowed(allowed), cancel);
        }

        let fixture = fixture();
        let mut delivery = fixture.delivery.clone();
        delivery.notification_kind = ImNotificationKind::Elicitation;
        let ImDeliveryPayload::Intervention { reference, .. } = &mut delivery.payload else {
            unreachable!()
        };
        reference.request = InterventionRequestIdentity::Elicitation {
            elicitation_id: "elicit-1".into(),
        };
        reference.allowed_actions = vec![
            InterventionAllowedAction::ElicitationAccept,
            InterventionAllowedAction::ElicitationDecline,
        ];
        let reference = reference.clone();
        let mut envelope = fixture.envelope.clone();
        for action in [
            ImSubmittedAction::PublishedAction { index: 0 },
            ImSubmittedAction::PublishedAction { index: 1 },
            ImSubmittedAction::ElicitationDecline,
        ] {
            envelope.action = action;
            let submitted = submitted_intervention_action(&envelope, &reference).unwrap();
            assert_ne!(submitted, cancel);
            assert!(!ImResolutionOutcome::from(&submitted).is_cancelled());
        }
    }

    #[test]
    fn form_violations_map_to_structured_form_error_codes() {
        let fixture = fixture();
        let mut delivery = fixture.delivery.clone();
        let ImDeliveryPayload::Intervention { reference, .. } = &mut delivery.payload else {
            unreachable!();
        };
        reference.allowed_actions = vec![InterventionAllowedAction::ElicitationFixedForm {
            form: RemoteElicitationForm {
                questions: vec![RemoteQuestion {
                    selector_key: "q0".into(),
                    question: ElicitationQuestion {
                        field_name: "note".into(),
                        title: "备注".into(),
                        description: None,
                        question_kind: ElicitationQuestionKind::FreeText,
                        options: Vec::new(),
                        allows_custom_answer: false,
                    },
                    constraints: RemoteQuestionConstraints {
                        required: true,
                        allows_empty: false,
                        max_chars: 4,
                        multiline: false,
                    },
                    custom_answer_key: None,
                }],
                decline_allowed: false,
            },
        }];
        let submit = |answers: BTreeMap<String, RemoteElicitationAnswer>| {
            let mut envelope = fixture.envelope.clone();
            envelope.action_token = None;
            envelope.action = ImSubmittedAction::ElicitationForm { answers };
            fixture
                .service
                .handle_with_delivery(&envelope, &delivery, 100, |_| unreachable!())
                .unwrap_err()
                .code()
        };
        assert_eq!(submit(BTreeMap::new()), ImErrorCode::FormAnswerMissing);
        assert_eq!(
            submit(BTreeMap::from([(
                "q0".to_string(),
                RemoteElicitationAnswer::Text {
                    text: "too long".into()
                }
            )])),
            ImErrorCode::FormTextTooLong
        );
        let mut indexed = fixture.envelope.clone();
        indexed.action_token = None;
        indexed.action = ImSubmittedAction::PublishedAction { index: 0 };
        assert!(matches!(
            fixture
                .service
                .handle_with_delivery(&indexed, &delivery, 100, |_| unreachable!()),
            Err(ImInboundError::ActionNotAllowed)
        ));
    }

    #[test]
    fn transport_form_answers_are_normalized_from_the_delivery_form() {
        let question =
            |selector: &str, kind: ElicitationQuestionKind, custom: bool| RemoteQuestion {
                selector_key: selector.into(),
                question: ElicitationQuestion {
                    field_name: format!("field_{selector}"),
                    title: selector.into(),
                    description: None,
                    question_kind: kind,
                    options: Vec::new(),
                    allows_custom_answer: custom,
                },
                constraints: RemoteQuestionConstraints {
                    required: false,
                    allows_empty: true,
                    max_chars: 100,
                    multiline: false,
                },
                custom_answer_key: custom.then(|| format!("field_{selector}_custom")),
            };
        let form = RemoteElicitationForm {
            questions: vec![
                question("q0", ElicitationQuestionKind::SingleSelect, true),
                question("q1", ElicitationQuestionKind::MultiSelect, true),
                question("q2", ElicitationQuestionKind::FreeText, false),
            ],
            decline_allowed: false,
        };
        let text = |value: &str| RemoteElicitationAnswer::Text { text: value.into() };
        let answers = BTreeMap::from([
            ("q0".into(), text(REMOTE_CUSTOM_ANSWER_OPTION)),
            ("q0_other".into(), text(" Custom answer ")),
            (
                "q1".into(),
                RemoteElicitationAnswer::Choice {
                    option_ids: vec!["0".into()],
                    custom_text: None,
                },
            ),
            ("q1_other".into(), text("ignored unless other is selected")),
            ("q2".into(), text("note")),
        ]);

        let normalized = normalized_form_answers(&form, &answers).unwrap();
        assert_eq!(
            normalized.get("q0"),
            Some(&RemoteElicitationAnswer::Choice {
                option_ids: Vec::new(),
                custom_text: Some(" Custom answer ".into()),
            })
        );
        assert_eq!(
            normalized.get("q1"),
            Some(&RemoteElicitationAnswer::Choice {
                option_ids: vec!["0".into()],
                custom_text: None,
            })
        );
        assert_eq!(normalized.get("q2"), Some(&text("note")));

        let missing_multi =
            normalized_form_answers(&form, &BTreeMap::from([("q0".into(), text("0"))])).unwrap();
        assert_eq!(
            missing_multi.get("q1"),
            Some(&RemoteElicitationAnswer::Choice {
                option_ids: Vec::new(),
                custom_text: None,
            })
        );

        let empty_custom = normalized_form_answers(
            &form,
            &BTreeMap::from([
                ("q0".into(), text(REMOTE_CUSTOM_ANSWER_OPTION)),
                ("q0_other".into(), text(" ")),
            ]),
        )
        .unwrap_err();
        assert_eq!(empty_custom.code(), ImErrorCode::FormCustomAnswerMissing);

        let unknown =
            normalized_form_answers(&form, &BTreeMap::from([("q9".into(), text("unexpected"))]))
                .unwrap_err();
        assert_eq!(unknown.code(), ImErrorCode::ActionInvalid);
    }

    #[test]
    fn runtime_success_before_audit_write_recovers_as_already_applied() {
        let fixture = fixture();
        let ImDeliveryPayload::Intervention { reference, .. } = &fixture.delivery.payload else {
            unreachable!()
        };
        InterventionCommandService::new(&fixture.app)
            .execute(InterventionCommand {
                locator: reference.locator.clone(),
                request: reference.request.clone(),
                expected_state: reference.expected_state.clone(),
                action: InterventionAction::PermissionOption {
                    option_id: "allow".into(),
                },
                expires_at_ms: None,
            })
            .unwrap();
        assert!(
            fixture
                .repository
                .inbound_result(ImChannelKind::WeCom, "callback-1")
                .unwrap()
                .is_none()
        );
        let recovered = fixture
            .service
            .handle(&fixture.envelope, 101, |command| {
                InterventionCommandService::new(&fixture.app).execute(command)
            })
            .unwrap();
        assert_eq!(recovered.result.outcome, ImInboundOutcome::AlreadyApplied);
    }

    #[test]
    fn actor_and_private_conversation_must_match_the_binding() {
        let fixture = fixture();
        let mut wrong_actor = fixture.envelope.clone();
        wrong_actor.actor_id = "other-actor".into();
        assert!(matches!(
            fixture
                .service
                .handle(&wrong_actor, 100, |_| unreachable!()),
            Err(ImInboundError::ActorMismatch)
        ));
        let mut wrong_chat = fixture.envelope.clone();
        wrong_chat.conversation_id = "group-chat".into();
        assert!(matches!(
            fixture.service.handle(&wrong_chat, 100, |_| unreachable!()),
            Err(ImInboundError::ConversationMismatch)
        ));
    }

    #[test]
    fn non_actionable_deliveries_reject_forged_inbound_actions() {
        let fixture = fixture();
        let mut delivery = fixture.delivery.clone();
        delivery.notification_kind = ImNotificationKind::RunFailure;
        delivery.canonical_event_id = "information-event".into();
        delivery.refresh_id();
        delivery.payload = ImDeliveryPayload::Information {
            version: IM_PAYLOAD_VERSION,
            notification: InformationalNotification {
                canonical_event_id: delivery.canonical_event_id.clone(),
                notification_kind: ImNotificationKind::RunFailure,
                locator: ImNavigationLocator {
                    project_id: fixture.app.paths.project_id.clone(),
                    task_id: Some("task-1".into()),
                    run_id: Some("run-1".into()),
                    round_id: None,
                    node_id: None,
                    attempt_id: None,
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                outcome: "failure".into(),
                summary_key: "im.notification.run_failure.summary".into(),
                parameters: Default::default(),
                missed_count: None,
            },
        };
        fixture.repository.enqueue(&delivery, 100).unwrap();
        let mut envelope = fixture.envelope.clone();
        envelope.delivery_id = delivery.delivery_id.clone();
        envelope.platform_event_id = "forged-information-callback".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        assert!(matches!(
            fixture.service.handle(&envelope, 100, |_| unreachable!(
                "information must not reach Runtime"
            )),
            Err(ImInboundError::InformationNotActionable)
        ));

        delivery.notification_kind = ImNotificationKind::Permission;
        delivery.canonical_event_id = resolution_event_id("event-1");
        delivery.refresh_id();
        delivery.payload = ImDeliveryPayload::InterventionResolution {
            version: IM_PAYLOAD_VERSION,
            resolution: InterventionResolutionNotification {
                canonical_event_id: delivery.canonical_event_id.clone(),
                source_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::Permission,
                locator: ImNavigationLocator {
                    project_id: fixture.app.paths.project_id.clone(),
                    task_id: Some("task-1".into()),
                    run_id: Some("run-1".into()),
                    round_id: Some("round-1".into()),
                    node_id: Some("node-1".into()),
                    attempt_id: Some("attempt-1".into()),
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                display_ref: Some(1),
                resolved_by: ImResolvedBy::Desktop,
                outcome: ImResolutionOutcome::PermissionOption {
                    option_id: "allow".into(),
                },
            },
        };
        fixture.repository.enqueue(&delivery, 100).unwrap();
        envelope.delivery_id = delivery.delivery_id;
        envelope.platform_event_id = "forged-resolution-callback".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        assert!(matches!(
            fixture.service.handle(&envelope, 100, |_| unreachable!(
                "desktop resolution must not reach Runtime"
            )),
            Err(ImInboundError::InformationNotActionable)
        ));
    }
}
