use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::acp::events::{
    AcpUiEvent, AgentTranscriptRelation, agent_transcript_tool_output,
    extract_agent_transcript_relation, load_timeline_items, write_timeline_items,
};
use crate::storage::{atomic_write_file, ensure_parent_dir, write_json};

pub const ROOT_BRANCH_ID: &str = "root";
const BRANCH_META_KEY: &str = "goldBandConversation";
const AGENT_RESULT_MIGRATION_MARKER: &str = ".acp-agent-result-migration-v2";
const AGENT_NAMESPACE: Uuid = Uuid::from_u128(0x63c7f8ac_1498_4f6e_8f6d_62f2f04033f1);
const AGENT_INDEX_CACHE_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConversationBranchError {
    #[error("invalid conversation branch id")]
    InvalidBranchId,
}
impl ConversationBranchError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidBranchId => "acp.invalid-conversation-branch-id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentIndexSourceFileSignature {
    path: String,
    len: u64,
    modified_nanos: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentIndexSourceSignature {
    session_status: String,
    files: Vec<AgentIndexSourceFileSignature>,
}

#[derive(Clone)]
struct AgentIndexCacheEntry {
    attempt_dir: String,
    signature: AgentIndexSourceSignature,
    records: Vec<AgentExecutionRecord>,
}

fn agent_index_cache() -> &'static Mutex<VecDeque<AgentIndexCacheEntry>> {
    static CACHE: OnceLock<Mutex<VecDeque<AgentIndexCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBranchRoute {
    pub branch_id: String,
    pub launched_agent_execution_id: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConversationPlanOwnership {
    Branch,
    Unscoped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentExecutionRecord {
    pub agent_execution_id: String,
    pub parent_agent_execution_id: Option<String>,
    pub launch_tool_call_id: String,
    pub session_id: String,
    pub status: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub event_count: usize,
    pub tool_call_count: usize,
    pub read_file_count: usize,
    pub written_file_count: usize,
    pub has_attention: bool,
    pub latest_cursor: Option<String>,
    #[serde(default)]
    pub todo_entries: Vec<Value>,
}

pub fn stable_agent_execution_id(session_id: &str, launch_tool_call_id: &str) -> String {
    let name = format!("{session_id}\u{0}{launch_tool_call_id}");
    format!(
        "agent-{}",
        Uuid::new_v5(&AGENT_NAMESPACE, name.as_bytes()).simple()
    )
}

pub fn validate_conversation_branch_id(branch_id: &str) -> Result<()> {
    if branch_id == ROOT_BRANCH_ID {
        return Ok(());
    }
    let Some(value) = branch_id.strip_prefix("agent-") else {
        return Err(ConversationBranchError::InvalidBranchId.into());
    };
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ConversationBranchError::InvalidBranchId.into());
    }
    Ok(())
}

pub fn agent_relation(event: &AcpUiEvent) -> Option<AgentTranscriptRelation> {
    event
        .raw
        .as_ref()
        .and_then(extract_agent_transcript_relation)
}

pub fn branch_route_for_event(event: &AcpUiEvent) -> ConversationBranchRoute {
    let relation = agent_relation(event);
    let session_id = event.session_id.as_deref().unwrap_or("unknown-session");
    let parent_agent_execution_id = relation
        .as_ref()
        .and_then(|relation| relation.parent_tool_call_id.as_deref())
        .map(|tool_call_id| stable_agent_execution_id(session_id, tool_call_id));
    let launched_agent_execution_id = relation
        .as_ref()
        .filter(|relation| relation.agent_launch)
        .and_then(|_| event.tool_call_id.as_deref())
        .map(|tool_call_id| stable_agent_execution_id(session_id, tool_call_id));
    let tool_name = relation.and_then(|relation| relation.tool_name);
    // A persisted branch-id override determines which transcript an event
    // belongs to, but it must not discard the launch relation it carries.
    // Migration and agent-index accounting still need launched_agent_execution_id
    // and tool_name, which are orthogonal to branch ownership.
    let branch_id = event
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer(&format!("/_meta/{BRANCH_META_KEY}/branchId")))
        .and_then(Value::as_str)
        .filter(|branch_id| validate_conversation_branch_id(branch_id).is_ok())
        .map(|branch_id| branch_id.to_string())
        .unwrap_or_else(|| {
            parent_agent_execution_id
                .clone()
                .unwrap_or_else(|| ROOT_BRANCH_ID.to_string())
        });
    ConversationBranchRoute {
        branch_id,
        launched_agent_execution_id,
        tool_name,
    }
}

pub fn annotate_event_branch(event: &mut AcpUiEvent) -> ConversationBranchRoute {
    let route = branch_route_for_event(event);
    let plan_ownership = (event.kind == "plan")
        .then(|| conversation_plan_ownership(event.raw.as_ref(), route.branch_id.as_str()));
    // Agent launch results belong to the launched branch's formal transcript.
    // Keep ordinary tool output normalized for the shared tool renderer, but do
    // not duplicate an Agent's final response into its parent link event.
    let normalized_tool_output = route
        .launched_agent_execution_id
        .is_none()
        .then(|| {
            event
                .raw
                .as_ref()
                .and_then(agent_transcript_tool_output)
                .cloned()
        })
        .flatten();
    let raw = event.raw.get_or_insert_with(|| json!({}));
    if !raw.is_object() {
        *raw = json!({ "providerPayload": raw.clone() });
    }
    let object = raw
        .as_object_mut()
        .expect("normalized branch raw must be an object");
    if route.launched_agent_execution_id.is_some() {
        strip_agent_launch_output(object);
    }
    let meta = object.entry("_meta").or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    let mut conversation = json!({
        "branchId": route.branch_id,
        "launchedAgentExecutionId": route.launched_agent_execution_id,
        "toolName": route.tool_name,
        "toolOutput": normalized_tool_output,
    });
    if let Some(ownership) = plan_ownership {
        conversation
            .as_object_mut()
            .expect("normalized conversation metadata must be an object")
            .insert(
                "planOwnership".to_string(),
                serde_json::to_value(ownership).expect("ConversationPlanOwnership must serialize"),
            );
    }
    meta.as_object_mut()
        .expect("normalized branch meta must be an object")
        .insert(BRANCH_META_KEY.to_string(), conversation);
    route
}

pub fn conversation_plan_ownership(
    raw: Option<&Value>,
    branch_id: &str,
) -> ConversationPlanOwnership {
    let explicit = raw
        .and_then(|raw| raw.pointer(&format!("/_meta/{BRANCH_META_KEY}/planOwnership")))
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    if let Some(ownership) = explicit {
        return ownership;
    }
    if branch_id != ROOT_BRANCH_ID
        || raw
            .and_then(extract_agent_transcript_relation)
            .and_then(|relation| relation.parent_tool_call_id)
            .is_some()
    {
        ConversationPlanOwnership::Branch
    } else {
        ConversationPlanOwnership::Unscoped
    }
}

