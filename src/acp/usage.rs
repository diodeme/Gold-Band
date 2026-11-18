use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};

use anyhow::Result;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::acp::events::AcpRawFrame;
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

    /// 是否为有意义的用量：全零或全空的 `usage` 通常表示适配器「未上报」，
    /// 不能当作权威结算值（否则会吞掉流式通道的实测数据）。
    pub fn has_positive_tokens(&self) -> bool {
        [
            self.input_tokens,
            self.output_tokens,
            self.cached_read_tokens,
            self.cached_write_tokens,
            self.total_tokens,
        ]
        .into_iter()
        .flatten()
        .any(|value| value > 0)
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

/// 一条可累加的流式 token 样本：来自逐次模型调用的用量通知（ACP `usage_update`、
/// provider `_meta.usage` 等）。它与「结算通道」（`session/prompt` 响应自带 `usage`）
/// 互斥使用，见 `AcpUsageState::finish_stream_turn`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpStreamUsageSample {
    /// 样本稳定标识。同一 `id` 的重复通知只记一次；缺失 id 的样本不可信
    /// （重复推送无法去重），消费端必须丢弃并计数。
    pub id: Option<String>,
    pub usage: AcpPromptTokenUsage,
}

/// 用量承载位置：顶层，或 `_meta` 下一层。`used`/`size`（上下文占用）、
/// `cost`（额度/货币）与 `usageByCategory`（估算拆分）都不是消费量，不在此列。
const STREAM_USAGE_LOCATIONS: [&str; 4] = ["usage", "rawUsage", "modelUsage", "tokenUsage"];

/// 输入 token 的显式（不含缓存）键名。
const INPUT_EXCLUSIVE_KEYS: [&str; 2] = ["input_tokens", "inputTokens"];
/// 输入 token 的包含缓存键名（OpenAI 语义：`prompt_tokens` 含缓存命中）。
const INPUT_INCLUSIVE_KEYS: [&str; 2] = ["prompt_tokens", "promptTokens"];
const OUTPUT_KEYS: [&str; 4] = [
    "output_tokens",
    "outputTokens",
    "completion_tokens",
    "completionTokens",
];
const CACHE_READ_KEYS: [&str; 9] = [
    "cache_read_input_tokens",
    "cacheReadInputTokens",
    "cache_read_tokens",
    "cacheReadTokens",
    "cached_read_tokens",
    "cachedReadTokens",
    "prompt_cache_hit_tokens",
    "promptTokensCached",
    "prompt_tokens_cached",
];
const CACHE_WRITE_KEYS: [&str; 6] = [
    "cache_creation_input_tokens",
    "cacheCreationInputTokens",
    "cache_write_tokens",
    "cacheWriteTokens",
    "cached_write_tokens",
    "cachedWriteTokens",
];
/// 嵌套缓存明细：这些字段所在的输入口径**包含**缓存（OpenAI usage details）。
const CACHED_DETAIL_PARENTS: [&str; 4] = [
    "input_tokens_details",
    "inputTokensDetails",
    "prompt_tokens_details",
    "promptTokensDetails",
];
const TOTAL_KEYS: [&str; 2] = ["total_tokens", "totalTokens"];

/// 从任一 ACP 帧里识别可累加的 token 用量样本。
///
/// 通用识别（不按 agent 分支）：只认标准键名的 token 四元组，不认上下文占用、
/// 额度/货币和估算拆分；同一帧多个承载位置都命中时由消费端按 `id` 去重。
pub fn stream_usage_samples(frame: &Value) -> Vec<AcpStreamUsageSample> {
    let mut samples = Vec::new();
    for location in STREAM_USAGE_LOCATIONS {
        if let Some(value) = frame.get(location) {
            push_stream_sample(&mut samples, frame, value);
        }
    }
    let Some(meta) = frame.get("_meta").filter(|meta| meta.is_object()) else {
        return samples;
    };
    for location in STREAM_USAGE_LOCATIONS {
        if let Some(value) = meta.get(location) {
            push_stream_sample(&mut samples, frame, value);
        }
    }
    // 少数适配器把 token 字段直接摊在 `_meta` 上。
    push_stream_sample(&mut samples, frame, meta);
    samples
}

