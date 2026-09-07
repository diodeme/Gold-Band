use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::app::intervention::{
    InterventionAllowedAction, RemoteElicitationForm, RemoteElicitationFormSelection,
};

use super::{ImChannelKind, ImDelivery, ImDeliveryBinding, ImInboundEnvelope};

pub const IM_INBOUND_QUEUE_CAPACITY: usize = 128;
pub const IM_LIFECYCLE_QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImLocale {
    #[default]
    ZhCn,
    En,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelCapabilities {
    pub proactive_delivery: bool,
    pub card_actions: bool,
    pub message_update: bool,
    pub private_chat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImConnectionIdentity {
    pub bot_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImObservedBinding {
    pub destination_id: String,
    pub conversation_id: String,
    pub actor_id: String,
    pub is_private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImErrorCode {
    ConfigInvalid,
    CredentialUnavailable,
    AuthenticationRequired,
    RateLimited,
    NetworkUnavailable,
    ProtocolInvalid,
    PrivateBindingRequired,
    ActorMismatch,
    ConversationMismatch,
    UnsupportedCapability,
    QueueCapacityExceeded,
    StorageUnavailable,
    ConnectionConflict,
}

impl ImErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInvalid => "IM_CONFIG_INVALID",
            Self::CredentialUnavailable => "IM_CREDENTIAL_UNAVAILABLE",
            Self::AuthenticationRequired => "IM_AUTHENTICATION_REQUIRED",
            Self::RateLimited => "IM_RATE_LIMITED",
            Self::NetworkUnavailable => "IM_NETWORK_UNAVAILABLE",
            Self::ProtocolInvalid => "IM_PROTOCOL_INVALID",
            Self::PrivateBindingRequired => "IM_PRIVATE_BINDING_REQUIRED",
            Self::ActorMismatch => "IM_ACTOR_MISMATCH",
            Self::ConversationMismatch => "IM_CONVERSATION_MISMATCH",
            Self::UnsupportedCapability => "IM_CAPABILITY_UNSUPPORTED",
            Self::QueueCapacityExceeded => "IM_QUEUE_CAPACITY_EXCEEDED",
            Self::StorageUnavailable => "IM_STORAGE_UNAVAILABLE",
            Self::ConnectionConflict => "IM_CONNECTION_CONFLICT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code:?}")]
pub struct ImIntegrationError {
    pub code: ImErrorCode,
    pub retryable: bool,
    pub retry_after: Option<Duration>,
    pub platform_code: Option<i64>,
}

impl ImIntegrationError {
    pub const fn permanent(code: ImErrorCode) -> Self {
        Self {
            code,
            retryable: false,
            retry_after: None,
            platform_code: None,
        }
    }

    pub const fn retryable(code: ImErrorCode, retry_after: Option<Duration>) -> Self {
        Self {
            code,
            retryable: true,
            retry_after,
            platform_code: None,
        }
    }

    pub const fn with_platform_code(mut self, platform_code: i64) -> Self {
        self.platform_code = Some(platform_code);
        self
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedImChannelConfig {
    pub public_identity: String,
    pub credential_fields: std::collections::BTreeMap<String, String>,
    pub locale: ImLocale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImDeliveryReceipt {
    pub platform_message_id: Option<String>,
    pub platform_chat_id: String,
    pub update_token_ref: Option<String>,
}

impl ImDeliveryReceipt {
    pub fn into_binding(
        self,
        channel: ImChannelKind,
        delivery_id: String,
    ) -> Option<ImDeliveryBinding> {
        Some(ImDeliveryBinding {
            delivery_id,
            channel,
            platform_message_id: self.platform_message_id?,
            platform_chat_id: self.platform_chat_id,
            update_token_ref: self.update_token_ref,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImConnectorEvent {
    Connected {
        generation: u64,
        identity: ImConnectionIdentity,
    },
    ReconnectScheduled {
        generation: u64,
        error: ImIntegrationError,
    },
    ConnectionFailed {
        generation: u64,
        error: ImIntegrationError,
    },
    InboundAction {
        generation: u64,
        envelope: ImInboundEnvelope,
        response_context: ImActionResponseContext,
    },
    BindingObserved {
        generation: u64,
        binding: ImObservedBinding,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub enum ImActionResponseContext {
    WeCom {
        req_id: String,
        task_id: String,
        vote_selection: Option<ImWeComVoteSelection>,
        form_selection: Option<ImWeComFormSelection>,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImWeComVoteSelection {
    pub selected_option_id: String,
    pub vote_actions: Vec<InterventionAllowedAction>,
    pub display_ref: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImWeComFormCardKind {
    VoteSingle,
    VoteMulti,
    Multiple,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ImWeComFormSelection {
    pub card_kind: ImWeComFormCardKind,
    pub selections: Vec<RemoteElicitationFormSelection>,
    pub form: Option<RemoteElicitationForm>,
    pub display_ref: Option<u16>,
}

impl std::fmt::Debug for ImWeComFormSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WeComFormSelection")
            .field("cardKind", &self.card_kind)
            .field("selectionCount", &self.selections.len())
            .field("hasForm", &self.form.is_some())
            .finish()
    }
}

impl std::fmt::Debug for ImWeComVoteSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WeComVoteSelection")
            .field("selected_option_id", &"[redacted]")
            .field("action_count", &self.vote_actions.len())
            .finish()
    }
}

impl std::fmt::Debug for ImActionResponseContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WeCom {
                task_id,
                vote_selection,
                form_selection,
                ..
            } => formatter
                .debug_struct("WeComActionResponseContext")
                .field("req_id", &"[redacted]")
                .field("task_id", task_id)
                .field("vote_selection", vote_selection)
                .field("form_selection", form_selection)
                .finish(),
        }
    }
}

impl ImConnectorEvent {
    pub const fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation, .. }
            | Self::ReconnectScheduled { generation, .. }
            | Self::ConnectionFailed { generation, .. }
            | Self::InboundAction { generation, .. }
            | Self::BindingObserved { generation, .. } => *generation,
        }
    }
}

#[async_trait]
pub trait ImConnector: Send + Sync {
    fn kind(&self) -> ImChannelKind;
    fn capabilities(&self) -> ImChannelCapabilities;
    fn advance_generation(&self, generation: u64);

    async fn connect(
        &self,
        config: ResolvedImChannelConfig,
        generation: u64,
        events: mpsc::Sender<ImConnectorEvent>,
        cancellation: CancellationToken,
    ) -> Result<(), ImIntegrationError>;

    async fn send(&self, delivery: ImDelivery) -> Result<ImDeliveryReceipt, ImIntegrationError>;

    async fn respond_to_action(
        &self,
        context: ImActionResponseContext,
        state: ImMessageState,
    ) -> Result<(), ImIntegrationError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImMessageState {
    Handled,
    Expired,
    Failed,
}