pub fn agent_prompt_event(launch: &AcpUiEvent) -> Option<AcpUiEvent> {
    let route = branch_route_for_event(launch);
    let agent_execution_id = route.launched_agent_execution_id?;
    let prompt = launch
        .raw
        .as_ref()
        .and_then(tool_raw_input)
        .and_then(|input| input.get("prompt"))
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.trim().is_empty())?;
    let mut event = AcpUiEvent {
        id: format!("agent-prompt-{agent_execution_id}"),
        seq: launch.started_seq.unwrap_or(launch.seq),
        timestamp: launch
            .started_at
            .clone()
            .unwrap_or_else(|| launch.timestamp.clone()),
        kind: "userTextDelta".to_string(),
        session_id: launch.session_id.clone(),
        content: Some(prompt.to_string()),
        title: Some("Agent prompt".to_string()),
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: Some(launch.started_seq.unwrap_or(launch.seq)),
        ended_seq: Some(launch.started_seq.unwrap_or(launch.seq)),
        started_at: Some(
            launch
                .started_at
                .clone()
                .unwrap_or_else(|| launch.timestamp.clone()),
        ),
        ended_at: Some(
            launch
                .started_at
                .clone()
                .unwrap_or_else(|| launch.timestamp.clone()),
        ),
        timing: None,
        raw: Some(json!({
            "source": "agentBranchPrompt",
            "_meta": {}
        })),
    };
    annotate_event_branch_override(&mut event, &agent_execution_id);
    Some(event)
}

pub fn agent_result_event(launch: &AcpUiEvent) -> Option<AcpUiEvent> {
    let output = launch.raw.as_ref().and_then(agent_transcript_tool_output)?;
    agent_result_event_from_output(launch, output)
}

fn legacy_agent_result_event(launch: &AcpUiEvent) -> Option<AcpUiEvent> {
    let started_seq = launch.started_seq.unwrap_or(launch.seq);
    let ended_seq = launch.ended_seq.unwrap_or(launch.seq);
    if ended_seq <= started_seq.saturating_add(1)
        || !matches!(
            launch
                .status
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "completed" | "success" | "succeeded"
        )
    {
        return None;
    }
    let raw = launch.raw.as_ref()?;
    raw.get("content")
        .or_else(|| raw.get("rawOutput"))
        .and_then(|output| agent_result_event_from_output(launch, output))
}

fn agent_result_event_from_output(launch: &AcpUiEvent, output: &Value) -> Option<AcpUiEvent> {
    // A background Agent tool result acknowledges that the execution was
    // launched; its formal response arrives later on the Agent transcript.
    // Treating that acknowledgement as a result would complete the branch
    // while it is still producing tools and text.
    if agent_launch_runs_in_background(launch) {
        return None;
    }
    let route = branch_route_for_event(launch);
    let agent_execution_id = route.launched_agent_execution_id?;
    let content = agent_output_text(output)?;
    let seq = launch.ended_seq.unwrap_or(launch.seq);
    let timestamp = launch
        .ended_at
        .clone()
        .unwrap_or_else(|| launch.timestamp.clone());
    let mut event = AcpUiEvent {
        id: format!("agent-result-{agent_execution_id}"),
        seq,
        timestamp: timestamp.clone(),
        kind: "textDelta".to_string(),
        session_id: launch.session_id.clone(),
        content: Some(content),
        title: Some("Agent result".to_string()),
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: Some(seq),
        ended_seq: Some(seq),
        started_at: Some(timestamp.clone()),
        ended_at: Some(timestamp),
        timing: launch.timing.clone(),
        raw: Some(json!({
            "source": "agentBranchResult",
            "_meta": {}
        })),
    };
    annotate_event_branch_override(&mut event, &agent_execution_id);
    Some(event)
}

fn agent_output_text(output: &Value) -> Option<String> {
    fn collect(value: &Value, parts: &mut Vec<String>) {
        match value {
            Value::String(text) if !text.trim().is_empty() => parts.push(text.clone()),
            Value::Array(items) => {
                for item in items {
                    collect(item, parts);
                }
            }
            Value::Object(object) => {
                if let Some(text) = object
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                {
                    parts.push(text.to_string());
                } else if let Some(content) = object.get("content") {
                    collect(content, parts);
                }
            }
            _ => {}
        }
    }

    let mut parts = Vec::new();
    collect(output, &mut parts);
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn strip_agent_launch_output(raw: &mut serde_json::Map<String, Value>) {
    for key in ["rawOutput", "output", "content"] {
        raw.remove(key);
    }
    if let Some(meta) = raw.get_mut("_meta").and_then(Value::as_object_mut) {
        if let Some(transcript) = meta
            .get_mut("agentTranscript")
            .and_then(Value::as_object_mut)
        {
            transcript.remove("toolOutput");
        }
        if let Some(provider) = meta.get_mut("claudeCode").and_then(Value::as_object_mut) {
            provider.remove("toolResponse");
        }
    }
}

fn agent_launch_has_embedded_output(event: &AcpUiEvent) -> bool {
    let Some(raw) = event.raw.as_ref() else {
        return false;
    };
    ["rawOutput", "output", "content"]
        .into_iter()
        .any(|key| raw.get(key).is_some())
        || raw.pointer("/_meta/agentTranscript/toolOutput").is_some()
        || raw.pointer("/_meta/claudeCode/toolResponse").is_some()
}

fn annotate_event_branch_override(event: &mut AcpUiEvent, branch_id: &str) {
    let raw = event.raw.get_or_insert_with(|| json!({}));
    let meta = raw
        .as_object_mut()
        .unwrap()
        .entry("_meta")
        .or_insert_with(|| json!({}));
    meta.as_object_mut().unwrap().insert(
        BRANCH_META_KEY.to_string(),
        json!({ "branchId": branch_id }),
    );
}

pub fn event_branch_id(event: &AcpUiEvent) -> String {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer(&format!("/_meta/{BRANCH_META_KEY}/branchId")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| branch_route_for_event(event).branch_id)
}

pub fn branch_timeline_path(attempt_dir: &Utf8Path, branch_id: &str) -> Utf8PathBuf {
    if branch_id == ROOT_BRANCH_ID {
        attempt_dir.join("acp.timeline.jsonl")
    } else {
        attempt_dir
            .join("agents")
            .join(branch_id)
            .join("timeline.jsonl")
    }
}

pub fn branch_snapshot_path(attempt_dir: &Utf8Path, branch_id: &str) -> Utf8PathBuf {
    attempt_dir
        .join("agents")
        .join(branch_id)
        .join("snapshot.json")
}

/// One-time conversion of the pre-timeline `acp.events.jsonl` format.
///
/// The legacy file is intentionally retained as an audit artifact, but all
/// runtime and query paths switch to branch timelines after this succeeds.
/// This avoids a permanent dual-read compatibility path while keeping existing
/// development conversations accessible.
pub fn migrate_legacy_events_timeline(attempt_dir: &Utf8Path) -> Result<bool> {
    let root_path = branch_timeline_path(attempt_dir, ROOT_BRANCH_ID);
    if root_path
        .metadata()
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
    {
        return Ok(false);
    }
    let legacy_path = attempt_dir.join("acp.events.jsonl");
    let Ok(file) = std::fs::File::open(legacy_path.as_std_path()) else {
        return Ok(false);
    };

    let mut canonical = Vec::<AcpUiEvent>::new();
    let mut keyed = HashMap::<String, usize>::new();
    let mut last_unkeyed_stream: Option<(String, String, usize)> = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(mut event) = serde_json::from_str::<AcpUiEvent>(&line) else {
            continue;
        };
        stamp_legacy_event_bounds(&mut event);
        let route = branch_route_for_event(&event);
        let key = legacy_event_merge_key(&event, &route.branch_id);
        if let Some(index) = key.as_ref().and_then(|key| keyed.get(key)).copied() {
            merge_legacy_event(&mut canonical[index], event);
            last_unkeyed_stream = None;
            continue;
        }
        let stream_kind = matches!(event.kind.as_str(), "textDelta" | "thoughtDelta" | "plan")
            .then(|| event.kind.clone());
        if key.is_none()
            && let Some(kind) = stream_kind.as_ref()
            && let Some((last_branch, last_kind, index)) = last_unkeyed_stream.as_ref()
            && last_branch == &route.branch_id
            && last_kind == kind
        {
            merge_legacy_event(&mut canonical[*index], event);
            continue;
        }
        if let Some(key) = key {
            keyed.insert(key, canonical.len());
        }
        last_unkeyed_stream =
            stream_kind.map(|kind| (route.branch_id.clone(), kind, canonical.len()));
        canonical.push(event);
    }
    if canonical.is_empty() {
        return Ok(false);
    }

    let mut by_branch = BTreeMap::<String, Vec<AcpUiEvent>>::new();
    for mut event in canonical {
        let prompt = agent_prompt_event(&event);
        let result = agent_result_event(&event).or_else(|| legacy_agent_result_event(&event));
        let route = annotate_event_branch(&mut event);
        by_branch.entry(route.branch_id).or_default().push(event);
        if let Some(prompt) = prompt {
            by_branch
                .entry(event_branch_id(&prompt))
                .or_default()
                .push(prompt);
        }
        if let Some(result) = result {
            by_branch
                .entry(event_branch_id(&result))
                .or_default()
                .push(result);
        }
    }
    for events in by_branch.values_mut() {
        events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
    }
    write_timeline_items(
        &root_path,
        by_branch
            .remove(ROOT_BRANCH_ID)
            .as_deref()
            .unwrap_or_default(),
    )?;
    for (branch_id, events) in by_branch {
        write_timeline_items(&branch_timeline_path(attempt_dir, &branch_id), &events)?;
    }
    Ok(true)
}

