use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use anyhow::Result;
use atomic_write_file::AtomicWriteFile;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::acp::events::{
    AcpUiEvent, AgentTranscriptRelation, extract_agent_transcript_relation, load_timeline_items,
    write_timeline_items,
};
use crate::storage::{ensure_parent_dir, write_json};

pub const ROOT_BRANCH_ID: &str = "root";
const BRANCH_META_KEY: &str = "goldBandConversation";
const AGENT_NAMESPACE: Uuid = Uuid::from_u128(0x63c7f8ac_1498_4f6e_8f6d_62f2f04033f1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationBranchRoute {
    pub branch_id: String,
    pub launched_agent_execution_id: Option<String>,
    pub tool_name: Option<String>,
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
    format!("agent-{}", Uuid::new_v5(&AGENT_NAMESPACE, name.as_bytes()).simple())
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
    ConversationBranchRoute {
        branch_id: parent_agent_execution_id
            .clone()
            .unwrap_or_else(|| ROOT_BRANCH_ID.to_string()),
        launched_agent_execution_id,
        tool_name: relation.and_then(|relation| relation.tool_name),
    }
}

pub fn annotate_event_branch(event: &mut AcpUiEvent) -> ConversationBranchRoute {
    let route = branch_route_for_event(event);
    let normalized_tool_output = event
        .raw
        .as_ref()
        .and_then(provider_tool_output)
        .cloned();
    let raw = event.raw.get_or_insert_with(|| json!({}));
    if !raw.is_object() {
        *raw = json!({ "providerPayload": raw.clone() });
    }
    let object = raw.as_object_mut().expect("normalized branch raw must be an object");
    let meta = object.entry("_meta").or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    meta.as_object_mut().expect("normalized branch meta must be an object").insert(
        BRANCH_META_KEY.to_string(),
        json!({
            "branchId": route.branch_id,
            "launchedAgentExecutionId": route.launched_agent_execution_id,
            "toolName": route.tool_name,
            "toolOutput": normalized_tool_output,
        }),
    );
    route
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
        timestamp: launch.started_at.clone().unwrap_or_else(|| launch.timestamp.clone()),
        kind: "userTextDelta".to_string(),
        session_id: launch.session_id.clone(),
        content: Some(prompt.to_string()),
        title: Some("Agent prompt".to_string()),
        tool_call_id: None,
        status: Some("completed".to_string()),
        started_seq: Some(launch.started_seq.unwrap_or(launch.seq)),
        ended_seq: Some(launch.started_seq.unwrap_or(launch.seq)),
        started_at: Some(launch.started_at.clone().unwrap_or_else(|| launch.timestamp.clone())),
        ended_at: Some(launch.started_at.clone().unwrap_or_else(|| launch.timestamp.clone())),
        timing: None,
        raw: Some(json!({
            "source": "agentBranchPrompt",
            "_meta": {}
        })),
    };
    annotate_event_branch_override(&mut event, &agent_execution_id);
    Some(event)
}

fn annotate_event_branch_override(event: &mut AcpUiEvent, branch_id: &str) {
    let raw = event.raw.get_or_insert_with(|| json!({}));
    let meta = raw.as_object_mut().unwrap().entry("_meta").or_insert_with(|| json!({}));
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
        attempt_dir.join("agents").join(branch_id).join("timeline.jsonl")
    }
}

pub fn branch_snapshot_path(attempt_dir: &Utf8Path, branch_id: &str) -> Utf8PathBuf {
    attempt_dir.join("agents").join(branch_id).join("snapshot.json")
}

