use std::collections::HashMap;
use std::io::{BufRead as _, BufReader};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use gold_band::acp::events::{
    AcpRawFrame, AcpUiEvent, load_timeline_items, normalize_session_update,
};
use gold_band::acp::timeline::{TimelineCompactionPolicy, TimelineStore};
use serde_json::json;
use tempfile::tempdir;

const DEFAULT_STRESS_UPDATE_COUNT: usize = 100_000;
const STRESS_FRAME_BATCH_SIZE: usize = 256;
const STRESS_UPDATE_COUNT_ENV: &str = "GOLD_BAND_TIMELINE_STRESS_UPDATES";
const RAW_REPLAY_PATH_ENV: &str = "GOLD_BAND_ACP_RAW_REPLAY_PATH";
const RAW_REPLAY_REQUEST_ID_ENV: &str = "GOLD_BAND_ACP_RAW_REPLAY_REQUEST_ID";
const RAW_REPLAY_START_REVISION_ENV: &str = "GOLD_BAND_ACP_RAW_REPLAY_START_REVISION";

fn stress_update_count() -> usize {
    std::env::var(STRESS_UPDATE_COUNT_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|count| *count > 0)
        .unwrap_or(DEFAULT_STRESS_UPDATE_COUNT)
}

fn tool_update(identity: usize, seq: u64) -> AcpUiEvent {
    let tool_call_id = format!("tool-{identity}");
    AcpUiEvent {
        id: format!("tool-call-{tool_call_id}"),
        seq,
        timestamp: format!("{seq}Z"),
        kind: "toolCall".to_string(),
        session_id: Some("stress-session".to_string()),
        content: Some(format!("latest-output-{seq}")),
        title: Some(format!("Stress tool {identity}")),
        tool_call_id: Some(tool_call_id.clone()),
        status: Some("in_progress".to_string()),
        started_seq: Some(identity as u64 + 1),
        ended_seq: Some(seq),
        started_at: Some("1Z".to_string()),
        ended_at: Some(format!("{seq}Z")),
        timing: None,
        raw: Some(json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": tool_call_id,
            "sequence": seq,
        })),
    }
}

#[derive(Debug)]
struct StressResult {
    identities: usize,
    updates: usize,
    commits: usize,
    compactions: usize,
    elapsed: Duration,
    max_commit: Duration,
    max_compacting_commit: Duration,
    max_regular_commit: Duration,
    max_compaction: Duration,
    p95_commit: Duration,
    p99_commit: Duration,
    timeline_bytes: u64,
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = samples.len().saturating_sub(1).saturating_mul(percentile) / 100;
    samples[index]
}