fn stamp_legacy_event_bounds(event: &mut AcpUiEvent) {
    event.started_seq.get_or_insert(event.seq);
    event.ended_seq.get_or_insert(event.seq);
    event
        .started_at
        .get_or_insert_with(|| event.timestamp.clone());
    event
        .ended_at
        .get_or_insert_with(|| event.timestamp.clone());
}

fn legacy_event_merge_key(event: &AcpUiEvent, branch_id: &str) -> Option<String> {
    match event.kind.as_str() {
        "toolCall" | "toolCallUpdate" => event
            .tool_call_id
            .as_deref()
            .map(|id| format!("{branch_id}:tool:{id}")),
        "permissionRequest" => Some(format!(
            "{branch_id}:permission:{}",
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.get("requestId"))
                .and_then(Value::as_str)
                .unwrap_or(event.id.as_str())
                .trim_start_matches("permission-")
        )),
        "textDelta" | "thoughtDelta" | "plan" => legacy_stream_identity(event)
            .map(|identity| format!("{branch_id}:stream:{}:{identity}", event.kind)),
        _ => None,
    }
}

fn legacy_stream_identity(event: &AcpUiEvent) -> Option<&str> {
    let raw = event.raw.as_ref()?;
    [
        "/messageId",
        "/thoughtId",
        "/planId",
        "/toolCallId",
        "/content/id",
        "/_meta/messageId",
    ]
    .into_iter()
    .find_map(|pointer| raw.pointer(pointer).and_then(Value::as_str))
}

fn merge_legacy_event(existing: &mut AcpUiEvent, incoming: AcpUiEvent) {
    let existing_start_seq = existing.started_seq.unwrap_or(existing.seq);
    let existing_started_at = existing
        .started_at
        .clone()
        .unwrap_or_else(|| existing.timestamp.clone());
    if matches!(incoming.kind.as_str(), "textDelta" | "thoughtDelta") {
        if let Some(chunk) = incoming.content.as_deref() {
            existing
                .content
                .get_or_insert_with(String::new)
                .push_str(chunk);
        }
    } else {
        if incoming.content.is_some() {
            existing.content = incoming.content.clone();
        }
        if incoming.title.is_some() {
            existing.title = incoming.title.clone();
        }
    }
    existing.kind = if incoming.kind == "toolCallUpdate" {
        "toolCall".to_string()
    } else {
        incoming.kind.clone()
    };
    existing.seq = incoming.seq;
    existing.timestamp = incoming.timestamp.clone();
    existing.status = incoming.status.clone().or_else(|| existing.status.clone());
    existing.tool_call_id = incoming
        .tool_call_id
        .clone()
        .or_else(|| existing.tool_call_id.clone());
    existing.started_seq = Some(existing_start_seq);
    existing.ended_seq = Some(incoming.ended_seq.unwrap_or(incoming.seq));
    existing.started_at = Some(existing_started_at);
    existing.ended_at = Some(
        incoming
            .ended_at
            .clone()
            .unwrap_or_else(|| incoming.timestamp.clone()),
    );
    existing.timing = incoming.timing.clone().or_else(|| existing.timing.clone());
    existing.raw = merge_legacy_raw(existing.raw.take(), incoming.raw);
}

fn merge_legacy_raw(existing: Option<Value>, incoming: Option<Value>) -> Option<Value> {
    match (existing, incoming) {
        (Some(Value::Object(mut existing)), Some(Value::Object(incoming))) => {
            for (key, value) in incoming {
                existing.insert(key, value);
            }
            Some(Value::Object(existing))
        }
        (_, Some(incoming)) => Some(incoming),
        (existing, None) => existing,
    }
}

pub fn migrate_legacy_agent_timeline(attempt_dir: &Utf8Path) -> Result<bool> {
    migrate_legacy_events_timeline(attempt_dir)?;
    let root_path = branch_timeline_path(attempt_dir, ROOT_BRANCH_ID);
    let mut events = load_timeline_items(&root_path)?;
    if !events
        .iter()
        .any(|event| branch_route_for_event(event).branch_id != ROOT_BRANCH_ID)
    {
        return Ok(false);
    }
    let mut by_branch = BTreeMap::<String, Vec<AcpUiEvent>>::new();
    for mut event in events.drain(..) {
        let prompt = agent_prompt_event(&event);
        let result = agent_result_event(&event).or_else(|| legacy_agent_result_event(&event));
        let route = annotate_event_branch(&mut event);
        by_branch.entry(route.branch_id).or_default().push(event);
        if let Some(prompt) = prompt {
            by_branch
                .entry(event_branch_id(&prompt))
                .or_default()
                .push(prompt);
        }
        if let Some(result) = result {
            by_branch
                .entry(event_branch_id(&result))
                .or_default()
                .push(result);
        }
    }
    write_timeline_items(
        &root_path,
        by_branch
            .remove(ROOT_BRANCH_ID)
            .as_deref()
            .unwrap_or_default(),
    )?;
    for (branch_id, branch_events) in by_branch {
        write_timeline_items(
            &branch_timeline_path(attempt_dir, &branch_id),
            &branch_events,
        )?;
    }
    Ok(true)
}

pub fn load_all_branch_events(attempt_dir: &Utf8Path) -> Result<Vec<AcpUiEvent>> {
    let mut events = load_timeline_items(&branch_timeline_path(attempt_dir, ROOT_BRANCH_ID))?;
    let agents_dir = attempt_dir.join("agents");
    if agents_dir.exists() {
        for entry in std::fs::read_dir(agents_dir.as_std_path())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(branch_dir) = Utf8PathBuf::from_path_buf(entry.path()).ok() else {
                continue;
            };
            events.extend(load_timeline_items(&branch_dir.join("timeline.jsonl"))?);
        }
    }
    events.sort_by_key(|event| event.started_seq.unwrap_or(event.seq));
    Ok(events)
}

