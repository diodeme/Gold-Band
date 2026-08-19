use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path};

use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, SecondsFormat, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use walkdir::WalkDir;

pub mod index;

pub const PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION: &str = "2.1.0";
pub const PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS: usize = 120;
pub const PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS: usize = 1_200;
pub const PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS: usize = 72_000;
const PROGRESS_PUBLISH_FILE_INTERVAL: usize = 32;

const CANONICAL_JSON_NAMES: &[&str] = &[
    "project.json",
    "task.json",
    "conversation.json",
    "run.json",
    "round.json",
    "node.json",
    "turn.json",
    "acp.snapshot.json",
    "dynamic-run.json",
    "graph.json",
    "observability.snapshot.json",
];

const CANONICAL_JSONL_NAMES: &[&str] = &["acp.prompt-usage.jsonl", "acp.timeline.jsonl"];
const SEMANTIC_TEXT_NAMES: &[&str] = &["requirement.md"];
const EXCLUDED_FILE_NAMES: &[&str] = &[
    "acp.raw.jsonl",
    "acp.diagnostics.jsonl",
    "provider.pid",
    "gold-band.db",
    "gold-band.db-wal",
    "gold-band.db-shm",
];
const EXCLUDED_DIRECTORY_NAMES: &[&str] = &["doctor", "logs", "diagnostics"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PersonalAnalyticsOperationStatus {
    Queued,
    Scanning,
    Analyzing,
    ValidatingReport,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl PersonalAnalyticsOperationStatus {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Scanning
                | Self::Analyzing
                | Self::ValidatingReport
                | Self::Cancelling
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsError {
    pub code: String,
    pub params: Value,
}

impl PersonalAnalyticsError {
    pub fn new(code: impl Into<String>, params: Value) -> Self {
        Self {
            code: code.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsProgress {
    pub stage: PersonalAnalyticsOperationStatus,
    pub processed_units: u64,
    pub total_units: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsSnapshot {
    pub operation: Option<PersonalAnalyticsOperation>,
    #[serde(default)]
    pub insight_operation: Option<PersonalAnalyticsOperation>,
    pub latest_report: Option<PersonalAnalyticsReport>,
}

impl Default for PersonalAnalyticsSnapshot {
    fn default() -> Self {
        Self {
            operation: None,
            insight_operation: None,
            latest_report: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsOperation {
    pub operation_id: String,
    pub agent_type: String,
    pub status: PersonalAnalyticsOperationStatus,
    pub revision: u64,
    pub progress: PersonalAnalyticsProgress,
    pub source_watermark: String,
    pub report_id: Option<String>,
    pub error: Option<PersonalAnalyticsError>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsCoverage {
    pub discovered_files: u64,
    pub eligible_files: u64,
    pub parsed_files: u64,
    pub skipped_files: u64,
    pub corrupt_files: u64,
    pub unknown_version_files: u64,
    pub discovered_bytes: u64,
    pub semantic_eligible_items: u64,
    pub semantic_sampled_items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsOverview {
    pub project_count: u64,
    pub task_count: u64,
    pub conversation_count: u64,
    pub run_count: u64,
    pub turn_count: u64,
    pub attempt_count: u64,
    pub earliest_at: Option<String>,
    pub latest_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsRateMetric {
    pub metric_id: String,
    pub numerator: u64,
    pub denominator: u64,
    pub unknown_count: u64,
    pub rate: Option<f64>,
    pub evidence_locators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsReliability {
    pub direct_reply_completion_rate: AnalyticsRateMetric,
    pub workflow_run_terminal_success_rate: AnalyticsRateMetric,
    pub auto_outer_run_terminal_success_rate: AnalyticsRateMetric,
    pub failed_count: u64,
    pub cancelled_count: u64,
    pub non_terminal_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsQuality {
    pub retry_reentry_rate: AnalyticsRateMetric,
    pub recovered_after_retry_count: u64,
    pub terminal_signals: Vec<AnalyticsNamedCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsTaskSummary {
    pub task_locator: String,
    pub title: String,
    pub mode: String,
    pub status: String,
    pub outcome: Option<String>,
    pub agent_names: Vec<String>,
    pub total_tokens: u64,
    pub active_duration_seconds: u64,
    pub active_duration_zero_filled: bool,
    pub terminal_node: Option<String>,
    pub last_activity_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsNodeAggregate {
    pub node_id: String,
    pub call_count: u64,
    pub retry_count: u64,
    pub total_active_duration_seconds: u64,
    pub average_active_duration_seconds: f64,
    pub active_duration_share: Option<f64>,
    pub active_duration_zero_filled_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsEfficiency {
    pub observed_terminal_run_active_seconds: u64,
    pub average_terminal_run_active_seconds: Option<f64>,
    pub terminal_run_sample_count: u64,
    pub active_duration_zero_filled_count: u64,
    pub pause_count: u64,
    pub resume_count: u64,
    pub manual_continue_count: u64,
    pub top_duration_tasks: Vec<AnalyticsTaskSummary>,
    pub node_aggregates: Vec<AnalyticsNodeAggregate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub observed_prompt_count: u64,
    pub top_token_tasks: Vec<AnalyticsTaskSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsNamedCount {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalyticsContextAndTools {
    pub tool_call_count: u64,
    pub permission_request_count: u64,
    pub elicitation_request_count: u64,
    pub top_tools: Vec<AnalyticsNamedCount>,
    pub top_agents: Vec<AnalyticsNamedCount>,
    pub verified_skill_call_count: u64,
    pub top_skills: Vec<AnalyticsNamedCount>,
    pub event_kinds: Vec<AnalyticsNamedCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsWarning {
    pub code: String,
    pub params: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AnalyticsInsightConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsInsight {
    pub section: AnalyticsInsightSection,
    pub title: String,
    pub summary: String,
    pub recommendation: String,
    pub confidence: AnalyticsInsightConfidence,
    pub sample_count: u64,
    pub evidence_locators: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AnalyticsInsightSection {
    Quality,
    Efficiency,
    TokenUsage,
    ContextAndSkills,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsNarrative {
    pub schema_version: String,
    pub insights: Vec<PersonalAnalyticsInsight>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsReport {
    pub schema_version: String,
    pub report_id: String,
    pub generated_at: String,
    pub source_watermark: String,
    #[serde(default)]
    pub index_revision: u64,
    #[serde(default)]
    pub range: index::PersonalAnalyticsDateRange,
    pub source_coverage: AnalyticsCoverage,
    pub overview: AnalyticsOverview,
    pub recent_tasks: Vec<AnalyticsTaskSummary>,
    pub reliability: AnalyticsReliability,
    pub quality: AnalyticsQuality,
    pub efficiency: AnalyticsEfficiency,
    pub token_usage: AnalyticsTokenUsage,
    pub context_and_tools: AnalyticsContextAndTools,
    pub insights: Vec<PersonalAnalyticsInsight>,
    pub warnings: Vec<PersonalAnalyticsWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsProjection {
    pub schema_version: String,
    pub source_watermark: String,
    pub source_coverage: AnalyticsCoverage,
    pub overview: AnalyticsOverview,
    pub recent_tasks: Vec<AnalyticsTaskSummary>,
    pub reliability: AnalyticsReliability,
    pub quality: AnalyticsQuality,
    pub efficiency: AnalyticsEfficiency,
    pub token_usage: AnalyticsTokenUsage,
    pub context_and_tools: AnalyticsContextAndTools,
    pub warnings: Vec<PersonalAnalyticsWarning>,
    pub evidence_locators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsSemanticItem {
    pub locator: String,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalAnalyticsSemanticBatch {
    pub schema_version: String,
    pub items: Vec<PersonalAnalyticsSemanticItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersonalAnalyticsProjectionOutput {
    pub projection: PersonalAnalyticsProjection,
    pub semantic_batch: PersonalAnalyticsSemanticBatch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TokenCounters {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    total: u64,
}

#[derive(Debug)]
struct RunFact {
    task_key: String,
    run_key: String,
    status: String,
    outcome: Option<String>,
    updated_epoch: Option<i64>,
    terminal_node: Option<String>,
    pause_reason: Option<String>,
    locator: String,
}

#[derive(Debug, Default)]
struct TaskFact {
    title: String,
    mode: String,
    agent_names: BTreeSet<String>,
    token_usage: TokenCounters,
    last_activity_epoch: Option<i64>,
    last_activity_at: Option<String>,
}

#[derive(Debug)]
struct NodeFact {
    task_key: String,
    run_key: String,
    attempt_key: String,
    group_key: String,
    node_id: String,
    outcome: Option<String>,
}

#[derive(Default)]
struct ProjectionAccumulator {
    coverage: AnalyticsCoverage,
    project_count: u64,
    task_count: u64,
    conversation_count: u64,
    attempt_count: u64,
    tasks: BTreeMap<String, TaskFact>,
    runs: Vec<RunFact>,
    nodes: Vec<NodeFact>,
    session_duration_seconds_by_attempt: BTreeMap<String, u64>,
    direct_started: u64,
    direct_completed: u64,
    direct_failed: u64,
    direct_cancelled: u64,
    direct_unknown: u64,
    token_usage: AnalyticsTokenUsage,
    pause_count: u64,
    resume_count: u64,
    manual_continue_count: u64,
    tool_counts: BTreeMap<String, u64>,
    agent_counts: BTreeMap<String, u64>,
    skill_counts: BTreeMap<String, u64>,
    event_counts: BTreeMap<String, u64>,
    permission_request_count: u64,
    elicitation_request_count: u64,
    earliest_epoch: Option<i64>,
    latest_epoch: Option<i64>,
    evidence: BTreeSet<String>,
    semantic_items: Vec<PersonalAnalyticsSemanticItem>,
    semantic_chars: usize,
    warnings: Vec<PersonalAnalyticsWarning>,
}

impl Default for AnalyticsCoverage {
    fn default() -> Self {
        Self {
            discovered_files: 0,
            eligible_files: 0,
            parsed_files: 0,
            skipped_files: 0,
            corrupt_files: 0,
            unknown_version_files: 0,
            discovered_bytes: 0,
            semantic_eligible_items: 0,
            semantic_sampled_items: 0,
        }
    }
}

impl Default for AnalyticsTokenUsage {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            total_tokens: 0,
            observed_prompt_count: 0,
            top_token_tasks: Vec::new(),
        }
    }
}

pub fn personal_analytics_narrative_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(PersonalAnalyticsNarrative))
        .expect("personal analytics narrative schema should serialize")
}

pub fn canonicalize_personal_analytics_report(
    projection: &PersonalAnalyticsProjection,
    narrative: PersonalAnalyticsNarrative,
    report_id: String,
    generated_at: String,
) -> PersonalAnalyticsReport {
    let insights = narrative
        .insights
        .into_iter()
        .filter(|insight| insight.sample_count > 0 && !insight.evidence_locators.is_empty())
        .filter(|insight| {
            insight
                .evidence_locators
                .iter()
                .all(|locator| safe_locator(locator))
        })
        .take(12)
        .collect();
    PersonalAnalyticsReport {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        report_id,
        generated_at,
        source_watermark: projection.source_watermark.clone(),
        index_revision: 0,
        range: index::PersonalAnalyticsDateRange::default(),
        source_coverage: projection.source_coverage.clone(),
        overview: projection.overview.clone(),
        recent_tasks: projection.recent_tasks.clone(),
        reliability: projection.reliability.clone(),
        quality: projection.quality.clone(),
        efficiency: projection.efficiency.clone(),
        token_usage: projection.token_usage.clone(),
        context_and_tools: projection.context_and_tools.clone(),
        insights,
        warnings: projection.warnings.iter().take(24).cloned().collect(),
    }
}

pub fn build_personal_analytics_projection<F, C>(
    projects_root: &Utf8Path,
    source_watermark: String,
    mut progress: F,
    cancelled: C,
) -> Result<PersonalAnalyticsProjectionOutput>
where
    F: FnMut(u64, u64),
    C: Fn() -> bool,
{
    let canonical_root = projects_root
        .canonicalize_utf8()
        .with_context(|| format!("canonicalize analytics source root {projects_root}"))?;
    let mut files = Vec::new();
    for entry in WalkDir::new(projects_root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.file_type().is_file() || entry.file_type().is_symlink() {
            files.push(
                Utf8PathBuf::from_path_buf(entry.path().to_path_buf()).map_err(|path| {
                    anyhow!(
                        "analytics source path is not valid UTF-8: {}",
                        path.display()
                    )
                })?,
            );
        }
    }
    let total = files.len() as u64;
    let mut accumulator = ProjectionAccumulator::default();
    accumulator.coverage.discovered_files = total;

    for (index, path) in files.iter().enumerate() {
        if cancelled() {
            bail!("analytics.cancelled");
        }
        if index % PROGRESS_PUBLISH_FILE_INTERVAL == 0 || index + 1 == files.len() {
            progress(index as u64, total);
        }
        accumulator.coverage.discovered_bytes = accumulator
            .coverage
            .discovered_bytes
            .saturating_add(path.metadata().map(|metadata| metadata.len()).unwrap_or(0));
        let relative = match safe_relative_path(&canonical_root, path) {
            Some(relative) => relative,
            None => {
                accumulator.coverage.skipped_files += 1;
                continue;
            }
        };
        let file_name = path.file_name().unwrap_or_default();
        if is_excluded(&relative, file_name) {
            accumulator.coverage.skipped_files += 1;
            continue;
        }
        let result = if CANONICAL_JSON_NAMES.contains(&file_name) {
            accumulator.coverage.eligible_files += 1;
            process_json_file(path, &relative, file_name, &mut accumulator)
        } else if CANONICAL_JSONL_NAMES.contains(&file_name) {
            accumulator.coverage.eligible_files += 1;
            process_jsonl_file(path, &relative, file_name, &mut accumulator)
        } else if SEMANTIC_TEXT_NAMES.contains(&file_name) {
            accumulator.coverage.eligible_files += 1;
            process_semantic_file(path, &relative, &mut accumulator)
        } else {
            accumulator.coverage.skipped_files += 1;
            continue;
        };
        match result {
            Ok(()) => accumulator.coverage.parsed_files += 1,
            Err(_) => accumulator.coverage.corrupt_files += 1,
        }
    }
    progress(total, total);
    Ok(finalize_projection(accumulator, source_watermark))
}

fn process_json_file(
    path: &Utf8Path,
    relative: &str,
    file_name: &str,
    accumulator: &mut ProjectionAccumulator,
) -> Result<()> {
    let file = File::open(path)?;
    let value: Value = serde_json::from_reader(BufReader::new(file))?;
    observe_version(&value, accumulator);
    observe_timestamps(&value, accumulator);
    match file_name {
        "project.json" => accumulator.project_count += 1,
        "task.json" => {
            accumulator.task_count += 1;
            if let Some(task_key) = task_key(relative) {
                let task = accumulator.tasks.entry(task_key).or_default();
                task.title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| value.get("id").and_then(Value::as_str).unwrap_or("Task"))
                    .to_string();
            }
        }
        "conversation.json" => {
            accumulator.conversation_count += 1;
            if let Some(task_key) = task_key(relative) {
                let task = accumulator.tasks.entry(task_key).or_default();
                if let Some(mode) = value.get("runMode").and_then(Value::as_str) {
                    task.mode = mode.to_string();
                }
                if let Some(agent_name) = value
                    .pointer("/agentIdentity/displayName")
                    .and_then(Value::as_str)
                {
                    task.agent_names.insert(agent_name.to_string());
                    *accumulator
                        .agent_counts
                        .entry(agent_name.to_string())
                        .or_default() += 1;
                }
                if let Some(raw) = string_at(&value, &["/lastActivityAt", "/createdAt"])
                    && let Some(epoch) = timestamp_epoch(raw)
                    && task
                        .last_activity_epoch
                        .is_none_or(|current| epoch > current)
                {
                    task.last_activity_epoch = Some(epoch);
                    task.last_activity_at = normalized_timestamp(epoch);
                }
            }
        }
        "run.json" => {
            let updated = string_at(&value, &["/updated_at", "/updatedAt"]);
            accumulator.runs.push(RunFact {
                task_key: task_key(relative).unwrap_or_default(),
                run_key: run_key(relative).unwrap_or_default(),
                status: value
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                outcome: value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                updated_epoch: updated.and_then(timestamp_epoch),
                terminal_node: value
                    .get("last_executed_node")
                    .or_else(|| value.get("current_node"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                pause_reason: value
                    .get("pause_reason")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                locator: relative.to_string(),
            });
        }
        "node.json" => {
            accumulator.attempt_count += 1;
            let task_key = task_key(relative).unwrap_or_default();
            if let Some(provider) = value
                .pointer("/resolved_config/provider")
                .and_then(Value::as_str)
            {
                accumulator
                    .tasks
                    .entry(task_key.clone())
                    .or_default()
                    .agent_names
                    .insert(provider.to_string());
                *accumulator
                    .agent_counts
                    .entry(provider.to_string())
                    .or_default() += 1;
            }
            accumulator.nodes.push(NodeFact {
                task_key,
                run_key: run_key(relative).unwrap_or_default(),
                attempt_key: attempt_key(relative).unwrap_or_default(),
                group_key: node_group_key(relative).unwrap_or_else(|| relative.to_string()),
                node_id: value
                    .get("node_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                outcome: value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        "acp.snapshot.json" => {
            if let Some(attempt_key) = attempt_key(relative)
                && let Some(seconds) = u64_at_optional(
                    &value,
                    &[
                        "/timing/sessionElapsedSeconds",
                        "/timing/session_elapsed_seconds",
                    ],
                )
            {
                accumulator
                    .session_duration_seconds_by_attempt
                    .entry(attempt_key)
                    .and_modify(|current| *current = (*current).max(seconds))
                    .or_insert(seconds);
            }
        }
        "turn.json" => observe_turn(&value, relative, accumulator),
        "observability.snapshot.json" => observe_observability(&value, accumulator),
        _ => {}
    }
    accumulator.evidence.insert(relative.to_string());
    Ok(())
}

fn process_jsonl_file(
    path: &Utf8Path,
    relative: &str,
    file_name: &str,
    accumulator: &mut ProjectionAccumulator,
) -> Result<()> {
    let reader = BufReader::new(File::open(path)?);
    let mut valid_lines = 0u64;
    if file_name == "acp.prompt-usage.jsonl" {
        let mut turns = BTreeMap::<String, TokenCounters>::new();
        let mut legacy_totals = None;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)?;
            valid_lines += 1;
            if let Some(usage) = value.get("usage") {
                let key = value
                    .get("turn_id")
                    .or_else(|| value.get("turnId"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                turns.insert(key, token_counters(usage));
            } else if let Some(totals) = value.get("totals") {
                legacy_totals = Some(token_counters(totals));
            }
        }
        let task_key = task_key(relative);
        let usages = if !turns.is_empty() {
            turns.into_values().collect::<Vec<_>>()
        } else {
            legacy_totals.into_iter().collect::<Vec<_>>()
        };
        for usage in usages {
            add_usage(&mut accumulator.token_usage, usage);
            if let Some(task_key) = task_key.as_ref() {
                add_token_counters(
                    &mut accumulator
                        .tasks
                        .entry(task_key.clone())
                        .or_default()
                        .token_usage,
                    usage,
                );
            }
        }
    } else {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)?;
            valid_lines += 1;
            let item = value.get("item").unwrap_or(&value);
            let kind = item
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            *accumulator
                .event_counts
                .entry(kind.to_string())
                .or_default() += 1;
            match kind {
                "toolCall" => {
                    let name = normalized_tool_name(item);
                    *accumulator.tool_counts.entry(name).or_default() += 1;
                }
                "permissionRequest" => accumulator.permission_request_count += 1,
                "elicitationRequest" => accumulator.elicitation_request_count += 1,
                "skillInvocation" => {
                    let name = normalized_tool_name(item);
                    *accumulator.skill_counts.entry(name).or_default() += 1;
                }
                _ => {}
            }
        }
    }
    if valid_lines == 0 && path.metadata()?.len() > 0 {
        bail!("analytics JSONL contains no records")
    }
    accumulator.evidence.insert(relative.to_string());
    Ok(())
}

fn process_semantic_file(
    path: &Utf8Path,
    relative: &str,
    accumulator: &mut ProjectionAccumulator,
) -> Result<()> {
    accumulator.coverage.semantic_eligible_items += 1;
    if accumulator.semantic_items.len() >= PERSONAL_ANALYTICS_SEMANTIC_MAX_ITEMS
        || accumulator.semantic_chars >= PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS
    {
        return Ok(());
    }
    let mut content = String::new();
    File::open(path)?
        .take((PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS * 4) as u64)
        .read_to_string(&mut content)?;
    let content = content
        .chars()
        .take(PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS)
        .collect::<String>();
    if content.trim().is_empty() {
        return Ok(());
    }
    let remaining = PERSONAL_ANALYTICS_SEMANTIC_MAX_CHARS - accumulator.semantic_chars;
    let content = content.chars().take(remaining).collect::<String>();
    accumulator.semantic_chars += content.chars().count();
    accumulator
        .semantic_items
        .push(PersonalAnalyticsSemanticItem {
            locator: relative.to_string(),
            kind: "requirement".to_string(),
            content,
        });
    accumulator.coverage.semantic_sampled_items += 1;
    accumulator.evidence.insert(relative.to_string());
    Ok(())
}

fn finalize_projection(
    mut accumulator: ProjectionAccumulator,
    source_watermark: String,
) -> PersonalAnalyticsProjectionOutput {
    let mut workflow_started = 0;
    let mut workflow_success = 0;
    let mut auto_started = 0;
    let mut auto_success = 0;
    let mut workflow_unknown = 0;
    let mut auto_unknown = 0;
    let mut failed_count = accumulator.direct_failed;
    let mut cancelled_count = accumulator.direct_cancelled;
    let mut non_terminal_count = 0;
    let mut terminal_active_duration_total = 0u64;
    let mut terminal_active_duration_count = 0u64;
    let mut active_duration_zero_filled_count = 0u64;
    let mut terminal_signals = BTreeMap::<String, u64>::new();
    let mut run_active_durations = BTreeMap::<String, (u64, u64, u64)>::new();
    for node in &accumulator.nodes {
        let entry = run_active_durations
            .entry(node.run_key.clone())
            .or_default();
        entry.2 += 1;
        if let Some(duration) = accumulator
            .session_duration_seconds_by_attempt
            .get(&node.attempt_key)
        {
            entry.0 = entry.0.saturating_add(*duration);
        } else {
            entry.1 += 1;
        }
    }
    for run in &accumulator.runs {
        let mode = accumulator
            .tasks
            .get(&run.task_key)
            .map(|task| task.mode.as_str());
        match mode {
            Some("workflow") => {
                workflow_started += 1;
                if run.outcome.as_deref() == Some("success") {
                    workflow_success += 1;
                } else if run.outcome.is_none() {
                    workflow_unknown += 1;
                }
            }
            Some("auto") => {
                auto_started += 1;
                if run.outcome.as_deref() == Some("success") {
                    auto_success += 1;
                } else if run.outcome.is_none() {
                    auto_unknown += 1;
                }
            }
            _ => {}
        }
        if run.status == "completed" {
            terminal_active_duration_count += 1;
            let (duration, missing_count, node_count) = run_active_durations
                .get(&run.run_key)
                .copied()
                .unwrap_or_default();
            if node_count == 0 || missing_count > 0 {
                active_duration_zero_filled_count += 1;
            }
            terminal_active_duration_total =
                terminal_active_duration_total.saturating_add(duration);
            if matches!(run.outcome.as_deref(), Some("failure")) {
                failed_count += 1;
                *terminal_signals
                    .entry("outcome.failure".to_string())
                    .or_default() += 1;
            } else if matches!(run.outcome.as_deref(), Some("killed")) {
                cancelled_count += 1;
                *terminal_signals
                    .entry("outcome.killed".to_string())
                    .or_default() += 1;
            }
        } else {
            non_terminal_count += 1;
            *terminal_signals
                .entry(format!("status.{}", run.status))
                .or_default() += 1;
        }
        if let Some(reason) = run.pause_reason.as_deref() {
            *terminal_signals
                .entry(format!("pause.{reason}"))
                .or_default() += 1;
        }
    }
    if accumulator.direct_failed > 0 {
        terminal_signals.insert("direct.failed".to_string(), accumulator.direct_failed);
    }
    if accumulator.direct_cancelled > 0 {
        terminal_signals.insert("direct.cancelled".to_string(), accumulator.direct_cancelled);
    }
    non_terminal_count += accumulator.direct_unknown;

    let mut group_counts = BTreeMap::<String, (String, u64, bool)>::new();
    for node in &accumulator.nodes {
        let entry = group_counts
            .entry(node.group_key.clone())
            .or_insert_with(|| (node.node_id.clone(), 0, false));
        entry.1 += 1;
        entry.2 |= node.outcome.as_deref() == Some("success");
    }
    let retried_groups = group_counts
        .values()
        .filter(|(_, count, _)| *count > 1)
        .count() as u64;
    let recovered_after_retry_count = group_counts
        .values()
        .filter(|(_, count, recovered)| *count > 1 && *recovered)
        .count() as u64;
    let mut node_stats = BTreeMap::<String, (u64, u64, u64, u64)>::new();
    for node in &accumulator.nodes {
        let entry = node_stats.entry(node.node_id.clone()).or_default();
        entry.0 += 1;
        if let Some(duration) = accumulator
            .session_duration_seconds_by_attempt
            .get(&node.attempt_key)
        {
            entry.2 = entry.2.saturating_add(*duration);
        } else {
            entry.3 += 1;
        }
    }
    for (node_id, count, _) in group_counts.values() {
        if *count > 1 {
            node_stats.entry(node_id.clone()).or_default().1 += count - 1;
        }
    }
    let node_active_duration_total = node_stats
        .values()
        .map(|(_, _, total, _)| *total)
        .sum::<u64>();
    let mut node_aggregates = node_stats
        .into_iter()
        .map(
            |(node_id, (call_count, retry_count, total_active_duration_seconds, zero_count))| {
                AnalyticsNodeAggregate {
                    node_id,
                    call_count,
                    retry_count,
                    total_active_duration_seconds,
                    average_active_duration_seconds: if call_count == 0 {
                        0.0
                    } else {
                        total_active_duration_seconds as f64 / call_count as f64
                    },
                    active_duration_share: (node_active_duration_total > 0).then_some(
                        total_active_duration_seconds as f64 / node_active_duration_total as f64,
                    ),
                    active_duration_zero_filled_count: zero_count,
                }
            },
        )
        .collect::<Vec<_>>();
    node_aggregates.sort_by(|left, right| {
        right
            .total_active_duration_seconds
            .cmp(&left.total_active_duration_seconds)
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    node_aggregates.truncate(12);

    let mut task_summaries = accumulator
        .tasks
        .iter()
        .filter_map(|(task_key, task)| {
            task_summary(
                task_key,
                task,
                &accumulator.runs,
                &accumulator.nodes,
                &accumulator.session_duration_seconds_by_attempt,
            )
        })
        .collect::<Vec<_>>();
    let mut recent_tasks = task_summaries
        .iter()
        .filter(|task| matches!(task.mode.as_str(), "workflow" | "auto"))
        .cloned()
        .collect::<Vec<_>>();
    recent_tasks.sort_by(|left, right| {
        timestamp_epoch(right.last_activity_at.as_deref().unwrap_or_default())
            .cmp(&timestamp_epoch(
                left.last_activity_at.as_deref().unwrap_or_default(),
            ))
            .then_with(|| right.task_locator.cmp(&left.task_locator))
    });
    recent_tasks.truncate(10);
    task_summaries.sort_by(|left, right| {
        right
            .active_duration_seconds
            .cmp(&left.active_duration_seconds)
            .then_with(|| left.task_locator.cmp(&right.task_locator))
    });
    let top_duration_tasks = task_summaries.iter().take(10).cloned().collect::<Vec<_>>();
    task_summaries.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| left.task_locator.cmp(&right.task_locator))
    });
    let top_token_tasks = task_summaries.iter().take(10).cloned().collect::<Vec<_>>();
    let direct_evidence = accumulator
        .evidence
        .iter()
        .filter(|locator| locator.ends_with("turn.json"))
        .take(24)
        .cloned()
        .collect();
    let run_evidence = accumulator
        .runs
        .iter()
        .map(|run| run.locator.clone())
        .take(24)
        .collect::<Vec<_>>();
    if accumulator.direct_started < accumulator.conversation_count {
        accumulator.warnings.push(PersonalAnalyticsWarning {
            code: "analytics.direct-turn-coverage-limited".to_string(),
            params: json!({
                "observedTurns": accumulator.direct_started,
                "conversationCount": accumulator.conversation_count,
            }),
        });
    }
    if accumulator.coverage.corrupt_files > 0 {
        accumulator.warnings.push(PersonalAnalyticsWarning {
            code: "analytics.source-partially-corrupt".to_string(),
            params: json!({ "count": accumulator.coverage.corrupt_files }),
        });
    }
    if active_duration_zero_filled_count > 0 {
        accumulator.warnings.push(PersonalAnalyticsWarning {
            code: "analytics.active-duration-zero-filled".to_string(),
            params: json!({ "count": active_duration_zero_filled_count }),
        });
    }
    let top_tools = named_counts(&accumulator.tool_counts, 12);
    let top_agents = named_counts(&accumulator.agent_counts, 12);
    let top_skills = named_counts(&accumulator.skill_counts, 12);
    let event_kinds = named_counts(&accumulator.event_counts, 16);
    let mut token_usage = accumulator.token_usage;
    token_usage.top_token_tasks = top_token_tasks;
    let projection = PersonalAnalyticsProjection {
        schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
        source_watermark: source_watermark.clone(),
        source_coverage: accumulator.coverage,
        overview: AnalyticsOverview {
            project_count: accumulator.project_count,
            task_count: accumulator.task_count,
            conversation_count: accumulator.conversation_count,
            run_count: accumulator.runs.len() as u64,
            turn_count: accumulator.direct_started,
            attempt_count: accumulator.attempt_count,
            earliest_at: accumulator.earliest_epoch.and_then(normalized_timestamp),
            latest_at: accumulator.latest_epoch.and_then(normalized_timestamp),
        },
        recent_tasks,
        reliability: AnalyticsReliability {
            direct_reply_completion_rate: rate_metric(
                "direct.reply_completion_rate",
                accumulator.direct_completed,
                accumulator.direct_started,
                accumulator.direct_unknown,
                direct_evidence,
            ),
            workflow_run_terminal_success_rate: rate_metric(
                "workflow.run_terminal_success_rate",
                workflow_success,
                workflow_started,
                workflow_unknown,
                run_evidence.clone(),
            ),
            auto_outer_run_terminal_success_rate: rate_metric(
                "auto.outer_run_terminal_success_rate",
                auto_success,
                auto_started,
                auto_unknown,
                run_evidence,
            ),
            failed_count,
            cancelled_count,
            non_terminal_count,
        },
        quality: AnalyticsQuality {
            retry_reentry_rate: rate_metric(
                "node.retry_reentry_rate",
                retried_groups,
                group_counts.len() as u64,
                0,
                accumulator
                    .evidence
                    .iter()
                    .filter(|locator| locator.ends_with("node.json"))
                    .take(24)
                    .cloned()
                    .collect(),
            ),
            recovered_after_retry_count,
            terminal_signals: named_counts(&terminal_signals, 12),
        },
        efficiency: AnalyticsEfficiency {
            observed_terminal_run_active_seconds: terminal_active_duration_total,
            average_terminal_run_active_seconds: (terminal_active_duration_count > 0).then_some(
                terminal_active_duration_total as f64 / terminal_active_duration_count as f64,
            ),
            terminal_run_sample_count: terminal_active_duration_count,
            active_duration_zero_filled_count,
            pause_count: accumulator.pause_count,
            resume_count: accumulator.resume_count,
            manual_continue_count: accumulator.manual_continue_count,
            top_duration_tasks,
            node_aggregates,
        },
        token_usage,
        context_and_tools: AnalyticsContextAndTools {
            tool_call_count: accumulator.tool_counts.values().sum(),
            permission_request_count: accumulator.permission_request_count,
            elicitation_request_count: accumulator.elicitation_request_count,
            top_tools,
            top_agents,
            verified_skill_call_count: accumulator.skill_counts.values().sum(),
            top_skills,
            event_kinds,
        },
        warnings: accumulator.warnings,
        evidence_locators: accumulator.evidence.into_iter().take(512).collect(),
    };
    PersonalAnalyticsProjectionOutput {
        projection,
        semantic_batch: PersonalAnalyticsSemanticBatch {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            items: accumulator.semantic_items,
        },
    }
}

fn observe_turn(value: &Value, relative: &str, accumulator: &mut ProjectionAccumulator) {
    let data = value.pointer("/record/data").unwrap_or(value);
    accumulator.direct_started += 1;
    match data.get("status").and_then(Value::as_str) {
        Some("completed") => accumulator.direct_completed += 1,
        Some("failed") => accumulator.direct_failed += 1,
        Some("cancelled" | "canceled") => accumulator.direct_cancelled += 1,
        _ => accumulator.direct_unknown += 1,
    }
    accumulator.evidence.insert(relative.to_string());
}

fn observe_observability(value: &Value, accumulator: &mut ProjectionAccumulator) {
    accumulator.pause_count = accumulator
        .pause_count
        .saturating_add(u64_at(value, &["/counters/pauseCount"]));
    accumulator.resume_count = accumulator
        .resume_count
        .saturating_add(u64_at(value, &["/counters/resumeCount"]));
    accumulator.manual_continue_count = accumulator
        .manual_continue_count
        .saturating_add(u64_at(value, &["/counters/manualContinueCount"]));
}

fn observe_version(value: &Value, accumulator: &mut ProjectionAccumulator) {
    if let Some(version) = value.get("version").and_then(Value::as_str)
        && !matches!(version, "0.1" | "1" | "1.0")
    {
        accumulator.coverage.unknown_version_files += 1;
    }
}

fn observe_timestamps(value: &Value, accumulator: &mut ProjectionAccumulator) {
    for pointer in [
        "/createdAt",
        "/updatedAt",
        "/started_at",
        "/updated_at",
        "/record/updatedAt",
        "/record/data/startedAt",
        "/record/data/finishedAt",
    ] {
        let Some(timestamp) = value.pointer(pointer).and_then(Value::as_str) else {
            continue;
        };
        let Some(epoch) = timestamp_epoch(timestamp) else {
            continue;
        };
        if accumulator
            .earliest_epoch
            .is_none_or(|current| epoch < current)
        {
            accumulator.earliest_epoch = Some(epoch);
        }
        if accumulator
            .latest_epoch
            .is_none_or(|current| epoch > current)
        {
            accumulator.latest_epoch = Some(epoch);
        }
    }
}

fn rate_metric(
    metric_id: &str,
    numerator: u64,
    denominator: u64,
    unknown_count: u64,
    evidence_locators: Vec<String>,
) -> AnalyticsRateMetric {
    AnalyticsRateMetric {
        metric_id: metric_id.to_string(),
        numerator,
        denominator,
        unknown_count,
        rate: (denominator > 0).then_some(numerator as f64 / denominator as f64),
        evidence_locators,
    }
}

fn add_usage(target: &mut AnalyticsTokenUsage, usage: TokenCounters) {
    target.input_tokens = target.input_tokens.saturating_add(usage.input);
    target.output_tokens = target.output_tokens.saturating_add(usage.output);
    target.cache_read_tokens = target.cache_read_tokens.saturating_add(usage.cache_read);
    target.cache_write_tokens = target.cache_write_tokens.saturating_add(usage.cache_write);
    target.total_tokens = target.total_tokens.saturating_add(if usage.total > 0 {
        usage.total
    } else {
        usage
            .input
            .saturating_add(usage.output)
            .saturating_add(usage.cache_read)
            .saturating_add(usage.cache_write)
    });
    target.observed_prompt_count += 1;
}

fn add_token_counters(target: &mut TokenCounters, usage: TokenCounters) {
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.cache_read = target.cache_read.saturating_add(usage.cache_read);
    target.cache_write = target.cache_write.saturating_add(usage.cache_write);
    target.total = target.total.saturating_add(if usage.total > 0 {
        usage.total
    } else {
        usage
            .input
            .saturating_add(usage.output)
            .saturating_add(usage.cache_read)
            .saturating_add(usage.cache_write)
    });
}

fn token_counters(value: &Value) -> TokenCounters {
    TokenCounters {
        input: u64_at(value, &["/inputTokens", "/input_tokens"]),
        output: u64_at(value, &["/outputTokens", "/output_tokens"]),
        cache_read: u64_at(
            value,
            &[
                "/cachedReadTokens",
                "/cacheReadTokens",
                "/cached_read_tokens",
            ],
        ),
        cache_write: u64_at(
            value,
            &[
                "/cachedWriteTokens",
                "/cacheWriteTokens",
                "/cached_write_tokens",
            ],
        ),
        total: u64_at(value, &["/totalTokens", "/total_tokens"]),
    }
}

fn normalized_tool_name(item: &Value) -> String {
    for pointer in [
        "/raw/_meta/claudeCode/toolName",
        "/raw/toolName",
        "/raw/name",
        "/toolName",
    ] {
        if let Some(value) = item.pointer(pointer).and_then(Value::as_str)
            && is_safe_tool_name(value)
        {
            return value.to_ascii_lowercase();
        }
    }
    let title = item
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let candidate = title.split_whitespace().next().unwrap_or_default();
    if is_safe_tool_name(candidate) {
        candidate.to_ascii_lowercase()
    } else {
        "tool".to_string()
    }
}

fn is_safe_tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn named_counts(values: &BTreeMap<String, u64>, limit: usize) -> Vec<AnalyticsNamedCount> {
    let mut values = values
        .iter()
        .map(|(name, count)| AnalyticsNamedCount {
            name: name.clone(),
            count: *count,
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
    });
    values.truncate(limit);
    values
}

fn safe_relative_path(root: &Utf8Path, path: &Utf8Path) -> Option<String> {
    if path.is_symlink() {
        return None;
    }
    let canonical = path.canonicalize_utf8().ok()?;
    if !path_starts_with(&canonical, root) {
        return None;
    }
    canonical
        .strip_prefix(root)
        .ok()
        .map(|relative| relative.as_str().replace('\\', "/"))
}

fn path_starts_with(path: &Utf8Path, root: &Utf8Path) -> bool {
    if cfg!(windows) {
        path.as_str()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .starts_with(&root.as_str().replace('\\', "/").to_ascii_lowercase())
    } else {
        path.starts_with(root)
    }
}

fn is_excluded(relative: &str, file_name: &str) -> bool {
    EXCLUDED_FILE_NAMES.contains(&file_name)
        || Path::new(relative).components().any(|component| {
            let Component::Normal(name) = component else {
                return false;
            };
            let name = name.to_string_lossy();
            EXCLUDED_DIRECTORY_NAMES.contains(&name.as_ref())
                || matches!(name.as_ref(), "target" | ".git")
        })
        || matches!(
            Path::new(file_name)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "db" | "wal" | "shm" | "zip" | "pid" | "class" | "exe" | "dll" | "bin"
        )
}

fn task_key(relative: &str) -> Option<String> {
    let components = relative.split('/').collect::<Vec<_>>();
    let index = components
        .iter()
        .position(|component| *component == "tasks")?;
    if index == 0 || index + 1 >= components.len() {
        return None;
    }
    Some(format!(
        "{}/{}",
        components[index - 1],
        components[index + 1]
    ))
}

fn run_key(relative: &str) -> Option<String> {
    let components = relative.split('/').collect::<Vec<_>>();
    let index = components
        .iter()
        .position(|component| *component == "runs")?;
    (index + 1 < components.len()).then(|| components[..=index + 1].join("/"))
}

fn attempt_key(relative: &str) -> Option<String> {
    relative
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

fn node_group_key(relative: &str) -> Option<String> {
    let marker = "/attempt-";
    let index = relative.rfind(marker)?;
    Some(relative[..index].to_string())
}

fn task_summary(
    task_key: &str,
    task: &TaskFact,
    runs: &[RunFact],
    nodes: &[NodeFact],
    session_duration_seconds_by_attempt: &BTreeMap<String, u64>,
) -> Option<AnalyticsTaskSummary> {
    let task_runs = runs
        .iter()
        .filter(|run| run.task_key == task_key)
        .collect::<Vec<_>>();
    let latest = task_runs.iter().copied().max_by(|left, right| {
        left.updated_epoch
            .cmp(&right.updated_epoch)
            .then_with(|| left.locator.cmp(&right.locator))
    })?;
    let task_nodes = nodes
        .iter()
        .filter(|node| node.task_key == task_key)
        .collect::<Vec<_>>();
    let active_duration_zero_filled = task_nodes.is_empty()
        || task_nodes
            .iter()
            .any(|node| !session_duration_seconds_by_attempt.contains_key(&node.attempt_key));
    let active_duration_seconds = task_nodes
        .iter()
        .filter_map(|node| session_duration_seconds_by_attempt.get(&node.attempt_key))
        .copied()
        .sum();
    let last_activity_epoch = task
        .last_activity_epoch
        .into_iter()
        .chain(task_runs.iter().filter_map(|run| run.updated_epoch))
        .max();
    Some(AnalyticsTaskSummary {
        task_locator: task_key.to_string(),
        title: if task.title.trim().is_empty() {
            task_key.rsplit('/').next().unwrap_or("Task").to_string()
        } else {
            task.title.clone()
        },
        mode: if task.mode.trim().is_empty() {
            "unknown".to_string()
        } else {
            task.mode.clone()
        },
        status: latest.status.clone(),
        outcome: latest.outcome.clone(),
        agent_names: task.agent_names.iter().cloned().collect(),
        total_tokens: task.token_usage.total,
        active_duration_seconds,
        active_duration_zero_filled,
        terminal_node: latest.terminal_node.clone(),
        last_activity_at: last_activity_epoch.and_then(normalized_timestamp),
    })
}

fn timestamp_epoch(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp())
        .ok()
        .or_else(|| value.strip_suffix('Z')?.parse::<i64>().ok())
}

fn normalized_timestamp(epoch: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn string_at<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
}

fn u64_at(value: &Value, pointers: &[&str]) -> u64 {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
        .unwrap_or(0)
}

fn u64_at_optional(value: &Value, pointers: &[&str]) -> Option<u64> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_u64))
}

fn safe_locator(locator: &str) -> bool {
    !locator.trim().is_empty()
        && !locator.contains("..")
        && !locator.contains(':')
        && !locator.starts_with('/')
        && locator.len() <= 512
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn projection_uses_mode_specific_denominators_and_excludes_raw_frames() {
        let root = tempdir().unwrap();
        let project = root.path().join("project-a");
        write(
            &project.join("project.json"),
            r#"{"version":"0.1","projectId":"project-a"}"#,
        );
        for (task, mode, outcome) in [
            ("task-direct", "direct", "success"),
            ("task-workflow", "workflow", "success"),
            ("task-auto", "auto", "failure"),
        ] {
            write(
                &project.join(format!("tasks/{task}/authoring/task.json")),
                &format!(r#"{{"version":"0.1","id":"{task}"}}"#),
            );
            write(
                &project.join(format!("tasks/{task}/authoring/conversation.json")),
                &format!(r#"{{"version":"0.1","runMode":"{mode}"}}"#),
            );
            write(
                &project.join(format!("tasks/{task}/runs/run-001/run.json")),
                &format!(
                    r#"{{"version":"0.1","status":"completed","outcome":"{outcome}","started_at":"2026-08-17T10:00:00Z","updated_at":"2026-08-17T10:01:00Z"}}"#
                ),
            );
        }
        write(
            &project.join("tasks/task-direct/turns/turn-1/turn.json"),
            r#"{"record":{"data":{"status":"completed","startedAt":"2026-08-17T10:00:00Z","finishedAt":"2026-08-17T10:00:05Z"}}}"#,
        );
        write(
            &project.join("tasks/task-direct/turns/turn-2/turn.json"),
            r#"{"record":{"data":{"status":"cancelled"}}}"#,
        );
        write(
            &project.join("tasks/task-direct/runs/run-001/acp.raw.jsonl"),
            "not-json-and-must-never-be-read",
        );

        let output = build_personal_analytics_projection(
            Utf8Path::from_path(root.path()).unwrap(),
            "watermark".to_string(),
            |_, _| {},
            || false,
        )
        .unwrap();

        assert_eq!(
            output
                .projection
                .reliability
                .direct_reply_completion_rate
                .denominator,
            2
        );
        assert_eq!(
            output
                .projection
                .reliability
                .direct_reply_completion_rate
                .numerator,
            1
        );
        assert_eq!(
            output
                .projection
                .reliability
                .workflow_run_terminal_success_rate
                .numerator,
            1
        );
        assert_eq!(
            output
                .projection
                .reliability
                .auto_outer_run_terminal_success_rate
                .numerator,
            0
        );
        assert_eq!(output.projection.source_coverage.corrupt_files, 0);
        assert!(output.projection.source_coverage.skipped_files >= 1);
        assert_eq!(output.projection.recent_tasks.len(), 2);
        assert!(
            output
                .projection
                .recent_tasks
                .iter()
                .all(|task| task.mode != "direct")
        );
    }

    #[test]
    fn active_durations_use_session_timing_and_zero_fill_missing_snapshots() {
        let root = tempdir().unwrap();
        let project = root.path().join("project-a");
        for (task, updated_at) in [
            ("task-session", "1786868389Z"),
            ("task-missing", "1780976400Z"),
        ] {
            write(
                &project.join(format!("tasks/{task}/authoring/task.json")),
                &format!(r#"{{"version":"0.1","id":"{task}","title":"{task}"}}"#),
            );
            write(
                &project.join(format!("tasks/{task}/authoring/conversation.json")),
                r#"{"version":"2","runMode":"workflow"}"#,
            );
            write(
                &project.join(format!("tasks/{task}/runs/run-001/run.json")),
                &format!(
                    r#"{{"version":"0.1","status":"completed","outcome":"success","started_at":"1780976340Z","updated_at":"{updated_at}"}}"#
                ),
            );
            write(
                &project.join(format!(
                    "tasks/{task}/runs/run-001/rounds/round-001/nodes/dev/attempt-001/node.json"
                )),
                r#"{"version":"0.1","node_id":"dev","status":"completed","outcome":"success"}"#,
            );
        }
        write(
            &project.join("tasks/task-session/runs/run-001/rounds/round-001/nodes/dev/attempt-001/acp.snapshot.json"),
            r#"{"version":"0.1","timing":{"sessionElapsedSeconds":60,"paused":true}}"#,
        );

        let output = build_personal_analytics_projection(
            Utf8Path::from_path(root.path()).unwrap(),
            "watermark".to_string(),
            |_, _| {},
            || false,
        )
        .unwrap();

        assert_eq!(output.projection.efficiency.terminal_run_sample_count, 2);
        assert_eq!(
            output
                .projection
                .efficiency
                .observed_terminal_run_active_seconds,
            60
        );
        assert_eq!(
            output
                .projection
                .efficiency
                .average_terminal_run_active_seconds,
            Some(30.0)
        );
        assert_eq!(
            output
                .projection
                .efficiency
                .active_duration_zero_filled_count,
            1
        );
        assert_eq!(output.projection.recent_tasks.len(), 2);
        assert_eq!(
            output.projection.efficiency.top_duration_tasks[0].active_duration_seconds,
            60
        );
    }

    #[test]
    fn projection_isolates_corrupt_files_and_bounds_semantic_content() {
        let root = tempdir().unwrap();
        let project = root.path().join("project-a/tasks/task-1");
        write(&project.join("authoring/task.json"), "{broken");
        write(
            &project.join("authoring/requirement.md"),
            &"x".repeat(PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS + 500),
        );

        let output = build_personal_analytics_projection(
            Utf8Path::from_path(root.path()).unwrap(),
            "watermark".to_string(),
            |_, _| {},
            || false,
        )
        .unwrap();

        assert_eq!(output.projection.source_coverage.corrupt_files, 1);
        assert_eq!(output.semantic_batch.items.len(), 1);
        assert_eq!(
            output.semantic_batch.items[0].content.chars().count(),
            PERSONAL_ANALYTICS_SEMANTIC_ITEM_MAX_CHARS
        );
    }

    #[test]
    fn canonical_report_overwrites_agent_facts_and_rejects_unsafe_insights() {
        let projection = PersonalAnalyticsProjection {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            source_watermark: "source".to_string(),
            source_coverage: AnalyticsCoverage::default(),
            overview: AnalyticsOverview {
                project_count: 4,
                task_count: 8,
                conversation_count: 0,
                run_count: 0,
                turn_count: 0,
                attempt_count: 0,
                earliest_at: None,
                latest_at: None,
            },
            recent_tasks: vec![],
            reliability: AnalyticsReliability {
                direct_reply_completion_rate: rate_metric(
                    "direct.reply_completion_rate",
                    1,
                    2,
                    0,
                    vec![],
                ),
                workflow_run_terminal_success_rate: rate_metric(
                    "workflow.run_terminal_success_rate",
                    0,
                    0,
                    0,
                    vec![],
                ),
                auto_outer_run_terminal_success_rate: rate_metric(
                    "auto.outer_run_terminal_success_rate",
                    0,
                    0,
                    0,
                    vec![],
                ),
                failed_count: 0,
                cancelled_count: 0,
                non_terminal_count: 0,
            },
            quality: AnalyticsQuality {
                retry_reentry_rate: rate_metric("node.retry_reentry_rate", 0, 0, 0, vec![]),
                recovered_after_retry_count: 0,
                terminal_signals: vec![],
            },
            efficiency: AnalyticsEfficiency {
                observed_terminal_run_active_seconds: 0,
                average_terminal_run_active_seconds: None,
                terminal_run_sample_count: 0,
                active_duration_zero_filled_count: 0,
                pause_count: 0,
                resume_count: 0,
                manual_continue_count: 0,
                top_duration_tasks: vec![],
                node_aggregates: vec![],
            },
            token_usage: AnalyticsTokenUsage::default(),
            context_and_tools: AnalyticsContextAndTools {
                tool_call_count: 0,
                permission_request_count: 0,
                elicitation_request_count: 0,
                top_tools: vec![],
                top_agents: vec![],
                verified_skill_call_count: 0,
                top_skills: vec![],
                event_kinds: vec![],
            },
            warnings: vec![],
            evidence_locators: vec![],
        };
        let mut narrative = PersonalAnalyticsNarrative {
            schema_version: PERSONAL_ANALYTICS_REPORT_SCHEMA_VERSION.to_string(),
            insights: vec![],
        };
        narrative.insights.push(PersonalAnalyticsInsight {
            section: AnalyticsInsightSection::Quality,
            title: "unsafe".to_string(),
            summary: "unsafe".to_string(),
            recommendation: "unsafe".to_string(),
            confidence: AnalyticsInsightConfidence::High,
            sample_count: 1,
            evidence_locators: vec!["C:/secret.txt".to_string()],
        });

        let report = canonicalize_personal_analytics_report(
            &projection,
            narrative,
            "report-1".to_string(),
            "generated".to_string(),
        );
        assert_eq!(report.overview.project_count, 4);
        assert_eq!(report.report_id, "report-1");
        assert!(report.insights.is_empty());
    }
}
