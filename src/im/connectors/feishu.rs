//! Feishu / Lark connector: an outbound long connection for events (§6.3) and
//! the OpenAPI for delivery.

pub(crate) mod api;
pub(crate) mod card;
pub(crate) mod ws;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use self::api::{
    FeishuBrand, FeishuClientConfig, FeishuHostTable, FeishuOpenApi, feishu_http_client,
};
use self::card::{
    action_reply, decode_card_action, decode_error_reply, delivery_card_content, processing_reply,
    resolution_payload_card_content,
};
use self::ws::{
    Connection, EventReply, FeishuAckResult, InboundEvent, PendingActionResponses,
    reconnect_allowed, reconnect_delay,
};
use crate::im::{
    ImActionResponse, ImActionResponseContext, ImActionTokenCodec, ImChannelCapabilities,
    ImChannelKind, ImConnectionIdentity, ImConnector, ImConnectorEvent, ImCustomAnswerSupport,
    ImDelivery, ImDeliveryBinding, ImDeliveryPayload, ImDeliveryReceipt, ImErrorCode,
    ImFreeTextCapability, ImIntegrationError, ImInteractionCapabilities, ImLocale,
    ImMessageUpdateMode, ImMultiChoiceCapability, ImObservedBinding, ImOptionLimit,
    ResolvedImChannelConfig,
};

/// Keyring credential fields; `public_identity` carries the app_id.
pub(crate) const FEISHU_APP_SECRET_FIELD: &str = "app_secret";
pub(crate) const FEISHU_OPEN_ID_FIELD: &str = "open_id";
pub(crate) const FEISHU_BRAND_FIELD: &str = "brand";
const EVENT_MESSAGE_RECEIVE: &str = "im.message.receive_v1";
const EVENT_CARD_ACTION: &str = "card.action.trigger";
const CHAT_TYPE_P2P: &str = "p2p";
const MSG_TYPE_INTERACTIVE: &str = "interactive";

/// Limits verified against Feishu Card JSON 2.0 in the 2026-10-12 PoC.
const FEISHU_INTERACTION_CAPABILITIES: ImInteractionCapabilities = ImInteractionCapabilities {
    permission_max_options: 20,
    max_questions: 5,
    single_choice_max_options: ImOptionLimit {
        single_question: 20,
        multi_question: 20,
    },
    multi_choice: Some(ImMultiChoiceCapability {
        max_options: 4,
        within_multi_question: true,
    }),
    free_text: Some(ImFreeTextCapability {
        max_chars: 500,
        multiline: true,
    }),
    custom_answer: ImCustomAnswerSupport::Supported { max_chars: 100 },
    decline: true,
};
pub(crate) const FEISHU_CHANNEL_CAPABILITIES: ImChannelCapabilities = ImChannelCapabilities {
    proactive_delivery: true,
    card_actions: true,
    private_chat: true,
    message_update: ImMessageUpdateMode::Anytime,
    interaction: FEISHU_INTERACTION_CAPABILITIES,
};

pub struct FeishuConnector {
    hosts: FeishuHostTable,
    client: Option<Client>,
    slot: Mutex<GenerationApiSlot>,
    token_codec: ImActionTokenCodec,
    pending_responses: Mutex<HashMap<u64, Arc<PendingActionResponses>>>,
    #[cfg(test)]
    reconnect_delay_override: Option<Duration>,
}

/// The OpenAPI handle of the live connection; replaced on every generation so a
/// send never uses credentials of a superseded configuration.
#[derive(Default)]
struct GenerationApiSlot {
    generation: u64,
    active: Option<ActiveApi>,
}

impl GenerationApiSlot {
    fn clear(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
        }
    }
}

#[derive(Clone)]
struct ActiveApi {
    api: Arc<FeishuOpenApi>,
    locale: ImLocale,
    cancellation: CancellationToken,
}

