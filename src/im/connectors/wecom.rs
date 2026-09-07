//! WeCom AI Bot WebSocket connector using the official wire protocol.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    RenderedAction, action_name, intervention_actions, localized, message_state_label,
    notification_markdown, presentation_detail_text,
};
use crate::app::intervention::{
    InterventionAllowedAction, PermissionActionKind, PermissionActionQualifier,
    RemoteElicitationForm, RemoteElicitationFormSelection, permission_action_qualifier,
};
use crate::im::{
    ImActionResponseContext, ImActionTokenCodec, ImChannelCapabilities, ImChannelKind,
    ImConnectionIdentity, ImConnector, ImConnectorEvent, ImDelivery, ImDeliveryPayload,
    ImDeliveryReceipt, ImErrorCode, ImInboundActionSelection, ImInboundEnvelope,
    ImIntegrationError, ImLocale, ImMessageState, ImNotificationKind, ImObservedBinding,
    ImWeComFormCardKind, ImWeComFormSelection, ImWeComVoteSelection, MAX_IM_PRESENTATION_BYTES,
    ResolvedImChannelConfig, deterministic_action_id,
};

pub const WECOM_WEBSOCKET_ENDPOINT: &str = "wss://openws.work.weixin.qq.com";
const WECOM_SECRET_FIELD: &str = "secret";
const WECOM_COMMAND_CAPACITY: usize = 64;
const WECOM_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const WECOM_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const WECOM_MAX_RECONNECTS: usize = 8;
const WECOM_MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
const WECOM_MAX_CARD_BUTTONS: usize = 2;
const WECOM_MAX_VOTE_OPTIONS: usize = 20;
const WECOM_MAX_BUTTON_KEY_BYTES: usize = 1024;
const WECOM_MAX_OPTION_ID_BYTES: usize = 128;
const WECOM_MAX_TASK_ID_BYTES: usize = 128;
const WECOM_VOTE_QUESTION_KEY: &str = "permission_choice";
const PERMISSION_FIELD_PRIORITY: [&str; 4] = [
    "permissionTool",
    "permissionCommand",
    "permissionPath",
    "permissionParameter",
];

enum WeComCommand {
    Send {
        delivery: ImDelivery,
        response: oneshot::Sender<Result<ImDeliveryReceipt, ImIntegrationError>>,
    },
    RespondToAction {
        context: ImActionResponseContext,
        state: ImMessageState,
        response: oneshot::Sender<Result<(), ImIntegrationError>>,
    },
}

enum PendingRequest {
    Send {
        delivery: ImDelivery,
        response: oneshot::Sender<Result<ImDeliveryReceipt, ImIntegrationError>>,
    },
    LinkedDetail {
        delivery: ImDelivery,
        card_request: Value,
        card_req_id: String,
        response: oneshot::Sender<Result<ImDeliveryReceipt, ImIntegrationError>>,
    },
    ActionResponse {
        response: oneshot::Sender<Result<(), ImIntegrationError>>,
    },
}

pub struct WeComConnector {
    token_codec: ImActionTokenCodec,
    endpoint: String,
    commands: Mutex<GenerationSenderSlot>,
}

#[derive(Default)]
struct GenerationSenderSlot {
    generation: u64,
    sender: Option<mpsc::Sender<WeComCommand>>,
}

impl WeComConnector {
    pub fn new(token_codec: ImActionTokenCodec) -> Self {
        Self {
            token_codec,
            endpoint: WECOM_WEBSOCKET_ENDPOINT.to_owned(),
            commands: Mutex::new(GenerationSenderSlot::default()),
        }
    }

    #[cfg(test)]
    fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    fn command_sender(&self) -> Result<mpsc::Sender<WeComCommand>, ImIntegrationError> {
        self.commands
            .lock()
            .expect("WeCom connector command lock poisoned")
            .sender
            .clone()
            .ok_or_else(|| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))
    }

    fn install_sender(&self, generation: u64, sender: mpsc::Sender<WeComCommand>) -> bool {
        let mut slot = self
            .commands
            .lock()
            .expect("WeCom connector command lock poisoned");
        if slot.generation != generation {
            return false;
        }
        slot.sender = Some(sender);
        true
    }

    fn clear_sender(&self, generation: u64) -> bool {
        let mut slot = self
            .commands
            .lock()
            .expect("WeCom connector command lock poisoned");
        if slot.generation != generation {
            return false;
        }
        slot.sender.take();
        true
    }

    async fn run_session(
        &self,
        bot_id: &str,
        secret: &str,
        generation: u64,
        events: &mpsc::Sender<ImConnectorEvent>,
        cancellation: &CancellationToken,
        locale: ImLocale,
    ) -> Result<(), ImIntegrationError> {
        let (mut socket, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .map_err(map_websocket_error)?;
        subscribe(&mut socket, bot_id, secret).await?;

        let (command_tx, mut command_rx) = mpsc::channel(WECOM_COMMAND_CAPACITY);
        if !self.install_sender(generation, command_tx.clone()) {
            return Ok(());
        }
        events
            .send(ImConnectorEvent::Connected {
                generation,
                identity: ImConnectionIdentity {
                    bot_id: bot_id.to_owned(),
                    display_name: bot_id.to_owned(),
                },
            })
            .await
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::NetworkUnavailable))?;

        let mut pending = HashMap::<String, PendingRequest>::new();
        let mut heartbeat = tokio::time::interval(WECOM_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        heartbeat.tick().await;

        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    self.clear_sender(generation);
                    fail_pending(pending);
                    return Ok(());
                }
                _ = heartbeat.tick() => {
                    let req_id = request_id();
                    socket.send(Message::Text(json!({
                        "cmd": "ping",
                        "headers": { "req_id": req_id },
                    }).to_string().into())).await.map_err(map_websocket_error)?;
                }
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        self.clear_sender(generation);
                        fail_pending(pending);
                        return Ok(());
                    };
                    let (req_id, request, pending_request) = match command {
                        WeComCommand::Send { delivery, response } => {
                            let req_id = request_id();
                            if delivery_requires_linked_detail(&delivery) {
                                let card_req_id = request_id();
                                let plan = match linked_detail_send_plan(
                                    &req_id,
                                    &card_req_id,
                                    &delivery,
                                    &self.token_codec,
                                    locale,
                                ) {
                                    Ok(plan) => plan,
                                    Err(error) => {
                                        let _ = response.send(Err(error));
                                        continue;
                                    }
                                };
                                (
                                    req_id,
                                    plan.detail_request,
                                    PendingRequest::LinkedDetail {
                                        delivery,
                                        card_request: plan.card_request,
                                        card_req_id,
                                        response,
                                    },
                                )
                            } else {
                                let request = match send_request(
                                    &req_id,
                                    &delivery,
                                    &self.token_codec,
                                    locale,
                                ) {
                                    Ok(request) => request,
                                    Err(error) => {
                                        let _ = response.send(Err(error));
                                        continue;
                                    }
                                };
                                (req_id, request, PendingRequest::Send { delivery, response })
                            }
                        }
                        WeComCommand::RespondToAction { context, state, response } => {
                            let ImActionResponseContext::WeCom {
                                req_id,
                                task_id,
                                vote_selection,
                                form_selection,
                            } = context;
                            let request = action_response_request(
                                &req_id,
                                &task_id,
                                state,
                                locale,
                                vote_selection.as_ref(),
                                form_selection.as_ref(),
                            );
                            (req_id, request, PendingRequest::ActionResponse { response })
                        }
                    };
                    pending.insert(req_id.clone(), pending_request);
                    if let Err(error) = socket.send(Message::Text(request.to_string().into())).await {
                        if let Some(request) = pending.remove(&req_id) {
                            fail_request(request, map_websocket_error(error));
                        }
                        return Err(ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None));
                    }
                }
                message = socket.next() => {
                    let Some(message) = message else {
                        self.clear_sender(generation);
                        fail_pending(pending);
                        return Err(ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None));
                    };
                    let message = message.map_err(map_websocket_error)?;
                    if message.is_close() {
                        self.clear_sender(generation);
                        fail_pending(pending);
                        return Err(ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None));
                    }
                    let Some(value) = parse_json_message(&message)? else { continue; };
                    if is_request_ack(&value)
                        && let Some(req_id) = header_req_id(&value)
                        && let Some(request) = pending.remove(req_id)
                    {
                        match request {
                            PendingRequest::LinkedDetail {
                                delivery,
                                card_request,
                                card_req_id,
                                response,
                            } => {
                                if let Some(error) = response_error(&value) {
                                    fail_request(
                            PendingRequest::LinkedDetail {
                                delivery,
                                card_request,
                                card_req_id,
                                            response,
                                        },
                                        error,
                                    );
                                    continue;
                                }
                                if let Err(error) = socket
                                    .send(Message::Text(card_request.to_string().into()))
                                    .await
                                {
                                    let error = map_websocket_error(error);
                                    let _ = response.send(Err(error.clone()));
                                    return Err(error);
                                }
                                pending.insert(
                                    card_req_id,
                                    PendingRequest::Send {
                                        delivery,
                                        response,
                                    },
                                );
                            }
                            request => settle_response(request, &value),
                        }
                        continue;
                    }
                    handle_callback(generation, events, &command_tx, value).await?;
                }
            }
        }
    }
}

#[async_trait]
impl ImConnector for WeComConnector {
    fn kind(&self) -> ImChannelKind {
        ImChannelKind::WeCom
    }

    fn capabilities(&self) -> ImChannelCapabilities {
        ImChannelCapabilities {
            proactive_delivery: true,
            card_actions: true,
            message_update: true,
            private_chat: true,
        }
    }

    fn advance_generation(&self, generation: u64) {
        let mut slot = self
            .commands
            .lock()
            .expect("WeCom connector command lock poisoned");
        if generation >= slot.generation {
            slot.generation = generation;
            slot.sender = None;
        }
    }

    async fn connect(
        &self,
        config: ResolvedImChannelConfig,
        generation: u64,
        events: mpsc::Sender<ImConnectorEvent>,
        cancellation: CancellationToken,
    ) -> Result<(), ImIntegrationError> {
        self.advance_generation(generation);
        let bot_id = non_empty(&config.public_identity)?;
        let secret = config
            .credential_fields
            .get(WECOM_SECRET_FIELD)
            .and_then(|value| (!value.trim().is_empty()).then_some(value.clone()))
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::CredentialUnavailable))?;

        for reconnect in 0..=WECOM_MAX_RECONNECTS {
            if cancellation.is_cancelled() {
                self.clear_sender(generation);
                return Ok(());
            }
            let session_result = self
                .run_session(
                    &bot_id,
                    &secret,
                    generation,
                    &events,
                    &cancellation,
                    config.locale,
                )
                .await;
            self.clear_sender(generation);
            match session_result {
                Ok(()) => return Ok(()),
                Err(error) if !error.retryable => return Err(error),
                Err(error) => {
                    self.clear_sender(generation);
                    let _ = events
                        .send(ImConnectorEvent::Disconnected {
                            generation,
                            error: error.clone(),
                        })
                        .await;
                    if reconnect == WECOM_MAX_RECONNECTS {
                        return Err(error);
                    }
                    let delay = reconnect_delay(reconnect);
                    tokio::select! {
                        _ = cancellation.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
        Err(ImIntegrationError::retryable(
            ImErrorCode::NetworkUnavailable,
            None,
        ))
    }

    async fn send(&self, delivery: ImDelivery) -> Result<ImDeliveryReceipt, ImIntegrationError> {
        let sender = self.command_sender()?;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(WeComCommand::Send {
                delivery,
                response: response_tx,
            })
            .await
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?;
        tokio::time::timeout(WECOM_REQUEST_TIMEOUT, response_rx)
            .await
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?
    }

    async fn respond_to_action(
        &self,
        context: ImActionResponseContext,
        state: ImMessageState,
    ) -> Result<(), ImIntegrationError> {
        let sender = self.command_sender()?;
        let (response_tx, response_rx) = oneshot::channel();
        sender
            .send(WeComCommand::RespondToAction {
                context,
                state,
                response: response_tx,
            })
            .await
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?;
        tokio::time::timeout(WECOM_REQUEST_TIMEOUT, response_rx)
            .await
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?
            .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?
    }
}

async fn subscribe<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    bot_id: &str,
    secret: &str,
) -> Result<(), ImIntegrationError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let req_id = request_id();
    socket
        .send(Message::Text(
            json!({
                "cmd": "aibot_subscribe",
                "headers": { "req_id": req_id },
                "body": { "bot_id": bot_id, "secret": secret },
            })
            .to_string()
            .into(),
        ))
        .await
        .map_err(map_websocket_error)?;
    let response = tokio::time::timeout(WECOM_REQUEST_TIMEOUT, async {
        while let Some(message) = socket.next().await {
            let message = message.map_err(map_websocket_error)?;
            if let Some(value) = parse_json_message(&message)?
                && header_req_id(&value) == Some(req_id.as_str())
            {
                return Ok(value);
            }
        }
        Err(ImIntegrationError::retryable(
            ImErrorCode::NetworkUnavailable,
            None,
        ))
    })
    .await
    .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))??;
    response_error(&response).map_or(Ok(()), Err)
}

