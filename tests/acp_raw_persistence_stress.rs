use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use gold_band::acp::events::{AcpRawFrame, append_raw_frame, append_raw_frames};
use serde_json::{Value, json};
use tempfile::tempdir;

const STRESS_FRAME_COUNT: usize = 13_245;
const STRESS_TOOL_IDENTITY_COUNT: usize = 19;
const STRESS_FRAME_BYTES: usize = 69_752_109;
const STRESS_BATCH_FRAMES: usize = 128;
const STRESS_RAW_MAX_BYTES: u64 = 2 * 1024 * 1024;
const STRESS_RAW_TARGET_BYTES: u64 = 1024 * 1024;
const DEFAULT_MAX_TOTAL_MS: u64 = 3_000;
const MAX_TOTAL_MS_ENV: &str = "GOLD_BAND_RAW_STRESS_MAX_TOTAL_MS";

fn max_total_duration() -> Duration {
    Duration::from_millis(
        std::env::var(MAX_TOTAL_MS_ENV)
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_TOTAL_MS),
    )
}

fn frame_with_encoded_size(index: usize, target_bytes: usize) -> Value {
    let tool_call_id = format!("tool-{}", index % STRESS_TOOL_IDENTITY_COUNT);
    let mut frame = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "raw-persistence-stress",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": tool_call_id,
                "status": "in_progress",
                "content": "",
            }
        }
    });
    let base_bytes = serde_json::to_vec(&frame).unwrap().len();
    assert!(target_bytes >= base_bytes);
    frame["params"]["update"]["content"] = Value::String("x".repeat(target_bytes - base_bytes));
    assert_eq!(serde_json::to_vec(&frame).unwrap().len(), target_bytes);
    frame
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = samples.len().saturating_sub(1).saturating_mul(percentile) / 100;
    samples[index]
}

#[test]
fn raw_group_commit_preserves_fifo_across_rolls() {
    let dir = tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.raw.jsonl")).unwrap();
    append_raw_frame(
        &path,
        "outbound",
        json!({"jsonrpc":"2.0","id":1,"method":"session/new"}),
        8 * 1024,
        4 * 1024,
    )
    .unwrap();

    for batch_start in (0..200).step_by(20) {
        let frames = (batch_start..batch_start + 20)
            .map(|index| {
                json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "update": {
                            "sessionUpdate": "tool_call_update",
                            "index": index,
                            "content": "x".repeat(200),
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        append_raw_frames(&path, "inbound", &frames, 8 * 1024, 4 * 1024).unwrap();
    }

    let records = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<AcpRawFrame>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records[0].frame["method"], "session/new");
    let retained_indices = records
        .iter()
        .filter_map(|record| record.frame.pointer("/params/update/index")?.as_u64())
        .collect::<Vec<_>>();
    assert!(!retained_indices.is_empty());
    assert_eq!(retained_indices.last(), Some(&199));
    assert!(retained_indices.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
#[ignore = "manual release-mode Raw persistence stress test"]
fn raw_persistence_stress_keeps_up_with_the_recorded_tool_burst() {
    let dir = tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("acp.raw.jsonl")).unwrap();
    let bytes_per_frame = STRESS_FRAME_BYTES / STRESS_FRAME_COUNT;
    let extra_bytes = STRESS_FRAME_BYTES % STRESS_FRAME_COUNT;
    let started_at = Instant::now();
    let mut batch_latencies = Vec::new();
    let mut written_frames = 0usize;

    for batch_start in (0..STRESS_FRAME_COUNT).step_by(STRESS_BATCH_FRAMES) {
        let batch_end = (batch_start + STRESS_BATCH_FRAMES).min(STRESS_FRAME_COUNT);
        let frames = (batch_start..batch_end)
            .map(|index| {
                frame_with_encoded_size(index, bytes_per_frame + usize::from(index < extra_bytes))
            })
            .collect::<Vec<_>>();
        let batch_started_at = Instant::now();
        append_raw_frames(
            &path,
            "inbound",
            &frames,
            STRESS_RAW_MAX_BYTES,
            STRESS_RAW_TARGET_BYTES,
        )
        .unwrap();
        batch_latencies.push(batch_started_at.elapsed());
        written_frames = written_frames.saturating_add(frames.len());
    }

    let elapsed = started_at.elapsed();
    batch_latencies.sort_unstable();
    let max_batch = batch_latencies.last().copied().unwrap_or_default();
    let p95_batch = percentile(&batch_latencies, 95);
    let p99_batch = percentile(&batch_latencies, 99);
    let final_bytes = std::fs::metadata(&path).unwrap().len();
    eprintln!(
        "raw stress: frames={written_frames} input_bytes={STRESS_FRAME_BYTES} batches={} total={elapsed:?} p95={p95_batch:?} p99={p99_batch:?} max={max_batch:?} final_bytes={final_bytes}",
        batch_latencies.len(),
    );

    assert_eq!(written_frames, STRESS_FRAME_COUNT);
    assert!(final_bytes > 0);
    assert!(
        elapsed <= max_total_duration(),
        "Raw persistence took {elapsed:?}, exceeding the {:?} recorded-burst budget",
        max_total_duration()
    );
}
