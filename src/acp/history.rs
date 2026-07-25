use serde_json::{Value, json};

use crate::acp::events::AcpUiEvent;

pub const CLAUDE_REQUEST_INTERRUPTED: &str = "[Request interrupted by user]";
pub const CLAUDE_TOOL_USE_INTERRUPTED: &str = "[Request interrupted by user for tool use]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayTurnOrigin {
    Local,
    External,
    Control,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplayUpdateDecision {
    Suppress,
    Import { items: Vec<ProviderHistoryImport> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderHistoryImport {
    pub update: Value,
    pub event_id: Option<String>,
}

#[derive(Debug, Clone)]
struct LocalPromptAnchor {
    id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct PendingReplayItem {
    update: Value,
    turn_index: u64,
    gap_turn_index: u64,
    item_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderHistoryPlacement {
    after_prompt_id: Option<String>,
    before_prompt_id: Option<String>,
    gap_turn_index: u64,
}

impl ReplayUpdateDecision {
    fn import(items: Vec<ProviderHistoryImport>) -> Self {
        if items.is_empty() {
            Self::Suppress
        } else {
            Self::Import { items }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FinalizedReplayItem {
    update: Value,
    event_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderHistoryReplay {
    local_prompt_anchors: Vec<LocalPromptAnchor>,
    local_prompt_cursor: usize,
    provider_id: String,
    session_id: String,
    turn_index: u64,
    turn_origin: Option<ReplayTurnOrigin>,
    item_index: u64,
    active_item_key: Option<String>,
    gap_turn_index: u64,
    pending_items: Vec<PendingReplayItem>,
}

impl ProviderHistoryReplay {
    pub fn from_timeline(items: &[AcpUiEvent]) -> Self {
        let local_prompt_anchors = items
            .iter()
            .filter(|item| {
                item.kind == "userTextDelta"
                    && item
                        .raw
                        .as_ref()
                        .and_then(|raw| raw.get("source"))
                        .and_then(Value::as_str)
                        == Some("goldBandPrompt")
            })
            .filter_map(|item| {
                let text = item.content.as_deref()?;
                Some(LocalPromptAnchor {
                    id: prompt_anchor_id(item),
                    text: normalize_prompt_text(text),
                })
            })
            .collect();
        Self {
            local_prompt_anchors,
            local_prompt_cursor: 0,
            provider_id: String::new(),
            session_id: String::new(),
            turn_index: 0,
            turn_origin: None,
            item_index: 0,
            active_item_key: None,
            gap_turn_index: 0,
            pending_items: Vec::new(),
        }
    }

    pub fn begin(&mut self, provider_id: &str, session_id: &str) {
        self.local_prompt_cursor = 0;
        self.provider_id.clear();
        self.provider_id.push_str(provider_id);
        self.session_id.clear();
        self.session_id.push_str(session_id);
        self.turn_index = 0;
        self.turn_origin = None;
        self.item_index = 0;
        self.active_item_key = None;
        self.gap_turn_index = 0;
        self.pending_items.clear();
    }

    pub fn observe(&mut self, update: &Value) -> ReplayUpdateDecision {
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind == "user_message_chunk" {
            return self.observe_user_message(update);
        }
        if !is_replay_content_update(kind) {
            self.active_item_key = None;
            return ReplayUpdateDecision::Suppress;
        }
        if self.turn_origin != Some(ReplayTurnOrigin::External) {
            return ReplayUpdateDecision::Suppress;
        }

        let logical_key = replay_item_key(kind, update);
        if self.active_item_key.as_deref() != Some(logical_key.as_str()) {
            self.item_index = self.item_index.saturating_add(1);
            self.active_item_key = Some(logical_key);
        }
        self.pending_items.push(PendingReplayItem {
            update: update.clone(),
            turn_index: self.turn_index,
            gap_turn_index: self.gap_turn_index,
            item_index: self.item_index,
        });
        ReplayUpdateDecision::Suppress
    }

    fn observe_user_message(&mut self, update: &Value) -> ReplayUpdateDecision {
        self.turn_index = self.turn_index.saturating_add(1);
        self.item_index = 0;
        self.active_item_key = None;
        let content = update_text(update).unwrap_or_default();
        if is_known_claude_control_message(&self.provider_id, content) {
            // Temporary provider adapter rule: ACP currently exposes these Claude Code
            // interruption controls as ordinary user_message_chunk records. Keep the raw
            // frame for audit and remove this exact-text fallback when ACP adds a typed
            // interruption/control notification.
            self.turn_origin = Some(ReplayTurnOrigin::Control);
            return ReplayUpdateDecision::Suppress;
        }

        let normalized = normalize_prompt_text(content);
        if let Some(relative_index) = self.local_prompt_anchors[self.local_prompt_cursor..]
            .iter()
            .position(|local| local.text == normalized)
        {
            // Provider replay is not guaranteed to contain every Gold Band prompt. Treat
            // the remaining local prompts as ordered anchors so a missing replay turn does
            // not shift every later local turn into external history.
            let matched_index = self.local_prompt_cursor.saturating_add(relative_index);
            let pending = self.flush_pending(Some(matched_index));
            self.local_prompt_cursor = matched_index.saturating_add(1);
            self.turn_origin = Some(ReplayTurnOrigin::Local);
            self.gap_turn_index = 0;
            return ReplayUpdateDecision::import(pending);
        }

        self.turn_origin = Some(ReplayTurnOrigin::External);
        self.gap_turn_index = self.gap_turn_index.saturating_add(1);
        self.item_index = 1;
        self.pending_items.push(PendingReplayItem {
            update: update.clone(),
            turn_index: self.turn_index,
            gap_turn_index: self.gap_turn_index,
            item_index: self.item_index,
        });
        ReplayUpdateDecision::Suppress
    }

    pub fn finish(&mut self) -> ReplayUpdateDecision {
        let pending = self.flush_pending(None);
        self.turn_origin = None;
        self.active_item_key = None;
        ReplayUpdateDecision::import(pending)
    }

    fn flush_pending(&mut self, before_anchor_index: Option<usize>) -> Vec<ProviderHistoryImport> {
        if self.pending_items.is_empty() {
            return Vec::new();
        }
        let after_anchor_index = before_anchor_index
            .and_then(|index| index.checked_sub(1))
            .or_else(|| {
                before_anchor_index
                    .is_none()
                    .then(|| self.local_prompt_cursor.checked_sub(1))
                    .flatten()
            });
        let placement = ProviderHistoryPlacement {
            after_prompt_id: after_anchor_index
                .and_then(|index| self.local_prompt_anchors.get(index))
                .map(|anchor| anchor.id.clone()),
            before_prompt_id: before_anchor_index
                .and_then(|index| self.local_prompt_anchors.get(index))
                .map(|anchor| anchor.id.clone()),
            gap_turn_index: 0,
        };
        let provider_id = self.provider_id.clone();
        let session_id = self.session_id.clone();
        self.pending_items
            .drain(..)
            .map(|pending| finalize_replay_item(&provider_id, &session_id, pending, &placement))
            .map(|item| ProviderHistoryImport {
                update: item.update,
                event_id: item.event_id,
            })
            .collect()
    }
}

fn finalize_replay_item(
    provider_id: &str,
    session_id: &str,
    pending: PendingReplayItem,
    placement: &ProviderHistoryPlacement,
) -> FinalizedReplayItem {
    let kind = pending
        .update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let placement = ProviderHistoryPlacement {
        gap_turn_index: pending.gap_turn_index,
        ..placement.clone()
    };
    let provider_history_item_id = provider_history_item_id(
        kind,
        &pending.update,
        session_id,
        &placement,
        pending.item_index,
    );
    let event_id = (kind == "user_message_chunk").then(|| provider_history_item_id.clone());
    FinalizedReplayItem {
        update: annotate_provider_history_update(
            &pending.update,
            provider_id,
            pending.turn_index,
            pending.item_index,
            &provider_history_item_id,
            &placement,
        ),
        event_id,
    }
}

fn provider_history_item_id(
    kind: &str,
    update: &Value,
    session_id: &str,
    placement: &ProviderHistoryPlacement,
    item_index: u64,
) -> String {
    if kind == "user_message_chunk" {
        if let Some(message_id) = update
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return format!("provider-user-{message_id}");
        }
        return format!(
            "provider-history-user-{}-{}-{}",
            stable_id_component(session_id),
            stable_anchor_component(placement.after_prompt_id.as_deref()),
            placement.gap_turn_index,
        );
    }
    if let Some(message_id) = update
        .get("messageId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return match kind {
            "agent_message_chunk" => format!("assistant-message-{message_id}"),
            "agent_thought_chunk" => format!("assistant-thought-{message_id}"),
            _ => format!("provider-history-{kind}-{message_id}"),
        };
    }
    if let Some(tool_call_id) = update
        .get("toolCallId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return format!("tool-call-{tool_call_id}");
    }
    format!(
        "provider-history-{}-{}-{}-{}-{}",
        stable_id_component(session_id),
        stable_anchor_component(placement.after_prompt_id.as_deref()),
        placement.gap_turn_index,
        stable_id_component(kind),
        item_index
    )
}

fn annotate_provider_history_update(
    update: &Value,
    provider_id: &str,
    turn_index: u64,
    item_index: u64,
    item_id: &str,
    placement: &ProviderHistoryPlacement,
) -> Value {
    let mut annotated = update.clone();
    let Some(object) = annotated.as_object_mut() else {
        return annotated;
    };
    object.insert("source".to_string(), json!("providerHistory"));
    object.insert("historyOrigin".to_string(), json!("external"));
    object.insert("historyProvider".to_string(), json!(provider_id));
    object.insert("historyTurnIndex".to_string(), json!(turn_index));
    object.insert("historyItemIndex".to_string(), json!(item_index));
    object.insert("providerHistoryItemId".to_string(), json!(item_id));
    object.insert(
        "historyPlacement".to_string(),
        json!({
            "version": 1,
            "afterPromptId": placement.after_prompt_id,
            "beforePromptId": placement.before_prompt_id,
            "gapTurnIndex": placement.gap_turn_index,
        }),
    );
    annotated
}

fn replay_item_key(kind: &str, update: &Value) -> String {
    if let Some(message_id) = update.get("messageId").and_then(Value::as_str) {
        return format!("{kind}:message:{message_id}");
    }
    if let Some(tool_call_id) = update.get("toolCallId").and_then(Value::as_str) {
        return format!("tool:{tool_call_id}");
    }
    kind.to_string()
}

fn is_replay_content_update(kind: &str) -> bool {
    matches!(
        kind,
        "agent_message_chunk" | "agent_thought_chunk" | "tool_call" | "tool_call_update" | "plan"
    )
}

fn update_text(update: &Value) -> Option<&str> {
    update
        .get("content")
        .and_then(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| content.as_str())
        })
        .map(str::trim)
}

fn normalize_prompt_text(value: &str) -> String {
    value.replace("\r\n", "\n").trim().to_string()
}

fn is_known_claude_control_message(provider_id: &str, content: &str) -> bool {
    provider_id == "claude-acp"
        && matches!(
            content.trim(),
            CLAUDE_REQUEST_INTERRUPTED | CLAUDE_TOOL_USE_INTERRUPTED
        )
}

fn stable_id_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn prompt_anchor_id(item: &AcpUiEvent) -> String {
    item.raw
        .as_ref()
        .and_then(|raw| raw.get("promptId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(item.id.as_str())
        .to_string()
}

fn stable_anchor_component(prompt_id: Option<&str>) -> String {
    prompt_id
        .filter(|value| !value.trim().is_empty())
        .map(stable_id_component)
        .unwrap_or_else(|| "root".to_string())
}

#[cfg(test)]
mod tests {
    use super::{ProviderHistoryReplay, ReplayUpdateDecision};
    use crate::acp::events::AcpUiEvent;
    use serde_json::{Value, json};

    fn local_prompt(index: u64, content: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: format!("gold-band-user-prompt-{index}"),
            seq: index,
            timestamp: format!("{index}Z"),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some(content.to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: Some(index),
            ended_seq: Some(index),
            started_at: Some(format!("{index}Z")),
            ended_at: Some(format!("{index}Z")),
            timing: None,
            raw: Some(json!({
                "source": "goldBandPrompt",
                "promptId": format!("prompt-{index}")
            })),
        }
    }

    #[test]
    fn suppresses_local_echo_and_imports_external_claude_turn() {
        let mut replay = ProviderHistoryReplay::from_timeline(&[
            local_prompt(1, "hi"),
            local_prompt(2, "next local prompt"),
        ]);
        replay.begin("claude-acp", "session-1");

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "local-user",
                "content": { "type": "text", "text": "hi" }
            })),
            ReplayUpdateDecision::Suppress
        );
        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "local-answer",
                "content": { "type": "text", "text": "hello" }
            })),
            ReplayUpdateDecision::Suppress
        );

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "external-user",
                "content": { "type": "text", "text": "external question" }
            })),
            ReplayUpdateDecision::Suppress
        );

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "external-answer",
                "content": { "type": "text", "text": "external answer" }
            })),
            ReplayUpdateDecision::Suppress
        );

        let imported = replay.observe(&json!({
            "sessionUpdate": "user_message_chunk",
            "messageId": "next-local-user",
            "content": { "type": "text", "text": "next local prompt" }
        }));
        let ReplayUpdateDecision::Import { items } = imported else {
            panic!("external turn should flush before the next local prompt");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].event_id.as_deref(),
            Some("provider-user-external-user")
        );
        let update = &items[0].update;
        assert_eq!(update["source"], json!("providerHistory"));
        assert_eq!(
            update["historyPlacement"]["afterPromptId"],
            json!("prompt-1")
        );
        assert_eq!(
            update["historyPlacement"]["beforePromptId"],
            json!("prompt-2")
        );
        assert_eq!(update["historyPlacement"]["gapTurnIndex"], json!(1));
        assert_eq!(
            items[1].update["providerHistoryItemId"],
            json!("assistant-message-external-answer")
        );
        assert_eq!(items[1].update["historyItemIndex"], json!(2));
    }

    #[test]
    fn codex_history_without_ids_gets_stable_turn_item_ids() {
        let mut replay = ProviderHistoryReplay::from_timeline(&[local_prompt(1, "hi")]);
        replay.begin("codex-acp", "session-1");
        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "hi" }
            })),
            ReplayUpdateDecision::Suppress
        );

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "external" }
            })),
            ReplayUpdateDecision::Suppress
        );

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "part one" }
            })),
            ReplayUpdateDecision::Suppress
        );
        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": " part two" }
            })),
            ReplayUpdateDecision::Suppress
        );
        let ReplayUpdateDecision::Import { items } = replay.finish() else {
            panic!("trailing external turn should flush at replay completion");
        };
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].event_id.as_deref(),
            Some("provider-history-user-session-1-prompt-1-1")
        );
        let ids = items[1..]
            .iter()
            .map(|item| {
                item.update
                    .get("providerHistoryItemId")
                    .and_then(Value::as_str)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(ids[0], ids[1]);
    }

    #[test]
    fn hides_only_exact_known_claude_interruption_messages() {
        let mut replay = ProviderHistoryReplay::from_timeline(&[]);
        replay.begin("claude-acp", "session-1");
        for content in [
            "[Request interrupted by user]",
            "[Request interrupted by user for tool use]",
        ] {
            assert_eq!(
                replay.observe(&json!({
                    "sessionUpdate": "user_message_chunk",
                    "content": { "type": "text", "text": content }
                })),
                ReplayUpdateDecision::Suppress
            );
        }

        assert!(matches!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "content": {
                    "type": "text",
                    "text": "please explain [Request interrupted by user]"
                }
            })),
            ReplayUpdateDecision::Suppress
        ));
        assert!(matches!(
            replay.finish(),
            ReplayUpdateDecision::Import { .. }
        ));
    }

    #[test]
    fn skips_missing_local_anchor_without_importing_later_local_tool_turn() {
        let mut replay = ProviderHistoryReplay::from_timeline(&[
            local_prompt(1, "hi"),
            local_prompt(2, "hi"),
            local_prompt(3, "use AskUserQuestion"),
        ]);
        replay.begin("claude-acp", "session-1");

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "first-hi",
                "content": { "type": "text", "text": "hi" }
            })),
            ReplayUpdateDecision::Suppress
        );

        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "external-user",
                "content": { "type": "text", "text": "external question" }
            })),
            ReplayUpdateDecision::Suppress
        );

        let decision = replay.observe(&json!({
            "sessionUpdate": "user_message_chunk",
            "messageId": "later-local-user",
            "content": { "type": "text", "text": "use AskUserQuestion" }
        }));
        let ReplayUpdateDecision::Import { items } = decision else {
            panic!("external history should flush before the later local anchor");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].update["historyPlacement"]["afterPromptId"],
            json!("prompt-2")
        );
        assert_eq!(
            items[0].update["historyPlacement"]["beforePromptId"],
            json!("prompt-3")
        );
        assert_eq!(
            replay.observe(&json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "ask-user-question-1",
                "title": "Asking for your input",
                "_meta": { "claudeCode": { "toolName": "AskUserQuestion" } }
            })),
            ReplayUpdateDecision::Suppress
        );
    }

    #[test]
    fn no_id_identity_does_not_change_when_a_right_anchor_appears_later() {
        let mut first_load = ProviderHistoryReplay::from_timeline(&[local_prompt(1, "hi")]);
        first_load.begin("codex-acp", "session-1");
        assert_eq!(
            first_load.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "hi" }
            })),
            ReplayUpdateDecision::Suppress
        );
        assert_eq!(
            first_load.observe(&json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "external" }
            })),
            ReplayUpdateDecision::Suppress
        );
        let ReplayUpdateDecision::Import { items: trailing } = first_load.finish() else {
            panic!("first load should flush trailing history");
        };

        let mut second_load = ProviderHistoryReplay::from_timeline(&[
            local_prompt(1, "hi"),
            local_prompt(2, "later"),
        ]);
        second_load.begin("codex-acp", "session-1");
        for update in [
            json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "hi" }
            }),
            json!({
                "sessionUpdate": "user_message_chunk",
                "content": { "type": "text", "text": "external" }
            }),
        ] {
            assert_eq!(second_load.observe(&update), ReplayUpdateDecision::Suppress);
        }
        let ReplayUpdateDecision::Import { items: anchored } = second_load.observe(&json!({
            "sessionUpdate": "user_message_chunk",
            "content": { "type": "text", "text": "later" }
        })) else {
            panic!("second load should anchor history before the new local prompt");
        };

        assert_eq!(trailing[0].event_id, anchored[0].event_id);
        assert_eq!(
            trailing[0].update["providerHistoryItemId"],
            anchored[0].update["providerHistoryItemId"]
        );
        assert_eq!(
            anchored[0].update["historyPlacement"]["beforePromptId"],
            json!("prompt-2")
        );
    }
}