fn safe_wecom_actions(actions: Vec<RenderedAction>) -> Vec<RenderedAction> {
    if actions.len() <= WECOM_MAX_CARD_BUTTONS {
        return actions;
    }
    let mut ranked = actions
        .into_iter()
        .map(|action| (remote_action_rank(&action), action.action_index, action))
        .collect::<Vec<_>>();
    ranked.sort_unstable_by_key(|(rank, index, _)| (*rank, *index));
    ranked.truncate(WECOM_MAX_CARD_BUTTONS);
    ranked.sort_unstable_by_key(|(_, index, _)| *index);
    ranked.into_iter().map(|(_, _, action)| action).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VoteOption {
    action_index: usize,
    label: String,
    presentation_rank: usize,
    safe_fallback: bool,
}

fn vote_options(actions: &[InterventionAllowedAction], locale: ImLocale) -> Vec<VoteOption> {
    let mut options = actions
        .iter()
        .enumerate()
        .filter_map(|(action_index, action)| {
            let (presentation_rank, safe_fallback) = vote_option_presentation(action)?;
            Some(VoteOption {
                action_index,
                label: super::action_label(locale, action),
                presentation_rank,
                safe_fallback,
            })
        })
        .collect::<Vec<_>>();
    if !options.iter().any(|option| option.safe_fallback) {
        return Vec::new();
    }
    if options.len() > WECOM_MAX_VOTE_OPTIONS {
        options.retain(|option| option.safe_fallback);
    }
    options.sort_unstable_by_key(|option| (option.presentation_rank, option.action_index));
    options.truncate(WECOM_MAX_VOTE_OPTIONS);
    options
}

fn vote_option_presentation(action: &InterventionAllowedAction) -> Option<(usize, bool)> {
    match action {
        InterventionAllowedAction::PermissionOption {
            option_id,
            permission_kind,
            ..
        } if *permission_kind != PermissionActionKind::Unknown => {
            match (
                *permission_kind,
                permission_action_qualifier(option_id, action_name(action)),
            ) {
                (_, PermissionActionQualifier::KeepPlanning) => Some((2, true)),
                (PermissionActionKind::AllowAlways, _) => Some((0, false)),
                (PermissionActionKind::AllowOnce, _) => Some((1, false)),
                (PermissionActionKind::RejectOnce | PermissionActionKind::RejectAlways, _) => {
                    Some((2, true))
                }
                _ => None,
            }
        }
        InterventionAllowedAction::ManualSuccess => Some((0, false)),
        InterventionAllowedAction::ManualFailure => Some((1, true)),
        _ => None,
    }
}

fn remote_action_rank(action: &RenderedAction) -> usize {
    match &action.value.action {
        crate::app::intervention::InterventionAction::PermissionOption { .. } => {
            match action.permission_kind {
                Some(crate::app::intervention::PermissionActionKind::RejectOnce) => 0,
                Some(crate::app::intervention::PermissionActionKind::AllowOnce) => 1,
                Some(crate::app::intervention::PermissionActionKind::RejectAlways) => 2,
                Some(crate::app::intervention::PermissionActionKind::AllowAlways) => 3,
                _ => 4,
            }
        }
        crate::app::intervention::InterventionAction::Elicitation {
            action: crate::acp::elicitation::ElicitationAction::Decline,
            ..
        } => 0,
        crate::app::intervention::InterventionAction::Elicitation { .. } => 1,
        crate::app::intervention::InterventionAction::ManualCheck { outcome } => {
            if matches!(outcome, crate::domain::NodeOutcome::Failure) {
                0
            } else {
                1
            }
        }
    }
}

fn delivery_elicitation_form(
    reference: &crate::im::InterventionRef,
) -> Result<&RemoteElicitationForm, ImIntegrationError> {
    reference
        .allowed_actions
        .iter()
        .find_map(|action| match action {
            InterventionAllowedAction::ElicitationFixedForm { form } => Some(form),
            _ => None,
        })
        .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))
}

fn elicitation_template_card(
    delivery: &ImDelivery,
    form: &RemoteElicitationForm,
    title: &str,
    desc: &str,
    fields: Vec<Value>,
    locale: ImLocale,
) -> Value {
    let task_id = delivery.delivery_id.clone();
    match form {
        RemoteElicitationForm::SingleScalarChoice { question }
        | RemoteElicitationForm::MultiScalarChoice { question, .. } => {
            let mode = matches!(form, RemoteElicitationForm::MultiScalarChoice { .. })
                .then_some(1)
                .unwrap_or(0);
            json!({
                "card_type": "vote_interaction",
                "source": { "desc": "Gold Band" },
                "main_title": {
                    "title": truncate_chars(title, 26),
                    "desc": truncate_chars(desc, 30),
                },
                "sub_title_text": localized(locale, "im.elicitation.vote.linkedSubtitle"),
                "horizontal_content_list": fields,
                "checkbox": {
                    "question_key": question.selector_key,
                    "mode": mode,
                    "option_list": question.options.iter().enumerate().map(|(index, option)| json!({
                        "id": index.to_string(),
                        "text": truncate_chars(&option.label, 11),
                        "is_checked": false,
                    })).collect::<Vec<_>>(),
                },
                "submit_button": {
                    "text": localized(locale, "im.permission.submit"),
                    "key": format!("{task_id}:submit"),
                },
                "task_id": task_id,
            })
        }
        RemoteElicitationForm::ScalarChoiceQuestions { questions } => json!({
            "card_type": "multiple_interaction",
            "source": { "desc": "Gold Band" },
            "main_title": {
                "title": truncate_chars(title, 26),
                "desc": truncate_chars(desc, 30),
            },
            "sub_title_text": localized(locale, "im.elicitation.vote.linkedSubtitle"),
            "horizontal_content_list": fields,
            "select_list": questions.iter().map(|question| json!({
                "question_key": question.selector_key,
                "title": truncate_chars(&question.title, 13),
                "option_list": question.options.iter().enumerate().map(|(index, option)| json!({
                    "id": index.to_string(),
                    "text": truncate_chars(&option.label, 10),
                })).collect::<Vec<_>>(),
                "selected_id": "0",
            })).collect::<Vec<_>>(),
            "submit_button": {
                "text": localized(locale, "im.permission.submit"),
                "key": format!("{task_id}:submit"),
            },
            "task_id": task_id,
        }),
    }
}

fn send_request(
    req_id: &str,
    delivery: &ImDelivery,
    token_codec: &ImActionTokenCodec,
    locale: ImLocale,
) -> Result<Value, ImIntegrationError> {
    let body = match &delivery.payload {
        ImDeliveryPayload::Information { .. }
        | ImDeliveryPayload::InterventionResolution { .. } => json!({
            "chatid": delivery.destination.destination_id,
            "msgtype": "markdown",
            "markdown": { "content": notification_markdown(delivery, locale) },
        }),
        ImDeliveryPayload::Intervention { .. } => {
            let ImDeliveryPayload::Intervention {
                presentation,
                reference,
                ..
            } = &delivery.payload
            else {
                unreachable!("intervention branch has intervention payload")
            };
            let fields = horizontal_content_fields(
                &presentation.fields,
                locale,
                delivery.notification_kind == ImNotificationKind::Permission,
            );
            let linked_detail = requires_linked_detail(delivery.notification_kind);
            let title = if linked_detail {
                linked_display_title(delivery.notification_kind, locale, delivery.display_ref)?
            } else {
                presentation
                    .title
                    .as_deref()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| localized(locale, &presentation.title_key))
                    .to_owned()
            };
            let desc = if linked_detail {
                localized(locale, card_summary_key(delivery.notification_kind))
            } else {
                presentation
                    .summary
                    .as_deref()
                    .filter(|desc| !desc.trim().is_empty())
                    .unwrap_or_else(|| localized(locale, &presentation.summary_key))
            };
            if !valid_wecom_task_id(&delivery.delivery_id) {
                return Err(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid));
            }
            let mut template_card = if linked_detail {
                if delivery.notification_kind == ImNotificationKind::Elicitation {
                    let form = delivery_elicitation_form(reference)?;
                    elicitation_template_card(delivery, form, &title, desc, fields, locale)
                } else {
                    let options = vote_options(&reference.allowed_actions, locale)
                        .into_iter()
                        .enumerate()
                        .map(|(display_index, option)| {
                            json!({
                                "id": option.action_index.to_string(),
                                "text": truncate_chars(&option.label, 11),
                                "is_checked": display_index == 0,
                            })
                        })
                        .collect::<Vec<_>>();
                    if options.is_empty() {
                        json!({
                            "card_type": "text_notice",
                            "source": { "desc": "Gold Band" },
                            "main_title": {
                                "title": truncate_chars(&title, 26),
                                "desc": truncate_chars(desc, 30),
                            },
                            "sub_title_text": localized(locale, "im.permission.desktopGuidance"),
                            "horizontal_content_list": fields,
                            "task_id": delivery.delivery_id,
                        })
                    } else {
                        json!({
                            "card_type": "vote_interaction",
                            "source": { "desc": "Gold Band" },
                            "main_title": {
                                "title": truncate_chars(&title, 26),
                                "desc": truncate_chars(desc, 30),
                            },
                            "sub_title_text": localized(locale, vote_linked_subtitle_key(delivery.notification_kind)),
                            "horizontal_content_list": fields,
                            "checkbox": {
                                "question_key": WECOM_VOTE_QUESTION_KEY,
                                "mode": 0,
                                "option_list": options,
                            },
                            "submit_button": {
                                "text": localized(locale, "im.permission.submit"),
                                "key": format!("{}:submit", delivery.delivery_id),
                            },
                            "task_id": delivery.delivery_id,
                        })
                    }
                }
            } else {
                let rendered_actions = intervention_actions(delivery, token_codec, locale)
                    .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
                let buttons = safe_wecom_actions(rendered_actions)
                    .into_iter()
                    .map(|action| {
                        let key = format!("{}:{}", delivery.delivery_id, action.action_index);
                        if key.len() > WECOM_MAX_BUTTON_KEY_BYTES {
                            return Err(ImIntegrationError::permanent(
                                ImErrorCode::ProtocolInvalid,
                            ));
                        }
                        Ok(json!({ "text": truncate_chars(&action.label, 10), "key": key }))
                    })
                    .collect::<Result<Vec<_>, ImIntegrationError>>()?;
                if buttons.is_empty() {
                    json!({
                        "card_type": "text_notice",
                        "main_title": {
                            "title": truncate_chars(&title, 26),
                            "desc": truncate_chars(desc, 30),
                        },
                        "horizontal_content_list": fields,
                        "task_id": delivery.delivery_id,
                    })
                } else {
                    json!({
                        "card_type": "button_interaction",
                        "main_title": {
                            "title": truncate_chars(&title, 26),
                            "desc": truncate_chars(desc, 30),
                        },
                        "horizontal_content_list": fields,
                        "button_list": buttons,
                        "task_id": delivery.delivery_id,
                    })
                }
            };
            let details = presentation_detail_text(presentation, locale);
            if !details.is_empty() && !linked_detail {
                template_card["sub_title_text"] = Value::String(truncate_chars(&details, 112));
            }
            json!({
                "chatid": delivery.destination.destination_id,
                "msgtype": "template_card",
                "template_card": template_card,
            })
        }
    };
    Ok(json!({
        "cmd": "aibot_send_msg",
        "headers": { "req_id": req_id },
        "body": body,
    }))
}

struct LinkedDetailSendPlan {
    detail_request: Value,
    card_request: Value,
}

fn requires_linked_detail(kind: ImNotificationKind) -> bool {
    matches!(
        kind,
        ImNotificationKind::Permission
            | ImNotificationKind::ManualCheck
            | ImNotificationKind::Elicitation
    )
}

fn delivery_requires_linked_detail(delivery: &ImDelivery) -> bool {
    matches!(&delivery.payload, ImDeliveryPayload::Intervention { .. })
        && requires_linked_detail(delivery.notification_kind)
}

fn card_summary_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.permission.card.summary",
        ImNotificationKind::ManualCheck => "im.notification.manualCheck.summary",
        ImNotificationKind::Elicitation => "im.notification.elicitation.summary",
        _ => unreachable!("linked detail interventions have a card summary"),
    }
}

fn card_title_with_ref_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.permission.card.titleWithRef",
        ImNotificationKind::ManualCheck => "im.manualCheck.card.titleWithRef",
        ImNotificationKind::Elicitation => "im.elicitation.card.titleWithRef",
        _ => unreachable!("linked detail interventions have a card title"),
    }
}

fn vote_linked_subtitle_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.permission.vote.linkedSubtitle",
        ImNotificationKind::ManualCheck => "im.manualCheck.vote.linkedSubtitle",
        ImNotificationKind::Elicitation => "im.elicitation.vote.linkedSubtitle",
        _ => unreachable!("linked detail interventions have a vote subtitle"),
    }
}

fn detail_link_key(kind: ImNotificationKind) -> &'static str {
    match kind {
        ImNotificationKind::Permission => "im.permission.detail.link",
        ImNotificationKind::ManualCheck => "im.manualCheck.detail.link",
        ImNotificationKind::Elicitation => "im.elicitation.detail.link",
        _ => unreachable!("linked detail interventions have a detail link"),
    }
}

fn linked_detail_send_plan(
    detail_req_id: &str,
    card_req_id: &str,
    delivery: &ImDelivery,
    token_codec: &ImActionTokenCodec,
    locale: ImLocale,
) -> Result<LinkedDetailSendPlan, ImIntegrationError> {
    let content = truncate_utf8_bytes(&linked_detail_markdown(delivery, locale)?);
    let card_request = send_request(card_req_id, delivery, token_codec, locale)?;
    Ok(LinkedDetailSendPlan {
        detail_request: json!({
            "cmd": "aibot_send_msg",
            "headers": { "req_id": detail_req_id },
            "body": {
                "chatid": delivery.destination.destination_id,
                "msgtype": "markdown",
                "markdown": { "content": content },
            },
        }),
        card_request,
    })
}

fn linked_display_title(
    kind: ImNotificationKind,
    locale: ImLocale,
    display_ref: Option<u16>,
) -> Result<String, ImIntegrationError> {
    let display_ref =
        display_ref.ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
    Ok(localized(locale, card_title_with_ref_key(kind))
        .replace("{ref}", &format!("{display_ref:04}")))
}

fn linked_detail_markdown(
    delivery: &ImDelivery,
    locale: ImLocale,
) -> Result<String, ImIntegrationError> {
    let ImDeliveryPayload::Intervention { presentation, .. } = &delivery.payload else {
        return Err(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid));
    };
    let title = linked_display_title(delivery.notification_kind, locale, delivery.display_ref)?;
    let mut lines = vec![
        format!("**{title}**"),
        localized(locale, card_summary_key(delivery.notification_kind)).to_owned(),
    ];
    if delivery.notification_kind == ImNotificationKind::ManualCheck {
        for key in ["nodeLabel", "taskTitle"] {
            if let Some(value) = presentation
                .fields
                .get(key)
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("- **{}**: {}", localized(locale, key), value));
            }
        }
        if let Some(output) = presentation
            .body
            .as_deref()
            .filter(|output| !output.trim().is_empty())
        {
            lines.push(format!(
                "### {}",
                localized(locale, "im.manualCheck.detail.output")
            ));
            lines.push(output.to_owned());
        }
    }
    if delivery.notification_kind == ImNotificationKind::Elicitation {
        for key in ["nodeLabel", "taskTitle"] {
            if let Some(value) = presentation
                .fields
                .get(key)
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("- **{}**: {}", localized(locale, key), value));
            }
        }
        if let Some(context) = presentation
            .context
            .as_deref()
            .filter(|context| !context.trim().is_empty())
            .filter(|context| {
                presentation
                    .body
                    .as_deref()
                    .is_none_or(|body| context.trim() != body.trim())
            })
        {
            lines.push(format!(
                "### {}",
                localized(locale, "im.elicitation.detail.context")
            ));
            lines.push(context.to_owned());
        }
        if let Some(message) = presentation
            .body
            .as_deref()
            .filter(|message| !message.trim().is_empty())
        {
            lines.push(message.to_owned());
        }
        let details = super::presentation_question_detail_text(presentation, locale);
        if !details.trim().is_empty() {
            lines.push(details);
        }
    }
    if delivery.notification_kind == ImNotificationKind::Permission {
        if let Some(description) = presentation
            .body
            .as_deref()
            .filter(|description| !description.trim().is_empty())
        {
            lines.push(format!(
                "- **{}**: {}",
                localized(locale, "im.permission.detail.description"),
                description
            ));
        }
        for key in PERMISSION_FIELD_PRIORITY {
            if let Some(value) = presentation
                .fields
                .get(key)
                .filter(|value| !value.trim().is_empty())
            {
                lines.push(format!("- **{}**: {}", localized(locale, key), value));
            }
        }
        for (key, value) in &presentation.fields {
            if !key.starts_with("permission")
                && !value.trim().is_empty()
                && key != "permissionTitle"
            {
                lines.push(format!("- **{}**: {}", localized(locale, key), value));
            }
        }
    }
    lines.push(localized(locale, detail_link_key(delivery.notification_kind)).to_owned());
    Ok(lines.join("\n"))
}