pub fn migrate_legacy_agent_timeline(attempt_dir: &Utf8Path) -> Result<bool> {
    let root_path = branch_timeline_path(attempt_dir, ROOT_BRANCH_ID);
    let mut events = load_timeline_items(&root_path)?;
    if !events.iter().any(|event| branch_route_for_event(event).branch_id != ROOT_BRANCH_ID) {
        return Ok(false);
    }
    let mut by_branch = BTreeMap::<String, Vec<AcpUiEvent>>::new();
    for mut event in events.drain(..) {
        let route = annotate_event_branch(&mut event);
        let prompt = agent_prompt_event(&event);
        by_branch.entry(route.branch_id).or_default().push(event);
        if let Some(prompt) = prompt {
            by_branch.entry(event_branch_id(&prompt)).or_default().push(prompt);
        }
    }
    write_timeline_items(
        &root_path,
        by_branch.remove(ROOT_BRANCH_ID).as_deref().unwrap_or_default(),
    )?;
    for (branch_id, branch_events) in by_branch {
        write_timeline_items(&branch_timeline_path(attempt_dir, &branch_id), &branch_events)?;
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

pub fn rebuild_agent_index(attempt_dir: &Utf8Path, session_status: &str) -> Result<Vec<AgentExecutionRecord>> {
    let session_active = matches!(session_status, "running" | "active" | "starting" | "cancelling" | "stopping");
    let session_interrupted = matches!(session_status, "cancelled" | "canceled" | "interrupted" | "stopped");
    migrate_legacy_agent_timeline(attempt_dir)?;
    let all_events = load_all_branch_events(attempt_dir)?;
    let mut launches = HashMap::<String, AcpUiEvent>::new();
    for event in &all_events {
        let Some(relation) = agent_relation(event) else { continue };
        if !relation.agent_launch {
            continue;
        }
        let Some(tool_call_id) = event.tool_call_id.as_ref() else { continue };
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
            let session_id = launch.session_id.clone().unwrap_or_else(|| "unknown-session".to_string());
            let agent_execution_id = stable_agent_execution_id(&session_id, &launch_tool_call_id);
            let relation = agent_relation(&launch).unwrap_or_default();
            let parent_agent_execution_id = relation.parent_tool_call_id.as_deref()
                .map(|tool_call_id| stable_agent_execution_id(&session_id, tool_call_id));
            let branch_events = load_timeline_items(&branch_timeline_path(attempt_dir, &agent_execution_id))
                .unwrap_or_default();
            let execution_events = branch_events
                .iter()
                .filter(|event| !is_agent_prompt_event(event))
                .collect::<Vec<_>>();
            let metrics = branch_metrics(&branch_events);
            let latest_seq = execution_events.iter().map(|event| event.ended_seq.unwrap_or(event.seq)).max();
            let latest_timestamp = execution_events.iter().max_by_key(|event| event.ended_seq.unwrap_or(event.seq))
                .map(|event| event.ended_at.clone().unwrap_or_else(|| event.timestamp.clone()));
            let launch_position = launch.ended_seq.unwrap_or(launch.seq);
            let launch_status = launch.status.as_deref().unwrap_or_default().to_ascii_lowercase();
            let failed = matches!(launch_status.as_str(), "failed" | "error");
            let has_attention = branch_events.iter().any(|event| {
                matches!(event.kind.as_str(), "permissionRequest" | "elicitationRequest")
                    && event.status.as_deref().unwrap_or("pending").eq_ignore_ascii_case("pending")
            });
            let status = if failed {
                "failed"
            } else if has_attention && session_active {
                "waiting_permission"
            } else if !session_active {
                if session_interrupted {
                    "interrupted"
                } else if has_agent_completion_evidence(&execution_events)
                    || matches!(launch_status.as_str(), "completed" | "success" | "succeeded")
                {
                    "completed"
                } else {
                    "interrupted"
                }
            } else if execution_events.is_empty() {
                "queued"
            } else if has_agent_completion_evidence(&execution_events) {
                "completed"
            } else if !agent_launch_runs_in_background(&launch)
                && matches!(launch_status.as_str(), "completed" | "success" | "succeeded")
                && latest_seq.is_some_and(|seq| seq <= launch_position)
            {
                "completed"
            } else {
                "running"
            }.to_string();
            let input = launch.raw.as_ref().and_then(tool_raw_input);
            let title = launch.title.clone();
            let description = input.and_then(|input| input.get("description")).and_then(Value::as_str).map(str::to_string);
            let ended_at = matches!(status.as_str(), "completed" | "failed" | "interrupted")
                .then(|| latest_timestamp.unwrap_or_else(|| launch.ended_at.clone().unwrap_or_else(|| launch.timestamp.clone())));
            AgentExecutionRecord {
                agent_execution_id,
                parent_agent_execution_id,
                launch_tool_call_id,
                session_id,
                status,
                title,
                description,
                started_at: launch.started_at.clone().unwrap_or_else(|| launch.timestamp.clone()),
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
    write_agent_index(attempt_dir, &records)?;
    for record in &records {
        write_json(
            &branch_snapshot_path(attempt_dir, &record.agent_execution_id),
            record,
        )?;
    }
    Ok(records)
}

fn write_agent_index(attempt_dir: &Utf8Path, records: &[AgentExecutionRecord]) -> Result<()> {
    let path = attempt_dir.join("acp.agents.jsonl");
    ensure_parent_dir(&path)?;
    let mut file = AtomicWriteFile::open(path.as_std_path())?;
    for record in records {
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
    }
    file.commit()?;
    Ok(())
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
        if relation.as_ref().is_some_and(|relation| relation.agent_launch) {
            continue;
        }
        let key = event.tool_call_id.clone().unwrap_or_else(|| event.id.clone());
        let should_replace = latest_tools
            .get(&key)
            .is_none_or(|current| event.ended_seq.unwrap_or(event.seq) >= current.ended_seq.unwrap_or(current.seq));
        if should_replace {
            latest_tools.insert(key, event);
        }
    }
    let mut read_files = std::collections::BTreeSet::<String>::new();
    let mut written_files = std::collections::BTreeSet::<String>::new();
    for event in latest_tools.values() {
        let relation = agent_relation(event);
        let tool_name = relation.and_then(|relation| relation.tool_name)
            .or_else(|| event.title.clone()).unwrap_or_default().to_ascii_lowercase();
        let paths = structured_tool_paths(event);
        if matches!(tool_name.as_str(), "read" | "get-content" | "read_file") {
            read_files.extend(paths);
        } else if matches!(tool_name.as_str(), "write" | "edit" | "applypatch" | "apply_patch" | "set-content" | "write_file") {
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

fn has_agent_completion_evidence(events: &[&AcpUiEvent]) -> bool {
    let Some(latest) = events
        .iter()
        .max_by_key(|event| event.ended_seq.unwrap_or(event.seq))
    else {
        return false;
    };
    latest.kind == "textDelta"
        && latest.content.as_deref().is_some_and(|content| !content.trim().is_empty())
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
        .and_then(|raw| raw.pointer("/toolCall/locations").or_else(|| raw.get("locations")))
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
    events.iter()
        .filter(|event| event.kind == "plan")
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

pub fn provider_tool_output(raw: &Value) -> Option<&Value> {
    raw.pointer("/_meta/claudeCode/toolResponse/content")
        .or_else(|| raw.pointer("/toolCall/_meta/claudeCode/toolResponse/content"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, tool_call_id: Option<&str>, relation: Value) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq: 1,
            timestamp: "1Z".to_string(),
            kind: "toolCall".to_string(),
            session_id: Some("session-1".to_string()),
            content: None,
            title: None,
            tool_call_id: tool_call_id.map(str::to_string),
            status: Some("pending".to_string()),
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: Some(json!({ "_meta": { "agentTranscript": relation } })),
        }
    }

    #[test]
    fn stable_agent_ids_do_not_expose_provider_tool_ids() {
        let id = stable_agent_execution_id("session-1", "tool/use:unsafe");
        assert!(id.starts_with("agent-"));
        assert!(!id.contains("tool/use"));
        assert_eq!(id, stable_agent_execution_id("session-1", "tool/use:unsafe"));
    }

    #[test]
    fn launch_is_stored_in_parent_and_children_in_launched_branch() {
        let launch = event("launch", Some("child-tool"), json!({ "agentLaunch": true }));
        assert_eq!(branch_route_for_event(&launch).branch_id, ROOT_BRANCH_ID);
        let child = event("read", Some("read-1"), json!({ "parentToolCallId": "child-tool" }));
        let route = branch_route_for_event(&child);
        assert_eq!(route.branch_id, stable_agent_execution_id("session-1", "child-tool"));
    }
}