/// One-time repair for conversations written before Agent launch prompts and
/// valid foreground results were materialized in the launched branch.
fn migrate_legacy_agent_results(attempt_dir: &Utf8Path) -> Result<bool> {
    let marker = attempt_dir.join(AGENT_RESULT_MIGRATION_MARKER);
    if marker.exists() {
        return Ok(false);
    }

    let mut timeline_paths = vec![branch_timeline_path(attempt_dir, ROOT_BRANCH_ID)];
    let agents_dir = attempt_dir.join("agents");
    if agents_dir.exists() {
        for entry in std::fs::read_dir(agents_dir.as_std_path())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(branch_dir) = Utf8PathBuf::from_path_buf(entry.path()).ok() else {
                continue;
            };
            timeline_paths.push(branch_dir.join("timeline.jsonl"));
        }
    }
    timeline_paths.sort();

    let mut background_branches = HashSet::<String>::new();
    for path in &timeline_paths {
        for event in load_timeline_items(path)? {
            let route = branch_route_for_event(&event);
            if agent_launch_runs_in_background(&event)
                && let Some(branch_id) = route.launched_agent_execution_id
            {
                background_branches.insert(branch_id);
            }
        }
    }

    let mut changed = false;
    let mut prompts = BTreeMap::<String, AcpUiEvent>::new();
    let mut results = BTreeMap::<String, AcpUiEvent>::new();
    for path in &timeline_paths {
        let mut events = load_timeline_items(path)?;
        let original_len = events.len();
        events.retain(|event| {
            !(is_agent_result_event(event) && background_branches.contains(&event_branch_id(event)))
        });
        let mut path_changed = events.len() != original_len;
        for event in &mut events {
            if branch_route_for_event(event)
                .launched_agent_execution_id
                .is_none()
            {
                continue;
            }
            if let Some(prompt) = agent_prompt_event(event) {
                prompts.insert(event_branch_id(&prompt), prompt);
            }
            if let Some(result) =
                agent_result_event(event).or_else(|| legacy_agent_result_event(event))
            {
                let branch_id = event_branch_id(&result);
                let should_replace = results
                    .get(&branch_id)
                    .is_none_or(|current| result.seq >= current.seq);
                if should_replace {
                    results.insert(branch_id, result);
                }
            }
            let had_embedded_output = agent_launch_has_embedded_output(event);
            annotate_event_branch(event);
            path_changed |= had_embedded_output;
        }
        if path_changed {
            write_timeline_items(path, &events)?;
            changed = true;
        }
    }

    for (branch_id, prompt) in prompts {
        let path = branch_timeline_path(attempt_dir, &branch_id);
        let mut events = load_timeline_items(&path)?;
        if events.iter().any(is_agent_prompt_event) {
            continue;
        }
        events.push(prompt);
        events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
        write_timeline_items(&path, &events)?;
        changed = true;
    }

    for (branch_id, result) in results {
        let path = branch_timeline_path(attempt_dir, &branch_id);
        let mut events = load_timeline_items(&path)?;
        if events
            .iter()
            .any(|event| equivalent_agent_result(event, &result))
        {
            continue;
        }
        events.push(result);
        events.sort_by_key(|event| (event.started_seq.unwrap_or(event.seq), event.seq));
        write_timeline_items(&path, &events)?;
        changed = true;
    }

    write_json(&marker, &json!({ "version": 2 }))?;
    Ok(changed)
}

pub fn rebuild_agent_index(
    attempt_dir: &Utf8Path,
    session_status: &str,
) -> Result<Vec<AgentExecutionRecord>> {
    let session_active = matches!(
        session_status,
        "running" | "active" | "starting" | "cancelling" | "stopping"
    );
    let session_interrupted = matches!(
        session_status,
        "cancelled" | "canceled" | "interrupted" | "stopped" | "failed" | "error"
    );
    migrate_legacy_agent_timeline(attempt_dir)?;
    migrate_legacy_agent_results(attempt_dir)?;
    let source_signature = agent_index_source_signature(attempt_dir, session_status)?;
    if let Some(records) = cached_agent_index(attempt_dir, &source_signature) {
        return Ok(records);
    }
    let all_events = load_all_branch_events(attempt_dir)?;
    let mut launches = HashMap::<String, AcpUiEvent>::new();
    for event in &all_events {
        let Some(relation) = agent_relation(event) else {
            continue;
        };
        if !relation.agent_launch {
            continue;
        }
        let Some(tool_call_id) = event.tool_call_id.as_ref() else {
            continue;
        };
        let should_replace = launches
            .get(tool_call_id)
            .map(|current| event.seq >= current.seq)
            .unwrap_or(true);
        if should_replace {
            launches.insert(tool_call_id.clone(), event.clone());
        }
    }
    let mut records = launches
        .into_iter()
        .map(|(launch_tool_call_id, launch)| {
            let session_id = launch
                .session_id
                .clone()
                .unwrap_or_else(|| "unknown-session".to_string());
            let agent_execution_id = stable_agent_execution_id(&session_id, &launch_tool_call_id);
            let relation = agent_relation(&launch).unwrap_or_default();
            let parent_agent_execution_id = relation
                .parent_tool_call_id
                .as_deref()
                .map(|tool_call_id| stable_agent_execution_id(&session_id, tool_call_id));
            let branch_events =
                load_timeline_items(&branch_timeline_path(attempt_dir, &agent_execution_id))
                    .unwrap_or_default();
            let execution_events = branch_events
                .iter()
                .filter(|event| !is_agent_prompt_event(event))
                .collect::<Vec<_>>();
            let metrics = branch_metrics(&branch_events);
            let latest_seq = execution_events
                .iter()
                .map(|event| event.ended_seq.unwrap_or(event.seq))
                .max();
            let latest_timestamp = execution_events
                .iter()
                .max_by_key(|event| event.ended_seq.unwrap_or(event.seq))
                .map(|event| {
                    event
                        .ended_at
                        .clone()
                        .unwrap_or_else(|| event.timestamp.clone())
                });
            let launch_status = launch
                .status
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let failed = matches!(launch_status.as_str(), "failed" | "error");
            let has_attention = branch_has_pending_interaction(&branch_events);
            let status = if failed {
                "failed"
            } else if has_agent_completion_evidence(&execution_events) {
                "completed"
            } else if has_attention && session_active {
                "waiting_permission"
            } else if !session_active {
                if session_interrupted {
                    "interrupted"
                } else if matches!(
                    launch_status.as_str(),
                    "completed" | "success" | "succeeded"
                ) {
                    "completed"
                } else {
                    "interrupted"
                }
            } else if execution_events.is_empty() {
                "queued"
            } else {
                "running"
            }
            .to_string();
            let input = launch.raw.as_ref().and_then(tool_raw_input);
            let title = launch.title.clone();
            let description = input
                .and_then(|input| input.get("description"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let updated_at = latest_timestamp.unwrap_or_else(|| {
                launch
                    .ended_at
                    .clone()
                    .or_else(|| launch.started_at.clone())
                    .unwrap_or_else(|| launch.timestamp.clone())
            });
            let ended_at = matches!(status.as_str(), "completed" | "failed" | "interrupted")
                .then(|| updated_at.clone());
            AgentExecutionRecord {
                agent_execution_id,
                parent_agent_execution_id,
                launch_tool_call_id,
                session_id,
                status,
                title,
                description,
                started_at: launch
                    .started_at
                    .clone()
                    .unwrap_or_else(|| launch.timestamp.clone()),
                updated_at,
                ended_at,
                event_count: execution_events.len(),
                tool_call_count: metrics.tool_call_count,
                read_file_count: metrics.read_file_count,
                written_file_count: metrics.written_file_count,
                has_attention,
                latest_cursor: latest_seq.map(|seq| format!("seq:{seq}")),
                todo_entries: latest_plan_entries(&branch_events),
            }
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.started_at.cmp(&right.started_at));
    propagate_agent_attention_to_ancestors(&mut records);
    write_agent_index(attempt_dir, &records)?;
    for record in &records {
        write_agent_snapshot_if_changed(
            &branch_snapshot_path(attempt_dir, &record.agent_execution_id),
            record,
        )?;
    }
    cache_agent_index(attempt_dir, source_signature, records.clone());
    Ok(records)
}

fn propagate_agent_attention_to_ancestors(records: &mut [AgentExecutionRecord]) {
    let index_by_id = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.agent_execution_id.clone(), index))
        .collect::<HashMap<_, _>>();
    let attention_sources = records
        .iter()
        .filter(|record| record.has_attention)
        .map(|record| record.agent_execution_id.clone())
        .collect::<Vec<_>>();
    for source in attention_sources {
        let mut current = source;
        let mut visited = std::collections::HashSet::new();
        while visited.insert(current.clone()) {
            let Some(index) = index_by_id.get(&current).copied() else {
                break;
            };
            let Some(parent_id) = records[index].parent_agent_execution_id.clone() else {
                break;
            };
            let Some(parent_index) = index_by_id.get(&parent_id).copied() else {
                break;
            };
            records[parent_index].has_attention = true;
            current = parent_id;
        }
    }
}

fn branch_has_pending_interaction(events: &[AcpUiEvent]) -> bool {
    let resolved_elicitation_ids = events
        .iter()
        .filter(|event| event.kind == "elicitationResponse")
        .filter_map(elicitation_id)
        .collect::<HashSet<_>>();
    events.iter().any(|event| {
        let pending = event
            .status
            .as_deref()
            .unwrap_or("pending")
            .eq_ignore_ascii_case("pending");
        if !pending {
            return false;
        }
        match event.kind.as_str() {
            "permissionRequest" => true,
            "elicitationRequest" => {
                elicitation_id(event).is_none_or(|id| !resolved_elicitation_ids.contains(&id))
            }
            _ => false,
        }
    })
}

fn elicitation_id(event: &AcpUiEvent) -> Option<String> {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("elicitationId"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let id = event.id.strip_suffix("-response").unwrap_or(&event.id);
            (!id.is_empty()).then(|| id.to_string())
        })
}