fn run_stress(identity_count: usize, update_count: usize) -> StressResult {
    let dir = tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
    let mut store = TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();
    let started_at = Instant::now();
    let mut latencies = Vec::new();
    let mut commits = 0usize;
    let mut compactions = 0usize;
    let mut max_compacting_commit = Duration::ZERO;
    let mut max_regular_commit = Duration::ZERO;
    let mut max_compaction = Duration::ZERO;

    for batch_start in (0..update_count).step_by(STRESS_FRAME_BATCH_SIZE) {
        let batch_end = (batch_start + STRESS_FRAME_BATCH_SIZE).min(update_count);
        let mut latest = HashMap::<String, (u64, AcpUiEvent)>::with_capacity(identity_count);
        for update_index in batch_start..batch_end {
            let identity = update_index % identity_count;
            let seq = update_index as u64 + 1;
            let event = tool_update(identity, seq);
            latest.insert(event.id.clone(), (seq, event));
        }
        let mut updates = latest.into_values().collect::<Vec<_>>();
        updates.sort_by_key(|(revision, _)| *revision);

        let commit_started_at = Instant::now();
        store.upsert_batch(&updates).unwrap();
        let commit_elapsed = commit_started_at.elapsed();
        if let Some(compaction_elapsed) = store.take_last_compaction_elapsed() {
            compactions = compactions.saturating_add(1);
            max_compacting_commit = max_compacting_commit.max(commit_elapsed);
            max_compaction = max_compaction.max(compaction_elapsed);
        } else {
            max_regular_commit = max_regular_commit.max(commit_elapsed);
        }
        latencies.push(commit_elapsed);
        commits = commits.saturating_add(1);
    }
    store.force_checkpoint().unwrap();
    let elapsed = started_at.elapsed();
    latencies.sort_unstable();
    let max_commit = latencies.last().copied().unwrap_or_default();
    let timeline_bytes = std::fs::metadata(&path).unwrap().len();
    let final_items = load_timeline_items(&path).unwrap();
    assert_eq!(final_items.len(), identity_count);
    let latest_item = final_items
        .iter()
        .max_by_key(|item| item.ended_seq)
        .expect("stress timeline must retain its latest canonical item");
    assert_eq!(latest_item.ended_seq, Some(update_count as u64));
    let expected_content = format!("latest-output-{update_count}");
    assert_eq!(
        latest_item.content.as_deref(),
        Some(expected_content.as_str())
    );

    StressResult {
        identities: identity_count,
        updates: update_count,
        commits,
        compactions,
        elapsed,
        max_commit,
        max_compacting_commit,
        max_regular_commit,
        max_compaction,
        p95_commit: percentile(&latencies, 95),
        p99_commit: percentile(&latencies, 99),
        timeline_bytes,
    }
}

fn rpc_id_matches(value: Option<&serde_json::Value>, expected: &str) -> bool {
    value.is_some_and(|value| {
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.as_u64().map(|value| value.to_string()))
            .as_deref()
            == Some(expected)
    })
}

fn replay_stream_item(
    streams: &mut HashMap<String, AcpUiEvent>,
    mut event: AcpUiEvent,
) -> AcpUiEvent {
    let prefix = match event.kind.as_str() {
        "textDelta" => "assistant-message-root",
        "thoughtDelta" => "assistant-thought-root",
        _ => return event,
    };
    let source_id = event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("messageId"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| event.id.clone());
    let item_id = format!("{prefix}-{source_id}");
    if let Some(previous) = streams.get(&item_id) {
        let mut content = previous.content.clone().unwrap_or_default();
        content.push_str(event.content.as_deref().unwrap_or_default());
        event.content = Some(content);
        event.started_seq = previous.started_seq.or(Some(previous.seq));
        event.started_at = previous
            .started_at
            .clone()
            .or_else(|| Some(previous.timestamp.clone()));
    } else {
        event.started_seq = Some(event.seq);
        event.started_at = Some(event.timestamp.clone());
    }
    event.id = item_id.clone();
    event.ended_seq = Some(event.seq);
    event.ended_at = Some(event.timestamp.clone());
    streams.insert(item_id, event.clone());
    event
}

fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic payload>")
}