fn push_stream_sample(samples: &mut Vec<AcpStreamUsageSample>, frame: &Value, value: &Value) {
    if !value.is_object() {
        return;
    }
    let Some(usage) = token_usage_from_value(value) else {
        return;
    };
    samples.push(AcpStreamUsageSample {
        id: stream_sample_id(frame, value),
        usage,
    });
}

/// 通用 token 字段识别：仅当对象里存在可识别的 token 字段时才得到样本。
fn token_usage_from_value(value: &Value) -> Option<AcpPromptTokenUsage> {
    let explicit_input = first_token(value, &INPUT_EXCLUSIVE_KEYS);
    let inclusive_input = first_token(value, &INPUT_INCLUSIVE_KEYS);
    let output = first_token(value, &OUTPUT_KEYS);
    let flat_cache_read = first_token(value, &CACHE_READ_KEYS);
    let nested_cache_read = CACHED_DETAIL_PARENTS.iter().find_map(|parent| {
        value
            .get(*parent)
            .and_then(|details| first_token(details, &["cached_tokens", "cachedTokens"]))
    });
    let cache_read = flat_cache_read.or(nested_cache_read);
    let cache_write = first_token(value, &CACHE_WRITE_KEYS);
    let total = first_token(value, &TOTAL_KEYS);

    let (mut input, input_inclusive) = match explicit_input {
        Some(input) => (Some(input), false),
        None => (inclusive_input, inclusive_input.is_some()),
    };
    if input.is_none() && output.is_none() && cache_read.is_none() && cache_write.is_none() {
        return None;
    }

    // 输入口径是否包含缓存：OpenAI 语义的 `prompt_tokens`、嵌套 details 明细，
    // 以及 `total_tokens == input + output`（排除缓存）都说明缓存已计入输入。
    let cache_sum = cache_read
        .unwrap_or(0)
        .saturating_add(cache_write.unwrap_or(0));
    let inclusive = input_inclusive
        || nested_cache_read.is_some()
        || total
            .zip(input)
            .zip(output)
            .is_some_and(|((total, input), output)| total == input.saturating_add(output))
            && cache_sum > 0;
    if inclusive {
        input = input.map(|input| input.saturating_sub(cache_sum));
    }

    let usage = AcpPromptTokenUsage {
        input_tokens: input,
        output_tokens: output,
        cached_read_tokens: cache_read,
        cached_write_tokens: cache_write,
        total_tokens: total,
    };
    usage.has_any_value().then_some(usage)
}

fn first_token(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| token_count(value.get(*key)?))
}

fn token_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|value| *value >= 0.0)
            .map(|value| value.round() as u64)
    })
}

/// 样本稳定 id：先看 usage 对象自带的字段，再看 `_meta` 的命名空间键
/// （`codebuddy.ai/messageId` 等）。按「每次模型调用唯一」的可靠度排序，
/// 名字里带限定词的键（`conversationRequestId`、`modelRequestId`）不会被当成样本 id。
const SAMPLE_ID_PRIORITY: [&str; 9] = [
    "messageid",
    "llmmessageid",
    "llmid",
    "callid",
    "responseid",
    "sampleid",
    "usageid",
    "requestid",
    "id",
];