impl ActiveApi {
    async fn request<T>(
        &self,
        request: impl std::future::Future<Output = Result<T, ImIntegrationError>>,
    ) -> Result<T, ImIntegrationError> {
        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => Err(network_unavailable()),
            result = request => result,
        }
    }
}

#[derive(Debug)]
struct FeishuResponseHandle {
    generation: u64,
    request_id: String,
    /// The reply speaks the language of the session that received the click.
    locale: ImLocale,
}

/// Immutable per-connection facts shared with event handler tasks.
#[derive(Clone)]
struct SessionContext {
    generation: u64,
    app_id: Arc<str>,
    owner_open_id: Arc<str>,
    locale: ImLocale,
    events: mpsc::Sender<ImConnectorEvent>,
}

impl FeishuConnector {
    pub fn new(token_codec: ImActionTokenCodec) -> Self {
        Self {
            hosts: FeishuHostTable::production(),
            client: None,
            slot: Mutex::new(GenerationApiSlot::default()),
            token_codec,
            pending_responses: Mutex::new(HashMap::new()),
            #[cfg(test)]
            reconnect_delay_override: None,
        }
    }

    #[cfg(test)]
    fn for_test(hosts: FeishuHostTable, client: Client, token_codec: ImActionTokenCodec) -> Self {
        Self {
            hosts,
            client: Some(client),
            slot: Mutex::new(GenerationApiSlot::default()),
            token_codec,
            pending_responses: Mutex::new(HashMap::new()),
            reconnect_delay_override: Some(Duration::from_secs(60)),
        }
    }

    fn http_client(&self) -> Result<Client, ImIntegrationError> {
        match &self.client {
            Some(client) => Ok(client.clone()),
            None => feishu_http_client(),
        }
    }

    fn reconnect_delay_for(
        &self,
        error: &ImIntegrationError,
        attempt: u32,
        config: &FeishuClientConfig,
    ) -> Duration {
        if let Some(retry_after) = error.retry_after {
            return retry_after;
        }
        #[cfg(test)]
        if let Some(delay) = self.reconnect_delay_override {
            return delay;
        }
        reconnect_delay(attempt, config)
    }

    fn active_api(&self) -> Result<ActiveApi, ImIntegrationError> {
        self.slot
            .lock()
            .expect("Feishu connector slot lock poisoned")
            .active
            .clone()
            .ok_or_else(network_unavailable)
    }

    fn install_api(&self, generation: u64, active: ActiveApi) -> bool {
        let mut slot = self
            .slot
            .lock()
            .expect("Feishu connector slot lock poisoned");
        if slot.generation != generation || active.cancellation.is_cancelled() {
            active.cancellation.cancel();
            return false;
        }
        slot.clear();
        slot.active = Some(active);
        true
    }

    fn clear_api(&self, generation: u64) {
        let mut slot = self
            .slot
            .lock()
            .expect("Feishu connector slot lock poisoned");
        if slot.generation == generation {
            slot.clear();
        }
    }

    fn install_pending_responses(
        &self,
        generation: u64,
        table: Arc<PendingActionResponses>,
    ) -> bool {
        let slot_generation = self
            .slot
            .lock()
            .expect("Feishu connector slot lock poisoned")
            .generation;
        if slot_generation != generation {
            return false;
        }
        self.pending_responses
            .lock()
            .expect("Feishu pending response lock poisoned")
            .insert(generation, table);
        true
    }

    fn clear_pending_responses(&self, generation: u64) {
        if let Some(table) = self
            .pending_responses
            .lock()
            .expect("Feishu pending response lock poisoned")
            .remove(&generation)
        {
            debug_assert_eq!(table.generation(), generation);
        }
    }