fn horizontal_content_fields(
    fields: &std::collections::BTreeMap<String, String>,
    locale: ImLocale,
    permission: bool,
) -> Vec<Value> {
    let visible = |key: &str, value: &str| !value.trim().is_empty() && key != "permissionTitle";
    let mut entries = Vec::new();
    if permission {
        for key in PERMISSION_FIELD_PRIORITY {
            if let Some(value) = fields.get(key)
                && visible(key, value)
            {
                entries.push((key, value));
            }
        }
    }
    for (key, value) in fields {
        if (!permission || !PERMISSION_FIELD_PRIORITY.contains(&key.as_str()))
            && visible(key, value)
        {
            entries.push((key.as_str(), value));
        }
    }
    entries
        .into_iter()
        .take(6)
        .map(|(key, value)| {
            json!({
                "keyname": truncate_chars(localized(locale, key), 5),
                "value": truncate_chars(value, 26),
            })
        })
        .collect()
}

fn valid_wecom_task_id(task_id: &str) -> bool {
    task_id.len() <= WECOM_MAX_TASK_ID_BYTES
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'@'))
}

fn action_response_request(
    req_id: &str,
    task_id: &str,
    state: ImMessageState,
    locale: ImLocale,
    vote_selection: Option<&ImWeComVoteSelection>,
    form_selection: Option<&ImWeComFormSelection>,
) -> Value {
    if let Some(form_selection) = form_selection {
        return form_action_response_request(req_id, task_id, state, locale, form_selection);
    }
    let Some(vote_selection) = vote_selection else {
        let content = message_state_label(locale, state);
        let action_key = format!("{task_id}:0");
        return json!({
            "cmd": "aibot_respond_update_msg",
            "headers": { "req_id": req_id },
            "body": {
                "response_type": "update_template_card",
                "template_card": {
                    "card_type": "button_interaction",
                    "main_title": { "title": &content },
                    "button_list": [
                        { "text": &content, "key": &action_key }
                    ],
                    "task_id": task_id,
                },
            },
        });
    };
    let selected_index = selected_vote_index(vote_selection);
    let selected_action = selected_index.and_then(|index| vote_selection.vote_actions.get(index));
    let mut options = if vote_selection.vote_actions.is_empty() {
        let fallback_index = selected_index.unwrap_or(0);
        vec![json!({
            "id": fallback_index.to_string(),
            "text": truncate_chars(message_state_label(locale, state), 11),
            "is_checked": true,
        })]
    } else {
        vote_options(&vote_selection.vote_actions, locale)
            .into_iter()
            .map(|option| {
                json!({
                    "id": option.action_index.to_string(),
                    "text": truncate_chars(&option.label, 11),
                    "is_checked": selected_index == Some(option.action_index),
                })
            })
            .collect()
    };
    if options.is_empty() {
        options.push(json!({
            "id": "0",
            "text": truncate_chars(message_state_label(locale, state), 11),
            "is_checked": false,
        }));
    }
    let title = vote_terminal_title(locale, state, selected_action, vote_selection.display_ref);
    json!({
        "cmd": "aibot_respond_update_msg",
        "headers": { "req_id": req_id },
        "body": {
            "response_type": "update_template_card",
                "template_card": {
                    "card_type": "vote_interaction",
                    "main_title": {
                        "title": truncate_chars(&title, 26),
                        "desc": localized(locale, vote_card_summary_key(selected_action)),
                    },
                "checkbox": {
                    "question_key": WECOM_VOTE_QUESTION_KEY,
                    "mode": 0,
                    "disable": true,
                    "option_list": options,
                },
                "submit_button": {
                    "text": localized(locale, "im.permission.submitted"),
                    "key": format!("{task_id}:submit"),
                },
                "task_id": task_id,
            },
        },
    })
}

fn vote_card_summary_key(selected_action: Option<&InterventionAllowedAction>) -> &'static str {
    match selected_action {
        Some(
            InterventionAllowedAction::ManualSuccess | InterventionAllowedAction::ManualFailure,
        ) => "im.notification.manualCheck.summary",
        _ => "im.permission.card.summary",
    }
}

