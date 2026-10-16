use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::elicitation::ElicitationAction;
pub use crate::app::intervention::RemoteElicitationAnswer as ImAnswer;
use crate::app::intervention::{
    InterventionAction, InterventionAllowedAction, InterventionLocator, InterventionRequestIdentity,
};
use crate::domain::NodeOutcome;

use super::ImErrorCode;

pub const IM_PAYLOAD_VERSION: u16 = 3;
pub const MAX_IM_PRESENTATION_BYTES: usize = 32 * 1024;
pub const IM_RESOLUTION_EVENT_SUFFIX: &str = "resolved";
pub const INTERVENTION_WORKSPACE_FIELD: &str = "workspaceLabel";
pub const INTERVENTION_TASK_FIELD: &str = "taskTitle";
pub const INTERVENTION_NODE_FIELD: &str = "nodeLabel";
pub const INTERVENTION_CONTEXT_FIELDS: [&str; 3] = [
    INTERVENTION_WORKSPACE_FIELD,
    INTERVENTION_TASK_FIELD,
    INTERVENTION_NODE_FIELD,
];

pub fn is_intervention_context_field(key: &str) -> bool {
    INTERVENTION_CONTEXT_FIELDS.contains(&key)
}

pub fn resolution_event_id(source_event_id: &str) -> String {
    format!("{source_event_id}:{IM_RESOLUTION_EVENT_SUFFIX}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImChannelKind {
    WeCom,
    Feishu,
}

impl ImChannelKind {
    pub const ALL: [Self; 2] = [Self::WeCom, Self::Feishu];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WeCom => "wecom",
            Self::Feishu => "feishu",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wecom" => Some(Self::WeCom),
            "feishu" => Some(Self::Feishu),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImNotificationKind {
    Permission,
    Elicitation,
    ManualCheck,
    RunSuccess,
    RunFailure,
    AcpTurnFinished,
}

impl ImNotificationKind {
    pub const ALL: [Self; 6] = [
        Self::Permission,
        Self::Elicitation,
        Self::ManualCheck,
        Self::RunSuccess,
        Self::RunFailure,
        Self::AcpTurnFinished,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Elicitation => "elicitation",
            Self::ManualCheck => "manual_check",
            Self::RunSuccess => "run_success",
            Self::RunFailure => "run_failure",
            Self::AcpTurnFinished => "acp_turn_finished",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    pub const fn is_intervention(self) -> bool {
        matches!(
            self,
            Self::Permission | Self::Elicitation | Self::ManualCheck
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImDestination {
    pub destination_id: String,
    pub conversation_id: String,
    pub authorized_actor_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionRef {
    pub locator: InterventionLocator,
    pub request: InterventionRequestIdentity,
    pub expected_state: String,
    pub allowed_actions: Vec<InterventionAllowedAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterventionQuestionKind {
    SingleSelect,
    MultiSelect,
    FreeText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionQuestionOption {
    pub value: Value,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionQuestion {
    pub field_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub question_kind: InterventionQuestionKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<InterventionQuestionOption>,
    pub allows_custom_answer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionPresentation {
    pub title_key: String,
    pub summary_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<InterventionQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImNavigationLocator {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_occurrence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InformationalNotification {
    pub canonical_event_id: String,
    pub notification_kind: ImNotificationKind,
    pub locator: ImNavigationLocator,
    pub outcome: String,
    pub summary_key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionResolutionNotification {
    pub canonical_event_id: String,
    pub source_event_id: String,
    pub notification_kind: ImNotificationKind,
    pub locator: ImNavigationLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_ref: Option<u16>,
    pub resolved_by: ImResolvedBy,
    pub outcome: ImResolutionOutcome,
}

/// Which surface won the intervention. Only the side whose command was
/// `Accepted` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channel", rename_all = "camelCase")]
pub enum ImResolvedBy {
    Desktop,
    Channel(ImChannelKind),
}

/// How a per-target resolution record is written. `Send` is delivered by the
/// worker; `TombstoneOnly` is persisted as `expired` only to block a late
/// source projection for a target that never showed the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImResolutionDispatch {
    Send,
    TombstoneOnly,
}

/// Structured result of a resolved intervention; carries no user-facing text.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ImResolutionOutcome {
    PermissionOption {
        option_id: String,
    },
    ElicitationSubmitted {
        content: serde_json::Value,
    },
    /// The intervention is resolved, but its accepted result is unavailable.
    ResultUnavailable,
    /// The user explicitly rejected the elicitation.
    ElicitationDeclined,
    /// The execution was stopped or expired before anyone answered. Only the
    /// desktop/runtime stop paths produce it; remote channels cannot submit it.
    ElicitationCancelled,
    ManualCheck {
        outcome: NodeOutcome,
    },
}

impl From<&InterventionAction> for ImResolutionOutcome {
    fn from(action: &InterventionAction) -> Self {
        match action {
            InterventionAction::PermissionOption { option_id } => Self::PermissionOption {
                option_id: option_id.clone(),
            },
            InterventionAction::Elicitation {
                action: ElicitationAction::Accept,
                content: Some(content),
            } => Self::ElicitationSubmitted {
                content: content.clone(),
            },
            InterventionAction::Elicitation {
                action: ElicitationAction::Accept,
                content: None,
            } => Self::ResultUnavailable,
            InterventionAction::Elicitation {
                action: ElicitationAction::Decline,
                ..
            } => Self::ElicitationDeclined,
            InterventionAction::Elicitation {
                action: ElicitationAction::Cancel,
                ..
            } => Self::ElicitationCancelled,
            InterventionAction::ManualCheck { outcome } => Self::ManualCheck { outcome: *outcome },
        }
    }
}

impl std::fmt::Debug for ImResolutionOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ElicitationSubmitted { .. } => {
                f.write_str("ElicitationSubmitted { content: [REDACTED] }")
            }
            Self::ResultUnavailable => f.write_str("ResultUnavailable"),
            Self::ElicitationDeclined => f.write_str("ElicitationDeclined"),
            Self::ElicitationCancelled => f.write_str("ElicitationCancelled"),
            Self::PermissionOption { option_id } => f
                .debug_struct("PermissionOption")
                .field("option_id", option_id)
                .finish(),
            Self::ManualCheck { outcome } => f
                .debug_struct("ManualCheck")
                .field("outcome", outcome)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ImDeliveryPayload {
    Intervention {
        version: u16,
        reference: InterventionRef,
        presentation: InterventionPresentation,
    },
    Information {
        version: u16,
        notification: InformationalNotification,
    },
    InterventionResolution {
        version: u16,
        resolution: InterventionResolutionNotification,
    },
}

impl ImDeliveryPayload {
    pub const fn payload_kind(&self) -> &'static str {
        match self {
            Self::Intervention { .. } => "intervention",
            Self::Information { .. } | Self::InterventionResolution { .. } => "information",
        }
    }

    pub fn validate_for(&self, notification_kind: ImNotificationKind) -> bool {
        match self {
            Self::Intervention { version, .. } => {
                *version == IM_PAYLOAD_VERSION && notification_kind.is_intervention()
            }
            Self::Information {
                version,
                notification,
            } => {
                *version == IM_PAYLOAD_VERSION
                    && !notification_kind.is_intervention()
                    && notification.notification_kind == notification_kind
            }
            Self::InterventionResolution {
                version,
                resolution,
            } => {
                *version == IM_PAYLOAD_VERSION
                    && notification_kind.is_intervention()
                    && resolution.notification_kind == notification_kind
                    && !resolution.source_event_id.trim().is_empty()
                    && !resolution.canonical_event_id.trim().is_empty()
                    && match &resolution.outcome {
                        ImResolutionOutcome::ElicitationSubmitted { content } => {
                            content.is_object()
                                && serde_json::to_vec(content).is_ok_and(|bytes| {
                                    bytes.len()
                                        <= crate::app::intervention::MAX_INTERVENTION_CONTENT_BYTES
                                })
                        }
                        _ => true,
                    }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImDelivery {
    pub delivery_id: String,
    pub channel: ImChannelKind,
    pub destination: ImDestination,
    pub notification_kind: ImNotificationKind,
    pub canonical_event_id: String,
    pub payload: ImDeliveryPayload,
    pub expires_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_ref: Option<u16>,
}

impl ImDelivery {
    pub fn deterministic_id(
        channel: ImChannelKind,
        destination_id: &str,
        notification_kind: ImNotificationKind,
        canonical_event_id: &str,
    ) -> String {
        let mut hasher = blake3::Hasher::new();
        for part in [
            channel.as_str(),
            destination_id,
            notification_kind.as_str(),
            canonical_event_id,
        ] {
            hasher.update(&(part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.finalize().to_hex().to_string()
    }

    pub fn refresh_id(&mut self) {
        self.delivery_id = Self::deterministic_id(
            self.channel,
            &self.destination.destination_id,
            self.notification_kind,
            &self.canonical_event_id,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImDeliveryState {
    Pending,
    Sending,
    Sent,
    Expired,
    DeadLetter,
}

impl ImDeliveryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Expired => "expired",
            Self::DeadLetter => "dead_letter",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "sending" => Some(Self::Sending),
            "sent" => Some(Self::Sent),
            "expired" => Some(Self::Expired),
            "dead_letter" => Some(Self::DeadLetter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedImDelivery {
    pub delivery: ImDelivery,
    pub attempt_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImDeliveryBinding {
    pub delivery_id: String,
    pub channel: ImChannelKind,
    pub platform_message_id: String,
    pub platform_chat_id: String,
    pub update_token_ref: Option<String>,
}

/// Channel-neutral submission decoded by a connector. Conversion to an
/// `InterventionAction` and its validation happen once, in the inbound service.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ImSubmittedAction {
    /// Index into the delivery's published `allowed_actions`.
    PublishedAction {
        index: usize,
    },
    PermissionOption {
        option_id: String,
    },
    ManualCheck {
        outcome: NodeOutcome,
    },
    /// Answers keyed by `RemoteQuestion::selector_key`.
    ElicitationForm {
        answers: BTreeMap<String, ImAnswer>,
    },
    ElicitationDecline,
}

impl std::fmt::Debug for ImSubmittedAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PublishedAction { index } => formatter
                .debug_struct("PublishedAction")
                .field("index", index)
                .finish(),
            Self::PermissionOption { option_id } => formatter
                .debug_struct("PermissionOption")
                .field("option_id", option_id)
                .finish(),
            Self::ManualCheck { outcome } => formatter
                .debug_struct("ManualCheck")
                .field("outcome", outcome)
                .finish(),
            Self::ElicitationForm { answers } => formatter
                .debug_struct("ElicitationForm")
                .field("answer_count", &answers.len())
                .finish_non_exhaustive(),
            Self::ElicitationDecline => formatter.write_str("ElicitationDecline"),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImInboundEnvelope {
    pub action_id: String,
    pub channel: ImChannelKind,
    pub platform_event_id: String,
    pub conversation_id: String,
    pub actor_id: String,
    pub delivery_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_token: Option<String>,
    pub action: ImSubmittedAction,
}

impl std::fmt::Debug for ImInboundEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImInboundEnvelope")
            .field("action_id", &self.action_id)
            .field("channel", &self.channel)
            .field("delivery_id", &self.delivery_id)
            .field("action", &self.action)
            .field(
                "action_token",
                &self.action_token.as_ref().map(|_| "[redacted]"),
            )
            .field("actor", &"[redacted]")
            .field("conversation", &"[redacted]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImInboundActionResult {
    pub action_id: String,
    pub channel: ImChannelKind,
    pub canonical_event_id: Option<String>,
    pub actor_id_hash: String,
    pub platform_event_id: String,
    pub outcome: ImInboundOutcome,
    pub result_revision: Option<i64>,
    pub received_at_ms: i64,
    pub completed_at_ms: i64,
}

/// Inbound processing outcome. Only accepted and replayed outcomes are
/// persisted; rejections are returned to the connector without a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImInboundOutcome {
    Accepted,
    AlreadyApplied,
    Rejected(ImErrorCode),
}

impl ImInboundOutcome {
    pub const ACCEPTED: &str = "ACCEPTED";
    pub const ALREADY_APPLIED: &str = "ALREADY_APPLIED";

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => Self::ACCEPTED,
            Self::AlreadyApplied => Self::ALREADY_APPLIED,
            Self::Rejected(code) => code.as_str(),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            Self::ACCEPTED => Some(Self::Accepted),
            Self::ALREADY_APPLIED => Some(Self::AlreadyApplied),
            other => ImErrorCode::parse(other).map(Self::Rejected),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImNotificationPreferences {
    pub permission: bool,
    pub elicitation: bool,
    pub manual_check: bool,
    pub run_success: bool,
    pub run_failure: bool,
    pub acp_turn_finished: bool,
}

impl Default for ImNotificationPreferences {
    fn default() -> Self {
        Self {
            permission: true,
            elicitation: true,
            manual_check: true,
            run_success: false,
            run_failure: true,
            acp_turn_finished: false,
        }
    }
}

impl ImNotificationPreferences {
    pub const fn enabled(&self, kind: ImNotificationKind) -> bool {
        match kind {
            ImNotificationKind::Permission => self.permission,
            ImNotificationKind::Elicitation => self.elicitation,
            ImNotificationKind::ManualCheck => self.manual_check,
            ImNotificationKind::RunSuccess => self.run_success,
            ImNotificationKind::RunFailure => self.run_failure,
            ImNotificationKind::AcpTurnFinished => self.acp_turn_finished,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImBindingSummary {
    pub destination_id: String,
    pub conversation_id: String,
    pub authorized_actor_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImChannelSettings {
    pub kind: ImChannelKind,
    pub enabled: bool,
    pub public_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<ImBindingSummary>,
    #[serde(default)]
    pub notifications: ImNotificationPreferences,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImIntegrationSettings {
    #[serde(default)]
    pub channels: Vec<ImChannelSettings>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_notification_defaults_are_explicit_per_kind() {
        let preferences = ImNotificationPreferences::default();
        let enabled = ImNotificationKind::ALL
            .into_iter()
            .filter(|kind| preferences.enabled(*kind))
            .collect::<Vec<_>>();
        assert_eq!(
            enabled,
            vec![
                ImNotificationKind::Permission,
                ImNotificationKind::Elicitation,
                ImNotificationKind::ManualCheck,
                ImNotificationKind::RunFailure,
            ]
        );
    }

    #[test]
    fn removed_scheduled_notification_preferences_are_rejected() {
        for (field, kind) in [
            ("scheduledCompletion", "scheduled_completion"),
            ("scheduledFailure", "scheduled_failure"),
            ("scheduledAttention", "scheduled_attention"),
            ("scheduledMissed", "scheduled_missed"),
        ] {
            let mut value = serde_json::to_value(ImNotificationPreferences::default()).unwrap();
            value[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<ImNotificationPreferences>(value).is_err());
            assert!(ImNotificationKind::parse(kind).is_none());
        }
    }

    #[test]
    fn inbound_debug_output_redacts_tokens_and_platform_context() {
        let envelope = ImInboundEnvelope {
            action_id: "action-hash".into(),
            channel: ImChannelKind::WeCom,
            platform_event_id: "platform-event-secret".into(),
            conversation_id: "conversation-secret".into(),
            actor_id: "actor-secret".into(),
            delivery_id: "delivery-id".into(),
            action_token: Some("signed-action-token-secret".into()),
            action: ImSubmittedAction::ElicitationForm {
                answers: BTreeMap::from([(
                    "q0".to_string(),
                    ImAnswer::Text {
                        text: "free-text-answer-secret".into(),
                    },
                )]),
            },
        };
        let debug = format!("{envelope:?}");
        for forbidden in [
            "platform-event-secret",
            "conversation-secret",
            "actor-secret",
            "signed-action-token-secret",
            "free-text-answer-secret",
        ] {
            assert!(!debug.contains(forbidden), "debug leaked {forbidden}");
        }
        assert!(debug.contains("[redacted]"));

        let context =
            crate::im::ImActionResponseContext::new("callback-request-secret".to_string());
        let debug = format!("{context:?}");
        assert!(!debug.contains("callback-request-secret"));
        assert!(debug.contains("[opaque]"));
    }

    #[test]
    fn resolution_content_is_durable_and_debug_redacted() {
        let action = InterventionAction::Elicitation {
            action: ElicitationAction::Accept,
            content: Some(serde_json::json!({ "answer": "private-answer" })),
        };
        let outcome = ImResolutionOutcome::from(&action);
        let wire = serde_json::to_string(&outcome).unwrap();
        assert_eq!(
            serde_json::from_str::<ImResolutionOutcome>(&wire).unwrap(),
            outcome
        );
        assert!(wire.contains("private-answer"));
        assert!(!format!("{outcome:?}").contains("private-answer"));
        assert!(
            serde_json::from_str::<ImResolutionOutcome>(r#"{"kind":"elicitationSubmitted"}"#)
                .is_err(),
            "old payloads must be migrated, not silently decoded"
        );
    }

    #[test]
    fn resolution_content_requires_an_object_within_the_domain_byte_limit() {
        let limit = crate::app::intervention::MAX_INTERVENTION_CONTENT_BYTES;
        let overhead = serde_json::to_vec(&serde_json::json!({ "answer": "" }))
            .unwrap()
            .len();
        for (content, valid) in [
            (serde_json::json!({}), true),
            (
                serde_json::json!({"answer": "a".repeat(limit - overhead)}),
                true,
            ),
            (
                serde_json::json!({"answer": "a".repeat(limit - overhead + 1)}),
                false,
            ),
            (serde_json::json!({"answer": "汉".repeat(limit / 2)}), false),
            (serde_json::json!([]), false),
            (Value::Null, false),
        ] {
            let payload: ImDeliveryPayload = serde_json::from_value(serde_json::json!({
                "kind": "interventionResolution",
                "data": {
                    "version": IM_PAYLOAD_VERSION,
                    "resolution": {
                        "canonicalEventId": "event:resolved",
                        "sourceEventId": "event",
                        "notificationKind": "elicitation",
                        "locator": { "projectId": "project-1" },
                        "resolvedBy": { "kind": "desktop" },
                        "outcome": { "kind": "elicitationSubmitted", "content": content }
                    }
                }
            }))
            .unwrap();
            assert_eq!(payload.validate_for(ImNotificationKind::Elicitation), valid);
        }
    }

    #[test]
    fn payload_kinds_cannot_cross_the_action_boundary() {
        let information = ImDeliveryPayload::Information {
            version: IM_PAYLOAD_VERSION,
            notification: InformationalNotification {
                canonical_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::RunFailure,
                locator: ImNavigationLocator {
                    project_id: "project-1".into(),
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
                summary_key: "im.notification.runFailure.summary".into(),
                parameters: BTreeMap::new(),
                missed_count: None,
            },
        };
        assert!(information.validate_for(ImNotificationKind::RunFailure));
        assert!(!information.validate_for(ImNotificationKind::Permission));

        let resolution = ImDeliveryPayload::InterventionResolution {
            version: IM_PAYLOAD_VERSION,
            resolution: InterventionResolutionNotification {
                canonical_event_id: resolution_event_id("event-1"),
                source_event_id: "event-1".into(),
                notification_kind: ImNotificationKind::Permission,
                locator: ImNavigationLocator {
                    project_id: "project-1".into(),
                    task_id: Some("task-1".into()),
                    run_id: None,
                    round_id: None,
                    node_id: None,
                    attempt_id: None,
                    outer_node_id: None,
                    outer_attempt_id: None,
                    scheduled_occurrence_id: None,
                },
                display_ref: Some(12),
                resolved_by: ImResolvedBy::Desktop,
                outcome: ImResolutionOutcome::PermissionOption {
                    option_id: "allow".into(),
                },
            },
        };
        assert_eq!(
            resolution_event_id("event-1"),
            format!("event-1:{IM_RESOLUTION_EVENT_SUFFIX}")
        );
        assert_eq!(resolution.payload_kind(), "information");
        assert!(resolution.validate_for(ImNotificationKind::Permission));
        assert!(!resolution.validate_for(ImNotificationKind::RunFailure));
    }
}
