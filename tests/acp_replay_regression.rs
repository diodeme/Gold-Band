use gold_band::acp::events::{
    AcpUiEvent, append_timeline_patch, latest_timeline_source_seq, load_timeline_items,
    write_timeline_items,
};
use gold_band::acp::history::{ProviderHistoryReplay, ReplayUpdateDecision};
use serde_json::json;
use tempfile::tempdir;

fn event(id: &str, seq: u64, kind: &str, content: &str) -> AcpUiEvent {
    AcpUiEvent {
        id: id.to_string(),
        seq,
        timestamp: format!("{seq}Z"),
        kind: kind.to_string(),
        session_id: Some("session-1".to_string()),
        content: Some(content.to_string()),
        title: None,
        tool_call_id: None,
        status: None,
        started_seq: Some(seq),
        ended_seq: Some(seq),
        started_at: Some(format!("{seq}Z")),
        ended_at: Some(format!("{seq}Z")),
        timing: None,
        raw: None,
    }
}

#[test]
fn replayed_provider_history_does_not_pollute_the_visible_timeline() {
    let dir = tempdir().unwrap();
    let path = camino::Utf8Path::from_path(dir.path())
        .unwrap()
        .join("acp.timeline.jsonl");

    let mut local_prompt = event("gold-band-user-prompt-1", 1, "userTextDelta", "hello");
    local_prompt.status = Some("completed".to_string());
    local_prompt.raw = Some(json!({
        "source": "goldBandPrompt",
        "promptId": "prompt-1"
    }));
    let mut provider_echo = event("acp-event-2", 2, "userTextDelta", "hello");
    provider_echo.raw = Some(json!({
        "sessionUpdate": "user_message_chunk",
        "messageId": "echo-1"
    }));
    let mut interrupted = event(
        "acp-event-3",
        3,
        "userTextDelta",
        "[Request interrupted by user]",
    );
    interrupted.raw = Some(json!({
        "sessionUpdate": "user_message_chunk",
        "messageId": "interrupt-1"
    }));
    let mut external_prompt = event(
        "provider-user-external-1",
        4,
        "userTextDelta",
        "external question",
    );
    external_prompt.status = Some("completed".to_string());
    external_prompt.raw = Some(json!({
        "source": "providerHistory",
        "historyOrigin": "external",
        "sessionUpdate": "user_message_chunk",
        "messageId": "external-1"
    }));
    write_timeline_items(
        &path,
        &[local_prompt, provider_echo, interrupted, external_prompt],
    )
    .unwrap();

    let mut original_answer = event("assistant-message-answer-1", 5, "textDelta", "answer");
    original_answer.raw = Some(json!({
        "sessionUpdate": "agent_message_chunk",
        "messageId": "answer-1"
    }));
    append_timeline_patch(&path, original_answer.id.clone(), 5, &original_answer).unwrap();
    let mut replayed_answer = original_answer.clone();
    replayed_answer.seq = 100;
    replayed_answer.timestamp = "100Z".to_string();
    replayed_answer.started_seq = Some(100);
    replayed_answer.ended_seq = Some(100);
    replayed_answer.started_at = Some("100Z".to_string());
    replayed_answer.ended_at = Some("100Z".to_string());
    append_timeline_patch(&path, replayed_answer.id.clone(), 100, &replayed_answer).unwrap();

    let items = load_timeline_items(&path).unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].id, "gold-band-user-prompt-1");
    assert_eq!(items[1].id, "provider-user-external-1");
    assert_eq!(items[2].id, "assistant-message-answer-1");
    assert_eq!(items[2].seq, 5);
    assert_eq!(items[2].timestamp, "5Z");
    assert_eq!(latest_timeline_source_seq(&path), 5);
    // Timeline patches are compacted into their canonical item at write time;
    // replayed provider history must not leave duplicate patch rows behind.
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
}

#[test]
fn replay_alignment_skips_missing_local_prompt_anchor() {
    let local_prompts = ["hi", "hi", "用askUserQuestion工具随便问几个问题给我"]
        .into_iter()
        .enumerate()
        .map(|(index, content)| {
            let mut item = event(
                &format!("gold-band-user-prompt-{index}"),
                index as u64 + 1,
                "userTextDelta",
                content,
            );
            item.raw = Some(json!({
                "source": "goldBandPrompt",
                "promptId": format!("prompt-{index}")
            }));
            item
        })
        .collect::<Vec<_>>();
    let mut replay = ProviderHistoryReplay::from_timeline(&local_prompts);
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
            "content": { "type": "text", "text": "这是我追加的信息" }
        })),
        ReplayUpdateDecision::Suppress
    );
    let imported = replay.observe(&json!({
        "sessionUpdate": "user_message_chunk",
        "messageId": "ask-user",
        "content": {
            "type": "text",
            "text": "用askUserQuestion工具随便问几个问题给我"
        }
    }));
    let ReplayUpdateDecision::Import { items } = imported else {
        panic!("external history should flush before the AskUserQuestion prompt");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].update["historyPlacement"],
        json!({
            "version": 1,
            "afterPromptId": "prompt-1",
            "beforePromptId": "prompt-2",
            "gapTurnIndex": 1
        })
    );
    assert_eq!(
        replay.observe(&json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "call_3wAeOFhjF2AXEeXTPBygYoDz",
            "title": "Asking for your input",
            "_meta": { "claudeCode": { "toolName": "AskUserQuestion" } }
        })),
        ReplayUpdateDecision::Suppress
    );
}