fn agent_index_source_signature(
    attempt_dir: &Utf8Path,
    session_status: &str,
) -> Result<AgentIndexSourceSignature> {
    let mut paths = vec![branch_timeline_path(attempt_dir, ROOT_BRANCH_ID)];
    let agents_dir = attempt_dir.join("agents");
    if agents_dir.exists() {
        for entry in std::fs::read_dir(agents_dir.as_std_path())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(branch_dir) = Utf8PathBuf::from_path_buf(entry.path()).ok() else {
                continue;
            };
            paths.push(branch_dir.join("timeline.jsonl"));
        }
    }
    paths.sort();
    let files = paths
        .into_iter()
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            let modified_nanos = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            Some(AgentIndexSourceFileSignature {
                path: path.to_string(),
                len: metadata.len(),
                modified_nanos,
            })
        })
        .collect();
    Ok(AgentIndexSourceSignature {
        session_status: session_status.to_string(),
        files,
    })
}

fn cached_agent_index(
    attempt_dir: &Utf8Path,
    signature: &AgentIndexSourceSignature,
) -> Option<Vec<AgentExecutionRecord>> {
    let mut cache = agent_index_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let attempt_dir = attempt_dir.as_str();
    let position = cache
        .iter()
        .position(|entry| entry.attempt_dir == attempt_dir && &entry.signature == signature)?;
    let entry = cache.remove(position)?;
    let records = entry.records.clone();
    cache.push_back(entry);
    Some(records)
}

fn cache_agent_index(
    attempt_dir: &Utf8Path,
    signature: AgentIndexSourceSignature,
    records: Vec<AgentExecutionRecord>,
) {
    let mut cache = agent_index_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|entry| entry.attempt_dir != attempt_dir.as_str());
    cache.push_back(AgentIndexCacheEntry {
        attempt_dir: attempt_dir.to_string(),
        signature,
        records,
    });
    while cache.len() > AGENT_INDEX_CACHE_CAPACITY {
        cache.pop_front();
    }
}

fn write_agent_index(attempt_dir: &Utf8Path, records: &[AgentExecutionRecord]) -> Result<bool> {
    let path = attempt_dir.join("acp.agents.jsonl");
    let mut content = Vec::new();
    for record in records {
        serde_json::to_writer(&mut content, record)?;
        content.push(b'\n');
    }
    if std::fs::read(path.as_std_path()).ok().as_deref() == Some(content.as_slice()) {
        return Ok(false);
    }
    ensure_parent_dir(&path)?;
    atomic_write_file(path.as_std_path(), |file| -> Result<()> {
        file.write_all(&content)?;
        Ok(())
    })?;
    Ok(true)
}

fn write_agent_snapshot_if_changed(path: &Utf8Path, record: &AgentExecutionRecord) -> Result<bool> {
    if path.exists()
        && crate::storage::read_json::<AgentExecutionRecord>(path)
            .ok()
            .as_ref()
            == Some(record)
    {
        return Ok(false);
    }
    write_json(path, record)?;
    Ok(true)
}

#[derive(Default)]
struct BranchMetrics {
    tool_call_count: usize,
    read_file_count: usize,
    written_file_count: usize,
}

fn branch_metrics(events: &[AcpUiEvent]) -> BranchMetrics {
    let mut latest_tools = BTreeMap::<String, &AcpUiEvent>::new();
    for event in events {
        if !matches!(event.kind.as_str(), "toolCall" | "toolCallUpdate") {
            continue;
        }
        let relation = agent_relation(event);
        if relation
            .as_ref()
            .is_some_and(|relation| relation.agent_launch)
        {
            continue;
        }
        let key = event
            .tool_call_id
            .clone()
            .unwrap_or_else(|| event.id.clone());
        let should_replace = latest_tools.get(&key).is_none_or(|current| {
            event.ended_seq.unwrap_or(event.seq) >= current.ended_seq.unwrap_or(current.seq)
        });
        if should_replace {
            latest_tools.insert(key, event);
        }
    }
    let mut read_files = std::collections::BTreeSet::<String>::new();
    let mut written_files = std::collections::BTreeSet::<String>::new();
    for event in latest_tools.values() {
        let relation = agent_relation(event);
        let tool_name = relation
            .and_then(|relation| relation.tool_name)
            .or_else(|| event.title.clone())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let paths = structured_tool_paths(event);
        if matches!(tool_name.as_str(), "read" | "get-content" | "read_file") {
            read_files.extend(paths);
        } else if matches!(
            tool_name.as_str(),
            "write" | "edit" | "applypatch" | "apply_patch" | "set-content" | "write_file"
        ) {
            written_files.extend(paths);
        }
    }
    BranchMetrics {
        tool_call_count: latest_tools.len(),
        read_file_count: read_files.len(),
        written_file_count: written_files.len(),
    }
}

fn is_agent_prompt_event(event: &AcpUiEvent) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("source"))
        .and_then(Value::as_str)
        == Some("agentBranchPrompt")
}

fn is_agent_result_event(event: &AcpUiEvent) -> bool {
    event
        .raw
        .as_ref()
        .and_then(|raw| raw.get("source"))
        .and_then(Value::as_str)
        == Some("agentBranchResult")
}

fn equivalent_agent_result(existing: &AcpUiEvent, result: &AcpUiEvent) -> bool {
    is_agent_result_event(existing)
        || (existing.kind == "textDelta"
            && existing.content.as_deref().map(str::trim)
                == result.content.as_deref().map(str::trim))
}

fn has_agent_completion_evidence(events: &[&AcpUiEvent]) -> bool {
    events.iter().any(|event| is_agent_result_event(event))
}