fn stream_sample_id(frame: &Value, usage: &Value) -> Option<String> {
    const DIRECT_KEYS: [&str; 11] = [
        "messageId",
        "message_id",
        "callId",
        "call_id",
        "responseId",
        "response_id",
        "sampleId",
        "usageId",
        "requestId",
        "request_id",
        "id",
    ];
    for key in DIRECT_KEYS {
        if let Some(id) = usage
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            return Some(id.to_string());
        }
    }
    let meta = frame.get("_meta")?.as_object()?;
    for wanted in SAMPLE_ID_PRIORITY {
        for (key, value) in meta {
            if sample_id_key_matches(key, wanted) {
                if let Some(id) = value.as_str().map(str::trim).filter(|id| !id.is_empty()) {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

/// `_meta` 键按末段（`/` 或 `.` 之后）归一化比较，兼容 `codebuddy.ai/`、`foo.bar/`
/// 等命名空间前缀。
fn sample_id_key_matches(key: &str, wanted: &str) -> bool {
    let terminal = key.rsplit(['/', '.']).next().unwrap_or(key);
    let normalized = terminal
        .chars()
        .filter(|character| *character != '_' && *character != '-')
        .filter_map(|character| character.to_lowercase().next())
        .collect::<String>();
    normalized == wanted
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum AcpPromptUsageJournalEntry {
    AttemptBaseline {
        timestamp: String,
        totals: AcpAttemptTokenTotals,
        latest_prompt: AcpPromptTokenUsage,
    },
    PromptStarted {
        turn_id: String,
        turn_seq: u64,
        timestamp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    PromptCompleted {
        turn_id: String,
        turn_seq: u64,
        timestamp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<Value>,
        usage: AcpPromptTokenUsage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
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
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptUsageSegment {
    pub turn_id: String,
    pub turn_seq: u64,
    pub elapsed_ms: Option<u64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: AcpPromptTokenUsage,
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

const USAGE_TRANSACTION_ATTEMPT_SEPARATOR: &str = ":attempt-";

/// Journal identity of one real `session/prompt` RPC. Retries of one logical
/// prompt share `prompt_event_id` but are accounted separately.
pub fn prompt_usage_transaction_id(
    prompt_event_id: &str,
    retry_attempt: u32,
    operation_seq: u64,
) -> String {
    format!("{prompt_event_id}{USAGE_TRANSACTION_ATTEMPT_SEPARATOR}{retry_attempt}:{operation_seq}")
}

/// Older journals keyed a prompt by its event id directly.
fn journal_has_transaction_for_event(journal_ids: &HashSet<String>, event_id: &str) -> bool {
    let prefix = format!("{event_id}{USAGE_TRANSACTION_ATTEMPT_SEPARATOR}");
    journal_ids
        .iter()
        .any(|id| id == event_id || id.starts_with(&prefix))
}

pub fn append_prompt_started(
    journal_path: &Utf8Path,
    turn_id: &str,
    turn_seq: u64,
    timestamp: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<()> {
    append_jsonl_durable(
        journal_path,
        &AcpPromptUsageJournalEntry::PromptStarted {
            turn_id: turn_id.to_string(),
            turn_seq,
            timestamp: timestamp.to_string(),
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
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
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<bool> {
    append_prompt_completed_inner(
        journal_path,
        turn_id,
        turn_seq,
        timestamp,
        request_id,
        usage,
        provider.map(str::to_string),
        model.map(str::to_string),
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
    let mut starts = prompt_starts_from_journal(journal_path)?;
    let known_start_ids = starts
        .iter()
        .map(|start| start.turn_id.clone())
        .collect::<HashSet<_>>();
    // The timeline only supplies prompts the journal never started (a crash
    // between the prompt event and its journal start). A journaled prompt is
    // keyed by usage transaction id, so match it by its canonical event id;
    // the logical promptId is a different identity space.
    for indexed in crate::acp::timeline::read_indexed_prompt_starts(timeline_path)? {
        if known_start_ids.contains(&indexed.prompt_id)
            || journal_has_transaction_for_event(&known_start_ids, &indexed.event_id)
        {
            continue;
        }
        starts.push(PromptTurnStart {
            turn_id: indexed.prompt_id,
            turn_seq: indexed.seq,
            timestamp: indexed.timestamp,
            provider: None,
            model: None,
        });
    }
    starts.sort_by_key(|start| start.turn_seq);

    let mut completed = prompt_completions_from_journal(journal_path)?;
    let mut recovered_turns = 0usize;
    let has_uncompleted_turn = starts
        .iter()
        .any(|start| !completed.contains_key(&start.turn_seq));
    if recover_missing_from_raw
        && has_uncompleted_turn
        && should_recover_missing_usage_from_raw(snapshot_path)
    {
        let transactions = raw_prompt_transactions(raw_path)?;
        let assignments = assign_transactions_to_turns(&starts, &transactions);
        for (start, transaction) in assignments {
            if completed.contains_key(&start.turn_seq) {
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
                start.provider.clone(),
                start.model.clone(),
                true,
            )? {
                recovered_turns = recovered_turns.saturating_add(1);
                completed.insert(start.turn_seq, (start.turn_id.clone(), usage.clone()));
            } else {
                // A competing writer may have completed this RPC. Only count
                // the journal's accepted record, including its runtime priority.
                completed = prompt_completions_from_journal(journal_path)?;
            }
        }
    }

    let mut ordered = completed.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(turn_seq, _)| *turn_seq);
    let baseline_has_usage = baseline.latest_prompt.has_any_value()
        || baseline.totals.input_tokens.is_some()
        || baseline.totals.output_tokens.is_some()
        || baseline.totals.cached_read_tokens.is_some()
        || baseline.totals.cached_write_tokens.is_some()
        || baseline.totals.total_tokens.is_some();
    let mut totals = baseline.totals;
    let mut latest_prompt = baseline.latest_prompt;
    for (_, (_, usage)) in &ordered {
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

/// Raw recovery is only needed for a currently active durable turn. Once a
/// session is terminal, an old missing journal completion must not make every
/// follow-up scan the entire raw frame log. Metadata-less legacy fixtures keep
/// the historical recovery behavior.
fn should_recover_missing_usage_from_raw(snapshot_path: &Utf8Path) -> bool {
    let attempt_dir = snapshot_path.parent().unwrap_or(snapshot_path);
    let mut found_metadata = false;
    for path in [
        snapshot_path.to_path_buf(),
        attempt_dir.join("acp.session.json"),
    ] {
        let Ok(value) = crate::storage::read_json::<Value>(&path) else {
            continue;
        };
        found_metadata = true;
        let activity = value
            .get("liveTurnActivity")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            activity,
            "starting" | "accepted" | "running" | "cancelRequested" | "cancelling"
        ) {
            return true;
        }
        if value
            .pointer("/promptRetry/status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "processing" | "running"))
        {
            return true;
        }
        if value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "processing" | "running" | "cancelling"))
        {
            return true;
        }
    }
    !found_metadata
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
    provider: Option<String>,
    model: Option<String>,
    recovered_from_raw: bool,
) -> Result<bool> {
    with_jsonl_file_lock(journal_path, || {
        let completed = prompt_completions_from_journal_unlocked(journal_path)?;
        // One RPC is completed once, whichever turn identity asks to record it.
        if completed.contains_key(&turn_seq)
            || completed
                .values()
                .any(|(completed_id, _)| completed_id == turn_id)
        {
            return Ok(false);
        }
        let entry = AcpPromptUsageJournalEntry::PromptCompleted {
            turn_id: turn_id.to_string(),
            turn_seq,
            timestamp: timestamp.to_string(),
            request_id,
            usage: usage.clone(),
            provider,
            model,
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
            provider,
            model,
        } = entry
        {
            starts.entry(turn_id.clone()).or_insert(PromptTurnStart {
                turn_id,
                turn_seq,
                timestamp,
                provider,
                model,
            });
        }
    }
    Ok(starts.into_values().collect())
}

pub fn read_prompt_usage_segments(attempt_dir: &Utf8Path) -> Vec<AcpPromptUsageSegment> {
    let path = attempt_dir.join("acp.prompt-usage.jsonl");
    let Ok(entries) = read_journal(&path) else {
        return Vec::new();
    };
    let model_metadata =
        crate::acp::events::read_attempt_session_metadata(&attempt_dir.join("acp.snapshot.json"))
            .or_else(|| {
                crate::acp::events::read_attempt_session_metadata(
                    &attempt_dir.join("acp.session.json"),
                )
            });
    let mut starts = std::collections::HashMap::new();
    let mut by_seq = HashMap::new();
    for entry in entries {
        match entry {
            AcpPromptUsageJournalEntry::PromptStarted {
                turn_id, timestamp, ..
            } => {
                starts.insert(turn_id, timestamp);
            }
            AcpPromptUsageJournalEntry::PromptCompleted {
                turn_id,
                turn_seq,
                provider,
                model,
                usage,
                timestamp,
                recovered_from_raw,
                ..
            } => {
                let elapsed_ms = starts
                    .get(&turn_id)
                    .and_then(|started_at| elapsed_ms_between(started_at, &timestamp));
                let segment = AcpPromptUsageSegment {
                    turn_id,
                    turn_seq,
                    elapsed_ms,
                    provider,
                    model: model_metadata
                        .as_ref()
                        .and_then(|metadata| {
                            model.as_deref().and_then(|model| {
                                crate::acp::events::metrics_model_display_name(metadata, model)
                            })
                        })
                        .or(model),
                    usage,
                };
                keep_one_completion_per_rpc(&mut by_seq, turn_seq, recovered_from_raw, segment);
            }
            _ => {}
        }
    }
    let mut segments = by_seq
        .into_values()
        .map(|(_, segment)| segment)
        .collect::<Vec<_>>();
    segments.sort_by_key(|segment| segment.turn_seq);
    segments
}

fn prompt_completions_from_journal(
    path: &Utf8Path,
) -> Result<HashMap<u64, (String, AcpPromptTokenUsage)>> {
    with_jsonl_file_lock(path, || prompt_completions_from_journal_unlocked(path))
}

fn prompt_completions_from_journal_unlocked(
    path: &Utf8Path,
) -> Result<HashMap<u64, (String, AcpPromptTokenUsage)>> {
    let mut by_seq = HashMap::<u64, (bool, (String, AcpPromptTokenUsage))>::new();
    for entry in read_journal_unlocked(path)? {
        if let AcpPromptUsageJournalEntry::PromptCompleted {
            turn_id,
            turn_seq,
            usage,
            recovered_from_raw,
            ..
        } = entry
        {
            keep_one_completion_per_rpc(
                &mut by_seq,
                turn_seq,
                recovered_from_raw,
                (turn_id, usage),
            );
        }
    }
    Ok(by_seq
        .into_iter()
        .map(|(turn_seq, (_, completion))| (turn_seq, completion))
        .collect())
}

/// `turn_seq` is the operation seq of one `session/prompt` RPC and is unique
/// within an attempt (request ids restart with each connection). Journals
/// written before the identity fix may hold a raw-recovered duplicate of a
/// journaled RPC under a logical-prompt alias; the runtime record wins.
fn keep_one_completion_per_rpc<T>(
    by_seq: &mut HashMap<u64, (bool, T)>,
    turn_seq: u64,
    recovered_from_raw: bool,
    value: T,
) {
    match by_seq.get(&turn_seq) {
        Some((existing_recovered, _)) if !*existing_recovered || recovered_from_raw => {}
        _ => {
            by_seq.insert(turn_seq, (recovered_from_raw, value));
        }
    }
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

fn timestamp_millis(timestamp: &str) -> Option<u64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        return u64::try_from(dt.timestamp_millis()).ok();
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(timestamp, format) {
            return u64::try_from(dt.and_utc().timestamp_millis()).ok();
        }
    }
    timestamp
        .trim_end_matches('Z')
        .parse::<u64>()
        .ok()
        .and_then(|seconds| seconds.checked_mul(1000))
}

fn elapsed_ms_between(started_at: &str, completed_at: &str) -> Option<u64> {
    timestamp_millis(completed_at)?.checked_sub(timestamp_millis(started_at)?)
}

fn rpc_id_key(request_id: &Value) -> String {
    serde_json::to_string(request_id).unwrap_or_else(|_| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::acp::events::{AcpAttemptPaths, AcpUiEvent, write_timeline_items};
    use crate::storage::append_jsonl;

    /// codebuddy 真实形状：`_meta.usage` 为 Anthropic 口径（`input_tokens` 不含缓存），
    /// id 在 `_meta["codebuddy.ai/messageId"]`；顶层 `used`/`size`/`cost` 是上下文占用
    /// 与额度，不是消费量。
    #[test]
    fn stream_usage_reads_codebuddy_meta_usage() {
        let frame = json!({
            "sessionUpdate": "usage_update",
            "used": 12_345,
            "size": 200_000,
            "cost": {"amount": 3, "currency": ""},
            "_meta": {
                "codebuddy.ai/requestId": "req-turn",
                "codebuddy.ai/messageId": "msg-1",
                "codebuddy.ai/usageByCategory": {"main": {"input_tokens": 9}},
                "usage": {
                    "input_tokens": 1_000,
                    "output_tokens": 200,
                    "cache_read_input_tokens": 5_000,
                    "cache_creation_input_tokens": 300,
                },
            },
        });
        let samples = stream_usage_samples(&frame);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].id.as_deref(), Some("msg-1"));
        let usage = &samples[0].usage;
        // Anthropic 口径：input 已排除缓存，不再做减法。
        assert_eq!(usage.input_tokens, Some(1_000));
        assert_eq!(usage.output_tokens, Some(200));
        assert_eq!(usage.cached_read_tokens, Some(5_000));
        assert_eq!(usage.cached_write_tokens, Some(300));
        assert_eq!(usage.total_tokens, None);
    }

    /// OpenAI 口径：`prompt_tokens` 含缓存命中，必须扣减后才与缓存字段相加。
    #[test]
    fn stream_usage_subtracts_cache_from_inclusive_prompt_tokens() {
        let frame = json!({
            "sessionUpdate": "usage_update",
            "usage": {"prompt_tokens": 1_200, "completion_tokens": 300, "total_tokens": 1_500},
            "_meta": {"provider.messageId": "resp-1"},
        });
        let samples = stream_usage_samples(&frame);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].id.as_deref(), Some("resp-1"));
        // 无缓存明细 → 不做扣减。
        assert_eq!(samples[0].usage.input_tokens, Some(1_200));
        assert_eq!(samples[0].usage.output_tokens, Some(300));
        assert_eq!(samples[0].usage.total_tokens, Some(1_500));

        let with_details = json!({
            "sessionUpdate": "usage_update",
            "usage": {
                "prompt_tokens": 1_200,
                "completion_tokens": 300,
                "prompt_tokens_details": {"cached_tokens": 900},
            },
            "_meta": {"provider.messageId": "resp-2"},
        });
        let samples = stream_usage_samples(&with_details);
        assert_eq!(samples[0].usage.input_tokens, Some(300));
        assert_eq!(samples[0].usage.cached_read_tokens, Some(900));
    }

    /// `total_tokens == input + output`（缓存已计入 input）也能判定包含口径。
    #[test]
    fn stream_usage_infers_inclusive_input_from_total() {
        let frame = json!({
            "sessionUpdate": "usage_update",
            "usage": {
                "input_tokens": 1_000,
                "output_tokens": 100,
                "cache_read_input_tokens": 400,
                "total_tokens": 1_100,
            },
            "_meta": {"codebuddy.ai/messageId": "msg-total"},
        });
        let samples = stream_usage_samples(&frame);
        assert_eq!(samples[0].usage.input_tokens, Some(600));
        assert_eq!(samples[0].usage.cached_read_tokens, Some(400));
    }

    /// 上下文占用、额度与估算拆分都不产生样本；无稳定 id 的样本标记为不可信。
    #[test]
    fn stream_usage_ignores_gauges_costs_and_needs_sample_id() {
        let gauge_only = json!({
            "sessionUpdate": "usage_update",
            "used": 42,
            "size": 100,
            "cost": {"amount": 7, "currency": ""},
            "_meta": {"codebuddy.ai/usageByCategory": {"main": 12}},
        });
        assert!(stream_usage_samples(&gauge_only).is_empty());

        let no_id = json!({
            "sessionUpdate": "usage_update",
            "usage": {"input_tokens": 10, "output_tokens": 2},
        });
        let samples = stream_usage_samples(&no_id);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].id, None);
    }

    fn prompt_event(seq: u64, timestamp: &str) -> AcpUiEvent {
        prompt_event_with_id(seq, timestamp, None)
    }

    fn prompt_event_with_id(seq: u64, timestamp: &str, prompt_id: Option<&str>) -> AcpUiEvent {
        let mut raw = json!({ "source": "goldBandPrompt" });
        if let Some(prompt_id) = prompt_id {
            raw["promptId"] = json!(prompt_id);
        }
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
            raw: Some(raw),
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
    fn elapsed_ms_between_parses_iso_and_numeric_timestamps() {
        assert_eq!(
            elapsed_ms_between("2026-08-06T20:14:41.000Z", "2026-08-06T20:14:51.500Z"),
            Some(10_500)
        );
        assert_eq!(
            elapsed_ms_between(
                "2026-08-06T20:14:41.000+08:00",
                "2026-08-06T20:14:51.500+08:00"
            ),
            Some(10_500)
        );
        assert_eq!(elapsed_ms_between("100Z", "110Z"), Some(10_000));
        assert_eq!(elapsed_ms_between("110Z", "100Z"), None);
    }

    #[test]
    fn read_prompt_usage_segments_resolves_metrics_model_name() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        std::fs::write(
            attempt_dir.join("acp.snapshot.json").as_std_path(),
            r#"{
                "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
                "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
                "modelOverride":"opus",
                "configOptions":[
                    {
                        "id":"model",
                        "currentValue":"opus",
                        "options":[
                            {"value":"opus","name":"glm-5.2"}
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();
        let journal = attempt_dir.join("acp.prompt-usage.jsonl");
        append_prompt_started(
            &journal,
            "turn-1",
            1,
            "2026-08-06T20:14:41.000Z",
            Some("claude-acp"),
            Some("opus"),
        )
        .unwrap();
        append_prompt_completed(
            &journal,
            "turn-1",
            1,
            "2026-08-06T20:14:51.000Z",
            None,
            &AcpPromptTokenUsage {
                input_tokens: Some(10),
                total_tokens: Some(10),
                ..Default::default()
            },
            Some("claude-acp"),
            Some("opus"),
        )
        .unwrap();

        let segments = read_prompt_usage_segments(&attempt_dir);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].provider.as_deref(), Some("claude-acp"));
        assert_eq!(segments[0].model.as_deref(), Some("glm-5.2"));
    }

    #[test]
    fn repairs_completed_prompt_from_raw_when_snapshot_checkpoint_was_skipped_by_crash() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        let prompts = vec![prompt_event(1, "100Z"), prompt_event(15, "200Z")];
        write_timeline_items(&paths.timeline, &prompts).unwrap();
        append_prompt_started(
            &paths.prompt_usage,
            &prompts[0].id,
            1,
            "100Z",
            Some("provider-a"),
            Some("model-a"),
        )
        .unwrap();
        append_prompt_started(
            &paths.prompt_usage,
            &prompts[1].id,
            15,
            "200Z",
            Some("provider-b"),
            Some("model-b"),
        )
        .unwrap();
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
            Some("provider-a"),
            Some("model-a"),
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
        let segments = read_prompt_usage_segments(&attempt_dir);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].provider.as_deref(), Some("provider-a"));
        assert_eq!(segments[0].model.as_deref(), Some("model-a"));
        assert_eq!(segments[1].provider.as_deref(), Some("provider-b"));
        assert_eq!(segments[1].model.as_deref(), Some("model-b"));
        assert_eq!(segments[0].elapsed_ms, Some(10_000));
        assert_eq!(segments[1].elapsed_ms, Some(10_000));
        assert_eq!(segments[1].usage.total_tokens, Some(120));

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
    fn indexed_prompt_repair_uses_logical_prompt_id_instead_of_timeline_item_id() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        let prompt = prompt_event_with_id(1, "100Z", Some("logical-turn-1"));
        write_timeline_items(&paths.timeline, &[prompt]).unwrap();
        append_jsonl(
            &paths.raw,
            &raw_frame(
                "100Z",
                "outbound",
                json!({"jsonrpc":"2.0","id":1,"method":"session/prompt","params":{}}),
            ),
        )
        .unwrap();
        append_jsonl(
            &paths.raw,
            &raw_frame(
                "110Z",
                "inbound",
                json!({
                    "jsonrpc":"2.0",
                    "id":1,
                    "result":{"usage":{"inputTokens":1,"outputTokens":1,"totalTokens":2}}
                }),
            ),
        )
        .unwrap();

        repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();

        let segments = read_prompt_usage_segments(&attempt_dir);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].turn_id, "logical-turn-1");
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

    #[test]
    fn terminal_session_does_not_scan_raw_log_for_old_missing_completion() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir);
        write_timeline_items(&paths.timeline, &[prompt_event(1, "100Z")]).unwrap();
        crate::storage::write_json(
            &paths.snapshot,
            &json!({
                "sessionId": "session-1",
                "liveTurnActivity": "idle",
                "latestTurnStatus": "completed"
            }),
        )
        .unwrap();
        // Invalid UTF-8 makes an attempted raw-log scan fail. A terminal
        // session must instead leave historical missing usage for explicit
        // repair and complete startup without touching the raw log.
        std::fs::write(paths.raw.as_std_path(), [0xff]).unwrap();

        let recovered = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            true,
        )
        .unwrap();

        assert_eq!(recovered.recovered_turns, 0);
        assert_eq!(recovered.completed_turns, 0);
    }
    /// task-342 shape: the journal keys each RPC by usage transaction id while
    /// the timeline index names the same prompt by its logical promptId.
    fn write_transaction_keyed_prompt(paths: &AcpAttemptPaths) -> AcpUiEvent {
        let prompt = prompt_event_with_id(3, "100Z", Some("initial-direct-turn"));
        write_timeline_items(&paths.timeline, std::slice::from_ref(&prompt)).unwrap();
        let transaction_id = format!("{}:attempt-0:3", prompt.id);
        append_prompt_started(&paths.prompt_usage, &transaction_id, 3, "100Z", None, None).unwrap();
        append_prompt_completed(
            &paths.prompt_usage,
            &transaction_id,
            3,
            "110Z",
            Some(json!(41)),
            &AcpPromptTokenUsage {
                input_tokens: Some(20),
                output_tokens: Some(12_248),
                total_tokens: Some(800_237),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
        for frame in [
            raw_frame(
                "100Z",
                "outbound",
                json!({"jsonrpc":"2.0","id":41,"method":"session/prompt","params":{}}),
            ),
            raw_frame(
                "110Z",
                "inbound",
                json!({"jsonrpc":"2.0","id":41,"result":{"usage":{"inputTokens":20,"outputTokens":12248,"totalTokens":800237}}}),
            ),
        ] {
            append_jsonl(&paths.raw, &frame).unwrap();
        }
        prompt
    }

    #[test]
    fn repair_does_not_recount_a_journaled_transaction_under_its_logical_prompt_id() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        write_transaction_keyed_prompt(&paths);
        // A later prompt on a fresh runtime is active, which enables raw recovery.
        append_prompt_started(
            &paths.prompt_usage,
            "gold-band-user-prompt-9:attempt-0:9",
            9,
            "500Z",
            None,
            None,
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

        assert_eq!(recovered.recovered_turns, 0);
        assert_eq!(recovered.totals.total_tokens, Some(800_237));
        assert_eq!(read_prompt_usage_segments(&attempt_dir).len(), 1);
    }

    #[test]
    fn repair_does_not_recount_an_rpc_completed_only_under_a_legacy_alias() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        write_transaction_keyed_prompt(&paths);
        // An old runtime can have recovered the response under the logical id
        // after the original runtime exited without recording its completion.
        let mut entries = read_journal(&paths.prompt_usage).unwrap();
        for entry in &mut entries {
            if let AcpPromptUsageJournalEntry::PromptCompleted {
                turn_id,
                recovered_from_raw,
                ..
            } = entry
            {
                *turn_id = "initial-direct-turn".into();
                *recovered_from_raw = true;
            }
        }
        std::fs::write(&paths.prompt_usage, b"").unwrap();
        for entry in entries {
            append_jsonl(&paths.prompt_usage, &entry).unwrap();
        }
        append_prompt_started(
            &paths.prompt_usage,
            "gold-band-user-prompt-9:attempt-0:9",
            9,
            "500Z",
            None,
            None,
        )
        .unwrap();

        for recover_missing_from_raw in [false, true, true] {
            let recovered = repair_attempt_usage(
                &paths.snapshot,
                &paths.timeline,
                &paths.raw,
                &paths.prompt_usage,
                recover_missing_from_raw,
            )
            .unwrap();
            assert_eq!(recovered.recovered_turns, 0);
            assert_eq!(recovered.completed_turns, 1);
            assert_eq!(recovered.totals.total_tokens, Some(800_237));
            assert_eq!(read_prompt_usage_segments(&attempt_dir).len(), 1);
        }
    }

    #[test]
    fn journal_completions_of_one_rpc_count_once_even_under_an_alias() {
        let temp = tempfile::tempdir().unwrap();
        let attempt_dir = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        write_transaction_keyed_prompt(&paths);
        // Journals written before the fix already hold the alias duplicate.
        append_jsonl(
            &paths.prompt_usage,
            &AcpPromptUsageJournalEntry::PromptCompleted {
                turn_id: "initial-direct-turn".into(),
                turn_seq: 3,
                timestamp: "110Z".into(),
                request_id: Some(json!(41)),
                usage: AcpPromptTokenUsage {
                    input_tokens: Some(20),
                    output_tokens: Some(12_248),
                    total_tokens: Some(800_237),
                    ..Default::default()
                },
                provider: None,
                model: None,
                recovered_from_raw: true,
            },
        )
        .unwrap();

        let recovered = repair_attempt_usage(
            &paths.snapshot,
            &paths.timeline,
            &paths.raw,
            &paths.prompt_usage,
            false,
        )
        .unwrap();

        assert_eq!(recovered.totals.total_tokens, Some(800_237));
        assert_eq!(recovered.totals.output_tokens, Some(12_248));
        let segments = read_prompt_usage_segments(&attempt_dir);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].turn_id, "gold-band-user-prompt-3:attempt-0:3");
    }
}
