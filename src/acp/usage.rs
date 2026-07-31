use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};

use anyhow::Result;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::acp::events::{AcpRawFrame, AcpUiEvent, load_timeline_items};
use crate::storage::{append_jsonl_durable, append_jsonl_durable_unlocked, with_jsonl_file_lock};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptTokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl AcpPromptTokenUsage {
    pub fn from_prompt_result(result: &Value) -> Option<Self> {
        Self::from_usage_value(result.get("usage")?)
    }

    fn from_usage_value(usage: &Value) -> Option<Self> {
        let prompt_usage = Self {
            input_tokens: usage.get("inputTokens").and_then(Value::as_u64),
            output_tokens: usage.get("outputTokens").and_then(Value::as_u64),
            cached_read_tokens: usage.get("cachedReadTokens").and_then(Value::as_u64),
            cached_write_tokens: usage.get("cachedWriteTokens").and_then(Value::as_u64),
            total_tokens: usage.get("totalTokens").and_then(Value::as_u64),
        };
        prompt_usage.has_any_value().then_some(prompt_usage)
    }

    fn has_any_value(&self) -> bool {
        self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.cached_read_tokens.is_some()
            || self.cached_write_tokens.is_some()
            || self.total_tokens.is_some()
    }

    pub fn effective_total_tokens(&self) -> Option<u64> {
        self.total_tokens.or_else(|| {
            let components = [
                self.input_tokens,
                self.output_tokens,
                self.cached_read_tokens,
                self.cached_write_tokens,
            ];
            components.iter().any(Option::is_some).then(|| {
                components
                    .into_iter()
                    .flatten()
                    .fold(0u64, u64::saturating_add)
            })
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAttemptTokenTotals {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_read_tokens: Option<u64>,
    pub cached_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl AcpAttemptTokenTotals {
    pub fn accumulate_prompt(&mut self, prompt: &AcpPromptTokenUsage) {
        accumulate_optional_token(&mut self.input_tokens, prompt.input_tokens);
        accumulate_optional_token(&mut self.output_tokens, prompt.output_tokens);
        accumulate_optional_token(&mut self.cached_read_tokens, prompt.cached_read_tokens);
        accumulate_optional_token(&mut self.cached_write_tokens, prompt.cached_write_tokens);
        accumulate_optional_token(&mut self.total_tokens, prompt.effective_total_tokens());
    }
}

fn accumulate_optional_token(total: &mut Option<u64>, delta: Option<u64>) {
    if let Some(delta) = delta {
        *total = Some(total.unwrap_or(0).saturating_add(delta));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum AcpPromptUsageJournalEntry {
    AttemptBaseline {
        timestamp: String,
        totals: AcpAttemptTokenTotals,
        latest_prompt: AcpPromptTokenUsage,
    },
    PromptStarted {
        turn_id: String,
        turn_seq: u64,
        timestamp: String,
    },
    PromptCompleted {
        turn_id: String,
        turn_seq: u64,
        timestamp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<Value>,
        usage: AcpPromptTokenUsage,
        #[serde(default, skip_serializing_if = "is_false")]
        recovered_from_raw: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpAttemptUsageRecovery {
    pub totals: AcpAttemptTokenTotals,
    pub latest_prompt: AcpPromptTokenUsage,
    pub completed_turns: usize,
    pub recovered_turns: usize,
}

#[derive(Debug, Clone)]
struct PromptTurnStart {
    turn_id: String,
    turn_seq: u64,
    timestamp: String,
}

#[derive(Debug, Clone)]
struct RawPromptTransaction {
    request_id: Value,
    started_at: String,
    completed_at: Option<String>,
    usage: Option<AcpPromptTokenUsage>,
}

#[derive(Debug, Clone, Default)]
struct AcpAttemptUsageBaseline {
    timestamp: String,
    totals: AcpAttemptTokenTotals,
    latest_prompt: AcpPromptTokenUsage,
}

pub fn append_prompt_started(
    journal_path: &Utf8Path,
    turn_id: &str,
    turn_seq: u64,
    timestamp: &str,
) -> Result<()> {
    append_jsonl_durable(
        journal_path,
        &AcpPromptUsageJournalEntry::PromptStarted {
            turn_id: turn_id.to_string(),
            turn_seq,
            timestamp: timestamp.to_string(),
        },
    )
}

pub fn append_prompt_completed(
    journal_path: &Utf8Path,
    turn_id: &str,
    turn_seq: u64,
    timestamp: &str,
    request_id: Option<Value>,
    usage: &AcpPromptTokenUsage,
) -> Result<bool> {
    append_prompt_completed_inner(
        journal_path,
        turn_id,
        turn_seq,
        timestamp,
        request_id,
        usage,
        false,
    )
}

pub fn repair_attempt_usage(
    snapshot_path: &Utf8Path,
    timeline_path: &Utf8Path,
    raw_path: &Utf8Path,
    journal_path: &Utf8Path,
    recover_missing_from_raw: bool,
) -> Result<AcpAttemptUsageRecovery> {
    let baseline = ensure_attempt_baseline(snapshot_path, journal_path)?;
    let timeline_items = load_timeline_items(timeline_path)?;
    let mut starts = prompt_starts_from_journal(journal_path)?;
    let known_start_ids = starts
        .iter()
        .map(|start| start.turn_id.clone())
        .collect::<HashSet<_>>();
    for event in timeline_items
        .iter()
        .filter(|event| is_gold_band_prompt(event))
    {
        if known_start_ids.contains(&event.id) {
            continue;
        }
        starts.push(PromptTurnStart {
            turn_id: event.id.clone(),
            turn_seq: event.seq,
            timestamp: event.timestamp.clone(),
        });
    }
    starts.sort_by_key(|start| start.turn_seq);

    let mut completed = prompt_completions_from_journal(journal_path)?;
    let mut recovered_turns = 0usize;
    let has_uncompleted_turn = starts
        .iter()
        .any(|start| !completed.contains_key(&start.turn_id));
    if recover_missing_from_raw && has_uncompleted_turn {
        let transactions = raw_prompt_transactions(raw_path)?;
        let assignments = assign_transactions_to_turns(&starts, &transactions);
        for (start, transaction) in assignments {
            if completed.contains_key(&start.turn_id) {
                continue;
            }
            let (Some(completed_at), Some(usage)) = (
                transaction.completed_at.as_deref(),
                transaction.usage.as_ref(),
            ) else {
                continue;
            };
            if !transaction_is_after_baseline(completed_at, usage, &baseline) {
                continue;
            }
            if append_prompt_completed_inner(
                journal_path,
                &start.turn_id,
                start.turn_seq,
                completed_at,
                Some(transaction.request_id.clone()),
                usage,
                true,
            )? {
                recovered_turns = recovered_turns.saturating_add(1);
            }
            completed.insert(start.turn_id.clone(), (start.turn_seq, usage.clone()));
        }
    }

    let mut ordered = completed.into_values().collect::<Vec<_>>();
    ordered.sort_by_key(|(turn_seq, _)| *turn_seq);
    let baseline_has_usage = baseline.latest_prompt.has_any_value()
        || baseline.totals.input_tokens.is_some()
        || baseline.totals.output_tokens.is_some()
        || baseline.totals.cached_read_tokens.is_some()
        || baseline.totals.cached_write_tokens.is_some()
        || baseline.totals.total_tokens.is_some();
    let mut totals = baseline.totals;
    let mut latest_prompt = baseline.latest_prompt;
    for (_, usage) in &ordered {
        totals.accumulate_prompt(usage);
        latest_prompt = usage.clone();
    }
    Ok(AcpAttemptUsageRecovery {
        totals,
        latest_prompt,
        completed_turns: ordered.len() + usize::from(baseline_has_usage),
        recovered_turns,
    })
}

fn ensure_attempt_baseline(
    snapshot_path: &Utf8Path,
    journal_path: &Utf8Path,
) -> Result<AcpAttemptUsageBaseline> {
    with_jsonl_file_lock(journal_path, || {
        if let Some(baseline) = attempt_baseline_from_journal_unlocked(journal_path)? {
            return Ok(baseline);
        }
        let snapshot =
            crate::storage::read_json::<Value>(snapshot_path).unwrap_or_else(|_| json!({}));
        let latest_prompt = AcpPromptTokenUsage {
            input_tokens: snapshot.get("inputTokens").and_then(Value::as_u64),
            output_tokens: snapshot.get("outputTokens").and_then(Value::as_u64),
            cached_read_tokens: snapshot.get("cachedReadTokens").and_then(Value::as_u64),
            cached_write_tokens: snapshot.get("cachedWriteTokens").and_then(Value::as_u64),
            total_tokens: snapshot.get("totalTokens").and_then(Value::as_u64),
        };
        let has_attempt_totals = [
            "attemptInputTokens",
            "attemptOutputTokens",
            "attemptCachedReadTokens",
            "attemptCachedWriteTokens",
            "attemptTotalTokens",
        ]
        .iter()
        .any(|field| snapshot.get(field).and_then(Value::as_u64).is_some());
        let totals = if has_attempt_totals {
            AcpAttemptTokenTotals {
                input_tokens: snapshot.get("attemptInputTokens").and_then(Value::as_u64),
                output_tokens: snapshot.get("attemptOutputTokens").and_then(Value::as_u64),
                cached_read_tokens: snapshot
                    .get("attemptCachedReadTokens")
                    .and_then(Value::as_u64),
                cached_write_tokens: snapshot
                    .get("attemptCachedWriteTokens")
                    .and_then(Value::as_u64),
                total_tokens: snapshot.get("attemptTotalTokens").and_then(Value::as_u64),
            }
        } else {
            // One-time migration for attempts created before cumulative fields existed.
            // The latest prompt is the only durable lower bound available when raw rolled.
            AcpAttemptTokenTotals {
                input_tokens: latest_prompt.input_tokens,
                output_tokens: latest_prompt.output_tokens,
                cached_read_tokens: latest_prompt.cached_read_tokens,
                cached_write_tokens: latest_prompt.cached_write_tokens,
                total_tokens: latest_prompt.effective_total_tokens(),
            }
        };
        let baseline = AcpAttemptUsageBaseline {
            timestamp: snapshot
                .get("updatedAt")
                .and_then(Value::as_str)
                .unwrap_or("0Z")
                .to_string(),
            totals,
            latest_prompt,
        };
        append_journal_entry_durable_unlocked(
            journal_path,
            &AcpPromptUsageJournalEntry::AttemptBaseline {
                timestamp: baseline.timestamp.clone(),
                totals: baseline.totals.clone(),
                latest_prompt: baseline.latest_prompt.clone(),
            },
        )?;
        Ok(baseline)
    })
}

fn attempt_baseline_from_journal_unlocked(
    path: &Utf8Path,
) -> Result<Option<AcpAttemptUsageBaseline>> {
    for entry in read_journal_unlocked(path)? {
        if let AcpPromptUsageJournalEntry::AttemptBaseline {
            timestamp,
            totals,
            latest_prompt,
        } = entry
        {
            return Ok(Some(AcpAttemptUsageBaseline {
                timestamp,
                totals,
                latest_prompt,
            }));
        }
    }
    Ok(None)
}

fn transaction_is_after_baseline(
    completed_at: &str,
    usage: &AcpPromptTokenUsage,
    baseline: &AcpAttemptUsageBaseline,
) -> bool {
    match timestamp_key(completed_at).cmp(&timestamp_key(&baseline.timestamp)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => usage != &baseline.latest_prompt,
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn append_prompt_completed_inner(
    journal_path: &Utf8Path,
    turn_id: &str,
    turn_seq: u64,
    timestamp: &str,
    request_id: Option<Value>,
    usage: &AcpPromptTokenUsage,
    recovered_from_raw: bool,
) -> Result<bool> {
    with_jsonl_file_lock(journal_path, || {
        if prompt_completions_from_journal_unlocked(journal_path)?.contains_key(turn_id) {
            return Ok(false);
        }
        let entry = AcpPromptUsageJournalEntry::PromptCompleted {
            turn_id: turn_id.to_string(),
            turn_seq,
            timestamp: timestamp.to_string(),
            request_id,
            usage: usage.clone(),
            recovered_from_raw,
        };
        append_journal_entry_durable_unlocked(journal_path, &entry)?;
        Ok(true)
    })
}

fn append_journal_entry_durable_unlocked(
    journal_path: &Utf8Path,
    entry: &AcpPromptUsageJournalEntry,
) -> Result<()> {
    append_jsonl_durable_unlocked(journal_path, entry)
}

fn prompt_starts_from_journal(path: &Utf8Path) -> Result<Vec<PromptTurnStart>> {
    let mut starts = HashMap::<String, PromptTurnStart>::new();
    for entry in read_journal(path)? {
        if let AcpPromptUsageJournalEntry::PromptStarted {
            turn_id,
            turn_seq,
            timestamp,
        } = entry
        {
            starts.entry(turn_id.clone()).or_insert(PromptTurnStart {
                turn_id,
                turn_seq,
                timestamp,
            });
        }
    }
    Ok(starts.into_values().collect())
}

fn prompt_completions_from_journal(
    path: &Utf8Path,
) -> Result<HashMap<String, (u64, AcpPromptTokenUsage)>> {
    with_jsonl_file_lock(path, || prompt_completions_from_journal_unlocked(path))
}

fn prompt_completions_from_journal_unlocked(
    path: &Utf8Path,
) -> Result<HashMap<String, (u64, AcpPromptTokenUsage)>> {
    let mut completed = HashMap::new();
    for entry in read_journal_unlocked(path)? {
        if let AcpPromptUsageJournalEntry::PromptCompleted {
            turn_id,
            turn_seq,
            usage,
            ..
        } = entry
        {
            completed.entry(turn_id).or_insert((turn_seq, usage));
        }
    }
    Ok(completed)
}

fn read_journal(path: &Utf8Path) -> Result<Vec<AcpPromptUsageJournalEntry>> {
    with_jsonl_file_lock(path, || read_journal_unlocked(path))
}

fn read_journal_unlocked(path: &Utf8Path) -> Result<Vec<AcpPromptUsageJournalEntry>> {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str(&line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn is_gold_band_prompt(event: &AcpUiEvent) -> bool {
    event.kind == "userTextDelta"
        && event
            .raw
            .as_ref()
            .and_then(|raw| raw.get("source"))
            .and_then(Value::as_str)
            == Some("goldBandPrompt")
}

fn raw_prompt_transactions(path: &Utf8Path) -> Result<Vec<RawPromptTransaction>> {
    let Ok(file) = std::fs::File::open(path.as_std_path()) else {
        return Ok(Vec::new());
    };
    let mut transactions = Vec::<RawPromptTransaction>::new();
    let mut pending = HashMap::<String, usize>::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(raw) = serde_json::from_str::<AcpRawFrame>(&line) else {
            continue;
        };
        let frame = &raw.frame;
        if raw.direction == "outbound"
            && frame.get("method").and_then(Value::as_str) == Some("session/prompt")
        {
            let Some(request_id) = frame.get("id").cloned() else {
                continue;
            };
            let index = transactions.len();
            pending.insert(rpc_id_key(&request_id), index);
            transactions.push(RawPromptTransaction {
                request_id,
                started_at: raw.timestamp,
                completed_at: None,
                usage: None,
            });
            continue;
        }
        if raw.direction != "inbound" {
            continue;
        }
        let Some(usage) = frame
            .get("result")
            .and_then(AcpPromptTokenUsage::from_prompt_result)
        else {
            continue;
        };
        let request_id = frame.get("id").cloned().unwrap_or(Value::Null);
        if let Some(index) = pending.remove(&rpc_id_key(&request_id)) {
            transactions[index].completed_at = Some(raw.timestamp);
            transactions[index].usage = Some(usage);
        } else {
            transactions.push(RawPromptTransaction {
                request_id,
                started_at: raw.timestamp.clone(),
                completed_at: Some(raw.timestamp),
                usage: Some(usage),
            });
        }
    }
    Ok(transactions)
}

fn assign_transactions_to_turns<'a>(
    starts: &'a [PromptTurnStart],
    transactions: &'a [RawPromptTransaction],
) -> Vec<(&'a PromptTurnStart, &'a RawPromptTransaction)> {
    let mut assignments = Vec::new();
    let mut last_turn_seq = None::<u64>;
    for transaction in transactions {
        let candidate = starts
            .iter()
            .filter(|start| {
                last_turn_seq.is_none_or(|last| start.turn_seq > last)
                    && timestamp_key(&start.timestamp) <= timestamp_key(&transaction.started_at)
            })
            .max_by(|left, right| {
                timestamp_key(&left.timestamp)
                    .cmp(&timestamp_key(&right.timestamp))
                    .then_with(|| right.turn_seq.cmp(&left.turn_seq))
            });
        if let Some(start) = candidate {
            last_turn_seq = Some(start.turn_seq);
            assignments.push((start, transaction));
        }
    }
    assignments
}

fn timestamp_key(timestamp: &str) -> u64 {
    timestamp.trim_end_matches('Z').parse().unwrap_or_default()
}

fn rpc_id_key(request_id: &Value) -> String {
    serde_json::to_string(request_id).unwrap_or_else(|_| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::acp::events::{AcpAttemptPaths, write_timeline_items};
    use crate::storage::append_jsonl;

    fn prompt_event(seq: u64, timestamp: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: format!("gold-band-user-prompt-{seq}"),
            seq,
            timestamp: timestamp.to_string(),
            kind: "userTextDelta".to_string(),
            session_id: Some("session-1".to_string()),
            content: Some("prompt".to_string()),
            title: None,
            tool_call_id: None,
            status: Some("completed".to_string()),
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: Some(json!({ "source": "goldBandPrompt" })),
        }
    }

    fn raw_frame(timestamp: &str, direction: &str, frame: Value) -> AcpRawFrame {
        AcpRawFrame {
            timestamp: timestamp.to_string(),
            direction: direction.to_string(),
            frame,
        }
    }

    #[test]
    fn repairs_completed_prompt_from_raw_when_snapshot_checkpoint_was_skipped_by_crash() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        let prompts = vec![prompt_event(1, "100Z"), prompt_event(15, "200Z")];
        write_timeline_items(&paths.timeline, &prompts).unwrap();
        append_prompt_started(&paths.prompt_usage, &prompts[0].id, 1, "100Z").unwrap();
        append_prompt_started(&paths.prompt_usage, &prompts[1].id, 15, "200Z").unwrap();
        append_prompt_completed(
            &paths.prompt_usage,
            &prompts[0].id,
            1,
            "110Z",
            Some(json!(3)),
            &AcpPromptTokenUsage {
                input_tokens: Some(100),
                output_tokens: Some(10),
                total_tokens: Some(110),
                ..Default::default()
            },
        )
        .unwrap();
        for frame in [
            raw_frame(
                "100Z",
                "outbound",
                json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{}}),
            ),
            raw_frame(
                "110Z",
                "inbound",
                json!({"jsonrpc":"2.0","id":3,"result":{"usage":{"inputTokens":100,"outputTokens":10,"totalTokens":110}}}),
            ),
            raw_frame(
                "200Z",
                "outbound",
                json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{}}),
            ),
            raw_frame(
                "210Z",
                "inbound",
                json!({"jsonrpc":"2.0","id":4,"result":{"usage":{"inputTokens":70,"outputTokens":20,"cachedReadTokens":30,"totalTokens":120}}}),
            ),
        ] {
            append_jsonl(&paths.raw, &frame).unwrap();
        }

        let recovered = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();

        assert_eq!(recovered.completed_turns, 2);
        assert_eq!(recovered.recovered_turns, 1);
        assert_eq!(recovered.totals.input_tokens, Some(170));
        assert_eq!(recovered.totals.output_tokens, Some(30));
        assert_eq!(recovered.totals.cached_read_tokens, Some(30));
        assert_eq!(recovered.totals.total_tokens, Some(230));

        let second_read = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();
        assert_eq!(second_read.recovered_turns, 0);
        assert_eq!(second_read.totals, recovered.totals);
    }

    #[test]
    fn recovers_task_118_crash_pattern_across_reused_rpc_ids() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        let prompt_specs = [
            (1, "1785431293Z"),
            (15, "1785431324Z"),
            (429, "1785431369Z"),
            (1429, "1785431727Z"),
            (2605, "1785431791Z"),
            (2640, "1785431874Z"),
            (3396, "1785431942Z"),
        ];
        let prompts = prompt_specs
            .into_iter()
            .map(|(seq, timestamp)| prompt_event(seq, timestamp))
            .collect::<Vec<_>>();
        write_timeline_items(&paths.timeline, &prompts).unwrap();

        let completed = [
            (3, "1785431293Z", "1785431298Z", 29_511, 12, 0, 29_523),
            (4, "1785431324Z", "1785431342Z", 39_419, 514, 36_352, 76_285),
            (
                5,
                "1785431369Z",
                "1785431414Z",
                4_095,
                1_231,
                117_248,
                122_574,
            ),
            (
                3,
                "1785431727Z",
                "1785431758Z",
                30_366,
                1_278,
                49_152,
                80_796,
            ),
            (
                4,
                "1785431791Z",
                "1785431812Z",
                13_702,
                329,
                186_368,
                200_399,
            ),
            (
                3,
                "1785431874Z",
                "1785431906Z",
                16_784,
                1_066,
                134_656,
                152_506,
            ),
        ];
        for (request_id, started_at, completed_at, input, output, cache_read, total) in completed {
            append_jsonl(
                &paths.raw,
                &raw_frame(
                    started_at,
                    "outbound",
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "method": "session/prompt",
                        "params": {}
                    }),
                ),
            )
            .unwrap();
            append_jsonl(
                &paths.raw,
                &raw_frame(
                    completed_at,
                    "inbound",
                    json!({
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {
                            "usage": {
                                "inputTokens": input,
                                "outputTokens": output,
                                "cachedReadTokens": cache_read,
                                "cachedWriteTokens": 0,
                                "totalTokens": total
                            }
                        }
                    }),
                ),
            )
            .unwrap();
        }

        let recovered = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();

        assert_eq!(recovered.completed_turns, 6);
        assert_eq!(recovered.recovered_turns, 6);
        assert_eq!(recovered.totals.input_tokens, Some(133_877));
        assert_eq!(recovered.totals.output_tokens, Some(4_430));
        assert_eq!(recovered.totals.cached_read_tokens, Some(523_776));
        assert_eq!(recovered.totals.total_tokens, Some(662_083));
        assert_eq!(recovered.latest_prompt.total_tokens, Some(152_506));
    }

    #[test]
    fn legacy_snapshot_becomes_a_non_decreasing_attempt_baseline() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        std::fs::write(
            paths.snapshot.as_std_path(),
            serde_json::to_vec(&json!({
                "updatedAt": "500Z",
                "inputTokens": 90_254,
                "outputTokens": 7_513,
                "cachedReadTokens": 559_616,
                "totalTokens": 657_383
            }))
            .unwrap(),
        )
        .unwrap();

        let recovered = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();

        assert_eq!(recovered.completed_turns, 1);
        assert_eq!(recovered.totals.input_tokens, Some(90_254));
        assert_eq!(recovered.totals.output_tokens, Some(7_513));
        assert_eq!(recovered.totals.cached_read_tokens, Some(559_616));
        assert_eq!(recovered.totals.total_tokens, Some(657_383));
    }
}
