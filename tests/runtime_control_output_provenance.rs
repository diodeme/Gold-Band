use gold_band::acp::events::{
    AcpUiEvent, annotate_runtime_control_output, load_timeline_items, write_timeline_items,
};
use gold_band::artifacts::json_artifact_display_span;
use serde_json::json;
use tempfile::tempdir;

fn text_event(id: &str, seq: u64, content: &str) -> AcpUiEvent {
    AcpUiEvent {
        id: id.to_string(),
        seq,
        timestamp: format!("{seq}Z"),
        kind: "textDelta".to_string(),
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
fn runtime_control_annotation_does_not_fall_through_to_later_direct_message() {
    let dir = tempdir().unwrap();
    let path = camino::Utf8Path::from_path(dir.path())
        .unwrap()
        .join("acp.timeline.jsonl");
    let runtime_output = text_event("runtime-output", 10, "```json\n{\"result\":true}\n```");
    let mut direct_follow_up = text_event(
        "direct-follow-up",
        20,
        "普通解释中的示例：```json\n{\"example\":true}\n```",
    );
    direct_follow_up.raw = Some(json!({
        "runtimeControlMode": "non-runtime-controlled"
    }));
    write_timeline_items(&path, &[runtime_output, direct_follow_up]).unwrap();

    let span = json_artifact_display_span("```json\n{\"result\":true}\n```").unwrap();
    assert!(
        annotate_runtime_control_output(
            &path,
            "runtime-output",
            "accept-result",
            "workflow-output",
            &span,
        )
        .unwrap()
    );

    let items = load_timeline_items(&path).unwrap();
    assert!(
        items[0]
            .raw
            .as_ref()
            .and_then(|raw| raw.get("runtimeControlOutputDisplay"))
            .is_some(),
        "the Runtime-selected source message must own the display annotation"
    );
    assert!(
        items[1]
            .raw
            .as_ref()
            .and_then(|raw| raw.get("runtimeControlOutputDisplay"))
            .is_none(),
        "a later Direct message must not be inferred as Runtime control output"
    );
}