    fn send_action_response(&self, generation: u64, request_id: &str, response: Value) -> bool {
        self.pending_responses
            .lock()
            .expect("Feishu pending response lock poisoned")
            .get(&generation)
            .is_some_and(|table| table.send(request_id, response))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_session(
        &self,
        api: &Arc<FeishuOpenApi>,
        context: &SessionContext,
        locale: ImLocale,
        identity: &mut Option<ImConnectionIdentity>,
        client_config: &mut FeishuClientConfig,
        cancellation: &CancellationToken,
        reached_connected: &mut bool,
    ) -> Result<(), ImIntegrationError> {
        if identity.is_none() {
            let bot = tokio::select! {
                _ = cancellation.cancelled() => return Ok(()),
                bot = api.bot_info() => bot?,
            };
            *identity = Some(ImConnectionIdentity {
                bot_id: api.app_id().to_owned(),
                display_name: bot.app_name,
            });
        }
        // Every attempt pulls a fresh endpoint: the URL embeds a one-time ticket.
        let endpoint = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            endpoint = api.ws_endpoint() => endpoint?,
        };
        *client_config = endpoint.config;
        let Some(connection) =
            Connection::open(endpoint.url.as_str(), endpoint.service_id, cancellation).await?
        else {
            return Ok(());
        };
        if !self.install_api(
            context.generation,
            ActiveApi {
                api: Arc::clone(api),
                locale,
                cancellation: cancellation.child_token(),
            },
        ) {
            return Ok(());
        }
        let pending_responses = Arc::new(PendingActionResponses::new(context.generation));
        if !self.install_pending_responses(context.generation, Arc::clone(&pending_responses)) {
            return Ok(());
        }
        context
            .events
            .send(ImConnectorEvent::Connected {
                generation: context.generation,
                identity: identity.clone().ok_or_else(network_unavailable)?,
            })
            .await
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::NetworkUnavailable))?;
        *reached_connected = true;
        tracing::info!(
            generation = context.generation,
            "Feishu long connection established"
        );

        let context = context.clone();
        connection
            .run(
                context.generation,
                client_config,
                cancellation,
                move |event| {
                    let context = context.clone();
                    let pending_responses = Arc::clone(&pending_responses);
                    async move { dispatch_event(event, context, pending_responses).await }
                },
            )
            .await
    }
}

/// Domain dispatch receives no pbbp2 metadata or socket handles.
async fn dispatch_event(
    event: InboundEvent,
    context: SessionContext,
    pending_responses: Arc<PendingActionResponses>,
) {
    let InboundEvent { payload, reply } = event;
    if is_card_action_event(&payload) {
        dispatch_card_action(payload, context, reply, pending_responses).await;
    } else {
        let result = handle_event(&payload, &context).await;
        reply.send(result).await;
    }
}

async fn dispatch_card_action(
    payload: Vec<u8>,
    context: SessionContext,
    reply: EventReply,
    pending_responses: Arc<PendingActionResponses>,
) {
    let action = match decode_card_action(&payload) {
        Ok(action) => action,
        Err(error) => {
            reply
                .send(FeishuAckResult::Handled(Some(decode_error_reply(
                    error,
                    context.locale,
                ))))
                .await;
            return;
        }
    };
    if action.app_id != *context.app_id {
        tracing::warn!(
            generation = context.generation,
            "Feishu card action for another app ignored"
        );
        reply.send(FeishuAckResult::Handled(None)).await;
        return;
    }
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let Some(response_rx) = pending_responses.open(&request_id) else {
        reply.send(FeishuAckResult::Failed).await;
        return;
    };
    let response_context = ImActionResponseContext::new(FeishuResponseHandle {
        generation: context.generation,
        request_id: request_id.clone(),
        locale: context.locale,
    });
    if context
        .events
        .try_send(ImConnectorEvent::InboundAction {
            generation: context.generation,
            envelope: action.envelope,
            response_context,
        })
        .is_err()
    {
        let _ = pending_responses.take(&request_id);
        reply.send(FeishuAckResult::Failed).await;
        return;
    }
    let response = match tokio::time::timeout_at(reply.deadline(), response_rx).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => processing_reply(context.locale),
        Err(_) => {
            let _ = pending_responses.take(&request_id);
            processing_reply(context.locale)
        }
    };
    reply.send(FeishuAckResult::Handled(Some(response))).await;
}

