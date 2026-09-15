use camino::Utf8PathBuf;
use gold_band::acp::events::AcpUiEvent;
use gold_band::acp::timeline::{
    TimelineCompactionPolicy, TimelineStore, read_indexed_timeline_page,
};
use serde_json::json;
use tempfile::tempdir;

fn event(id: &str, seq: u64, content: &str) -> AcpUiEvent {
    AcpUiEvent {
        id: id.to_string(),
        seq,
        timestamp: format!("{seq}Z"),
        kind: "textDelta".to_string(),
        session_id: Some("session-1".to_string()),
        content: Some(content.to_string()),
        title: None,
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: Some(seq),
        ended_seq: Some(seq),
        started_at: Some(format!("{seq}Z")),
        ended_at: Some(format!("{seq}Z")),
        timing: None,
        raw: Some(json!({ "source": "providerHistory" })),
    }
}

#[test]
fn backward_page_uses_semantic_position_when_an_earlier_block_finishes_later() {
    let dir = tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
    let mut store = TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();

    let mut prompt = event("user-prompt", 2, "why is startup slow?");
    prompt.kind = "userTextDelta".to_string();
    prompt.status = Some("processing".to_string());
    prompt.raw = Some(json!({ "source": "goldBandPrompt" }));
    store.upsert(2, &prompt).unwrap();
    store
        .upsert(4, &event("warning", 4, "skill descriptions shortened"))
        .unwrap();
    let mut thought = event("thought", 5, "diagnosing");
    thought.kind = "thoughtDelta".to_string();
    store.upsert(5, &thought).unwrap();

    prompt.status = Some("cancelled".to_string());
    prompt.ended_seq = Some(511);
    prompt.ended_at = Some("511Z".to_string());
    store.upsert(511, &prompt).unwrap();
    store.force_checkpoint().unwrap();

    let page = read_indexed_timeline_page(&path, Some(5), None, None, 30).unwrap();

    assert_eq!(
        page.events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["user-prompt", "warning"]
    );
    assert!(!page.has_older);
    assert!(page.has_newer);
}
