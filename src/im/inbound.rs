use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::app::intervention::{
    InterventionAllowedAction, InterventionCommand, InterventionCommandResult,
    InterventionCommandStatus, InterventionError, InterventionErrorCode,
    intervention_action_for_allowed, intervention_action_for_elicitation_form,
    intervention_action_is_allowed,
};

use super::{
    ImChannelKind, ImDelivery, ImDeliveryPayload, ImInboundActionResult, ImInboundActionSelection,
    ImInboundEnvelope, ImRepository, ImRepositoryError,
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
    #[error("action identity is invalid")]
    ActionIdentityInvalid,
    #[error("action was not published for this delivery")]
    ActionNotAllowed,
    #[error("IM repository failed")]
    Repository(#[from] ImRepositoryError),
    #[error("Runtime intervention failed")]
    Intervention(#[from] InterventionError),
}

impl ImInboundError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TokenInvalid => "IM_ACTION_TOKEN_INVALID",
            Self::Expired => "IM_ACTION_EXPIRED",
            Self::InformationNotActionable => "IM_INFORMATION_NOT_ACTIONABLE",
            Self::ConversationMismatch => "IM_CONVERSATION_MISMATCH",
            Self::ActorMismatch => "IM_ACTOR_MISMATCH",
            Self::ActionIdentityInvalid => "IM_ACTION_ID_INVALID",
            Self::ActionNotAllowed => "IM_ACTION_INVALID",
            Self::Repository(error) => error.code(),
            Self::Intervention(error) => error.code.as_str(),
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
    ) -> Result<ImInboundActionResult, ImInboundError> {
        let delivery = self
            .repository
            .delivery(&envelope.delivery_id)?
            .ok_or(ImInboundError::TokenInvalid)?;
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
    ) -> Result<ImInboundActionResult, ImInboundError> {
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
    ) -> Result<ImInboundActionResult, ImInboundError> {
        if let Some(existing) = self
            .repository
            .inbound_result(envelope.channel, &envelope.platform_event_id)?
        {
            return Ok(existing);
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
                result_code: "ALREADY_APPLIED".into(),
                result_revision: existing.result_revision,
                received_at_ms: now_ms,
                completed_at_ms: now_ms,
            };
            self.repository.record_inbound_result(&result)?;
            return Ok(result);
        }
        let action = match &envelope.action {
            ImInboundActionSelection::Resolved {
                action_token,
                action,
            } => {
                let claims = self.token_codec.decode(action_token)?;
                if claims.expires_at_ms <= now_ms || delivery.expires_at_ms <= now_ms {
                    return Err(ImInboundError::Expired);
                }
                if claims.delivery_id != delivery.delivery_id
                    || claims.canonical_event_id != delivery.canonical_event_id
                    || claims.channel != delivery.channel
                    || claims.destination_id != delivery.destination.destination_id
                {
                    return Err(ImInboundError::TokenInvalid);
                }
                if !intervention_action_is_allowed(&reference.allowed_actions, action) {
                    return Err(ImInboundError::ActionNotAllowed);
                }
                action.clone()
            }
            ImInboundActionSelection::LocalIndex { index } => {
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
            ImInboundActionSelection::Form { selections } => {
                let mut forms =
                    reference
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
                intervention_action_for_elicitation_form(form, selections)
                    .map_err(ImInboundError::Intervention)?
            }
        };
        if delivery.expires_at_ms <= now_ms {
            return Err(ImInboundError::Expired);
        }
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
            result_code: match command_result.status {
                InterventionCommandStatus::Accepted => "ACCEPTED",
                InterventionCommandStatus::AlreadyApplied => "ALREADY_APPLIED",
            }
            .into(),
            result_revision: None,
            received_at_ms: now_ms,
            completed_at_ms: now_ms,
        };
        self.repository.record_inbound_result(&result)?;
        Ok(result)
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    use super::*;
    use crate::acp::permission::write_pending_permission;
    use crate::app::App;
    use crate::app::intervention::{
        InterventionAction, InterventionCommandService, InterventionLocator,
        InterventionRequestIdentity, RemoteElicitationForm, RemoteElicitationFormSelection,
        RemoteElicitationOption, RemoteScalarChoiceQuestion,
    };
    use crate::domain::{NodeType, PauseReason, RunStatus, VERSION};
    use crate::im::{
        IM_PAYLOAD_VERSION, ImDelivery, ImDestination, ImNavigationLocator, ImNotificationKind,
        InformationalNotification, InterventionPresentation, InterventionRef,
        InterventionResolutionNotification,
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
            action: ImInboundActionSelection::Resolved {
                action_token: token,
                action: InterventionAction::PermissionOption {
                    option_id: "allow".into(),
                },
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
        assert_eq!(late_result.result_code, "ALREADY_APPLIED");
        assert_eq!(
            fixture
                .repository
                .inbound_result(ImChannelKind::WeCom, "callback-2")
                .unwrap()
                .map(|result| result.result_code),
            Some("ALREADY_APPLIED".to_owned())
        );
    }

    #[test]
    fn local_index_action_resolves_from_authoritative_outbox() {
        let fixture = fixture();
        let mut envelope = fixture.envelope.clone();
        envelope.platform_event_id = "local-index-callback".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        envelope.action = ImInboundActionSelection::LocalIndex { index: 0 };

        let result = fixture
            .service
            .handle(&envelope, 100, |command| {
                InterventionCommandService::new(&fixture.app).execute(command)
            })
            .unwrap();
        assert_eq!(result.result_code, "ACCEPTED");

        let mut invalid = envelope.clone();
        invalid.platform_event_id = "local-index-invalid-callback".into();
        invalid.action_id = deterministic_action_id(invalid.channel, &invalid.platform_event_id);
        invalid.action = ImInboundActionSelection::LocalIndex { index: 1 };
        let late_invalid = fixture
            .service
            .handle(&invalid, 100, |_| unreachable!())
            .unwrap();
        assert_eq!(late_invalid.result_code, "ALREADY_APPLIED");
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
            form: RemoteElicitationForm::MultiScalarChoice {
                question: RemoteScalarChoiceQuestion {
                    selector_key: "q0".into(),
                    field_name: "features".into(),
                    title: "功能".into(),
                    description: None,
                    required: true,
                    options: vec![
                        RemoteElicitationOption {
                            value: serde_json::json!("auth"),
                            label: "认证".into(),
                            description: None,
                        },
                        RemoteElicitationOption {
                            value: serde_json::json!("logging"),
                            label: "日志".into(),
                            description: None,
                        },
                    ],
                },
                allows_empty: true,
            },
        }];
        presentation.questions = Vec::new();
        fixture.repository.enqueue(&delivery, 100).unwrap();

        let mut envelope = fixture.envelope.clone();
        envelope.delivery_id = delivery.delivery_id.clone();
        envelope.platform_event_id = "form-callback-1".into();
        envelope.action_id = deterministic_action_id(envelope.channel, &envelope.platform_event_id);
        envelope.action = ImInboundActionSelection::Form {
            selections: vec![RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["1".into(), "0".into()],
            }],
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
        assert_eq!(result.result_code, "ACCEPTED");
        assert_eq!(
            seen_content,
            Some(serde_json::json!({"features": ["logging", "auth"]}))
        );

        let mut duplicate = envelope.clone();
        duplicate.platform_event_id = "form-callback-duplicate".into();
        duplicate.action_id =
            deterministic_action_id(duplicate.channel, &duplicate.platform_event_id);
        duplicate.action = ImInboundActionSelection::Form {
            selections: vec![RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["0".into(), "0".into()],
            }],
        };
        let duplicate_result = fixture
            .service
            .handle_with_delivery(&duplicate, &delivery, 100, |_| unreachable!())
            .unwrap();
        assert_eq!(duplicate_result.result_code, "ALREADY_APPLIED");
    }

    #[test]
    fn runtime_success_before_audit_write_recovers_as_already_applied() {
        let fixture = fixture();
        let ImDeliveryPayload::Intervention { reference, .. } = &fixture.delivery.payload else {
            unreachable!()
        };
        let ImInboundActionSelection::Resolved { action, .. } = &fixture.envelope.action else {
            unreachable!("fixture uses a resolved remote action")
        };
        InterventionCommandService::new(&fixture.app)
            .execute(InterventionCommand {
                locator: reference.locator.clone(),
                request: reference.request.clone(),
                expected_state: reference.expected_state.clone(),
                action: action.clone(),
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
        assert_eq!(recovered.result_code, "ALREADY_APPLIED");
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
        delivery.canonical_event_id = "event-1:desktop-resolved".into();
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