fn is_card_action_event(payload: &[u8]) -> bool {
    serde_json::from_slice::<Value>(payload)
        .ok()
        .and_then(|event| event.pointer("/header/event_type").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
        .is_some_and(|event_type| event_type == EVENT_CARD_ACTION)
}

#[derive(Deserialize)]
struct EventEnvelope {
    header: EventHeader,
    #[serde(default)]
    event: Value,
}

#[derive(Deserialize)]
struct EventHeader {
    #[serde(default)]
    event_type: String,
    #[serde(default)]
    app_id: String,
}

#[derive(Deserialize)]
struct MessageReceiveEvent {
    sender: EventSender,
    message: ReceivedMessage,
}

#[derive(Deserialize)]
struct EventSender {
    sender_id: EventSenderId,
}

#[derive(Deserialize)]
struct EventSenderId {
    open_id: Option<String>,
}

#[derive(Deserialize)]
struct ReceivedMessage {
    chat_id: String,
    chat_type: String,
}

/// Malformed or foreign events are acknowledged as handled: a platform retry
/// cannot fix them. Only an unavailable runtime asks the platform to retry.
async fn handle_event(payload: &[u8], context: &SessionContext) -> FeishuAckResult {
    let Ok(envelope) = serde_json::from_slice::<EventEnvelope>(payload) else {
        tracing::warn!(
            generation = context.generation,
            len = payload.len(),
            "Feishu event envelope is invalid"
        );
        return FeishuAckResult::Handled(None);
    };
    if envelope.header.app_id != *context.app_id {
        tracing::warn!(
            generation = context.generation,
            "Feishu event for another app ignored"
        );
        return FeishuAckResult::Handled(None);
    }
    match envelope.header.event_type.as_str() {
        EVENT_MESSAGE_RECEIVE => observe_binding(envelope.event, context).await,
        other => {
            tracing::debug!(
                generation = context.generation,
                event_type = other,
                "Feishu event type not handled"
            );
            FeishuAckResult::Handled(None)
        }
    }
}

/// The binding is the private chat in which the scanning user messages the
/// bot; messages from anyone else never bind.
async fn observe_binding(event: Value, context: &SessionContext) -> FeishuAckResult {
    let Ok(event) = serde_json::from_value::<MessageReceiveEvent>(event) else {
        return FeishuAckResult::Handled(None);
    };
    let Some(sender) = event.sender.sender_id.open_id else {
        return FeishuAckResult::Handled(None);
    };
    if sender != *context.owner_open_id || event.message.chat_id.trim().is_empty() {
        return FeishuAckResult::Handled(None);
    }
    let observed = context
        .events
        .send(ImConnectorEvent::BindingObserved {
            generation: context.generation,
            binding: ImObservedBinding {
                destination_id: sender.clone(),
                conversation_id: event.message.chat_id,
                actor_id: sender,
                is_private: event.message.chat_type == CHAT_TYPE_P2P,
            },
        })
        .await;
    match observed {
        Ok(()) => FeishuAckResult::Handled(None),
        Err(_) => FeishuAckResult::Failed,
    }
}

fn credential_field(
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<String, ImIntegrationError> {
    fields
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::CredentialUnavailable))
}

fn network_unavailable() -> ImIntegrationError {
    ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None)
}

#[async_trait]
impl ImConnector for FeishuConnector {
    fn kind(&self) -> ImChannelKind {
        ImChannelKind::Feishu
    }

    fn capabilities(&self) -> ImChannelCapabilities {
        FEISHU_CHANNEL_CAPABILITIES
    }