#[test]
#[ignore = "requires an explicit ACP raw capture path"]
fn captured_prompt_session_updates_replay_through_timeline_storage() {
    let raw_path = std::env::var(RAW_REPLAY_PATH_ENV)
        .unwrap_or_else(|_| panic!("set {RAW_REPLAY_PATH_ENV} to an acp.raw.jsonl capture"));
    let request_id = std::env::var(RAW_REPLAY_REQUEST_ID_ENV).unwrap_or_else(|_| "88".to_string());
    let mut revision = std::env::var(RAW_REPLAY_START_REVISION_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_466);
    let file = std::fs::File::open(&raw_path).unwrap();
    let dir = tempdir().unwrap();
    let timeline_path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
    let mut store =
        TimelineStore::open(timeline_path.clone(), TimelineCompactionPolicy::default()).unwrap();
    let mut prompt_seen = false;
    let mut terminal_response_seen = false;
    let mut capture_line = 0usize;
    let mut replayed_updates = 0usize;
    let mut streams = HashMap::<String, AcpUiEvent>::new();

    for line in BufReader::new(file).lines() {
        capture_line = capture_line.saturating_add(1);
        let raw: AcpRawFrame = serde_json::from_str(&line.unwrap())
            .unwrap_or_else(|error| panic!("invalid raw frame at line {capture_line}: {error}"));
        let frame = raw.frame;
        if !prompt_seen {
            prompt_seen = frame.get("method").and_then(serde_json::Value::as_str)
                == Some("session/prompt")
                && rpc_id_matches(frame.get("id"), &request_id);
            continue;
        }
        if rpc_id_matches(frame.get("id"), &request_id)
            && (frame.get("result").is_some() || frame.get("error").is_some())
        {
            terminal_response_seen = true;
            break;
        }
        if raw.direction != "inbound"
            || frame.get("method").and_then(serde_json::Value::as_str) != Some("session/update")
        {
            continue;
        }

        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        let session_id = params
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let update = params.get("update").cloned().unwrap_or(params);
        revision = revision.saturating_add(1);
        let mut event = normalize_session_update(revision, session_id, &update);
        event.timestamp = raw.timestamp;
        let event = replay_stream_item(&mut streams, event);
        let outcome = catch_unwind(AssertUnwindSafe(|| store.upsert(revision, &event)));
        match outcome {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                panic!(
                    "timeline replay failed at raw line {capture_line}, revision {revision}, kind {}: {error:#}",
                    event.kind
                );
            }
            Err(payload) => {
                panic!(
                    "timeline replay panicked at raw line {capture_line}, revision {revision}, kind {}: {}",
                    event.kind,
                    panic_payload_text(payload.as_ref())
                );
            }
        }
        replayed_updates = replayed_updates.saturating_add(1);
    }

    assert!(
        prompt_seen,
        "request {request_id} was not found in raw capture"
    );
    assert!(
        replayed_updates > 0,
        "request {request_id} had no session updates"
    );
    store.force_checkpoint().unwrap();
    let items = load_timeline_items(&timeline_path).unwrap();
    assert!(!items.is_empty());
    eprintln!(
        "replayed request {request_id}: updates={replayed_updates}, final_revision={revision}, items={}, terminal_response_seen={terminal_response_seen}",
        items.len()
    );
}

#[test]
fn ratio_compaction_waits_for_a_meaningful_patch_volume() {
    let dir = tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.timeline.jsonl")).unwrap();
    let mut store = TimelineStore::open(path.clone(), TimelineCompactionPolicy::default()).unwrap();

    for revision in 1..=5 {
        store.upsert(revision, &tool_update(1, revision)).unwrap();
        assert!(store.take_last_compaction_elapsed().is_none());
    }

    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 5);
}

#[test]
#[ignore = "manual release-mode persistence stress test"]
fn timeline_persistence_stress_reports_large_backlog_latency() {
    let update_count = stress_update_count();
    let same_identity = run_stress(1, update_count);
    let many_identities = run_stress(256, update_count);

    for (name, result) in [
        ("same_identity", &same_identity),
        ("many_identities", &many_identities),
    ] {
        eprintln!(
            "{name}: identities={} updates={} commits={} compactions={} total={:?} max={:?} p95={:?} p99={:?} max_compacting={:?} max_regular={:?} max_compaction={:?} bytes={}",
            result.identities,
            result.updates,
            result.commits,
            result.compactions,
            result.elapsed,
            result.max_commit,
            result.p95_commit,
            result.p99_commit,
            result.max_compacting_commit,
            result.max_regular_commit,
            result.max_compaction,
            result.timeline_bytes,
        );
    }

    assert_eq!(same_identity.updates, update_count);
    assert_eq!(many_identities.updates, update_count);
    let expected_commits = update_count.div_ceil(STRESS_FRAME_BATCH_SIZE);
    assert_eq!(same_identity.commits, expected_commits);
    assert_eq!(many_identities.commits, expected_commits);
    assert!(same_identity.timeline_bytes > 0);
    assert!(many_identities.timeline_bytes > 0);
}