fn selected_form_option_ids<'a>(
    form_selection: &'a ImWeComFormSelection,
    selector_key: &str,
) -> Vec<&'a str> {
    form_selection
        .selections
        .iter()
        .find(|selection| selection.selector_key == selector_key)
        .map(|selection| {
            selection
                .option_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn form_terminal_summary(
    locale: ImLocale,
    state: ImMessageState,
    form_selection: &ImWeComFormSelection,
) -> String {
    if state != ImMessageState::Handled {
        return message_state_label(locale, state).to_owned();
    }
    let Some(form) = form_selection.form.as_ref() else {
        return message_state_label(locale, state).to_owned();
    };
    match form {
        RemoteElicitationForm::SingleScalarChoice { question }
        | RemoteElicitationForm::MultiScalarChoice { question, .. } => {
            let selected = selected_form_option_ids(form_selection, &question.selector_key);
            selected
                .iter()
                .filter_map(|option_id| option_id.parse::<usize>().ok())
                .filter_map(|index| question.options.get(index))
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
        RemoteElicitationForm::ScalarChoiceQuestions { questions } => questions
            .iter()
            .map(|question| {
                let selected = selected_form_option_ids(form_selection, &question.selector_key);
                let label = selected
                    .first()
                    .and_then(|option_id| option_id.parse::<usize>().ok())
                    .and_then(|index| question.options.get(index))
                    .map(|option| option.label.as_str())
                    .unwrap_or("-");
                format!("{}: {label}", question.title)
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn form_terminal_title(
    locale: ImLocale,
    state: ImMessageState,
    form_selection: &ImWeComFormSelection,
) -> String {
    let summary = form_terminal_summary(locale, state, form_selection);
    terminal_title_with_leading_ref(locale, state, &summary, form_selection.display_ref)
}

fn form_action_response_request(
    req_id: &str,
    task_id: &str,
    state: ImMessageState,
    locale: ImLocale,
    form_selection: &ImWeComFormSelection,
) -> Value {
    let title = form_terminal_title(locale, state, form_selection);
    let desc = localized(locale, "im.notification.elicitation.summary");
    let mut template_card = match (form_selection.card_kind, form_selection.form.as_ref()) {
        (
            ImWeComFormCardKind::VoteSingle | ImWeComFormCardKind::VoteMulti,
            Some(RemoteElicitationForm::SingleScalarChoice { question })
            | Some(RemoteElicitationForm::MultiScalarChoice { question, .. }),
        ) => {
            let selected = selected_form_option_ids(form_selection, &question.selector_key);
            let mode = matches!(form_selection.card_kind, ImWeComFormCardKind::VoteMulti)
                .then_some(1)
                .unwrap_or(0);
            json!({
                "card_type": "vote_interaction",
                "checkbox": {
                    "question_key": question.selector_key,
                    "mode": mode,
                    "disable": true,
                    "option_list": question.options.iter().enumerate().map(|(index, option)| {
                        json!({
                            "id": index.to_string(),
                            "text": truncate_chars(&option.label, 11),
                            "is_checked": selected
                                .iter()
                                .any(|selected_id| *selected_id == index.to_string()),
                        })
                    }).collect::<Vec<_>>(),
                },
            })
        }
        (
            ImWeComFormCardKind::Multiple,
            Some(RemoteElicitationForm::ScalarChoiceQuestions { questions }),
        ) => json!({
            "card_type": "multiple_interaction",
            "select_list": questions.iter().map(|question| {
                let selected = selected_form_option_ids(form_selection, &question.selector_key);
                json!({
                    "question_key": question.selector_key,
                    "title": truncate_chars(&question.title, 13),
                    "disable": true,
                    "option_list": question.options.iter().enumerate().map(|(index, option)| {
                        json!({
                            "id": index.to_string(),
                            "text": truncate_chars(&option.label, 10),
                        })
                    }).collect::<Vec<_>>(),
                    "selected_id": selected.first().cloned().unwrap_or_else(|| "0".into()),
                })
            }).collect::<Vec<_>>(),
        }),
        (ImWeComFormCardKind::VoteSingle | ImWeComFormCardKind::VoteMulti, None) => {
            let selection = form_selection
                .selections
                .first()
                .map(|selection| json!({
                    "question_key": selection.selector_key,
                    "mode": matches!(form_selection.card_kind, ImWeComFormCardKind::VoteMulti).then_some(1).unwrap_or(0),
                    "disable": true,
                    "option_list": selection.option_ids.iter().map(|option_id| json!({
                        "id": option_id,
                        "text": truncate_chars(message_state_label(locale, state), 11),
                        "is_checked": true,
                    })).collect::<Vec<_>>(),
                }))
                .unwrap_or_else(|| json!({
                    "question_key": "q0",
                    "mode": if matches!(form_selection.card_kind, ImWeComFormCardKind::VoteMulti) { 1 } else { 0 },
                    "disable": true,
                    "option_list": [{
                        "id": "0",
                        "text": truncate_chars(message_state_label(locale, state), 11),
                        "is_checked": false,
                    }],
                }));
            json!({
                "card_type": "vote_interaction",
                "checkbox": selection,
            })
        }
        (ImWeComFormCardKind::Multiple, None) => json!({
            "card_type": "multiple_interaction",
            "select_list": if form_selection.selections.is_empty() {
                vec![json!({
                    "question_key": "q0",
                    "title": truncate_chars(message_state_label(locale, state), 13),
                    "disable": true,
                    "option_list": [{
                        "id": "0",
                        "text": truncate_chars(message_state_label(locale, state), 10),
                    }],
                    "selected_id": "0",
                })]
            } else {
                form_selection.selections.iter().map(|selection| json!({
                    "question_key": selection.selector_key,
                    "title": truncate_chars(message_state_label(locale, state), 13),
                    "disable": true,
                    "option_list": selection.option_ids.iter().map(|option_id| json!({
                        "id": option_id,
                        "text": truncate_chars(message_state_label(locale, state), 10),
                    })).collect::<Vec<_>>(),
                    "selected_id": selection.option_ids.first().cloned().unwrap_or_else(|| "0".into()),
                })).collect::<Vec<_>>()
            },
        }),
        _ => json!({
            "card_type": "text_notice",
            "main_title": { "title": truncate_chars(&title, 26) },
            "task_id": task_id,
        }),
    };
    template_card["source"] = json!({ "desc": "Gold Band" });
    template_card["main_title"] = json!({
        "title": truncate_chars(&title, 26),
        "desc": desc,
    });
    if template_card.get("sub_title_text").is_none() {
        template_card["sub_title_text"] =
            Value::String(localized(locale, "im.elicitation.vote.linkedSubtitle").to_owned());
    }
    template_card["submit_button"] = json!({
        "text": localized(locale, "im.permission.submitted"),
        "key": format!("{task_id}:submit"),
    });
    template_card["task_id"] = Value::String(task_id.to_owned());
    json!({
        "cmd": "aibot_respond_update_msg",
        "headers": { "req_id": req_id },
        "body": {
            "response_type": "update_template_card",
            "template_card": template_card,
        },
    })
}

fn selected_vote_index(selection: &ImWeComVoteSelection) -> Option<usize> {
    let id = selection.selected_option_id.as_str();
    if id.len() > WECOM_MAX_OPTION_ID_BYTES {
        return None;
    }
    let index = id.parse().ok()?;
    selection.vote_actions.get(index).map(|_| index)
}

fn vote_terminal_title(
    locale: ImLocale,
    state: ImMessageState,
    selected_action: Option<&InterventionAllowedAction>,
    display_ref: Option<u16>,
) -> String {
    let summary = selected_action.map(|action| super::action_label(locale, action));
    terminal_title_with_leading_ref(
        locale,
        state,
        summary.as_deref().unwrap_or_default(),
        display_ref,
    )
}

fn terminal_title_with_leading_ref(
    locale: ImLocale,
    state: ImMessageState,
    summary: &str,
    display_ref: Option<u16>,
) -> String {
    let Some(display_ref) = display_ref else {
        return if state == ImMessageState::Handled && !summary.is_empty() {
            let prefix = match locale {
                ImLocale::ZhCn => "已处理：",
                ImLocale::En => "Handled: ",
            };
            format!("{prefix}{}", truncate_chars(summary, 22))
        } else {
            message_state_label(locale, state).to_owned()
        };
    };
    let id_prefix = format!("id={display_ref:04} ");
    if state != ImMessageState::Handled || summary.is_empty() {
        return format!("{id_prefix}{}", message_state_label(locale, state));
    }
    let state_prefix = match locale {
        ImLocale::ZhCn => "已处理：",
        ImLocale::En => "Handled: ",
    };
    let max_summary_chars = 26usize
        .saturating_sub(id_prefix.chars().count())
        .saturating_sub(state_prefix.chars().count());
    format!(
        "{id_prefix}{state_prefix}{}",
        truncate_chars(summary, max_summary_chars)
    )
}

async fn handle_callback(
    generation: u64,
    events: &mpsc::Sender<ImConnectorEvent>,
    commands: &mpsc::Sender<WeComCommand>,
    value: Value,
) -> Result<(), ImIntegrationError> {
    match value.get("cmd").and_then(Value::as_str) {
        Some("aibot_msg_callback") => {
            if let Some(binding) = observed_binding(&value) {
                let _ = events
                    .send(ImConnectorEvent::BindingObserved {
                        generation,
                        binding,
                    })
                    .await;
            }
        }
        Some("aibot_event_callback") => {
            if value
                .pointer("/body/event/eventtype")
                .and_then(Value::as_str)
                == Some("disconnected_event")
            {
                return Err(ImIntegrationError::permanent(
                    ImErrorCode::ConnectionConflict,
                ));
            }
            if value
                .pointer("/body/event/eventtype")
                .and_then(Value::as_str)
                != Some("template_card_event")
            {
                return Ok(());
            }
            match action_envelope(&value) {
                Ok((envelope, response_context)) => {
                    let _ = events
                        .send(ImConnectorEvent::InboundAction {
                            generation,
                            envelope,
                            response_context,
                        })
                        .await;
                }
                Err(failure) => {
                    let feedback_queued = failure
                        .response_context
                        .as_ref()
                        .filter(|_| failure.private_candidate)
                        .map(|context| queue_failed_action_response(commands, context))
                        .unwrap_or(false);
                    tracing::warn!(
                        channel = ImChannelKind::WeCom.as_str(),
                        generation,
                        parse_reason = failure.reason.as_str(),
                        private_candidate = failure.private_candidate,
                        has_response_context = failure.response_context.is_some(),
                        feedback_queued,
                        error_code = ImErrorCode::ProtocolInvalid.as_str(),
                        "invalid IM action callback rejected"
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn queue_failed_action_response(
    commands: &mpsc::Sender<WeComCommand>,
    context: &ImActionResponseContext,
) -> bool {
    let (response, _) = oneshot::channel();
    commands
        .try_send(WeComCommand::RespondToAction {
            context: context.clone(),
            state: ImMessageState::Failed,
            response,
        })
        .is_ok()
}

fn observed_binding(value: &Value) -> Option<ImObservedBinding> {
    let body = value.get("body")?;
    let actor_id = body
        .pointer("/from/userid")
        .or_else(|| body.get("userid"))?
        .as_str()?
        .to_owned();
    let conversation_id = body
        .get("chatid")
        .and_then(Value::as_str)
        .unwrap_or(actor_id.as_str())
        .to_owned();
    let has_chat_id = body
        .get("chatid")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let is_private = if body
        .get("chat_type")
        .and_then(Value::as_i64)
        .is_some_and(|value| value == 1)
    {
        true
    } else {
        match body.get("chattype").and_then(Value::as_str) {
            Some(value) => matches!(value, "single" | "private"),
            None => !has_chat_id,
        }
    };
    Some(ImObservedBinding {
        destination_id: actor_id.clone(),
        conversation_id,
        actor_id,
        is_private,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeComActionParseReason {
    BodyMissing,
    MsgIdMissing,
    ChatTypeInvalid,
    FromUserMissing,
    EventTypeInvalid,
    EventKeyMissing,
    EventKeyInvalid,
    EventKeyDecodeFailed,
    CardTypeInvalid,
    TaskIdMissing,
    SelectionMissing,
    SelectionInvalid,
    TaskMismatch,
    ReqIdMissing,
}

impl WeComActionParseReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::BodyMissing => "BODY_MISSING",
            Self::MsgIdMissing => "MSGID_MISSING",
            Self::ChatTypeInvalid => "CHAT_TYPE_INVALID",
            Self::FromUserMissing => "FROM_USER_MISSING",
            Self::EventTypeInvalid => "EVENT_TYPE_INVALID",
            Self::EventKeyMissing => "EVENT_KEY_MISSING",
            Self::EventKeyInvalid => "EVENT_KEY_INVALID",
            Self::EventKeyDecodeFailed => "EVENT_KEY_DECODE_FAILED",
            Self::CardTypeInvalid => "CARD_TYPE_INVALID",
            Self::TaskIdMissing => "TASK_ID_MISSING",
            Self::SelectionMissing => "SELECTION_MISSING",
            Self::SelectionInvalid => "SELECTION_INVALID",
            Self::TaskMismatch => "TASK_MISMATCH",
            Self::ReqIdMissing => "REQ_ID_MISSING",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeComActionParseFailure {
    reason: WeComActionParseReason,
    private_candidate: bool,
    response_context: Option<ImActionResponseContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeComActionReference {
    delivery_id: String,
    action_index: usize,
}

fn action_envelope(
    value: &Value,
) -> Result<(ImInboundEnvelope, ImActionResponseContext), WeComActionParseFailure> {
    let Some(body) = value.get("body") else {
        return Err(action_parse_failure(
            WeComActionParseReason::BodyMissing,
            false,
            None,
        ));
    };
    let Some(platform_event_id) = non_empty_json_str(body.get("msgid")).map(str::to_owned) else {
        return Err(action_parse_failure(
            WeComActionParseReason::MsgIdMissing,
            false,
            callback_response_context(value),
        ));
    };
    let private_candidate = private_action_callback(body);
    let parse_failure =
        |reason| action_parse_failure(reason, private_candidate, callback_response_context(value));
    if !private_candidate {
        return Err(parse_failure(WeComActionParseReason::ChatTypeInvalid));
    }
    let Some(actor_id) = non_empty_json_str(body.pointer("/from/userid")).map(str::to_owned) else {
        return Err(parse_failure(WeComActionParseReason::FromUserMissing));
    };
    let Some(event) = body.get("event") else {
        return Err(parse_failure(WeComActionParseReason::EventTypeInvalid));
    };
    if event.get("eventtype").and_then(Value::as_str) != Some("template_card_event") {
        return Err(parse_failure(WeComActionParseReason::EventTypeInvalid));
    }
    let Some(card_event) = event.get("template_card_event") else {
        return Err(parse_failure(WeComActionParseReason::EventKeyMissing));
    };
    let card_type = card_event
        .get("card_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (delivery_id, action, task_id, vote_selection, form_selection) = match card_type {
        "button_interaction" => {
            let Some(raw_action) = card_event.get("event_key") else {
                return Err(parse_failure(WeComActionParseReason::EventKeyMissing));
            };
            let action = decode_event_action(raw_action)
                .ok_or_else(|| parse_failure(WeComActionParseReason::EventKeyDecodeFailed))?;
            let callback_task_id = non_empty_json_str(card_event.get("task_id")).map(str::to_owned);
            let task_id = callback_task_id.unwrap_or_else(|| action.delivery_id.clone());
            if task_id != action.delivery_id {
                return Err(parse_failure(WeComActionParseReason::TaskMismatch));
            }
            (
                action.delivery_id,
                ImInboundActionSelection::LocalIndex {
                    index: action.action_index,
                },
                task_id,
                None,
                None,
            )
        }
        "vote_interaction" => {
            let Some(raw_action) = card_event.get("event_key") else {
                return Err(parse_failure(WeComActionParseReason::EventKeyMissing));
            };
            let reference = decode_submit_action(raw_action)
                .ok_or_else(|| parse_failure(WeComActionParseReason::EventKeyInvalid))?;
            let Some(task_id) = non_empty_json_str(card_event.get("task_id")).map(str::to_owned)
            else {
                return Err(parse_failure(WeComActionParseReason::TaskIdMissing));
            };
            if task_id != reference.delivery_id {
                return Err(parse_failure(WeComActionParseReason::TaskMismatch));
            }
            let selections =
                selected_form_items(card_event).map_err(|reason| parse_failure(reason))?;
            if let [selection] = selections.as_slice()
                && selection.selector_key == WECOM_VOTE_QUESTION_KEY
            {
                let [option_id] = selection.option_ids.as_slice() else {
                    return Err(parse_failure(WeComActionParseReason::SelectionInvalid));
                };
                let action_index = option_id
                    .parse()
                    .map_err(|_| parse_failure(WeComActionParseReason::SelectionInvalid))?;
                (
                    reference.delivery_id,
                    ImInboundActionSelection::LocalIndex {
                        index: action_index,
                    },
                    task_id,
                    Some(ImWeComVoteSelection {
                        selected_option_id: option_id.clone(),
                        vote_actions: Vec::new(),
                        display_ref: None,
                    }),
                    None,
                )
            } else if let [selection] = selections.as_slice()
                && selection.selector_key == "q0"
            {
                let card_kind = if selection.option_ids.is_empty() || selection.option_ids.len() > 1
                {
                    ImWeComFormCardKind::VoteMulti
                } else {
                    ImWeComFormCardKind::VoteSingle
                };
                (
                    reference.delivery_id,
                    ImInboundActionSelection::Form {
                        selections: selections.clone(),
                    },
                    task_id,
                    None,
                    Some(ImWeComFormSelection {
                        card_kind,
                        selections,
                        form: None,
                        display_ref: None,
                    }),
                )
            } else {
                return Err(parse_failure(WeComActionParseReason::SelectionInvalid));
            }
        }
        "multiple_interaction" => {
            let Some(raw_action) = card_event.get("event_key") else {
                return Err(parse_failure(WeComActionParseReason::EventKeyMissing));
            };
            let reference = decode_submit_action(raw_action)
                .ok_or_else(|| parse_failure(WeComActionParseReason::EventKeyInvalid))?;
            let Some(task_id) = non_empty_json_str(card_event.get("task_id")).map(str::to_owned)
            else {
                return Err(parse_failure(WeComActionParseReason::TaskIdMissing));
            };
            if task_id != reference.delivery_id {
                return Err(parse_failure(WeComActionParseReason::TaskMismatch));
            }
            let selections =
                selected_form_items(card_event).map_err(|reason| parse_failure(reason))?;
            if selections.is_empty() || selections.len() > 3 {
                return Err(parse_failure(WeComActionParseReason::SelectionInvalid));
            }
            (
                reference.delivery_id,
                ImInboundActionSelection::Form {
                    selections: selections.clone(),
                },
                task_id,
                None,
                Some(ImWeComFormSelection {
                    card_kind: ImWeComFormCardKind::Multiple,
                    selections,
                    form: None,
                    display_ref: None,
                }),
            )
        }
        _ => return Err(parse_failure(WeComActionParseReason::CardTypeInvalid)),
    };
    if !valid_wecom_task_id(&task_id) {
        return Err(parse_failure(WeComActionParseReason::TaskMismatch));
    }
    let Some(req_id) = non_empty_json_str(value.pointer("/headers/req_id")) else {
        return Err(parse_failure(WeComActionParseReason::ReqIdMissing));
    };
    Ok((
        ImInboundEnvelope {
            action_id: deterministic_action_id(ImChannelKind::WeCom, &platform_event_id),
            channel: ImChannelKind::WeCom,
            platform_event_id,
            conversation_id: actor_id.clone(),
            actor_id,
            delivery_id,
            action,
        },
        ImActionResponseContext::WeCom {
            req_id: req_id.to_owned(),
            task_id,
            vote_selection,
            form_selection,
        },
    ))
}

fn action_parse_failure(
    reason: WeComActionParseReason,
    private_candidate: bool,
    response_context: Option<ImActionResponseContext>,
) -> WeComActionParseFailure {
    WeComActionParseFailure {
        reason,
        private_candidate,
        response_context,
    }
}

fn private_action_callback(body: &Value) -> bool {
    match body.get("chattype").and_then(Value::as_str) {
        Some("single" | "private") => true,
        Some(_) => false,
        None => body
            .get("chatid")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty()),
    }
}

fn callback_response_context(value: &Value) -> Option<ImActionResponseContext> {
    let task_id = match non_empty_json_str(value.pointer("/body/event/template_card_event/task_id"))
    {
        Some(task_id) => task_id.to_owned(),
        None => {
            decode_event_action(value.pointer("/body/event/template_card_event/event_key")?)?
                .delivery_id
        }
    };
    Some(ImActionResponseContext::WeCom {
        req_id: non_empty_json_str(value.pointer("/headers/req_id"))?.to_owned(),
        task_id,
        vote_selection: permission_vote_callback_context(value),
        form_selection: form_callback_response_context(value),
    })
}

fn permission_vote_callback_context(value: &Value) -> Option<ImWeComVoteSelection> {
    let card_event = value.pointer("/body/event/template_card_event")?;
    if card_event.get("card_type").and_then(Value::as_str) != Some("vote_interaction") {
        return None;
    }
    let selections = selected_form_items(card_event).ok()?;
    let [selection] = selections.as_slice() else {
        return None;
    };
    if selection.selector_key != WECOM_VOTE_QUESTION_KEY || selection.option_ids.len() != 1 {
        return None;
    }
    Some(ImWeComVoteSelection {
        selected_option_id: selection.option_ids.first()?.clone(),
        vote_actions: Vec::new(),
        display_ref: None,
    })
}

fn form_callback_response_context(value: &Value) -> Option<ImWeComFormSelection> {
    let card_event = value.pointer("/body/event/template_card_event")?;
    let selections = selected_form_items(card_event).ok()?;
    let card_kind = match card_event.get("card_type").and_then(Value::as_str)? {
        "vote_interaction" => {
            let [selection] = selections.as_slice() else {
                return None;
            };
            if selection.selector_key == WECOM_VOTE_QUESTION_KEY {
                return None;
            }
            if selection.option_ids.len() > 1 || selection.option_ids.is_empty() {
                ImWeComFormCardKind::VoteMulti
            } else {
                ImWeComFormCardKind::VoteSingle
            }
        }
        "multiple_interaction" => ImWeComFormCardKind::Multiple,
        _ => return None,
    };
    Some(ImWeComFormSelection {
        card_kind,
        selections,
        form: None,
        display_ref: None,
    })
}

fn selected_form_items(
    card_event: &Value,
) -> Result<Vec<RemoteElicitationFormSelection>, WeComActionParseReason> {
    let Some(items) = card_event
        .pointer("/selected_items/selected_item")
        .and_then(Value::as_array)
    else {
        return Err(WeComActionParseReason::SelectionMissing);
    };
    let mut selectors = std::collections::BTreeSet::new();
    let mut selections = Vec::with_capacity(items.len());
    for item in items {
        let question_key = item
            .get("question_key")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or(WeComActionParseReason::SelectionInvalid)?;
        if !selectors.insert(question_key.to_owned()) {
            return Err(WeComActionParseReason::SelectionInvalid);
        }
        let Some(option_values) = item
            .pointer("/option_ids/option_id")
            .and_then(Value::as_array)
        else {
            return Err(WeComActionParseReason::SelectionMissing);
        };
        let mut option_ids = Vec::with_capacity(option_values.len());
        let mut seen_options = std::collections::BTreeSet::new();
        for option_value in option_values {
            let option_id = option_value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or(WeComActionParseReason::SelectionInvalid)?;
            if option_id.len() > 2
                || !matches!(option_id.parse::<usize>(), Ok(index) if index < WECOM_MAX_VOTE_OPTIONS)
                || !seen_options.insert(option_id.to_owned())
            {
                return Err(WeComActionParseReason::SelectionInvalid);
            }
            option_ids.push(option_id.to_owned());
        }
        selections.push(RemoteElicitationFormSelection {
            selector_key: question_key.to_owned(),
            option_ids,
        });
    }
    Ok(selections)
}

fn decode_event_action(raw_action: &Value) -> Option<WeComActionReference> {
    let encoded = raw_action.as_str()?;
    if encoded.len() > WECOM_MAX_BUTTON_KEY_BYTES {
        return None;
    }
    let (delivery_id, action_index) = encoded.rsplit_once(':')?;
    if delivery_id.is_empty() {
        return None;
    }
    Some(WeComActionReference {
        delivery_id: delivery_id.to_owned(),
        action_index: action_index.parse().ok()?,
    })
}

fn decode_submit_action(raw_action: &Value) -> Option<WeComActionReference> {
    let encoded = raw_action.as_str()?;
    if encoded.len() > WECOM_MAX_BUTTON_KEY_BYTES {
        return None;
    }
    let (delivery_id, suffix) = encoded.rsplit_once(':')?;
    if delivery_id.is_empty() || suffix != "submit" || !valid_wecom_task_id(delivery_id) {
        return None;
    }
    Some(WeComActionReference {
        delivery_id: delivery_id.to_owned(),
        action_index: 0,
    })
}

fn non_empty_json_str(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn settle_response(request: PendingRequest, value: &Value) {
    if let Some(error) = response_error(value) {
        if matches!(request, PendingRequest::ActionResponse { .. }) {
            log_rejected_action_response(value, &error);
        }
        fail_request(request, error);
        return;
    }
    match request {
        PendingRequest::Send { delivery, response } => {
            let body = value.get("body").unwrap_or(&Value::Null);
            let message_id = body
                .get("msgid")
                .or_else(|| body.get("msg_id"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let _ = response.send(Ok(ImDeliveryReceipt {
                platform_message_id: message_id,
                platform_chat_id: delivery.destination.conversation_id,
                update_token_ref: None,
            }));
        }
        PendingRequest::ActionResponse { response } => {
            let _ = response.send(Ok(()));
        }
        PendingRequest::LinkedDetail { response, .. } => {
            let _ = response.send(Err(ImIntegrationError::permanent(
                ImErrorCode::ProtocolInvalid,
            )));
        }
    }
}

fn response_error(value: &Value) -> Option<ImIntegrationError> {
    let Some(code) = value.get("errcode").and_then(Value::as_i64) else {
        return Some(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid));
    };
    match code {
        0 => None,
        40001 | 40014 | 42001 => Some(
            ImIntegrationError::permanent(ImErrorCode::AuthenticationRequired)
                .with_platform_code(code),
        ),
        45009 => Some(
            ImIntegrationError::retryable(ImErrorCode::RateLimited, Some(Duration::from_secs(1)))
                .with_platform_code(code),
        ),
        _ => Some(
            ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid).with_platform_code(code),
        ),
    }
}

fn log_rejected_action_response(value: &Value, error: &ImIntegrationError) {
    let field_names = |object: Option<&serde_json::Map<String, Value>>| {
        object
            .map(|object| {
                let mut names = object.keys().map(String::as_str).collect::<Vec<_>>();
                names.sort_unstable();
                names.join(",")
            })
            .unwrap_or_default()
    };
    let json_type = |value: Option<&Value>| match value {
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "bool",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
        None => "missing",
    };
    tracing::warn!(
        channel = ImChannelKind::WeCom.as_str(),
        error_code = error.code.as_str(),
        retryable = error.retryable,
        platform_code = ?error.platform_code,
        has_cmd = value.get("cmd").is_some(),
        top_level_fields = %field_names(value.as_object()),
        body_fields = %field_names(value.get("body").and_then(|body| body.as_object())),
        errcode_type = json_type(value.get("errcode")),
        body_errcode_type = json_type(value.pointer("/body/errcode")),
        "WeCom action response ACK rejected"
    );
}

fn parse_json_message(message: &Message) -> Result<Option<Value>, ImIntegrationError> {
    match message {
        Message::Text(text) => serde_json::from_str(text.as_str())
            .map(Some)
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid)),
        Message::Binary(bytes) => serde_json::from_slice(bytes)
            .map(Some)
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid)),
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_) => Ok(None),
    }
}

fn header_req_id(value: &Value) -> Option<&str> {
    value.pointer("/headers/req_id").and_then(Value::as_str)
}

fn is_request_ack(value: &Value) -> bool {
    value.get("cmd").is_none()
}

fn fail_pending(pending: HashMap<String, PendingRequest>) {
    for request in pending.into_values() {
        fail_request(
            request,
            ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None),
        );
    }
}

fn fail_request(request: PendingRequest, error: ImIntegrationError) {
    match request {
        PendingRequest::Send { response, .. } => {
            let _ = response.send(Err(error));
        }
        PendingRequest::LinkedDetail { response, .. } => {
            let _ = response.send(Err(error));
        }
        PendingRequest::ActionResponse { response } => {
            let _ = response.send(Err(error));
        }
    }
}

fn request_id() -> String {
    Uuid::new_v4().to_string()
}

fn reconnect_delay(reconnect: usize) -> Duration {
    let shift = u32::try_from(reconnect.min(5)).unwrap_or(5);
    Duration::from_secs(1_u64 << shift).min(WECOM_MAX_RECONNECT_DELAY)
}

fn non_empty(value: &str) -> Result<String, ImIntegrationError> {
    (!value.trim().is_empty())
        .then(|| value.to_owned())
        .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ConfigInvalid))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_none() {
        return prefix;
    }
    let keep = max_chars.saturating_sub(3);
    format!("{}...", value.chars().take(keep).collect::<String>())
}

fn truncate_utf8_bytes(value: &str) -> String {
    const MARKER: &str = "...[truncated]";
    if value.len() <= MAX_IM_PRESENTATION_BYTES {
        return value.to_owned();
    }
    let keep_limit = MAX_IM_PRESENTATION_BYTES.saturating_sub(MARKER.len());
    let cut = value
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| *end <= keep_limit)
        .last()
        .unwrap_or(0);
    let mut truncated = value[..cut].to_owned();
    truncated.push_str(MARKER);
    truncated
}

fn map_websocket_error(_: tokio_tungstenite::tungstenite::Error) -> ImIntegrationError {
    ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::app::intervention::{
        InterventionAllowedAction, InterventionLocator, InterventionRequestIdentity,
        PermissionActionKind, RemoteElicitationForm, RemoteElicitationFormSelection,
        RemoteElicitationOption, RemoteScalarChoiceQuestion,
    };
    use crate::im::connectors::PlatformActionValue;
    use crate::im::{
        IM_PAYLOAD_VERSION, ImNavigationLocator, ImNotificationKind, InformationalNotification,
        InterventionPresentation, InterventionQuestion, InterventionQuestionKind,
        InterventionQuestionOption, InterventionRef, InterventionResolutionNotification,
    };

    use super::*;

    fn sample_delivery(payload: ImDeliveryPayload, kind: ImNotificationKind) -> ImDelivery {
        ImDelivery {
            delivery_id: "delivery-1".into(),
            channel: ImChannelKind::WeCom,
            destination: crate::im::ImDestination {
                destination_id: "user-1".into(),
                conversation_id: "chat-1".into(),
                authorized_actor_id: "user-1".into(),
            },
            notification_kind: kind,
            canonical_event_id: "event-1".into(),
            payload,
            expires_at_ms: i64::MAX,
            display_ref: None,
        }
    }

    fn official_action_callback(
        action: &WeComActionReference,
        req_id: &str,
        msg_id: &str,
        task_id: &str,
        chat_type: Option<&str>,
    ) -> Value {
        let mut callback = json!({
            "cmd": "aibot_event_callback",
            "headers": { "req_id": req_id },
            "body": {
                "msgid": msg_id,
                "chattype": chat_type,
                "from": { "userid": "single-user" },
                "event": {
                    "eventtype": "template_card_event",
                    "template_card_event": {
                        "card_type": "button_interaction",
                        "event_key": format!("{}:{}", action.delivery_id, action.action_index),
                        "task_id": task_id
                    }
                }
            }
        });
        if chat_type.is_none()
            && let Some(body) = callback.get_mut("body").and_then(Value::as_object_mut)
        {
            body.remove("chattype");
        }
        callback
    }

    fn permission_delivery(allowed_actions: Vec<InterventionAllowedAction>) -> ImDelivery {
        let mut delivery = sample_delivery(
            ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: InterventionRef {
                    locator: InterventionLocator {
                        project_id: "p".into(),
                        task_id: "t".into(),
                        run_id: "r".into(),
                        round_id: "round".into(),
                        node_id: "n".into(),
                        attempt_id: "a".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request: InterventionRequestIdentity::Permission {
                        request_id: "req".into(),
                    },
                    expected_state: "state".into(),
                    allowed_actions,
                    expires_at_ms: None,
                },
                presentation: InterventionPresentation {
                    title_key: "im.notification.permission.title".into(),
                    summary_key: "im.notification.permission.summary".into(),
                    title: Some("Make edits?".into()),
                    summary: Some("command failed; retry without sandbox?".into()),
                    body: Some("command failed; retry without sandbox?".into()),
                    context: None,
                    fields: BTreeMap::from([
                        ("permissionTitle".into(), "Make edits?".into()),
                        ("permissionTool".into(), "Edit files".into()),
                        ("permissionCommand".into(), "cargo build --release".into()),
                        ("permissionPath".into(), "D:\\work".into()),
                        ("permissionParameter".into(), "fallback".into()),
                    ]),
                    questions: Vec::new(),
                },
            },
            ImNotificationKind::Permission,
        );
        delivery.display_ref = Some(7);
        delivery
    }

    fn manual_check_delivery() -> ImDelivery {
        let mut delivery = sample_delivery(
            ImDeliveryPayload::Intervention {
                version: IM_PAYLOAD_VERSION,
                reference: InterventionRef {
                    locator: InterventionLocator {
                        project_id: "p".into(),
                        task_id: "t".into(),
                        run_id: "r".into(),
                        round_id: "round".into(),
                        node_id: "n".into(),
                        attempt_id: "a".into(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                    },
                    request: InterventionRequestIdentity::ManualCheck,
                    expected_state: "state".into(),
                    allowed_actions: vec![
                        InterventionAllowedAction::ManualSuccess,
                        InterventionAllowedAction::ManualFailure,
                    ],
                    expires_at_ms: None,
                },
                presentation: InterventionPresentation {
                    title_key: "im.notification.manualCheck.title".into(),
                    summary_key: "im.notification.manualCheck.summary".into(),
                    title: None,
                    summary: None,
                    body: Some("latest model output".into()),
                    context: None,
                    fields: BTreeMap::from([
                        ("nodeLabel".into(), "Interview".into()),
                        ("taskTitle".into(), "Prepare release".into()),
                    ]),
                    questions: Vec::new(),
                },
            },
            ImNotificationKind::ManualCheck,
        );
        delivery.display_ref = Some(123);
        delivery
    }

    fn desktop_resolution_delivery(kind: ImNotificationKind) -> ImDelivery {
        let mut delivery = sample_delivery(
            ImDeliveryPayload::InterventionResolution {
                version: IM_PAYLOAD_VERSION,
                resolution: InterventionResolutionNotification {
                    canonical_event_id: "event-1:desktop-resolved".into(),
                    source_event_id: "event-1".into(),
                    notification_kind: kind,
                    locator: ImNavigationLocator {
                        project_id: "p".into(),
                        task_id: Some("t".into()),
                        run_id: Some("r".into()),
                        round_id: Some("round".into()),
                        node_id: Some("n".into()),
                        attempt_id: Some("a".into()),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    display_ref: Some(12),
                },
            },
            kind,
        );
        delivery.canonical_event_id = "event-1:desktop-resolved".into();
        delivery.display_ref = Some(12);
        delivery
    }

    fn permission_option(
        option_id: &str,
        name: &str,
        kind: PermissionActionKind,
    ) -> InterventionAllowedAction {
        InterventionAllowedAction::PermissionOption {
            option_id: option_id.into(),
            name: name.into(),
            permission_kind: kind,
        }
    }

    fn official_vote_callback(
        delivery_id: &str,
        selected_option_id: &str,
        req_id: &str,
        msg_id: &str,
        task_id: Option<&str>,
    ) -> Value {
        json!({
            "cmd": "aibot_event_callback",
            "headers": { "req_id": req_id },
            "body": {
                "msgid": msg_id,
                "chattype": "single",
                "from": { "userid": "single-user" },
                "event": {
                    "eventtype": "template_card_event",
                    "template_card_event": {
                        "card_type": "vote_interaction",
                        "event_key": format!("{delivery_id}:submit"),
                        "task_id": task_id.unwrap_or(delivery_id),
                        "selected_items": {
                            "selected_item": [{
                                "question_key": WECOM_VOTE_QUESTION_KEY,
                                "option_ids": { "option_id": [selected_option_id] }
                            }]
                        }
                    }
                }
            }
        })
    }

    fn official_form_callback(
        card_type: &str,
        selections: &[RemoteElicitationFormSelection],
        req_id: &str,
        msg_id: &str,
        delivery_id: &str,
    ) -> Value {
        json!({
            "cmd": "aibot_event_callback",
            "headers": { "req_id": req_id },
            "body": {
                "msgid": msg_id,
                "chattype": "single",
                "from": { "userid": "single-user" },
                "event": {
                    "eventtype": "template_card_event",
                    "template_card_event": {
                        "card_type": card_type,
                        "event_key": format!("{delivery_id}:submit"),
                        "task_id": delivery_id,
                        "selected_items": {
                            "selected_item": selections.iter().map(|selection| json!({
                                "question_key": selection.selector_key,
                                "option_ids": { "option_id": selection.option_ids }
                            })).collect::<Vec<_>>()
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn official_subscribe_and_proactive_send_shapes_are_stable() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let delivery = sample_delivery(
            ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: "event-1".into(),
                    notification_kind: ImNotificationKind::RunFailure,
                    locator: crate::im::ImNavigationLocator {
                        project_id: "p".into(),
                        task_id: None,
                        run_id: None,
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "failure".into(),
                    summary_key: "summary".into(),
                    parameters: BTreeMap::new(),
                    missed_count: None,
                },
            },
            ImNotificationKind::RunFailure,
        );
        let request = send_request("req-1", &delivery, &codec, ImLocale::ZhCn).unwrap();
        assert_eq!(request["cmd"], "aibot_send_msg");
        assert!(request["body"].get("chat_type").is_none());
        assert_eq!(request["body"]["msgtype"], "markdown");
        assert!(request["body"].get("template_card").is_none());
    }

    #[test]
    fn desktop_resolution_is_a_proactive_markdown_without_actions() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let delivery = desktop_resolution_delivery(ImNotificationKind::Permission);
        let request = send_request("req-1", &delivery, &codec, ImLocale::ZhCn).unwrap();
        assert_eq!(request["body"]["msgtype"], "markdown");
        assert_eq!(
            request["body"]["markdown"]["content"],
            "**id=0012 已在桌面端处理**"
        );
        let encoded = request.to_string();
        for forbidden in ["template_card", "submit_button", "actionToken", "task_id"] {
            assert!(!encoded.contains(forbidden), "found {forbidden}");
        }
    }

    #[test]
    fn only_pending_interventions_use_the_linked_detail_flow() {
        assert!(delivery_requires_linked_detail(&permission_delivery(vec![
            permission_option("cancel", "No", PermissionActionKind::RejectOnce),
        ])));
        assert!(!delivery_requires_linked_detail(
            &desktop_resolution_delivery(ImNotificationKind::Elicitation)
        ));
    }

    #[tokio::test]
    async fn official_success_ack_does_not_require_a_message_id() {
        let delivery = sample_delivery(
            ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: "event-1".into(),
                    notification_kind: ImNotificationKind::RunSuccess,
                    locator: crate::im::ImNavigationLocator {
                        project_id: "p".into(),
                        task_id: None,
                        run_id: None,
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "success".into(),
                    summary_key: "summary".into(),
                    parameters: BTreeMap::new(),
                    missed_count: None,
                },
            },
            ImNotificationKind::RunSuccess,
        );
        let (sender, receiver) = oneshot::channel();
        settle_response(
            PendingRequest::Send {
                delivery,
                response: sender,
            },
            &json!({ "headers": { "req_id": "req-1" }, "errcode": 0, "errmsg": "ok" }),
        );
        let receipt = receiver.await.unwrap().unwrap();
        assert_eq!(receipt.platform_message_id, None);
    }

    #[tokio::test]
    async fn unknown_platform_error_keeps_only_the_numeric_diagnostic() {
        let delivery = sample_delivery(
            ImDeliveryPayload::Information {
                version: IM_PAYLOAD_VERSION,
                notification: InformationalNotification {
                    canonical_event_id: "event-1".into(),
                    notification_kind: ImNotificationKind::RunFailure,
                    locator: crate::im::ImNavigationLocator {
                        project_id: "p".into(),
                        task_id: None,
                        run_id: None,
                        round_id: None,
                        node_id: None,
                        attempt_id: None,
                        outer_node_id: None,
                        outer_attempt_id: None,
                        scheduled_occurrence_id: None,
                    },
                    outcome: "failure".into(),
                    summary_key: "summary".into(),
                    parameters: BTreeMap::new(),
                    missed_count: None,
                },
            },
            ImNotificationKind::RunFailure,
        );
        let (sender, receiver) = oneshot::channel();
        settle_response(
            PendingRequest::Send {
                delivery,
                response: sender,
            },
            &json!({ "headers": { "req_id": "req-1" }, "errcode": 49999, "errmsg": "sensitive platform text" }),
        );
        let error = receiver.await.unwrap().unwrap_err();
        assert_eq!(error.code, ImErrorCode::ProtocolInvalid);
        assert_eq!(error.platform_code, Some(49999));
        assert!(!error.retryable);
    }

    #[test]
    fn ack_without_top_level_errcode_is_rejected() {
        let error = response_error(&json!({
            "headers": { "req_id": "req-1" },
            "body": { "errcode": 0 }
        }))
        .unwrap();
        assert_eq!(error.code, ImErrorCode::ProtocolInvalid);
        assert_eq!(error.platform_code, None);
    }

    #[test]
    fn event_frame_with_pending_req_id_is_not_an_ack() {
        let event = json!({
            "cmd": "aibot_event_callback",
            "headers": { "req_id": "callback-req-1" },
            "body": { "event": { "eventtype": "template_card_event" } }
        });
        assert!(!is_request_ack(&event));

        let ack = json!({
            "headers": { "req_id": "callback-req-1" },
            "errcode": 0
        });
        assert!(is_request_ack(&ack));
    }

    #[test]
    fn permission_card_uses_single_select_vote_interaction() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let delivery = permission_delivery(vec![
            permission_option(
                "allow_once",
                "Yes, proceed",
                PermissionActionKind::AllowOnce,
            ),
            permission_option(
                "allow_for_session",
                "Allow for this session",
                PermissionActionKind::AllowAlways,
            ),
            permission_option("cancel", "Cancel", PermissionActionKind::RejectOnce),
        ]);
        let plan = linked_detail_send_plan(
            "detail-req-1",
            "card-req-1",
            &delivery,
            &codec,
            ImLocale::ZhCn,
        )
        .unwrap();
        let detail = &plan.detail_request;
        assert_eq!(detail["cmd"], "aibot_send_msg");
        assert_eq!(detail["body"]["msgtype"], "markdown");
        let detail_text = detail["body"]["markdown"]["content"].as_str().unwrap();
        assert!(detail_text.contains("权限审批（id=0007）"));
        assert!(detail_text.contains("Agent 请求执行命令"));
        assert!(detail_text.contains("command failed; retry without sandbox?"));
        assert!(detail_text.contains("- **工具**: Edit files"));
        assert!(detail_text.contains("- **命令**: cargo build --release"));
        assert!(detail_text.contains("- **路径**: D:\\work"));
        assert!(detail_text.contains("- **参数**: fallback"));
        assert!(detail_text.contains("下一条权限审批卡片"));
        let request = &plan.card_request;
        let card = &request["body"]["template_card"];
        assert_eq!(card["card_type"], "vote_interaction");
        assert_eq!(card["checkbox"]["question_key"], WECOM_VOTE_QUESTION_KEY);
        assert_eq!(card["checkbox"]["mode"], 0);
        assert!(card.get("button_list").is_none());
        let options = card["checkbox"]["option_list"].as_array().unwrap();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0]["id"], "1");
        assert_eq!(options[0]["text"], "记住选择");
        assert_eq!(options[0]["is_checked"], true);
        assert_eq!(options[1]["id"], "0");
        assert_eq!(options[1]["text"], "允许一次");
        assert_eq!(options[1]["is_checked"], false);
        assert_eq!(options[2]["id"], "2");
        assert_eq!(options[2]["text"], "拒绝");
        assert_eq!(options[2]["is_checked"], false);
        assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
        assert_eq!(card["submit_button"]["text"], "提交");
        assert_eq!(card["task_id"], delivery.delivery_id);
        assert_eq!(card["main_title"]["title"], "权限审批（id=0007）");
        assert_eq!(card["main_title"]["desc"], "Agent 请求执行命令");
        assert_eq!(
            card["sub_title_text"],
            "请求详情见上一条消息，请选择授权范围后提交"
        );
        assert_eq!(
            card["horizontal_content_list"],
            json!([
                { "keyname": "工具", "value": "Edit files" },
                { "keyname": "命令", "value": "cargo build --release" },
                { "keyname": "路径", "value": "D:\\work" },
                { "keyname": "参数", "value": "fallback" },
            ])
        );
        assert!(!card["main_title"]["title"].as_str().unwrap().contains('\n'));
        assert!(card["submit_button"]["key"].as_str().unwrap().len() <= WECOM_MAX_BUTTON_KEY_BYTES);
        assert!(
            options
                .iter()
                .all(|option| option["id"].as_str().unwrap().len() <= WECOM_MAX_OPTION_ID_BYTES)
        );
    }

    #[test]
    fn keep_planning_stays_last_even_when_the_provider_marks_it_as_allowing() {
        let options = vote_options(
            &[
                permission_option(
                    "plan",
                    "No, keep planning",
                    PermissionActionKind::AllowAlways,
                ),
                permission_option(
                    "allow_for_session",
                    "Allow for this session",
                    PermissionActionKind::AllowAlways,
                ),
            ],
            ImLocale::ZhCn,
        );

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].action_index, 1);
        assert_eq!(options[0].label, "记住选择");
        assert_eq!(options[1].action_index, 0);
        assert_eq!(options[1].label, "保持计划");
        assert!(options[1].safe_fallback);
    }

    #[test]
    fn permission_detail_and_vote_card_share_the_same_display_ref() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let mut delivery = permission_delivery(vec![permission_option(
            "allow_once",
            "Yes, proceed",
            PermissionActionKind::AllowOnce,
        )]);
        delivery.display_ref = Some(123);
        let plan = linked_detail_send_plan(
            "detail-req-1",
            "card-req-1",
            &delivery,
            &codec,
            ImLocale::En,
        )
        .unwrap();

        let detail = plan.detail_request["body"]["markdown"]["content"]
            .as_str()
            .unwrap();
        let card_title = &plan.card_request["body"]["template_card"]["main_title"]["title"];
        assert!(detail.contains("Permission (id=0123)"));
        assert_eq!(card_title, "Permission (id=0123)");
        assert_eq!(
            plan.card_request["body"]["template_card"]["task_id"],
            delivery.delivery_id
        );
    }

    #[test]
    fn manual_check_detail_and_vote_card_share_the_same_display_ref() {
        let delivery = manual_check_delivery();
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let plan = linked_detail_send_plan(
            "detail-req-1",
            "card-req-1",
            &delivery,
            &codec,
            ImLocale::ZhCn,
        )
        .unwrap();

        let detail = plan.detail_request["body"]["markdown"]["content"]
            .as_str()
            .unwrap();
        assert!(detail.contains("人工检查（id=0123）"));
        assert!(detail.contains("最后一轮模型输出"));
        assert!(detail.contains("latest model output"));

        let card = &plan.card_request["body"]["template_card"];
        assert_eq!(card["card_type"], "vote_interaction");
        assert_eq!(card["main_title"]["title"], "人工检查（id=0123）");
        assert_eq!(
            card["sub_title_text"],
            "模型输出见上一条消息，请选择检查结果后提交"
        );
        let options = card["checkbox"]["option_list"].as_array().unwrap();
        assert_eq!(options[0]["id"], "0");
        assert_eq!(options[0]["text"], "成功");
        assert!(options[0]["is_checked"].as_bool().unwrap());
        assert_eq!(options[1]["id"], "1");
        assert_eq!(options[1]["text"], "失败");
        assert_eq!(options[1]["is_checked"], false);
        assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
    }

    #[test]
    fn permission_card_without_durable_display_ref_is_rejected() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let mut delivery = permission_delivery(vec![permission_option(
            "allow_once",
            "Yes, proceed",
            PermissionActionKind::AllowOnce,
        )]);
        delivery.display_ref = None;

        let result = linked_detail_send_plan(
            "detail-req-1",
            "card-req-1",
            &delivery,
            &codec,
            ImLocale::ZhCn,
        );

        assert!(result.is_err());
    }

    #[test]
    fn overflowing_card_actions_keep_two_readable_actions() {
        let rendered = |option_id: &str| RenderedAction {
            label: option_id.into(),
            action_index: 0,
            permission_kind: Some(if option_id == "cancel" {
                PermissionActionKind::RejectOnce
            } else {
                PermissionActionKind::AllowOnce
            }),
            permission_qualifier: Some(PermissionActionQualifier::Standard),
            value: PlatformActionValue {
                delivery_id: "delivery-1".into(),
                action_token: "token-1".into(),
                action: crate::app::intervention::InterventionAction::PermissionOption {
                    option_id: option_id.into(),
                },
            },
        };
        let mut actions: Vec<_> = (1..=6)
            .enumerate()
            .map(|(index, number)| {
                let mut action = rendered(&format!("allow_{number}"));
                action.action_index = index;
                action
            })
            .collect();
        let mut cancel = rendered("cancel");
        cancel.action_index = actions.len();
        actions.push(cancel);

        let safe = safe_wecom_actions(actions);
        assert_eq!(safe.len(), WECOM_MAX_CARD_BUTTONS);
        assert!(matches!(
            &safe[0].value.action,
            crate::app::intervention::InterventionAction::PermissionOption { option_id }
                if option_id == "allow_1"
        ));
        assert!(matches!(
            &safe[1].value.action,
            crate::app::intervention::InterventionAction::PermissionOption { option_id }
                if option_id == "cancel"
        ));

        let mut reject_always = rendered("reject_always");
        reject_always.permission_kind = Some(PermissionActionKind::RejectAlways);
        assert_eq!(safe_wecom_actions(vec![reject_always]).len(), 1);
    }

    #[test]
    fn elicitation_linked_detail_uses_single_and_multi_vote_cards() {
        let codec = ImActionTokenCodec::new(vec![7; 32]).unwrap();
        let options = || {
            vec![
                RemoteElicitationOption {
                    value: json!("MySQL"),
                    label: "MySQL".into(),
                    description: Some("默认数据库".into()),
                },
                RemoteElicitationOption {
                    value: json!("PostgreSQL"),
                    label: "PostgreSQL".into(),
                    description: None,
                },
            ]
        };
        let question =
            |selector_key: &str, field_name: &str, options: Vec<RemoteElicitationOption>| {
                RemoteScalarChoiceQuestion {
                    selector_key: selector_key.into(),
                    field_name: field_name.into(),
                    title: "数据库选择".into(),
                    description: Some("请选择".into()),
                    required: true,
                    options,
                }
            };
        for (form, mode) in [
            (
                RemoteElicitationForm::SingleScalarChoice {
                    question: question("q0", "question_0", options()),
                },
                0,
            ),
            (
                RemoteElicitationForm::MultiScalarChoice {
                    question: question("q0", "features", options()),
                    allows_empty: true,
                },
                1,
            ),
        ] {
            let mut delivery = sample_delivery(
                ImDeliveryPayload::Intervention {
                    version: IM_PAYLOAD_VERSION,
                    reference: InterventionRef {
                        locator: InterventionLocator {
                            project_id: "p".into(),
                            task_id: "t".into(),
                            run_id: "r".into(),
                            round_id: "round".into(),
                            node_id: "n".into(),
                            attempt_id: "a".into(),
                            outer_node_id: None,
                            outer_attempt_id: None,
                        },
                        request: InterventionRequestIdentity::Elicitation {
                            elicitation_id: "elicit-1".into(),
                        },
                        expected_state: "state".into(),
                        allowed_actions: vec![InterventionAllowedAction::ElicitationFixedForm {
                            form: form.clone(),
                        }],
                        expires_at_ms: None,
                    },
                    presentation: InterventionPresentation {
                        title_key: "im.notification.elicitation.title".into(),
                        summary_key: "im.notification.elicitation.summary".into(),
                        title: None,
                        summary: None,
                        body: Some("请选择要使用的数据库？".into()),
                        context: Some("已有部署包含订单服务与用户服务。".into()),
                        fields: BTreeMap::from([
                            ("nodeLabel".into(), "Direct agent".into()),
                            ("taskTitle".into(), "Task".into()),
                        ]),
                        questions: vec![InterventionQuestion {
                            field_name: if mode == 0 { "question_0" } else { "features" }.into(),
                            title: "数据库选择".into(),
                            description: Some("请选择要使用的数据库？".into()),
                            question_kind: if mode == 0 {
                                InterventionQuestionKind::SingleSelect
                            } else {
                                InterventionQuestionKind::MultiSelect
                            },
                            options: options()
                                .into_iter()
                                .map(|option| InterventionQuestionOption {
                                    value: option.value,
                                    label: option.label,
                                    description: option.description,
                                })
                                .collect(),
                            allows_custom_answer: true,
                        }],
                    },
                },
                ImNotificationKind::Elicitation,
            );
            delivery.display_ref = Some(123);
            let plan = linked_detail_send_plan(
                "detail-req-1",
                "card-req-1",
                &delivery,
                &codec,
                ImLocale::ZhCn,
            )
            .unwrap();
            let detail = plan.detail_request["body"]["markdown"]["content"]
                .as_str()
                .unwrap();
            assert!(detail.contains("补充信息（id=0123）"));
            assert!(detail.contains("前序模型输出"));
            assert!(detail.contains("已有部署包含订单服务与用户服务。"));
            assert!(detail.contains("请选择要使用的数据库？"));
            assert_eq!(
                detail.matches("请选择要使用的数据库？").count(),
                1,
                "linked detail must render the request message exactly once"
            );
            assert!(detail.contains("其他答案请在桌面端填写"));
            let card = &plan.card_request["body"]["template_card"];
            assert_eq!(card["card_type"], "vote_interaction");
            assert_eq!(card["checkbox"]["question_key"], "q0");
            assert_eq!(card["checkbox"]["mode"], mode);
            assert_eq!(card["checkbox"]["option_list"][0]["id"], "0");
            assert_eq!(card["checkbox"]["option_list"][1]["id"], "1");
            assert!(card.get("button_list").is_none());
            assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
            assert_eq!(card["task_id"], "delivery-1");
            assert_eq!(card["main_title"]["title"], "补充信息（id=0123）");
        }
    }

    #[test]
    fn official_card_text_limits_are_applied_without_touching_action_identity() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("12345678901", 10), "1234567...");
        assert_eq!(truncate_chars("一二三四五六", 5), "一二...");
        let long = "一".repeat(MAX_IM_PRESENTATION_BYTES);
        let truncated = truncate_utf8_bytes(&long);
        assert!(truncated.len() <= MAX_IM_PRESENTATION_BYTES);
        assert!(truncated.ends_with("...[truncated]"));
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn private_binding_and_group_rejection_are_structural() {
        let private = observed_binding(&json!({
            "body": { "chatid": "chat", "chat_type": 1, "from": { "userid": "user" } }
        }))
        .unwrap();
        assert!(private.is_private);
        let group = observed_binding(&json!({
            "body": { "chatid": "group", "chat_type": 2, "from": { "userid": "user" } }
        }))
        .unwrap();
        assert!(!group.is_private);

        let single_without_chat_id = observed_binding(&json!({
            "body": { "chat_type": 1, "from": { "userid": "single-user" } }
        }))
        .unwrap();
        assert_eq!(single_without_chat_id.conversation_id, "single-user");
        assert_eq!(single_without_chat_id.destination_id, "single-user");

        let untyped_without_chat_id = observed_binding(&json!({
            "body": { "from": { "userid": "single-user" } }
        }))
        .unwrap();
        assert!(untyped_without_chat_id.is_private);
    }

    #[test]
    fn private_card_action_uses_sender_as_conversation_identity() {
        let action = WeComActionReference {
            delivery_id: "delivery-1".into(),
            action_index: 0,
        };
        let callback = official_action_callback(
            &action,
            "callback-req-1",
            "event-1",
            "delivery-1",
            Some("single"),
        );
        let (envelope, response_context) = action_envelope(&callback).unwrap();
        assert_eq!(envelope.conversation_id, "single-user");
        assert_eq!(envelope.actor_id, "single-user");
        assert_eq!(envelope.platform_event_id, "event-1");
        assert_eq!(
            response_context,
            ImActionResponseContext::WeCom {
                req_id: "callback-req-1".into(),
                task_id: "delivery-1".into(),
                vote_selection: None,
                form_selection: None,
            }
        );

        let update = action_response_request(
            "callback-req-1",
            "delivery-1",
            ImMessageState::Handled,
            ImLocale::ZhCn,
            None,
            None,
        );
        assert_eq!(update["headers"]["req_id"], "callback-req-1");
        assert_eq!(update["body"]["response_type"], "update_template_card");
        let card = &update["body"]["template_card"];
        assert_eq!(card["task_id"], "delivery-1");
        assert_eq!(card["card_type"], "button_interaction");
        assert_eq!(card["main_title"]["title"], "已处理");
        assert_eq!(card["button_list"][0]["text"], "已处理");
        assert_eq!(card["button_list"][0]["key"], "delivery-1:0");

        let repeated = official_action_callback(
            &action,
            "different-req-id",
            "event-1",
            "delivery-1",
            Some("single"),
        );
        assert_eq!(
            action_envelope(&repeated).unwrap().0.action_id,
            envelope.action_id,
            "msgid, not req_id, is the callback idempotency identity"
        );
        assert_eq!(
            action_envelope(&official_action_callback(
                &action,
                "req-2",
                "event-2",
                "wrong-delivery",
                Some("single"),
            ))
            .unwrap_err()
            .reason,
            WeComActionParseReason::TaskMismatch
        );
        assert_eq!(
            action_envelope(&official_action_callback(
                &action,
                "req-3",
                "event-3",
                "delivery-1",
                Some("group"),
            ))
            .unwrap_err()
            .reason,
            WeComActionParseReason::ChatTypeInvalid
        );
    }

    #[test]
    fn optional_chattype_without_chatid_is_a_private_callback() {
        let action = WeComActionReference {
            delivery_id: "delivery-1".into(),
            action_index: 0,
        };
        let callback =
            official_action_callback(&action, "callback-req-1", "event-1", "delivery-1", None);
        let (envelope, context) = action_envelope(&callback).unwrap();
        assert_eq!(envelope.conversation_id, "single-user");
        assert_eq!(envelope.actor_id, "single-user");
        assert_eq!(
            context,
            ImActionResponseContext::WeCom {
                req_id: "callback-req-1".into(),
                task_id: "delivery-1".into(),
                vote_selection: None,
                form_selection: None,
            }
        );

        let mut group_without_chattype = callback;
        group_without_chattype["body"]["chatid"] = json!("group-chat");
        assert_eq!(
            action_envelope(&group_without_chattype).unwrap_err().reason,
            WeComActionParseReason::ChatTypeInvalid
        );
    }

    #[test]
    fn missing_optional_task_id_derives_original_delivery_identity() {
        let action = WeComActionReference {
            delivery_id: "delivery-1".into(),
            action_index: 0,
        };
        let mut callback = official_action_callback(
            &action,
            "callback-req-1",
            "event-1",
            "delivery-1",
            Some("single"),
        );
        callback
            .pointer_mut("/body/event/template_card_event")
            .and_then(Value::as_object_mut)
            .expect("event object")
            .remove("task_id");

        let (envelope, context) = action_envelope(&callback).unwrap();
        assert_eq!(envelope.delivery_id, "delivery-1");
        assert_eq!(
            context,
            ImActionResponseContext::WeCom {
                req_id: "callback-req-1".into(),
                task_id: "delivery-1".into(),
                vote_selection: None,
                form_selection: None,
            }
        );
        assert_eq!(
            envelope.action,
            ImInboundActionSelection::LocalIndex { index: 0 }
        );
    }

    #[test]
    fn official_vote_callback_restores_the_unique_selected_action() {
        for (selected_option_id, expected_index) in [("0", 0_usize), ("1", 1_usize)] {
            let callback = official_vote_callback(
                "delivery-1",
                selected_option_id,
                "callback-req-1",
                "event-vote",
                None,
            );
            let (envelope, context) = action_envelope(&callback).unwrap();
            assert_eq!(envelope.delivery_id, "delivery-1");
            assert_eq!(
                envelope.action,
                ImInboundActionSelection::LocalIndex {
                    index: expected_index
                }
            );
            let ImActionResponseContext::WeCom {
                req_id,
                task_id,
                vote_selection,
                form_selection: _,
            } = context;
            assert_eq!(req_id, "callback-req-1");
            assert_eq!(task_id, "delivery-1");
            let selection = vote_selection.expect("vote selection");
            assert_eq!(selection.selected_option_id, selected_option_id);
            assert!(selection.vote_actions.is_empty());
        }
    }

    #[test]
    fn official_elicitation_form_callbacks_become_generic_selections() {
        let single = official_form_callback(
            "vote_interaction",
            &[RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["1".into()],
            }],
            "callback-req-1",
            "event-form-single",
            "delivery-1",
        );
        let (envelope, context) = action_envelope(&single).unwrap();
        assert_eq!(
            envelope.action,
            ImInboundActionSelection::Form {
                selections: vec![RemoteElicitationFormSelection {
                    selector_key: "q0".into(),
                    option_ids: vec!["1".into()],
                }]
            }
        );
        let ImActionResponseContext::WeCom {
            form_selection: Some(form_selection),
            ..
        } = context
        else {
            panic!("expected form context");
        };
        assert_eq!(form_selection.card_kind, ImWeComFormCardKind::VoteSingle);

        let multi = official_form_callback(
            "vote_interaction",
            &[RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["1".into(), "0".into()],
            }],
            "callback-req-1",
            "event-form-multi",
            "delivery-1",
        );
        let (_, context) = action_envelope(&multi).unwrap();
        let ImActionResponseContext::WeCom {
            form_selection: Some(form_selection),
            ..
        } = context
        else {
            panic!("expected form context");
        };
        assert_eq!(form_selection.card_kind, ImWeComFormCardKind::VoteMulti);
        assert_eq!(
            form_selection.selections[0].option_ids,
            vec!["1".to_string(), "0".to_string()]
        );

        let multiple = official_form_callback(
            "multiple_interaction",
            &[
                RemoteElicitationFormSelection {
                    selector_key: "q0".into(),
                    option_ids: vec!["0".into()],
                },
                RemoteElicitationFormSelection {
                    selector_key: "q1".into(),
                    option_ids: vec!["1".into()],
                },
            ],
            "callback-req-1",
            "event-form-multiple",
            "delivery-1",
        );
        let (envelope, context) = action_envelope(&multiple).unwrap();
        assert_eq!(
            envelope.delivery_id, "delivery-1",
            "task and submit key must resolve the same delivery"
        );
        let ImActionResponseContext::WeCom {
            form_selection: Some(form_selection),
            ..
        } = context
        else {
            panic!("expected form context");
        };
        assert_eq!(form_selection.card_kind, ImWeComFormCardKind::Multiple);
        assert_eq!(form_selection.selections.len(), 2);

        let duplicate = official_form_callback(
            "vote_interaction",
            &[RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["0".into(), "0".into()],
            }],
            "callback-req-1",
            "event-form-duplicate",
            "delivery-1",
        );
        assert_eq!(
            action_envelope(&duplicate).unwrap_err().reason,
            WeComActionParseReason::SelectionInvalid
        );
    }

    #[test]
    fn vote_callback_rejects_invalid_selections_and_task_identity() {
        let mut missing_task = official_vote_callback(
            "delivery-1",
            "0",
            "callback-req-1",
            "event-missing-task",
            None,
        );
        missing_task
            .pointer_mut("/body/event/template_card_event")
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove("task_id");
        assert_eq!(
            action_envelope(&missing_task).unwrap_err().reason,
            WeComActionParseReason::TaskIdMissing
        );

        assert_eq!(
            action_envelope(&official_vote_callback(
                "delivery-1",
                "0",
                "callback-req-2",
                "event-task-mismatch",
                Some("wrong-delivery"),
            ))
            .unwrap_err()
            .reason,
            WeComActionParseReason::TaskMismatch
        );

        let invalid_selections: [(&str, Box<dyn Fn(&mut Value)>, WeComActionParseReason); 7] = [
            (
                "missing selection",
                Box::new(|callback: &mut Value| {
                    callback
                        .pointer_mut("/body/event/template_card_event")
                        .and_then(Value::as_object_mut)
                        .unwrap()
                        .remove("selected_items");
                }),
                WeComActionParseReason::SelectionMissing,
            ),
            (
                "empty selection",
                Box::new(|callback: &mut Value| {
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"] =
                        json!([]);
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
            (
                "multiple selected items",
                Box::new(|callback: &mut Value| {
                    let selected = callback["body"]["event"]["template_card_event"]
                        ["selected_items"]["selected_item"]
                        .clone();
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"] =
                        json!([selected, selected]);
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
            (
                "multiple option ids",
                Box::new(|callback: &mut Value| {
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"]
                        [0]["option_ids"]["option_id"] = json!(["0", "1"]);
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
            (
                "wrong question",
                Box::new(|callback: &mut Value| {
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"]
                        [0]["question_key"] = json!("other_question");
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
            (
                "non-numeric option id",
                Box::new(|callback: &mut Value| {
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"]
                        [0]["option_ids"]["option_id"] = json!(["allow-once"]);
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
            (
                "out-of-range option id",
                Box::new(|callback: &mut Value| {
                    callback["body"]["event"]["template_card_event"]["selected_items"]["selected_item"]
                        [0]["option_ids"]["option_id"] = json!(["20"]);
                }),
                WeComActionParseReason::SelectionInvalid,
            ),
        ];
        for (case, mutate, expected_reason) in invalid_selections {
            let mut callback = official_vote_callback(
                "delivery-1",
                "0",
                "callback-req-invalid",
                "event-invalid-selection",
                None,
            );
            mutate(&mut callback);
            assert_eq!(
                action_envelope(&callback).unwrap_err().reason,
                expected_reason,
                "{case}"
            );
        }
    }

    #[test]
    fn successful_vote_update_stays_a_disabled_vote_card() {
        let selection = ImWeComVoteSelection {
            selected_option_id: "0".into(),
            vote_actions: vec![
                permission_option("allow_once", "Allow once", PermissionActionKind::AllowOnce),
                permission_option(
                    "allow_for_session",
                    "Allow for this session",
                    PermissionActionKind::AllowAlways,
                ),
                permission_option("cancel", "Cancel", PermissionActionKind::RejectOnce),
            ],
            display_ref: Some(80),
        };
        let update = action_response_request(
            "callback-req-1",
            "delivery-1",
            ImMessageState::Handled,
            ImLocale::ZhCn,
            Some(&selection),
            None,
        );
        let card = &update["body"]["template_card"];
        assert_eq!(update["cmd"], "aibot_respond_update_msg");
        assert_eq!(update["body"]["response_type"], "update_template_card");
        assert_eq!(card["card_type"], "vote_interaction");
        assert_eq!(card["task_id"], "delivery-1");
        assert_eq!(card["checkbox"]["disable"], true);
        assert_eq!(card["checkbox"]["mode"], 0);
        assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
        assert!(card["submit_button"].get("disable").is_none());
        assert_eq!(card["main_title"]["title"], "id=0080 已处理：允许一次");
        assert_eq!(card["main_title"]["desc"], "Agent 请求执行命令");
        let options = card["checkbox"]["option_list"].as_array().unwrap();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0]["id"], "1");
        assert_eq!(options[0]["text"], "记住选择");
        assert_eq!(options[0]["is_checked"], false);
        assert_eq!(options[1]["id"], "0");
        assert_eq!(options[1]["text"], "允许一次");
        assert_eq!(options[1]["is_checked"], true);
        assert_eq!(options[2]["id"], "2");
        assert_eq!(options[2]["text"], "拒绝");
        assert_eq!(options[2]["is_checked"], false);
    }

    #[test]
    fn successful_manual_check_vote_update_keeps_selected_outcome() {
        let selection = ImWeComVoteSelection {
            selected_option_id: "1".into(),
            vote_actions: vec![
                InterventionAllowedAction::ManualSuccess,
                InterventionAllowedAction::ManualFailure,
            ],
            display_ref: Some(80),
        };
        let update = action_response_request(
            "callback-req-1",
            "delivery-1",
            ImMessageState::Handled,
            ImLocale::ZhCn,
            Some(&selection),
            None,
        );
        let card = &update["body"]["template_card"];
        assert_eq!(card["card_type"], "vote_interaction");
        assert_eq!(card["main_title"]["title"], "id=0080 已处理：失败");
        assert_eq!(card["main_title"]["desc"], "任务正在等待人工检查结果。");
        let options = card["checkbox"]["option_list"].as_array().unwrap();
        assert_eq!(options[0]["id"], "0");
        assert_eq!(options[0]["text"], "成功");
        assert_eq!(options[0]["is_checked"], false);
        assert_eq!(options[1]["id"], "1");
        assert_eq!(options[1]["text"], "失败");
        assert_eq!(options[1]["is_checked"], true);
        assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
        assert!(card["submit_button"].get("disable").is_none());
    }

    #[test]
    fn elicitation_terminal_cards_keep_original_controls_and_display_ref() {
        let options = || {
            vec![
                RemoteElicitationOption {
                    value: json!("a"),
                    label: "选项A".into(),
                    description: None,
                },
                RemoteElicitationOption {
                    value: json!("b"),
                    label: "选项B".into(),
                    description: None,
                },
            ]
        };
        let question = |selector: &str, field: &str| RemoteScalarChoiceQuestion {
            selector_key: selector.into(),
            field_name: field.into(),
            title: format!("{selector} 标题"),
            description: None,
            required: true,
            options: options(),
        };
        let multi_form = RemoteElicitationForm::MultiScalarChoice {
            question: question("q0", "features"),
            allows_empty: false,
        };
        let delivery = permission_delivery(Vec::new());
        let outbound = elicitation_template_card(
            &delivery,
            &multi_form,
            "补充信息（id=0080）",
            "任务正在等待你的回答。",
            Vec::new(),
            ImLocale::ZhCn,
        );
        assert_eq!(outbound["card_type"], "vote_interaction");
        assert_eq!(outbound["checkbox"]["mode"], 1);
        let multi_context = ImWeComFormSelection {
            card_kind: ImWeComFormCardKind::VoteMulti,
            selections: vec![RemoteElicitationFormSelection {
                selector_key: "q0".into(),
                option_ids: vec!["0".into(), "1".into()],
            }],
            form: Some(multi_form.clone()),
            display_ref: Some(80),
        };
        let update = action_response_request(
            "callback-req-1",
            &delivery.delivery_id,
            ImMessageState::Handled,
            ImLocale::ZhCn,
            None,
            Some(&multi_context),
        );
        let card = &update["body"]["template_card"];
        assert_eq!(card["card_type"], "vote_interaction");
        assert_eq!(card["checkbox"]["mode"], 1);
        assert_eq!(card["checkbox"]["disable"], true);
        assert_eq!(card["checkbox"]["option_list"][0]["is_checked"], true);
        assert_eq!(card["checkbox"]["option_list"][1]["is_checked"], true);
        assert_eq!(card["submit_button"]["key"], "delivery-1:submit");
        assert!(card["submit_button"].get("disable").is_none());
        assert_eq!(card["main_title"]["title"], "id=0080 已处理：选项A, 选项B");

        let questions_form = RemoteElicitationForm::ScalarChoiceQuestions {
            questions: vec![question("q0", "q0"), question("q1", "q1")],
        };
        let outbound = elicitation_template_card(
            &delivery,
            &questions_form,
            "补充信息（id=0080）",
            "任务正在等待你的回答。",
            Vec::new(),
            ImLocale::ZhCn,
        );
        assert_eq!(outbound["card_type"], "multiple_interaction");
        assert_eq!(outbound["select_list"].as_array().unwrap().len(), 2);
        assert_eq!(outbound["select_list"][0]["question_key"], "q0");
        let multiple_context = ImWeComFormSelection {
            card_kind: ImWeComFormCardKind::Multiple,
            selections: vec![
                RemoteElicitationFormSelection {
                    selector_key: "q0".into(),
                    option_ids: vec!["0".into()],
                },
                RemoteElicitationFormSelection {
                    selector_key: "q1".into(),
                    option_ids: vec!["1".into()],
                },
            ],
            form: Some(questions_form),
            display_ref: Some(80),
        };
        let update = action_response_request(
            "callback-req-1",
            &delivery.delivery_id,
            ImMessageState::Handled,
            ImLocale::ZhCn,
            None,
            Some(&multiple_context),
        );
        let card = &update["body"]["template_card"];
        assert_eq!(card["card_type"], "multiple_interaction");
        assert_eq!(card["select_list"][0]["disable"], true);
        assert_eq!(card["select_list"][0]["selected_id"], "0");
        assert_eq!(card["select_list"][1]["disable"], true);
        assert_eq!(card["select_list"][1]["selected_id"], "1");
        assert!(card["submit_button"].get("disable").is_none());
        assert!(
            card["main_title"]["title"]
                .as_str()
                .unwrap()
                .contains("id=0080")
        );
    }

    #[test]
    fn flattened_sdk_type_shape_is_not_accepted_as_runtime_callback() {
        let action = WeComActionReference {
            delivery_id: "delivery-1".into(),
            action_index: 0,
        };
        let callback = json!({
            "cmd": "aibot_event_callback",
            "headers": { "req_id": "callback-req-1" },
            "body": {
                "msgid": "event-1",
                "chattype": "single",
                "from": { "userid": "single-user" },
                "event": {
                    "eventtype": "template_card_event",
                    "event_key": format!("{}:{}", action.delivery_id, action.action_index),
                    "task_id": action.delivery_id
                }
            }
        });

        assert_eq!(
            action_envelope(&callback).unwrap_err().reason,
            WeComActionParseReason::EventKeyMissing
        );
    }

    #[tokio::test]
    async fn invalid_private_callback_queues_original_card_failure_response() {
        let action = WeComActionReference {
            delivery_id: "delivery-1".into(),
            action_index: 0,
        };
        let callback = official_action_callback(
            &action,
            "callback-req-1",
            "event-1",
            "wrong-delivery",
            Some("single"),
        );
        let (events, _event_receiver) = mpsc::channel(1);
        let (commands, mut command_receiver) = mpsc::channel(1);
        handle_callback(1, &events, &commands, callback)
            .await
            .unwrap();

        match command_receiver.recv().await.unwrap() {
            WeComCommand::RespondToAction {
                context,
                state,
                response: _,
            } => {
                assert_eq!(
                    context,
                    ImActionResponseContext::WeCom {
                        req_id: "callback-req-1".into(),
                        task_id: "wrong-delivery".into(),
                        vote_selection: None,
                        form_selection: None,
                    }
                );
                assert_eq!(state, ImMessageState::Failed);
            }
            WeComCommand::Send { .. } => panic!("unexpected WeCom send command"),
        }
    }

    #[tokio::test]
    async fn disconnected_event_is_a_non_retryable_connection_conflict() {
        let (events, _receiver) = mpsc::channel(1);
        let (commands, _command_receiver) = mpsc::channel(1);
        let error = handle_callback(
            1,
            &events,
            &commands,
            json!({
                "cmd": "aibot_event_callback",
                "body": { "event": { "eventtype": "disconnected_event" } }
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ImErrorCode::ConnectionConflict);
        assert!(!error.retryable);
    }

    #[test]
    fn reconnect_backoff_is_bounded() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(99), WECOM_MAX_RECONNECT_DELAY);
        let connector = WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap())
            .with_endpoint("ws://127.0.0.1:1");
        assert_eq!(connector.endpoint, "ws://127.0.0.1:1");
    }

    #[test]
    fn old_generation_cannot_install_or_clear_new_sender() {
        let connector = WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap());
        let (old_sender, _old_receiver) = mpsc::channel(1);
        let (new_sender, _new_receiver) = mpsc::channel(1);
        connector.advance_generation(1);
        assert!(connector.install_sender(1, old_sender));
        connector.advance_generation(2);
        assert!(connector.command_sender().is_err());
        assert!(connector.install_sender(2, new_sender));
        assert!(!connector.clear_sender(1));
        assert!(!connector.install_sender(1, mpsc::channel(1).0));
        assert!(connector.command_sender().is_ok());
        assert!(connector.clear_sender(2));
        assert!(connector.command_sender().is_err());
    }

    #[tokio::test]
    async fn permission_detail_ack_gates_the_vote_card() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let connector = std::sync::Arc::new(
            WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap())
                .with_endpoint(endpoint),
        );
        let (events, mut event_receiver) = mpsc::channel(16);
        let cancellation = CancellationToken::new();
        let connect_cancellation = cancellation.clone();
        let connect_connector = std::sync::Arc::clone(&connector);
        let config = ResolvedImChannelConfig {
            public_identity: "bot-1".into(),
            credential_fields: std::collections::BTreeMap::from([(
                WECOM_SECRET_FIELD.into(),
                "secret".into(),
            )]),
            locale: ImLocale::ZhCn,
        };
        let connect_task = tokio::spawn(async move {
            connect_connector
                .connect(config, 1, events, connect_cancellation)
                .await
        });
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();

        let subscribe = server.next().await.unwrap().unwrap();
        let subscribe: Value = match subscribe {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            _ => panic!("expected text subscribe frame"),
        };
        assert_eq!(subscribe["cmd"], "aibot_subscribe");
        let subscribe_req_id = subscribe["headers"]["req_id"].as_str().unwrap().to_owned();
        server
            .send(Message::Text(
                json!({
                    "headers": { "req_id": subscribe_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        assert!(matches!(
            event_receiver.recv().await,
            Some(ImConnectorEvent::Connected { .. })
        ));

        let delivery = permission_delivery(vec![
            permission_option("allow_once", "Yes", PermissionActionKind::AllowOnce),
            permission_option("cancel", "No", PermissionActionKind::RejectOnce),
        ]);
        let send_connector = std::sync::Arc::clone(&connector);
        let send_task = tokio::spawn(async move { send_connector.send(delivery).await });

        let detail = server.next().await.unwrap().unwrap();
        let detail: Value = match detail {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            _ => panic!("expected text detail frame"),
        };
        assert_eq!(detail["body"]["msgtype"], "markdown");
        assert!(
            detail["body"]["markdown"]["content"]
                .as_str()
                .unwrap()
                .contains("cargo build --release")
        );
        let detail_req_id = detail["headers"]["req_id"].as_str().unwrap().to_owned();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), server.next())
                .await
                .is_err(),
            "vote card must not be sent before the detail ACK"
        );
        server
            .send(Message::Text(
                json!({
                    "headers": { "req_id": detail_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let card = server.next().await.unwrap().unwrap();
        let card: Value = match card {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            _ => panic!("expected text card frame"),
        };
        assert_eq!(card["body"]["msgtype"], "template_card");
        assert_eq!(
            card["body"]["template_card"]["card_type"],
            "vote_interaction"
        );
        let card_req_id = card["headers"]["req_id"].as_str().unwrap().to_owned();
        server
            .send(Message::Text(
                json!({
                    "headers": { "req_id": card_req_id },
                    "errcode": 0,
                    "body": { "msgid": "card-message-1" }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let receipt = tokio::time::timeout(Duration::from_secs(2), send_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.platform_message_id.as_deref(),
            Some("card-message-1")
        );
        cancellation.cancel();
        connect_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn desktop_resolution_and_invalid_delivery_do_not_disconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let connector = std::sync::Arc::new(
            WeComConnector::new(ImActionTokenCodec::new(vec![7; 32]).unwrap())
                .with_endpoint(endpoint),
        );
        let (events, mut event_receiver) = mpsc::channel(16);
        let cancellation = CancellationToken::new();
        let connect_cancellation = cancellation.clone();
        let connect_connector = std::sync::Arc::clone(&connector);
        let config = ResolvedImChannelConfig {
            public_identity: "bot-1".into(),
            credential_fields: std::collections::BTreeMap::from([(
                WECOM_SECRET_FIELD.into(),
                "secret".into(),
            )]),
            locale: ImLocale::ZhCn,
        };
        let connect_task = tokio::spawn(async move {
            connect_connector
                .connect(config, 1, events, connect_cancellation)
                .await
        });
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();

        let subscribe = server.next().await.unwrap().unwrap();
        let subscribe: Value = match subscribe {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            _ => panic!("expected text subscribe frame"),
        };
        let subscribe_req_id = subscribe["headers"]["req_id"].as_str().unwrap();
        server
            .send(Message::Text(
                json!({
                    "headers": { "req_id": subscribe_req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        assert!(matches!(
            event_receiver.recv().await,
            Some(ImConnectorEvent::Connected { .. })
        ));

        let send_connector = std::sync::Arc::clone(&connector);
        let send_task = tokio::spawn(async move {
            send_connector
                .send(desktop_resolution_delivery(ImNotificationKind::Elicitation))
                .await
        });
        let request = server.next().await.unwrap().unwrap();
        let request: Value = match request {
            Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
            _ => panic!("expected text resolution frame"),
        };
        assert_eq!(request["cmd"], "aibot_send_msg");
        assert_eq!(request["body"]["msgtype"], "markdown");
        assert_eq!(
            request["body"]["markdown"]["content"],
            "**id=0012 已在桌面端处理**"
        );
        let req_id = request["headers"]["req_id"].as_str().unwrap();
        server
            .send(Message::Text(
                json!({
                    "headers": { "req_id": req_id },
                    "errcode": 0
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let receipt = tokio::time::timeout(Duration::from_secs(2), send_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(receipt.platform_message_id, None);

        let mut invalid_delivery = permission_delivery(vec![permission_option(
            "cancel",
            "No",
            PermissionActionKind::RejectOnce,
        )]);
        invalid_delivery.display_ref = None;
        let error = connector.send(invalid_delivery).await.unwrap_err();
        assert_eq!(error.code, ImErrorCode::ProtocolInvalid);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), server.next())
                .await
                .is_err(),
            "invalid delivery must fail before writing a frame"
        );
        assert!(!connect_task.is_finished());
        assert!(
            tokio::time::timeout(Duration::from_millis(100), event_receiver.recv())
                .await
                .is_err(),
            "resolution delivery must not disconnect the channel"
        );
        cancellation.cancel();
        connect_task.await.unwrap().unwrap();
    }
}