    fn advance_generation(&self, generation: u64) {
        let mut slot = self
            .slot
            .lock()
            .expect("Feishu connector slot lock poisoned");
        if generation >= slot.generation {
            slot.generation = generation;
            slot.clear();
            self.pending_responses
                .lock()
                .expect("Feishu pending response lock poisoned")
                .retain(|candidate, _| *candidate >= generation);
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
        let app_id = config.public_identity.trim().to_owned();
        if app_id.is_empty() {
            return Err(ImIntegrationError::permanent(
                ImErrorCode::CredentialUnavailable,
            ));
        }
        let app_secret = credential_field(&config.credential_fields, FEISHU_APP_SECRET_FIELD)?;
        let owner_open_id = credential_field(&config.credential_fields, FEISHU_OPEN_ID_FIELD)?;
        let brand = FeishuBrand::parse(&credential_field(
            &config.credential_fields,
            FEISHU_BRAND_FIELD,
        )?)
        .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::CredentialUnavailable))?;
        let api = Arc::new(FeishuOpenApi::new(
            self.http_client()?,
            self.hosts.hosts(brand).open.clone(),
            app_id.clone(),
            app_secret,
        ));
        let context = SessionContext {
            generation,
            app_id: Arc::from(app_id),
            owner_open_id: Arc::from(owner_open_id),
            locale: config.locale,
            events,
        };

        let mut identity = None;
        let mut client_config = FeishuClientConfig::default();
        let mut attempt = 0_u32;
        loop {
            if cancellation.is_cancelled() {
                self.clear_api(generation);
                return Ok(());
            }
            let mut reached_connected = false;
            let result = self
                .run_session(
                    &api,
                    &context,
                    config.locale,
                    &mut identity,
                    &mut client_config,
                    &cancellation,
                    &mut reached_connected,
                )
                .await;
            self.clear_api(generation);
            self.clear_pending_responses(generation);
            if reached_connected {
                attempt = 0;
            }
            match result {
                Ok(()) => return Ok(()),
                Err(error) if !error.retryable => return Err(error),
                Err(error) if !reconnect_allowed(attempt, &client_config) => return Err(error),
                Err(error) => {
                    let _ = context
                        .events
                        .send(ImConnectorEvent::ReconnectScheduled {
                            generation,
                            error: error.clone(),
                        })
                        .await;
                    let delay = self.reconnect_delay_for(&error, attempt, &client_config);
                    attempt = attempt.saturating_add(1);
                    tokio::select! {
                        _ = cancellation.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    async fn send(
        &self,
        delivery: ImDelivery,
        source: Option<&ImDelivery>,
    ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
        let active = self.active_api()?;
        let content = match &delivery.payload {
            ImDeliveryPayload::InterventionResolution { .. } => resolution_payload_card_content(
                &delivery.payload,
                active.locale,
                source,
                &self.token_codec,
            ),
            _ => delivery_card_content(&delivery, &self.token_codec, active.locale)
                .map_err(|error| ImIntegrationError::permanent(error.code()))?,
        };
        active
            .request(active.api.create_message(
                &delivery.destination.destination_id,
                MSG_TYPE_INTERACTIVE,
                &content,
                &delivery.delivery_id,
            ))
            .await
    }

    async fn respond_to_action(
        &self,
        context: ImActionResponseContext,
        action_response: ImActionResponse,
    ) -> Result<(), ImIntegrationError> {
        let Some(handle) = context.downcast_ref::<FeishuResponseHandle>() else {
            return Err(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid));
        };
        let response = action_reply(&action_response, handle.locale, &self.token_codec);
        // A missing slot means the platform deadline or connection generation
        // expired first. The persisted domain result remains authoritative.
        let _ = self.send_action_response(handle.generation, &handle.request_id, response);
        Ok(())
    }

    async fn update_message(
        &self,
        binding: &ImDeliveryBinding,
        payload: &ImDeliveryPayload,
        source: Option<&ImDelivery>,
    ) -> Result<(), ImIntegrationError> {
        let active = self.active_api()?;
        let content =
            resolution_payload_card_content(payload, active.locale, source, &self.token_codec);
        active
            .request(
                active
                    .api
                    .patch_message(&binding.platform_message_id, &content),
            )
            .await
    }
}
