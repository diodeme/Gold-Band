use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    ImChannelKind, ImDelivery, ImDeliveryBinding, ImDeliveryPayload, ImInboundEnvelope,
    ImResolutionOutcome, ImResolvedBy,
};

pub const IM_INBOUND_QUEUE_CAPACITY: usize = 128;
pub const IM_LIFECYCLE_QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImLocale {
    #[default]
    ZhCn,
    En,
}

/// `Default` is a channel without any capability (no connector registered).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelCapabilities {
    pub proactive_delivery: bool,
    pub card_actions: bool,
    pub private_chat: bool,
    pub message_update: ImMessageUpdateMode,
    pub interaction: ImInteractionCapabilities,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImMessageUpdateMode {
    #[default]
    None,
    /// The clicked card can only be replaced inside the action callback.
    CallbackOnly,
    /// Delivered messages can be updated at any time.
    Anytime,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImInteractionCapabilities {
    pub permission_max_options: u16,
    pub max_questions: u8,
    pub single_choice_max_options: ImOptionLimit,
    pub multi_choice: Option<ImMultiChoiceCapability>,
    pub free_text: Option<ImFreeTextCapability>,
    pub custom_answer: ImCustomAnswerSupport,
    pub decline: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImOptionLimit {
    pub single_question: u16,
    pub multi_question: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImMultiChoiceCapability {
    pub max_options: u16,
    pub within_multi_question: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImFreeTextCapability {
    pub max_chars: u32,
    pub multiline: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum ImCustomAnswerSupport {
    #[default]
    Unsupported,
    Supported {
        max_chars: u32,
    },
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Generous capabilities for tests that do not exercise interaction fitting.
    pub(crate) fn capabilities(message_update: ImMessageUpdateMode) -> ImChannelCapabilities {
        ImChannelCapabilities {
            proactive_delivery: true,
            card_actions: true,
            private_chat: true,
            message_update,
            interaction: ImInteractionCapabilities {
                permission_max_options: 20,
                max_questions: 5,
                single_choice_max_options: ImOptionLimit {
                    single_question: 20,
                    multi_question: 20,
                },
                multi_choice: Some(ImMultiChoiceCapability {
                    max_options: 20,
                    within_multi_question: true,
                }),
                free_text: Some(ImFreeTextCapability {
                    max_chars: 2000,
                    multiline: true,
                }),
                custom_answer: ImCustomAnswerSupport::Supported { max_chars: 500 },
                decline: true,
            },
        }
    }
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

/// Serialized as [`ImErrorCode::as_str`], the single wire form shared with the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    FormAnswerMissing,
    FormOptionInvalid,
    FormCustomAnswerMissing,
    FormTextTooLong,
    MessageUpdateUnavailable,
    ScanDenied,
    ScopeMissing,
    ActionTokenInvalid,
    InformationNotActionable,
    BindingRevoked,
    ActionIdInvalid,
    ActionInvalid,
    ActionExpired,
    DeliveryNotFound,
    WorkspaceUnavailable,
    ActionProcessingFailed,
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
            Self::FormAnswerMissing => "IM_FORM_ANSWER_MISSING",
            Self::FormOptionInvalid => "IM_FORM_OPTION_INVALID",
            Self::FormCustomAnswerMissing => "IM_FORM_CUSTOM_ANSWER_MISSING",
            Self::FormTextTooLong => "IM_FORM_TEXT_TOO_LONG",
            Self::MessageUpdateUnavailable => "IM_MESSAGE_UPDATE_UNAVAILABLE",
            Self::ScanDenied => "IM_SCAN_DENIED",
            Self::ScopeMissing => "IM_SCOPE_MISSING",
            Self::ActionTokenInvalid => "IM_ACTION_TOKEN_INVALID",
            Self::InformationNotActionable => "IM_INFORMATION_NOT_ACTIONABLE",
            Self::BindingRevoked => "IM_BINDING_REVOKED",
            Self::ActionIdInvalid => "IM_ACTION_ID_INVALID",
            Self::ActionInvalid => "IM_ACTION_INVALID",
            Self::ActionExpired => "IM_ACTION_EXPIRED",
            Self::DeliveryNotFound => "IM_DELIVERY_NOT_FOUND",
            Self::WorkspaceUnavailable => "IM_WORKSPACE_UNAVAILABLE",
            Self::ActionProcessingFailed => "IM_ACTION_PROCESSING_FAILED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }

    pub const ALL: [Self; 29] = [
        Self::ConfigInvalid,
        Self::CredentialUnavailable,
        Self::AuthenticationRequired,
        Self::RateLimited,
        Self::NetworkUnavailable,
        Self::ProtocolInvalid,
        Self::PrivateBindingRequired,
        Self::ActorMismatch,
        Self::ConversationMismatch,
        Self::UnsupportedCapability,
        Self::QueueCapacityExceeded,
        Self::StorageUnavailable,
        Self::ConnectionConflict,
        Self::FormAnswerMissing,
        Self::FormOptionInvalid,
        Self::FormCustomAnswerMissing,
        Self::FormTextTooLong,
        Self::MessageUpdateUnavailable,
        Self::ScanDenied,
        Self::ScopeMissing,
        Self::ActionTokenInvalid,
        Self::InformationNotActionable,
        Self::BindingRevoked,
        Self::ActionIdInvalid,
        Self::ActionInvalid,
        Self::ActionExpired,
        Self::DeliveryNotFound,
        Self::WorkspaceUnavailable,
        Self::ActionProcessingFailed,
    ];
}

impl Serialize for ImErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
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

/// Opaque platform reply handle. Only the connector that created it interprets it;
/// the generic layer and the runtime pass it through unchanged.
#[derive(Clone)]
pub struct ImActionResponseContext(Arc<dyn Any + Send + Sync>);

impl ImActionResponseContext {
    pub fn new<T: Any + Send + Sync>(handle: T) -> Self {
        Self(Arc::new(handle))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl PartialEq for ImActionResponseContext {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ImActionResponseContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImActionResponseContext([opaque])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImActionResponseState {
    Handled,
    AlreadyHandled,
    Rejected(ImErrorCode),
}

impl ImActionResponseState {
    /// Connector render state of the clicked card.
    pub const fn message_state(self) -> ImMessageState {
        match self {
            Self::Handled | Self::AlreadyHandled => ImMessageState::Handled,
            Self::Rejected(ImErrorCode::ActionExpired) => ImMessageState::Expired,
            Self::Rejected(_) => ImMessageState::Failed,
        }
    }
}

/// Result of one inbound action, handed back to the connector that received it.
/// `delivery` is the delivery record the click belongs to; connectors render the
/// reply card from it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImActionResponse {
    pub state: ImActionResponseState,
    pub resolved_by: Option<ImResolvedBy>,
    pub outcome: Option<ImResolutionOutcome>,
    pub delivery: Option<ImDelivery>,
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

    async fn send(
        &self,
        delivery: ImDelivery,
        source: Option<&ImDelivery>,
    ) -> Result<ImDeliveryReceipt, ImIntegrationError>;

    async fn respond_to_action(
        &self,
        context: ImActionResponseContext,
        response: ImActionResponse,
    ) -> Result<(), ImIntegrationError>;

    /// Updates an already delivered message. Only called for `Anytime` channels.
    async fn update_message(
        &self,
        binding: &ImDeliveryBinding,
        payload: &ImDeliveryPayload,
        source: Option<&ImDelivery>,
    ) -> Result<(), ImIntegrationError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImMessageState {
    Handled,
    Expired,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_serialize_as_their_unique_wire_code_and_parse_back() {
        let mut seen = std::collections::HashSet::new();
        for code in ImErrorCode::ALL {
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                serde_json::Value::String(code.as_str().into())
            );
            assert!(code.as_str().starts_with("IM_"));
            assert!(seen.insert(code.as_str()));
            assert_eq!(ImErrorCode::parse(code.as_str()), Some(code));
        }
    }

    #[test]
    fn channel_capabilities_serialize_to_the_frontend_contract() {
        let value = serde_json::to_value(test_support::capabilities(
            ImMessageUpdateMode::CallbackOnly,
        ))
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "proactiveDelivery": true,
                "cardActions": true,
                "privateChat": true,
                "messageUpdate": "callbackOnly",
                "interaction": {
                    "permissionMaxOptions": 20,
                    "maxQuestions": 5,
                    "singleChoiceMaxOptions": { "singleQuestion": 20, "multiQuestion": 20 },
                    "multiChoice": { "maxOptions": 20, "withinMultiQuestion": true },
                    "freeText": { "maxChars": 2000, "multiline": true },
                    "customAnswer": { "kind": "supported", "maxChars": 500 },
                    "decline": true
                }
            })
        );
        assert_eq!(
            serde_json::to_value(ImChannelCapabilities::default()).unwrap()["interaction"]["customAnswer"],
            serde_json::json!({ "kind": "unsupported" })
        );
    }
}