fn agent_launch_runs_in_background(event: &AcpUiEvent) -> bool {
    event
        .raw
        .as_ref()
        .and_then(tool_raw_input)
        .and_then(|input| input.get("run_in_background"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn structured_tool_paths(event: &AcpUiEvent) -> Vec<String> {
    let mut paths = Vec::<String>::new();
    if let Some(input) = event.raw.as_ref().and_then(tool_raw_input) {
        for key in ["file_path", "path"] {
            if let Some(path) = input.get(key).and_then(Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    let locations = event
        .raw
        .as_ref()
        .and_then(|raw| {
            raw.pointer("/toolCall/locations")
                .or_else(|| raw.get("locations"))
        })
        .and_then(Value::as_array);
    if let Some(locations) = locations {
        for location in locations {
            if let Some(path) = location.get("path").and_then(Value::as_str) {
                paths.push(normalize_metric_path(path));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn normalize_metric_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn latest_plan_entries(events: &[AcpUiEvent]) -> Vec<Value> {
    events
        .iter()
        .filter(|event| {
            event.kind == "plan"
                && conversation_plan_ownership(event.raw.as_ref(), event_branch_id(event).as_str())
                    == ConversationPlanOwnership::Branch
        })
        .max_by_key(|event| event.ended_seq.unwrap_or(event.seq))
        .and_then(|event| event.raw.as_ref())
        .and_then(|raw| raw.get("entries").or_else(|| raw.pointer("/plan/entries")))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn tool_raw_input(raw: &Value) -> Option<&Value> {
    raw.get("rawInput")
        .or_else(|| raw.pointer("/toolCall/rawInput"))
        .or_else(|| raw.get("input"))
        .or_else(|| raw.pointer("/toolCall/input"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::events::append_ui_event;

    fn event_at(
        id: &str,
        seq: u64,
        kind: &str,
        tool_call_id: Option<&str>,
        status: Option<&str>,
        relation: Value,
        raw_input: Option<Value>,
    ) -> AcpUiEvent {
        let mut raw = json!({ "_meta": { "agentTranscript": relation } });
        if let Some(raw_input) = raw_input {
            raw["rawInput"] = raw_input;
        }
        AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: kind.to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: None,
            tool_call_id: tool_call_id.map(str::to_string),
            status: status.map(str::to_string),
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw: Some(raw),
        }
    }

    fn event(id: &str, tool_call_id: Option<&str>, relation: Value) -> AcpUiEvent {
        event_at(
            id,
            1,
            "toolCall",
            tool_call_id,
            Some("pending"),
            relation,
            None,
        )
    }

    fn temp_attempt(label: &str) -> Utf8PathBuf {
        let path =
            std::env::temp_dir().join(format!("gold-band-agent-branch-{label}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Utf8PathBuf::from_path_buf(path).unwrap()
    }

    fn persist_partitioned(attempt_dir: &Utf8Path, events: Vec<AcpUiEvent>) {
        let mut by_branch = BTreeMap::<String, Vec<AcpUiEvent>>::new();
        for mut event in events {
            let prompt = agent_prompt_event(&event);
            let result = agent_result_event(&event);
            let route = annotate_event_branch(&mut event);
            by_branch.entry(route.branch_id).or_default().push(event);
            if let Some(prompt) = prompt {
                by_branch
                    .entry(event_branch_id(&prompt))
                    .or_default()
                    .push(prompt);
            }
            if let Some(result) = result {
                by_branch
                    .entry(event_branch_id(&result))
                    .or_default()
                    .push(result);
            }
        }
        for (branch_id, events) in by_branch {
            write_timeline_items(&branch_timeline_path(attempt_dir, &branch_id), &events).unwrap();
        }
    }

    #[test]
    fn stable_agent_ids_do_not_expose_provider_tool_ids() {
        let id = stable_agent_execution_id("session-1", "tool/use:unsafe");
        assert!(id.starts_with("agent-"));
        assert!(!id.contains("tool/use"));
        assert_eq!(
            id,
            stable_agent_execution_id("session-1", "tool/use:unsafe")
        );
    }

    #[test]
    fn persisted_branch_route_preserves_launch_metadata() {
        let mut launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true, "toolName": "Agent" }),
            Some(json!({ "run_in_background": true })),
        );

        let expected = annotate_event_branch(&mut launch);
        assert_eq!(branch_route_for_event(&launch), expected);
    }

    #[test]
    fn branch_annotation_consumes_only_internal_tool_output() {
        let mut event = event_at(
            "tool",
            1,
            "toolCall",
            Some("provider-tool"),
            Some("completed"),
            json!({}),
            None,
        );
        event.raw = Some(json!({
            "_meta": {
                "agentTranscript": { "toolOutput": "internal output" },
                "claudeCode": { "toolResponse": { "content": "provider output" } }
            }
        }));

        annotate_event_branch(&mut event);

        assert_eq!(
            event
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/goldBandConversation/toolOutput")),
            Some(&json!("internal output"))
        );
    }

    #[test]
    fn branch_annotation_marks_plan_ownership_without_guessing_content() {
        let mut root_plan = event_at(
            "root-plan",
            1,
            "plan",
            None,
            Some("completed"),
            json!({}),
            None,
        );
        root_plan.raw.as_mut().unwrap()["entries"] = json!([
            { "content": "text mentioning a child is not ownership", "status": "pending" }
        ]);
        annotate_event_branch(&mut root_plan);
        assert_eq!(
            root_plan
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/goldBandConversation/planOwnership")),
            Some(&json!("unscoped"))
        );

        let mut branch_plan = event_at(
            "branch-plan",
            2,
            "plan",
            None,
            Some("completed"),
            json!({ "parentToolCallId": "provider-child" }),
            None,
        );
        let route = annotate_event_branch(&mut branch_plan);
        assert_ne!(route.branch_id, ROOT_BRANCH_ID);
        assert_eq!(
            branch_plan
                .raw
                .as_ref()
                .and_then(|raw| raw.pointer("/_meta/goldBandConversation/planOwnership")),
            Some(&json!("branch"))
        );
    }

    #[test]
    fn agent_launch_result_moves_to_the_launched_branch() {
        let mut launch = event_at(
            "launch",
            5,
            "toolCall",
            Some("provider-agent"),
            Some("completed"),
            json!({ "agentLaunch": true, "toolOutput": [{ "type": "text", "text": "final answer" }] }),
            Some(json!({ "prompt": "inspect" })),
        );
        launch.raw.as_mut().unwrap()["content"] =
            json!([{ "type": "text", "text": "duplicate parent output" }]);

        let result = agent_result_event(&launch).expect("canonical Agent result");
        let branch_id = stable_agent_execution_id("session-1", "provider-agent");
        assert_eq!(event_branch_id(&result), branch_id);
        assert_eq!(result.content.as_deref(), Some("final answer"));
        assert!(is_agent_result_event(&result));

        annotate_event_branch(&mut launch);
        let raw = launch.raw.as_ref().unwrap();
        assert!(raw.get("content").is_none());
        assert!(raw.pointer("/_meta/agentTranscript/toolOutput").is_none());
        assert_eq!(
            raw.pointer("/_meta/goldBandConversation/launchedAgentExecutionId")
                .and_then(Value::as_str),
            Some(branch_id.as_str())
        );
    }

    #[test]
    fn branch_ids_reject_provider_ids_and_path_segments() {
        assert!(validate_conversation_branch_id(ROOT_BRANCH_ID).is_ok());
        assert!(
            validate_conversation_branch_id(&stable_agent_execution_id("session", "tool")).is_ok()
        );
        for invalid in ["provider-tool-id", "../acp.timeline.jsonl"] {
            let error = validate_conversation_branch_id(invalid).unwrap_err();
            let domain = error
                .downcast_ref::<ConversationBranchError>()
                .expect("structured branch error");
            assert_eq!(domain.code(), "acp.invalid-conversation-branch-id");
        }
    }

    #[test]
    fn launch_is_stored_in_parent_and_children_in_launched_branch() {
        let launch = event("launch", Some("child-tool"), json!({ "agentLaunch": true }));
        assert_eq!(branch_route_for_event(&launch).branch_id, ROOT_BRANCH_ID);
        let child = event(
            "read",
            Some("read-1"),
            json!({ "parentToolCallId": "child-tool" }),
        );
        let route = branch_route_for_event(&child);
        assert_eq!(
            route.branch_id,
            stable_agent_execution_id("session-1", "child-tool")
        );
    }

    #[test]
    fn nested_launch_is_routed_to_parent_branch_and_indexes_parent_relation() {
        let attempt = temp_attempt("nested-route");
        let outer = event_at(
            "outer",
            1,
            "toolCall",
            Some("provider-outer"),
            Some("pending"),
            json!({ "agentLaunch": true }),
            Some(json!({ "description": "outer" })),
        );
        let nested = event_at(
            "nested",
            2,
            "toolCall",
            Some("provider-nested"),
            Some("pending"),
            json!({ "agentLaunch": true, "parentToolCallId": "provider-outer" }),
            Some(json!({ "description": "nested" })),
        );
        assert_eq!(
            branch_route_for_event(&nested).branch_id,
            stable_agent_execution_id("session-1", "provider-outer")
        );
        persist_partitioned(&attempt, vec![outer, nested]);
        let records = rebuild_agent_index(&attempt, "running").unwrap();
        let nested_record = records
            .iter()
            .find(|record| record.launch_tool_call_id == "provider-nested")
            .unwrap();
        let expected_parent = stable_agent_execution_id("session-1", "provider-outer");
        assert_eq!(
            nested_record.parent_agent_execution_id.as_deref(),
            Some(expected_parent.as_str())
        );
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn nested_pending_interaction_projects_attention_to_agent_ancestors() {
        let attempt = temp_attempt("nested-attention");
        let outer = event_at(
            "outer",
            1,
            "toolCall",
            Some("provider-outer"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({ "run_in_background": true })),
        );
        let nested = event_at(
            "nested",
            2,
            "toolCall",
            Some("provider-nested"),
            Some("completed"),
            json!({ "agentLaunch": true, "parentToolCallId": "provider-outer" }),
            Some(json!({ "run_in_background": true })),
        );
        let permission = event_at(
            "permission-request",
            3,
            "permissionRequest",
            Some("tool-needing-permission"),
            Some("pending"),
            json!({ "parentToolCallId": "provider-nested" }),
            None,
        );
        persist_partitioned(&attempt, vec![outer, nested, permission]);

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        let outer = records
            .iter()
            .find(|record| record.launch_tool_call_id == "provider-outer")
            .unwrap();
        let nested = records
            .iter()
            .find(|record| record.launch_tool_call_id == "provider-nested")
            .unwrap();
        assert!(outer.has_attention);
        assert_eq!(outer.status, "running");
        assert!(nested.has_attention);
        assert_eq!(nested.status, "waiting_permission");
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn answered_elicitation_clears_persisted_agent_attention() {
        let attempt = temp_attempt("resolved-elicitation-attention");
        let launch = event_at(
            "agent",
            1,
            "toolCall",
            Some("provider-agent"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({ "run_in_background": true })),
        );
        let mut request = event_at(
            "elicit-1",
            2,
            "elicitationRequest",
            None,
            Some("pending"),
            json!({ "parentToolCallId": "provider-agent" }),
            None,
        );
        request.raw.as_mut().unwrap()["elicitationId"] = json!("elicit-1");
        let mut response = event_at(
            "elicit-1-response",
            3,
            "elicitationResponse",
            None,
            Some("completed"),
            json!({ "parentToolCallId": "provider-agent" }),
            None,
        );
        response.raw.as_mut().unwrap()["elicitationId"] = json!("elicit-1");
        persist_partitioned(&attempt, vec![launch, request, response]);

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        let agent = records
            .iter()
            .find(|record| record.launch_tool_call_id == "provider-agent")
            .unwrap();
        assert!(!agent.has_attention);
        assert_eq!(agent.status, "running");
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn legacy_events_are_migrated_once_into_single_owner_timelines() {
        let attempt = temp_attempt("legacy-migration");
        let legacy = attempt.join("acp.events.jsonl");
        let mut user = event_at(
            "user",
            1,
            "userTextDelta",
            None,
            Some("completed"),
            json!({}),
            None,
        );
        user.content = Some("root prompt".to_string());
        let mut launch = event_at(
            "launch",
            2,
            "toolCall",
            Some("provider-child"),
            Some("pending"),
            json!({ "agentLaunch": true }),
            Some(json!({ "prompt": "child prompt", "description": "child" })),
        );
        launch.raw.as_mut().unwrap()["_meta"]["agentTranscript"]["toolOutput"] =
            json!("child result");
        let child = event_at(
            "child-read",
            3,
            "toolCall",
            Some("provider-read"),
            Some("completed"),
            json!({ "parentToolCallId": "provider-child", "toolName": "Read" }),
            Some(json!({ "file_path": "src/lib.rs" })),
        );
        for event in [&user, &launch, &child] {
            append_ui_event(&legacy, event).unwrap();
        }

        assert!(migrate_legacy_events_timeline(&attempt).unwrap());
        assert!(!migrate_legacy_events_timeline(&attempt).unwrap());
        let root = load_timeline_items(&branch_timeline_path(&attempt, ROOT_BRANCH_ID)).unwrap();
        let branch_id = stable_agent_execution_id("session-1", "provider-child");
        let branch = load_timeline_items(&branch_timeline_path(&attempt, &branch_id)).unwrap();
        assert_eq!(
            root.iter().filter(|event| event.id == "child-read").count(),
            0
        );
        assert!(root.iter().any(|event| event.id == "launch"));
        assert!(branch.iter().any(|event| event.id == "child-read"));
        assert!(branch.iter().any(is_agent_prompt_event));
        assert!(branch.iter().any(|event| {
            is_agent_result_event(event) && event.content.as_deref() == Some("child result")
        }));
        let root_launch = root.iter().find(|event| event.id == "launch").unwrap();
        assert!(
            root_launch
                .raw
                .as_ref()
                .unwrap()
                .pointer("/_meta/agentTranscript/toolOutput")
                .is_none()
        );
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn synthetic_agent_prompt_does_not_move_a_queued_agent_to_running() {
        let attempt = temp_attempt("queued-prompt");
        persist_partitioned(
            &attempt,
            vec![event_at(
                "launch",
                1,
                "toolCall",
                Some("provider-child"),
                Some("completed"),
                json!({ "agentLaunch": true }),
                Some(json!({
                    "prompt": "inspect",
                    "description": "child",
                    "run_in_background": true
                })),
            )],
        );
        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(records[0].status, "queued");
        assert_eq!(records[0].event_count, 0);
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn background_agent_with_streaming_text_remains_running_until_session_terminal() {
        let attempt = temp_attempt("background-running");
        let launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({
                "run_in_background": true,
                "description": "child",
                "prompt": "inspect"
            })),
        );
        let mut text = event_at(
            "child-text",
            2,
            "textDelta",
            None,
            None,
            json!({ "parentToolCallId": "provider-child" }),
            None,
        );
        text.content = Some("partial answer".to_string());
        persist_partitioned(&attempt, vec![launch, text]);

        let running = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(running[0].status, "running");
        assert_eq!(running[0].updated_at, "2Z");
        assert!(running[0].ended_at.is_none());
        assert_eq!(
            rebuild_agent_index(&attempt, "completed").unwrap()[0].status,
            "completed"
        );
        assert_eq!(
            rebuild_agent_index(&attempt, "stopped").unwrap()[0].status,
            "interrupted"
        );
        assert_eq!(
            rebuild_agent_index(&attempt, "failed").unwrap()[0].status,
            "interrupted"
        );
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn completed_launch_tool_does_not_complete_a_generating_foreground_agent() {
        let attempt = temp_attempt("foreground-still-running");
        let launch = event_at(
            "launch",
            10,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({ "description": "child" })),
        );
        let thought = event_at(
            "child-thought",
            9,
            "thoughtDelta",
            None,
            None,
            json!({ "parentToolCallId": "provider-child" }),
            None,
        );
        persist_partitioned(&attempt, vec![launch, thought]);

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(records[0].status, "running");
        assert!(records[0].ended_at.is_none());
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn background_agent_acknowledgement_does_not_complete_the_branch() {
        let attempt = temp_attempt("background-acknowledgement");
        let launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true, "toolOutput": "verified final answer" }),
            Some(json!({ "run_in_background": true, "description": "child" })),
        );
        persist_partitioned(&attempt, vec![launch]);

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(records[0].status, "queued");
        let branch = load_timeline_items(&branch_timeline_path(
            &attempt,
            &records[0].agent_execution_id,
        ))
        .unwrap();
        assert!(!branch.iter().any(is_agent_result_event));
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn result_migration_v2_removes_legacy_background_acknowledgements() {
        let attempt = temp_attempt("background-acknowledgement-v2");
        let launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({
                "run_in_background": true,
                "description": "child",
                "prompt": "inspect"
            })),
        );
        persist_partitioned(&attempt, vec![launch]);
        std::fs::write(
            attempt.join(".acp-agent-result-migration-v1").as_std_path(),
            b"{\"version\":1}",
        )
        .unwrap();

        let branch_id = stable_agent_execution_id("session-1", "provider-child");
        let mut invalid_result = event_at(
            "agent-result-legacy",
            2,
            "textDelta",
            None,
            Some("completed"),
            json!({}),
            None,
        );
        invalid_result.content = Some("Async agent launched successfully.".to_string());
        invalid_result.raw = Some(json!({ "source": "agentBranchResult", "_meta": {} }));
        annotate_event_branch_override(&mut invalid_result, &branch_id);
        write_timeline_items(
            &branch_timeline_path(&attempt, &branch_id),
            &[invalid_result],
        )
        .unwrap();

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(records[0].status, "queued");
        let branch = load_timeline_items(&branch_timeline_path(&attempt, &branch_id)).unwrap();
        assert!(!branch.iter().any(is_agent_result_event));
        assert!(branch.iter().any(is_agent_prompt_event));
        assert!(attempt.join(AGENT_RESULT_MIGRATION_MARKER).exists());
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn foreground_agent_result_is_formal_completion_evidence() {
        let attempt = temp_attempt("foreground-result");
        let launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true, "toolOutput": "verified final answer" }),
            Some(json!({ "description": "child" })),
        );
        persist_partitioned(&attempt, vec![launch]);

        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(records[0].status, "completed");
        assert_eq!(
            rebuild_agent_index(&attempt, "stopped").unwrap()[0].status,
            "completed"
        );
        let branch = load_timeline_items(&branch_timeline_path(
            &attempt,
            &records[0].agent_execution_id,
        ))
        .unwrap();
        let result = branch
            .iter()
            .find(|event| is_agent_result_event(event))
            .unwrap();
        assert_eq!(result.content.as_deref(), Some("verified final answer"));
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn metrics_and_todos_are_owned_by_branch_and_deduplicated() {
        let attempt = temp_attempt("metrics-todos");
        let launch = event_at(
            "launch",
            1,
            "toolCall",
            Some("provider-child"),
            Some("completed"),
            json!({ "agentLaunch": true }),
            Some(json!({ "run_in_background": true })),
        );
        let read_start = event_at(
            "read-start",
            2,
            "toolCall",
            Some("read-1"),
            Some("running"),
            json!({ "parentToolCallId": "provider-child", "toolName": "Read" }),
            Some(json!({ "file_path": "SRC/lib.rs" })),
        );
        let read_end = event_at(
            "read-end",
            3,
            "toolCallUpdate",
            Some("read-1"),
            Some("completed"),
            json!({ "parentToolCallId": "provider-child", "toolName": "Read" }),
            Some(json!({ "file_path": "src\\lib.rs" })),
        );
        let write = event_at(
            "write",
            4,
            "toolCall",
            Some("write-1"),
            Some("completed"),
            json!({ "parentToolCallId": "provider-child", "toolName": "Write" }),
            Some(json!({ "file_path": "src/out.rs" })),
        );
        let mut plan = event_at(
            "plan",
            5,
            "plan",
            None,
            None,
            json!({ "parentToolCallId": "provider-child" }),
            None,
        );
        plan.raw.as_mut().unwrap()["entries"] = json!([
            { "content": "child only", "status": "in_progress" }
        ]);
        persist_partitioned(&attempt, vec![launch, read_start, read_end, write, plan]);
        let record = rebuild_agent_index(&attempt, "running").unwrap().remove(0);
        assert_eq!(record.tool_call_count, 2);
        assert_eq!(record.read_file_count, 1);
        assert_eq!(record.written_file_count, 1);
        assert_eq!(
            record.todo_entries,
            vec![json!({ "content": "child only", "status": "in_progress" })]
        );
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn index_and_snapshot_restore_without_provider_ids_in_directory_names() {
        let attempt = temp_attempt("index-restore");
        persist_partitioned(
            &attempt,
            vec![event_at(
                "launch",
                1,
                "toolCall",
                Some("provider/use:unsafe"),
                Some("pending"),
                json!({ "agentLaunch": true }),
                Some(json!({ "description": "child" })),
            )],
        );
        let first = rebuild_agent_index(&attempt, "running").unwrap();
        let second = rebuild_agent_index(&attempt, "running").unwrap();
        assert_eq!(first, second);
        let agent_id = &first[0].agent_execution_id;
        assert!(branch_snapshot_path(&attempt, agent_id).exists());
        assert!(attempt.join("acp.agents.jsonl").exists());
        assert!(!agent_id.contains("provider/use"));
        assert!(!attempt.join("agents").join("provider").exists());
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }

    #[test]
    fn unchanged_agent_index_and_snapshot_are_not_rewritten() {
        let attempt = temp_attempt("index-write-dedup");
        persist_partitioned(
            &attempt,
            vec![event_at(
                "launch",
                1,
                "toolCall",
                Some("provider-child"),
                Some("pending"),
                json!({ "agentLaunch": true }),
                Some(json!({ "description": "child" })),
            )],
        );
        let records = rebuild_agent_index(&attempt, "running").unwrap();
        assert!(!write_agent_index(&attempt, &records).unwrap());
        let snapshot = branch_snapshot_path(&attempt, &records[0].agent_execution_id);
        assert!(!write_agent_snapshot_if_changed(&snapshot, &records[0]).unwrap());

        let mut changed = records.clone();
        changed[0].title = Some("updated".to_string());
        assert!(write_agent_index(&attempt, &changed).unwrap());
        assert!(write_agent_snapshot_if_changed(&snapshot, &changed[0]).unwrap());
        std::fs::remove_dir_all(attempt.as_std_path()).unwrap();
    }
}
