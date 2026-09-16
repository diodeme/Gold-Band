use anyhow::Result;
use camino::Utf8Path;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    acp::{events::AcpUiEvent, timeline::TimelineItemIdentity},
    storage::{read_json, write_json},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcpPromptInteractionKind {
    Permission,
    Elicitation,
}

impl AcpPromptInteractionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Elicitation => "elicitation",
        }
    }

    fn event_kind(self) -> &'static str {
        match self {
            Self::Permission => "permissionRequest",
            Self::Elicitation => "elicitationRequest",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptInteractionIdentity {
    pub interaction_id: String,
    pub kind: AcpPromptInteractionKind,
    pub turn_id: String,
    pub prompt_event_id: String,
}

impl AcpPromptInteractionIdentity {
    pub fn new(
        interaction_id: impl Into<String>,
        kind: AcpPromptInteractionKind,
        turn_id: impl Into<String>,
        prompt_event_id: impl Into<String>,
    ) -> Self {
        Self {
            interaction_id: interaction_id.into(),
            kind,
            turn_id: turn_id.into(),
            prompt_event_id: prompt_event_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAcpPromptInteractionState<T> {
    pub identity: AcpPromptInteractionIdentity,
    pub payload: T,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_identity: Option<TimelineItemIdentity>,
}

pub fn write_pending_prompt_interaction<T: Serialize>(
    path: &Utf8Path,
    state: &PendingAcpPromptInteractionState<T>,
) -> Result<()> {
    write_json(path, state)
}

pub fn bind_pending_prompt_interaction_timeline_identity<T>(
    path: &Utf8Path,
    identity: TimelineItemIdentity,
) -> Result<()>
where
    T: Serialize + DeserializeOwned,
{
    let mut pending: PendingAcpPromptInteractionState<T> = read_json(path)?;
    pending.timeline_identity = Some(identity);
    write_json(path, &pending)
}

pub fn annotate_prompt_interaction_identity(
    event: &mut AcpUiEvent,
    identity: &AcpPromptInteractionIdentity,
) {
    if event.kind != identity.kind.event_kind() {
        return;
    }
    let Some(conversation) = event
        .raw
        .as_mut()
        .and_then(|raw| raw.pointer_mut("/_meta/goldBandConversation"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    conversation.insert(
        "interactionId".to_string(),
        Value::String(identity.interaction_id.clone()),
    );
    conversation.insert(
        "interactionKind".to_string(),
        Value::String(identity.kind.as_str().to_string()),
    );
    conversation.insert(
        "turnId".to_string(),
        Value::String(identity.turn_id.clone()),
    );
    conversation.insert(
        "promptEventId".to_string(),
        Value::String(identity.prompt_event_id.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_uses_the_shared_prompt_interaction_contract() {
        let mut event = crate::acp::events::elicitation_request_event(
            1,
            "elicit-1".to_string(),
            &serde_json::from_value(serde_json::json!({
                "mode": "form",
                "sessionId": "session-1",
                "message": "Choose",
                "requestedSchema": { "type": "object", "properties": {} }
            }))
            .unwrap(),
        );
        crate::acp::branches::annotate_event_branch(&mut event);
        annotate_prompt_interaction_identity(
            &mut event,
            &AcpPromptInteractionIdentity::new(
                "elicit-1",
                AcpPromptInteractionKind::Elicitation,
                "turn-2",
                "prompt-turn-2",
            ),
        );

        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/goldBandConversation/interactionKind")),
            Some(&serde_json::json!("elicitation")),
        );
        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/goldBandConversation/turnId")),
            Some(&serde_json::json!("turn-2")),
        );
    }
}
