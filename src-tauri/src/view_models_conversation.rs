use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::view_models::{
    AssetItemVm, GraphVm, RuntimeDisplayVm, acp_session_vm, dynamic_acp_session_vm,
    round_detail_vm, runtime_display_vm, workflow_graph_vm,
};
use gold_band::app::App;
use gold_band::app::CreateTaskInput;
use gold_band::app::is_run_continuable;
use gold_band::config::StateConfig;
use gold_band::domain::NodeType;
use gold_band::domain::RunStatus;
use gold_band::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, DynamicAgentRef, DynamicControlDsl, END_NODE, EdgeDsl,
    EdgeOutcome, NodeDsl, WorkflowDsl,
};
use gold_band::dynamic::DynamicGraphState;
use gold_band::storage::{read_json, write_json};

// ── Conversation View Models ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorkspaceVm {
    pub project_id: String,
    pub workspace_path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSidebarVm {
    pub workspaces: Vec<ConversationWorkspaceVm>,
    pub pinned_tasks: Vec<ConversationTaskRowVm>,
    pub tasks_by_workspace: std::collections::HashMap<String, Vec<ConversationTaskRowVm>>,
    pub last_active_workspace_id: Option<String>,
    pub preferences: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTaskRowVm {
    pub project_id: String,
    pub task_id: String,
    pub title: String,
    pub auto_title: bool,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub latest_run: Option<ConversationRunSummaryVm>,
    pub runs: Vec<ConversationRunSummaryVm>,
    pub pinned: bool,
    pub pinned_order: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunSummaryVm {
    pub run_id: String,
    pub status: String,
    pub outcome: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub current_round: Option<String>,
    pub current_node: Option<String>,
    pub resumable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunVm {
    pub project_id: String,
    pub task_id: String,
    pub run_id: String,
    pub title: String,
    pub auto_title: bool,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub run_status: String,
    pub run_outcome: Option<String>,
    pub session_tree: ConversationSessionTreeVm,
    pub selected_session: Option<crate::view_models::AcpSessionVm>,
    pub active_sessions: Vec<ConversationActiveSessionVm>,
    pub artifacts: Vec<crate::view_models::AssetItemVm>,
    pub attachments: Vec<crate::view_models::AssetItemVm>,
    pub input_attachments: Vec<crate::view_models::AssetItemVm>,
    pub workflow_status: String,
    pub workflow_valid: bool,
    pub workflow_error: Option<crate::view_models::WorkflowErrorVm>,
    pub workflow_json: Option<String>,
    pub workflow_graph: GraphVm,
    pub resumable: bool,
    pub pause_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionSwitchVm {
    pub selected_session: Option<crate::view_models::AcpSessionVm>,
    pub artifacts: Vec<crate::view_models::AssetItemVm>,
    pub attachments: Vec<crate::view_models::AssetItemVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionTreeVm {
    pub rounds: Vec<ConversationRoundNodeVm>,
    pub selected_session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRoundNodeVm {
    pub round_id: String,
    pub index: u32,
    pub label: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub nodes: Vec<ConversationTreeNodeVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTreeNodeVm {
    pub node_id: String,
    pub label: String,
    pub node_type: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub attempts: Vec<ConversationSessionLeafVm>,
    pub outer_nodes: Option<Vec<ConversationTreeNodeVm>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSessionLeafVm {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub path_label: String,
    pub status: String,
    pub outcome: Option<String>,
    pub runtime_display: RuntimeDisplayVm,
    pub lifecycle: ConversationAttemptLifecycleVm,
    pub current: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub session_id: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAttemptLifecycleVm {
    pub runtime: ConversationRuntimeFacetVm,
    pub acp: ConversationAcpFacetVm,
    pub display_status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub continue_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRuntimeFacetVm {
    pub status: String,
    pub outcome: Option<String>,
    pub pause_reason: Option<String>,
    pub resumable: bool,
    pub current: bool,
    pub active: bool,
    pub continuable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAcpFacetVm {
    pub status: Option<String>,
    pub active: bool,
    pub stopping: bool,
    pub terminal: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationActiveSessionVm {
    pub round_id: String,
    pub node_id: String,
    pub attempt_id: String,
    pub outer_node_id: Option<String>,
    pub outer_attempt_id: Option<String>,
    pub path_label: String,
    pub status: String,
    pub runtime_display: RuntimeDisplayVm,
    pub lifecycle: ConversationAttemptLifecycleVm,
    pub session_id: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunModeVm {
    pub mode: String,
    pub workflow_template_id: Option<String>,
    pub auto_config: Option<ConversationAutoConfigVm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAutoConfigVm {
    pub agent_strategy: Option<String>,
    pub agent_type: String,
    pub bootstrap_agent_type: Option<String>,
    pub bootstrap_model_id: Option<String>,
    pub acceptance_model_id: Option<String>,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    pub available_agents: Option<Vec<ConversationDynamicAgentRefVm>>,
    pub routing_prompt: Option<String>,
    pub allowed_workflows: Option<Vec<ConversationAllowedWorkflowRefVm>>,
    pub allowed_profiles: Option<Vec<String>>,
    pub global_goal: Option<String>,
    pub control: Option<ConversationDynamicControlVm>,
    pub active_template_id: Option<String>,
    pub active_template_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicAgentRefVm {
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAllowedWorkflowRefVm {
    pub workflow_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDynamicControlVm {
    pub max_dynamic_nodes: u32,
    pub max_fanout: u32,
    pub max_depth: u32,
    pub max_parallel: u32,
    pub max_group_depth: u32,
    pub max_workflow_invocations: u32,
    pub allow_nested_dynamic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCreateInputVm {
    pub project_id: String,
    pub content: String,
    pub run_mode: String,
    pub workflow_template_id: Option<String>,
    pub auto_config: Option<ConversationAutoConfigVm>,
    pub attachment_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationValidationResultVm {
    pub valid: bool,
    pub missing_items: Vec<ConversationMissingItemVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMissingItemVm {
    pub code: String,
    pub label: String,
    pub recovery_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSearchResultVm {
    pub project_id: String,
    pub workspace_path: String,
    pub workspace_name: String,
    pub task_id: String,
    pub title: String,
    pub description: Option<String>,
    pub requirement_preview: String,
    pub latest_run: Option<ConversationRunSummaryVm>,
}

// ── Builder functions (stubs — full implementation in later phases) ──

pub fn conversation_sidebar_vm(app: &App, state: &StateConfig) -> ConversationSidebarVm {
    // Build workspaces: always include the default (current repo) workspace,
    // then merge stored workspaces, deduplicating by project_id.
    let default_repo = app.paths.repo_root.to_string();
    let default_name = std::path::Path::new(&default_repo)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Workspace".to_string());
    let default_project_id = default_repo
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-");

    let mut workspaces: Vec<ConversationWorkspaceVm> = vec![ConversationWorkspaceVm {
        project_id: default_project_id.clone(),
        workspace_path: default_repo,
        name: default_name,
    }];

    for w in &state.conversation_workspaces {
        if w.project_id != default_project_id {
            workspaces.push(ConversationWorkspaceVm {
                project_id: w.project_id.clone(),
                workspace_path: w.workspace_path.clone(),
                name: w.name.clone(),
            });
        }
    }

    let default_project_id = workspaces
        .first()
        .map(|w| w.project_id.clone())
        .unwrap_or_default();

    // Read real tasks from the app
    let mut pinned_tasks: Vec<ConversationTaskRowVm> = Vec::new();
    let mut tasks_by_workspace: HashMap<String, Vec<ConversationTaskRowVm>> = HashMap::new();
    let pinned_set: std::collections::HashSet<(String, String)> = state
        .conversation_pins
        .iter()
        .map(|p| (p.project_id.clone(), p.task_id.clone()))
        .collect();

    // Initialize empty task lists for each workspace
    for ws in &workspaces {
        tasks_by_workspace.entry(ws.project_id.clone()).or_default();
    }

    // Read task summaries from the app
    if let Ok(summaries) = app.task_summaries() {
        for summary in &summaries {
            let task_id = &summary.task.id;
            let project_id = &default_project_id; // Single workspace for now
            let pinned = pinned_set.contains(&(project_id.clone(), task_id.clone()));
            let pin_order = state
                .conversation_pins
                .iter()
                .find(|p| p.project_id == *project_id && p.task_id == *task_id)
                .map(|p| p.order);

            // Read conversation metadata if exists
            let conversation_json_path = app
                .paths
                .tasks_dir()
                .join(task_id)
                .join("authoring")
                .join("conversation.json");
            let run_mode = if conversation_json_path.exists() {
                gold_band::storage::read_json::<serde_json::Value>(&conversation_json_path)
                    .ok()
                    .and_then(|v| v.get("runMode").and_then(|m| m.as_str().map(String::from)))
                    .unwrap_or_else(|| "workflow".to_string())
            } else {
                "workflow".to_string()
            };

            let latest_run = summary.latest_run.as_ref().map(|run| {
                let resumable = is_run_continuable(run);
                ConversationRunSummaryVm {
                    run_id: run.id.clone(),
                    status: enum_label(&run.status),
                    outcome: run.outcome.map(|o| enum_label(&o)),
                    started_at: run.started_at.clone(),
                    updated_at: run.updated_at.clone(),
                    current_round: run.current_round.clone(),
                    current_node: run.current_node.clone(),
                    resumable,
                }
            });

            // Load all runs for this task (for expandable run list in sidebar)
            let runs: Vec<ConversationRunSummaryVm> = app
                .run_list(task_id)
                .map(|run_list| {
                    let mut vms: Vec<ConversationRunSummaryVm> = run_list
                        .iter()
                        .map(|run| {
                            let resumable = is_run_continuable(run);
                            ConversationRunSummaryVm {
                                run_id: run.id.clone(),
                                status: enum_label(&run.status),
                                outcome: run.outcome.map(|o| enum_label(&o)),
                                started_at: run.started_at.clone(),
                                updated_at: run.updated_at.clone(),
                                current_round: run.current_round.clone(),
                                current_node: run.current_node.clone(),
                                resumable,
                            }
                        })
                        .collect();
                    // Sort newest first
                    vms.sort_by(|a, b| b.started_at.cmp(&a.started_at));
                    vms
                })
                .unwrap_or_default();

            let row = ConversationTaskRowVm {
                project_id: project_id.clone(),
                task_id: task_id.clone(),
                title: summary
                    .task
                    .title
                    .clone()
                    .unwrap_or_else(|| task_id.clone()),
                auto_title: conversation_json_path.exists(),
                run_mode,
                workflow_template_id: None,
                latest_run,
                runs,
                pinned,
                pinned_order: pin_order,
            };

            if pinned {
                pinned_tasks.push(row.clone());
            }
            tasks_by_workspace
                .entry(project_id.clone())
                .or_default()
                .push(row);
        }
    }

    // Sort: pinned by order, others by latest run time (newest first)
    pinned_tasks.sort_by_key(|t| t.pinned_order.unwrap_or(usize::MAX));
    for tasks in tasks_by_workspace.values_mut() {
        tasks.sort_by(|a, b| {
            let a_time = a
                .latest_run
                .as_ref()
                .map(|r| r.started_at.as_str())
                .unwrap_or("");
            let b_time = b
                .latest_run
                .as_ref()
                .map(|r| r.started_at.as_str())
                .unwrap_or("");
            b_time.cmp(&a_time) // newest first
        });
    }

    let last_active_workspace_id = state
        .last_conversation_workspace
        .clone()
        .or_else(|| workspaces.first().map(|w| w.project_id.clone()));

    ConversationSidebarVm {
        workspaces,
        pinned_tasks,
        tasks_by_workspace,
        last_active_workspace_id,
        preferences: app.load_state().map(|s| s.preferences).unwrap_or_default(),
    }
}

fn enum_label<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(label)) => label,
        Ok(value) => value.to_string(),
        Err(_) => "unknown".to_string(),
    }
}

fn display_pause_reason_for_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    run_pause_reason: Option<&str>,
) -> Option<String> {
    if run_pause_reason == Some("error-blocked") {
        let snapshot_path = app
            .paths
            .acp_snapshot_file(task_id, run_id, round_id, node_id, attempt_id);
        let session_path = app
            .paths
            .acp_session_file(task_id, run_id, round_id, node_id, attempt_id);
        if acp_session_file_is_cancelled(&snapshot_path)
            || acp_session_file_is_cancelled(&session_path)
        {
            return Some("process-interrupted".to_string());
        }
    }
    run_pause_reason.map(str::to_string)
}

fn display_pause_reason_for_dynamic_attempt(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    outer_node_id: &str,
    outer_attempt_id: &str,
    node_id: &str,
    attempt_id: &str,
    run_pause_reason: Option<&str>,
) -> Option<String> {
    if run_pause_reason == Some("error-blocked") {
        let attempt_dir = app.paths.dynamic_node_attempt_dir(
            task_id,
            run_id,
            round_id,
            outer_node_id,
            outer_attempt_id,
            node_id,
            attempt_id,
        );
        if acp_session_file_is_cancelled(&attempt_dir.join("acp.snapshot.json"))
            || acp_session_file_is_cancelled(&attempt_dir.join("acp.session.json"))
        {
            return Some("process-interrupted".to_string());
        }
    }
    run_pause_reason.map(str::to_string)
}

fn acp_session_file_is_cancelled(path: &camino::Utf8Path) -> bool {
    read_json::<serde_json::Value>(path)
        .ok()
        .and_then(|session| {
            let status = session
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let stop_reason = session
                .get("stopReason")
                .or_else(|| session.get("stop_reason"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            (status.eq_ignore_ascii_case("cancelled")
                || status.eq_ignore_ascii_case("canceled")
                || stop_reason.eq_ignore_ascii_case("cancelled")
                || stop_reason.eq_ignore_ascii_case("canceled"))
            .then_some(())
        })
        .is_some()
}

fn asset_item_vm(
    kind: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    name: String,
) -> AssetItemVm {
    AssetItemVm {
        kind: kind.to_string(),
        title: name.clone(),
        preview: name.clone(),
        tone: if kind == "artifact" {
            "accent"
        } else {
            "neutral"
        }
        .to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        name,
    }
}

fn list_file_names_from_dir(
    dir: &camino::Utf8Path,
    logical_json_name: bool,
) -> anyhow::Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = std::fs::read_dir(dir.as_std_path())?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ty| ty.is_file()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|name| {
            if logical_json_name {
                name.strip_suffix(".json").unwrap_or(&name).to_string()
            } else {
                name
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

fn conversation_session_assets(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> anyhow::Result<(Vec<AssetItemVm>, Vec<AssetItemVm>)> {
    let (artifact_names, attachment_names) =
        if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id, outer_attempt_id) {
            let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                attempt_id,
            );
            let attachments_dir = app.paths.dynamic_node_attachments_dir(
                task_id,
                run_id,
                round_id,
                outer_node_id,
                outer_attempt_id,
                node_id,
                attempt_id,
            );
            (
                list_file_names_from_dir(&artifacts_dir, true)?,
                list_file_names_from_dir(&attachments_dir, false)?,
            )
        } else {
            (
                app.artifact_list(task_id, run_id, round_id, node_id, attempt_id)?,
                app.attachment_list(task_id, run_id, round_id, node_id, attempt_id)?,
            )
        };

    let artifacts = artifact_names
        .into_iter()
        .map(|name| asset_item_vm("artifact", round_id, node_id, attempt_id, name))
        .collect::<Vec<_>>();
    let attachments = attachment_names
        .into_iter()
        .map(|name| asset_item_vm("attachment", round_id, node_id, attempt_id, name))
        .collect::<Vec<_>>();
    Ok((artifacts, attachments))
}

fn find_leaf_by_key(
    rounds: &[ConversationRoundNodeVm],
    key: &str,
) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            // Check top-level attempts
            for leaf in &node.attempts {
                if format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id) == key {
                    return Some(leaf.clone());
                }
                if leaf.outer_node_id.is_some() {
                    let outer_key = format!(
                        "{}/{}/{}/{}/{}",
                        leaf.round_id,
                        leaf.outer_node_id.as_deref().unwrap_or(""),
                        leaf.outer_attempt_id.as_deref().unwrap_or(""),
                        leaf.node_id,
                        leaf.attempt_id,
                    );
                    if outer_key == key {
                        return Some(leaf.clone());
                    }
                }
            }
            // Check dynamic child nodes
            if let Some(ref outer_nodes) = node.outer_nodes {
                for on in outer_nodes {
                    for leaf in &on.attempts {
                        if let (Some(outer_id), Some(outer_attempt)) = (
                            leaf.outer_node_id.as_deref(),
                            leaf.outer_attempt_id.as_deref(),
                        ) {
                            let dyn_key = format!(
                                "{}/{}/{}/{}/{}",
                                leaf.round_id,
                                outer_id,
                                outer_attempt,
                                leaf.node_id,
                                leaf.attempt_id,
                            );
                            if dyn_key == key {
                                return Some(leaf.clone());
                            }
                        }
                        if format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id) == key
                        {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn latest_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    let mut latest: Option<ConversationSessionLeafVm> = None;
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if is_leaf_newer(leaf, latest.as_ref()) {
                    latest = Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if is_leaf_newer(leaf, latest.as_ref()) {
                            latest = Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    latest
}

fn current_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if leaf.current {
                    return Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if leaf.current {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn active_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    for round in rounds {
        for node in &round.nodes {
            for leaf in &node.attempts {
                if is_active_session_status(&leaf.status) {
                    return Some(leaf.clone());
                }
            }
            if let Some(ref outer_nodes) = node.outer_nodes {
                for outer_node in outer_nodes {
                    for leaf in &outer_node.attempts {
                        if is_active_session_status(&leaf.status) {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

fn default_session_leaf(rounds: &[ConversationRoundNodeVm]) -> Option<ConversationSessionLeafVm> {
    if let Some(leaf) = current_session_leaf(rounds) {
        return Some(leaf);
    }
    if let Some(leaf) = active_session_leaf(rounds) {
        return Some(leaf);
    }
    latest_session_leaf(rounds)
}

fn normalize_lifecycle_code(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn is_active_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "pending"
            | "running"
            | "in-progress"
            | "active"
            | "sending"
            | "cancelling"
            | "cancel-requested"
    )
}

fn is_stopping_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "cancelling" | "cancel-requested"
    )
}

fn is_terminal_session_status(status: &str) -> bool {
    matches!(
        normalize_lifecycle_code(status).as_str(),
        "completed"
            | "complete"
            | "cancelled"
            | "canceled"
            | "failed"
            | "failure"
            | "error"
            | "killed"
    )
}

fn is_runtime_continue_pause_reason(pause_reason: Option<&str>) -> bool {
    matches!(
        pause_reason.map(normalize_lifecycle_code).as_deref(),
        Some("process-interrupted") | Some("waiting-for-user-input")
    )
}

fn runtime_continue_kind(
    runtime_status: &str,
    pause_reason: Option<&str>,
    run_resumable: bool,
) -> Option<String> {
    if !run_resumable || normalize_lifecycle_code(runtime_status) != "paused" {
        return None;
    }
    match pause_reason.map(normalize_lifecycle_code).as_deref() {
        Some("process-interrupted") => Some("input".to_string()),
        Some("waiting-for-user-input") => Some("action".to_string()),
        _ => None,
    }
}

fn derive_conversation_attempt_lifecycle(
    session_status: Option<&str>,
    runtime_status: &str,
    runtime_outcome: Option<&str>,
    current: bool,
    pause_reason: Option<&str>,
    run_resumable: bool,
) -> ConversationAttemptLifecycleVm {
    let session_status = session_status
        .map(str::trim)
        .filter(|status| !status.is_empty() && !status.eq_ignore_ascii_case("unknown"))
        .map(str::to_string);
    let normalized_runtime_status = normalize_lifecycle_code(runtime_status);
    let runtime_active = is_active_session_status(runtime_status);
    let acp_stopping = session_status
        .as_deref()
        .is_some_and(is_stopping_session_status);
    let runtime_terminal = !runtime_active
        && !acp_stopping
        && matches!(
            normalized_runtime_status.as_str(),
            "completed"
                | "complete"
                | "failed"
                | "failure"
                | "error"
                | "killed"
                | "cancelled"
                | "canceled"
        );
    let suppress_stale_acp_active = runtime_terminal && !run_resumable;
    let acp_active = !suppress_stale_acp_active
        && session_status
            .as_deref()
            .is_some_and(is_active_session_status);
    let acp_terminal = suppress_stale_acp_active
        || session_status
            .as_deref()
            .is_some_and(is_terminal_session_status);
    let runtime_continue_pause = run_resumable
        && normalize_lifecycle_code(runtime_status) == "paused"
        && (is_runtime_continue_pause_reason(pause_reason)
            || matches!(
                pause_reason.map(normalize_lifecycle_code).as_deref(),
                Some("error-blocked")
            ));

    let display_status = if acp_stopping {
        session_status
            .clone()
            .unwrap_or_else(|| "cancelling".to_string())
    } else if runtime_active || suppress_stale_acp_active {
        runtime_status.to_string()
    } else if acp_active {
        session_status
            .clone()
            .unwrap_or_else(|| runtime_status.to_string())
    } else if runtime_continue_pause {
        runtime_status.to_string()
    } else {
        session_status
            .clone()
            .unwrap_or_else(|| runtime_status.to_string())
    };
    let runtime_display = runtime_display_vm(
        Some(&display_status),
        runtime_outcome,
        current,
        pause_reason,
        run_resumable,
    );
    let continue_kind = runtime_continue_kind(runtime_status, pause_reason, run_resumable);

    ConversationAttemptLifecycleVm {
        runtime: ConversationRuntimeFacetVm {
            status: runtime_status.to_string(),
            outcome: runtime_outcome.map(str::to_string),
            pause_reason: pause_reason.map(str::to_string),
            resumable: run_resumable,
            current,
            active: runtime_active,
            continuable: continue_kind.is_some(),
        },
        acp: ConversationAcpFacetVm {
            status: session_status,
            active: acp_active,
            stopping: acp_stopping,
            terminal: acp_terminal,
        },
        display_status,
        runtime_display,
        continue_kind,
    }
}

fn conversation_status_from_session(
    session_status: Option<&str>,
    runtime_status: &str,
    run_pause_reason: Option<&str>,
    run_resumable: bool,
) -> String {
    derive_conversation_attempt_lifecycle(
        session_status,
        runtime_status,
        None,
        false,
        run_pause_reason,
        run_resumable,
    )
    .display_status
}

fn lifecycle_is_active(lifecycle: &ConversationAttemptLifecycleVm) -> bool {
    lifecycle.runtime.active || lifecycle.acp.active || lifecycle.acp.stopping
}

fn is_leaf_newer(
    candidate: &ConversationSessionLeafVm,
    current: Option<&ConversationSessionLeafVm>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    leaf_order_key(candidate) > leaf_order_key(current)
}

fn leaf_order_key(leaf: &ConversationSessionLeafVm) -> (&str, &str, &str, &str, &str) {
    (
        leaf.started_at
            .as_deref()
            .or(leaf.finished_at.as_deref())
            .unwrap_or(""),
        leaf.round_id.as_str(),
        leaf.outer_node_id.as_deref().unwrap_or(""),
        leaf.node_id.as_str(),
        leaf.attempt_id.as_str(),
    )
}

fn conversation_leaf_key(leaf: &ConversationSessionLeafVm) -> String {
    if leaf.outer_node_id.is_some() {
        format!(
            "{}/{}/{}/{}/{}",
            leaf.round_id,
            leaf.outer_node_id.as_deref().unwrap_or(""),
            leaf.outer_attempt_id.as_deref().unwrap_or(""),
            leaf.node_id,
            leaf.attempt_id
        )
    } else {
        format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id)
    }
}

pub fn conversation_run_vm(
    app: &App,
    project_id: &str,
    task_id: &str,
    run_id: &str,
    selected_session_key: Option<&str>,
) -> anyhow::Result<ConversationRunVm> {
    // Read the run state from disk
    let run = match app.run_status(task_id, run_id) {
        Ok(r) => r,
        Err(e) => {
            return Err(anyhow::anyhow!("run not found: {task_id}/{run_id}: {e}"));
        }
    };

    // Read the task state for title
    let task_state = app
        .task_show(task_id)
        .map_err(|e| anyhow::anyhow!("task not found: {task_id}: {e}"))?;
    let title = task_state.title.unwrap_or_else(|| task_id.to_string());

    // Read conversation metadata if exists
    let conversation_json_path = app
        .paths
        .task_dir(task_id)
        .join("authoring")
        .join("conversation.json");
    let (run_mode, auto_title) = if conversation_json_path.exists() {
        let mode = gold_band::storage::read_json::<serde_json::Value>(&conversation_json_path)
            .ok()
            .and_then(|v| v.get("runMode").and_then(|m| m.as_str().map(String::from)))
            .unwrap_or_else(|| "workflow".to_string());
        (mode, true)
    } else {
        ("workflow".to_string(), false)
    };

    // Build the session tree from rounds/nodes/attempts
    // Read workflow snapshot once for node order + validity + raw JSON
    let workflow_snapshot: Option<WorkflowDsl> = gold_band::storage::read_json::<WorkflowDsl>(
        &app.paths.workflow_snapshot_file(task_id, run_id),
    )
    .ok();
    let workflow_node_order: HashMap<String, usize> = workflow_snapshot
        .as_ref()
        .map(|dsl| {
            dsl.nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id().to_string(), i))
                .collect()
        })
        .unwrap_or_default();

    let rounds = app.round_list(task_id, run_id)?;
    let mut tree_rounds: Vec<ConversationRoundNodeVm> = Vec::new();
    let mut active_sessions: Vec<ConversationActiveSessionVm> = Vec::new();
    let run_pause_reason = run.pause_reason.as_ref().map(enum_label);
    let run_resumable = is_run_continuable(&run);

    for round in &rounds {
        // List all nodes for this round (latest attempt per node)
        let mut nodes = app.node_list(task_id, run_id, &round.id)?;
        // Sort by workflow DSL order so the session tree matches the intended workflow sequence
        nodes.sort_by_key(|n| {
            workflow_node_order
                .get(&n.node_id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        let mut tree_nodes: Vec<ConversationTreeNodeVm> = Vec::new();

        for node in &nodes {
            let is_ai_dynamic = node.node_type == NodeType::AiDynamic;
            let all_attempts = app.attempt_list(task_id, run_id, &round.id, &node.node_id)?;

            // Build child nodes for AI-DYNAMIC
            let mut outer_nodes: Option<Vec<ConversationTreeNodeVm>> = None;
            if is_ai_dynamic {
                if let Some(latest_attempt) = all_attempts.last() {
                    let dynamic_path = app.paths.dynamic_graph_file(
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &latest_attempt.attempt_id,
                    );
                    if let Ok(dynamic_graph) = read_json::<DynamicGraphState>(&dynamic_path) {
                        let mut dynamic_tree_nodes: Vec<ConversationTreeNodeVm> = Vec::new();
                        for dyn_node in &dynamic_graph.nodes {
                            // Find the latest attempt for this dynamic child node
                            let dyn_node_dir = app.paths.dynamic_node_dir(
                                task_id,
                                run_id,
                                &round.id,
                                &node.node_id,
                                &latest_attempt.attempt_id,
                                &dyn_node.id,
                            );
                            let mut dyn_attempt_ids = std::fs::read_dir(dyn_node_dir.as_std_path())
                                .map(|entries| {
                                    entries
                                        .filter_map(|e| e.ok())
                                        .filter(|e| {
                                            e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                                        })
                                        .filter_map(|e| e.file_name().into_string().ok())
                                        .filter(|n| n.starts_with("attempt-"))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            dyn_attempt_ids.sort();

                            let mut dyn_leafs: Vec<ConversationSessionLeafVm> = Vec::new();
                            let dyn_status = enum_label(&dyn_node.status);
                            let dyn_outcome = dyn_node.outcome.as_ref().map(enum_label);
                            let dyn_current = dynamic_graph
                                .run
                                .current_node_ids
                                .iter()
                                .any(|id| id == &dyn_node.id);
                            for dyn_attempt_id in &dyn_attempt_ids {
                                let dyn_session_vm = dynamic_acp_session_vm(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &node.node_id,
                                    &latest_attempt.attempt_id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                    None,
                                    None,
                                )?;
                                let dyn_pause_reason = display_pause_reason_for_dynamic_attempt(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &node.node_id,
                                    &latest_attempt.attempt_id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                    run_pause_reason.as_deref(),
                                );
                                let lifecycle = derive_conversation_attempt_lifecycle(
                                    dyn_session_vm
                                        .as_ref()
                                        .map(|session| session.status.as_str()),
                                    &dyn_status,
                                    dyn_outcome.as_deref(),
                                    dyn_current,
                                    dyn_pause_reason.as_deref(),
                                    run_resumable,
                                );
                                let dyn_status = lifecycle.display_status.clone();
                                let dyn_runtime_display = lifecycle.runtime_display.clone();
                                let is_active = lifecycle_is_active(&lifecycle);
                                let (artifacts, attachments) = conversation_session_assets(
                                    app,
                                    task_id,
                                    run_id,
                                    &round.id,
                                    &dyn_node.id,
                                    dyn_attempt_id,
                                    Some(&node.node_id),
                                    Some(&latest_attempt.attempt_id),
                                )?;

                                dyn_leafs.push(ConversationSessionLeafVm {
                                    round_id: round.id.clone(),
                                    node_id: dyn_node.id.clone(),
                                    attempt_id: dyn_attempt_id.clone(),
                                    outer_node_id: Some(node.node_id.clone()),
                                    outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                    path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                    status: dyn_status.clone(),
                                    outcome: dyn_outcome.clone(),
                                    runtime_display: dyn_runtime_display.clone(),
                                    lifecycle: lifecycle.clone(),
                                    current: dyn_current,
                                    started_at: dyn_node.started_at.clone(),
                                    finished_at: dyn_node.finished_at.clone(),
                                    session_id: None,
                                    artifact_count: artifacts.len(),
                                    attachment_count: attachments.len(),
                                });

                                if is_active {
                                    active_sessions.push(ConversationActiveSessionVm {
                                        round_id: round.id.clone(),
                                        node_id: dyn_node.id.clone(),
                                        attempt_id: dyn_attempt_id.clone(),
                                        outer_node_id: Some(node.node_id.clone()),
                                        outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                        path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                        status: dyn_status.clone(),
                                        runtime_display: dyn_runtime_display.clone(),
                                        lifecycle: lifecycle.clone(),
                                        session_id: None,
                                        started_at: None,
                                    });
                                }
                            }

                            let dyn_node_status = dyn_leafs
                                .last()
                                .map(|l| l.status.clone())
                                .unwrap_or_else(|| dyn_status.clone());
                            let dyn_node_runtime_display = dyn_leafs
                                .last()
                                .map(|l| l.runtime_display.clone())
                                .unwrap_or_else(|| {
                                    runtime_display_vm(
                                        Some(&dyn_status),
                                        dyn_outcome.as_deref(),
                                        dyn_current,
                                        run_pause_reason.as_deref(),
                                        run_resumable,
                                    )
                                });

                            dynamic_tree_nodes.push(ConversationTreeNodeVm {
                                node_id: dyn_node.id.clone(),
                                label: dyn_node.title.clone(),
                                node_type: format!("dynamic-{}", enum_label(&dyn_node.kind)),
                                status: dyn_node_status,
                                runtime_display: dyn_node_runtime_display,
                                attempts: dyn_leafs,
                                outer_nodes: None,
                            });
                        }
                        outer_nodes = Some(dynamic_tree_nodes);
                    }
                }
            }

            // Build leafs for the top-level node itself.
            // AI-DYNAMIC nodes are containers — their real sessions live in outer_nodes.
            let mut leafs: Vec<ConversationSessionLeafVm> = Vec::new();
            if !is_ai_dynamic {
                for attempt in &all_attempts {
                    let session_vm = acp_session_vm(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                        None,
                        None,
                    )?;
                    let runtime_status = enum_label(&attempt.status);
                    let display_pause_reason = display_pause_reason_for_attempt(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                        run_pause_reason.as_deref(),
                    );
                    let outcome = attempt.outcome.as_ref().map(enum_label);
                    let current = run.current_round.as_deref() == Some(&round.id)
                        && run.current_node.as_deref() == Some(&node.node_id)
                        && run.current_attempt.as_deref() == Some(&attempt.attempt_id);
                    let lifecycle = derive_conversation_attempt_lifecycle(
                        session_vm.as_ref().map(|session| session.status.as_str()),
                        &runtime_status,
                        outcome.as_deref(),
                        current,
                        display_pause_reason.as_deref(),
                        run_resumable,
                    );
                    let status = lifecycle.display_status.clone();
                    let runtime_display = lifecycle.runtime_display.clone();
                    let is_active = lifecycle_is_active(&lifecycle);
                    let (artifacts, attachments) = conversation_session_assets(
                        app,
                        task_id,
                        run_id,
                        &round.id,
                        &node.node_id,
                        &attempt.attempt_id,
                        None,
                        None,
                    )?;
                    leafs.push(ConversationSessionLeafVm {
                        round_id: round.id.clone(),
                        node_id: node.node_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                        status: status.clone(),
                        outcome,
                        runtime_display: runtime_display.clone(),
                        lifecycle: lifecycle.clone(),
                        current,
                        started_at: Some(attempt.started_at.clone()),
                        finished_at: attempt.finished_at.clone(),
                        session_id: None,
                        artifact_count: artifacts.len(),
                        attachment_count: attachments.len(),
                    });

                    if is_active {
                        active_sessions.push(ConversationActiveSessionVm {
                            round_id: round.id.clone(),
                            node_id: node.node_id.clone(),
                            attempt_id: attempt.attempt_id.clone(),
                            outer_node_id: None,
                            outer_attempt_id: None,
                            path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                            status,
                            runtime_display: runtime_display.clone(),
                            lifecycle: lifecycle.clone(),
                            session_id: None,
                            started_at: Some(attempt.started_at.clone()),
                        });
                    }
                }
            }

            let node_status = if is_ai_dynamic {
                // Derive status from dynamic child nodes
                outer_nodes
                    .as_ref()
                    .and_then(|ons| ons.last())
                    .map(|on| on.status.clone())
                    .unwrap_or_else(|| "completed".to_string())
            } else {
                all_attempts
                    .last()
                    .map(|a| enum_label(&a.status))
                    .unwrap_or_else(|| "pending".to_string())
            };
            let node_runtime_display = if is_ai_dynamic {
                outer_nodes
                    .as_ref()
                    .and_then(|ons| ons.last())
                    .map(|on| on.runtime_display.clone())
                    .unwrap_or_else(|| {
                        runtime_display_vm(
                            Some(&node_status),
                            None,
                            false,
                            run_pause_reason.as_deref(),
                            run_resumable,
                        )
                    })
            } else {
                leafs
                    .last()
                    .map(|leaf| leaf.runtime_display.clone())
                    .unwrap_or_else(|| {
                        runtime_display_vm(
                            Some(&node_status),
                            None,
                            false,
                            run_pause_reason.as_deref(),
                            run_resumable,
                        )
                    })
            };

            tree_nodes.push(ConversationTreeNodeVm {
                node_id: node.node_id.clone(),
                label: node.node_id.clone(),
                node_type: enum_label(&node.node_type),
                status: node_status,
                runtime_display: node_runtime_display,
                attempts: leafs,
                outer_nodes,
            });
        }

        let round_status = enum_label(&round.status);
        let round_outcome = round.outcome.as_ref().map(enum_label);
        tree_rounds.push(ConversationRoundNodeVm {
            round_id: round.id.clone(),
            index: round.index,
            label: format!("round-{:03}", round.index),
            status: round_status.clone(),
            runtime_display: runtime_display_vm(
                Some(&round_status),
                round_outcome.as_deref(),
                run.current_round.as_deref() == Some(&round.id),
                run_pause_reason.as_deref(),
                run_resumable,
            ),
            nodes: tree_nodes,
        });
    }

    // Determine which session leaf to load.
    let selected_leaf: Option<ConversationSessionLeafVm> = if let Some(key) = selected_session_key {
        // Find the leaf matching the key by searching the tree.
        find_leaf_by_key(&tree_rounds, key)
    } else {
        // Runtime-owned sessions need a stable UI anchor as soon as the run starts.
        // Prefer the current/running attempt, then fall back to the newest conversation.
        default_session_leaf(&tree_rounds)
    };

    let effective_key: Option<String> = selected_leaf.as_ref().map(conversation_leaf_key);

    // Load the selected ACP session
    let selected_session = if let Some(ref leaf) = selected_leaf {
        if let (Some(outer_id), Some(outer_attempt)) = (
            leaf.outer_node_id.as_deref(),
            leaf.outer_attempt_id.as_deref(),
        ) {
            crate::view_models::dynamic_acp_session_vm(
                app,
                task_id,
                run_id,
                &leaf.round_id,
                outer_id,
                outer_attempt,
                &leaf.node_id,
                &leaf.attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                app,
                task_id,
                run_id,
                &leaf.round_id,
                &leaf.node_id,
                &leaf.attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        }
    } else {
        None
    };

    let (artifacts, attachments) = if let Some(ref leaf) = selected_leaf {
        conversation_session_assets(
            app,
            task_id,
            run_id,
            &leaf.round_id,
            &leaf.node_id,
            &leaf.attempt_id,
            leaf.outer_node_id.as_deref(),
            leaf.outer_attempt_id.as_deref(),
        )?
    } else {
        (Vec::new(), Vec::new())
    };

    let input_attachments = input_attachments_vm(app, task_id);

    let resumable = gold_band::app::is_run_continuable(&run);
    let run_status = enum_label(&run.status);
    let run_outcome = run.outcome.map(|o| enum_label(&o));

    let (workflow_valid, workflow_json) = if let Some(ref dsl) = workflow_snapshot {
        (true, Some(serde_json::to_string(dsl).unwrap_or_default()))
    } else {
        (true, None)
    };

    // Build workflow graph from the selected session's round so the conversation view
    // matches the old runtime graph, including status/icons/counts.
    let workflow_graph = selected_leaf
        .as_ref()
        .and_then(|leaf| round_detail_vm(app, task_id, run_id, &leaf.round_id, None).ok())
        .map(|detail| detail.graph)
        .or_else(|| workflow_snapshot.as_ref().map(|dsl| workflow_graph_vm(dsl)))
        .unwrap_or_else(|| GraphVm {
            nodes: Vec::new(),
            edges: Vec::new(),
        });

    Ok(ConversationRunVm {
        workflow_graph,
        project_id: project_id.to_string(),
        task_id: task_id.to_string(),
        run_id: run_id.to_string(),
        title,
        auto_title,
        run_mode,
        workflow_template_id: None,
        run_status,
        run_outcome,
        session_tree: ConversationSessionTreeVm {
            rounds: tree_rounds,
            selected_session_key: effective_key,
        },
        selected_session,
        active_sessions,
        artifacts,
        attachments,
        input_attachments,
        workflow_status: "valid".to_string(),
        workflow_valid,
        workflow_error: None,
        workflow_json,
        resumable,
        pause_reason: run.pause_reason.map(|r| enum_label(&r)),
    })
}

// ── Attachment validation helpers ──

pub(crate) const MAX_ATTACHMENT_COUNT: usize = 10;
pub(crate) const MAX_ATTACHMENT_PER_FILE: u64 = 25 * 1024 * 1024; // 25 MB
pub(crate) const MAX_ATTACHMENT_TOTAL: u64 = 100 * 1024 * 1024; // 100 MB

pub(crate) fn allowed_attachment_ext(ext: &str) -> bool {
    gold_band::provider::supported_attachment_extensions()
        .into_iter()
        .any(|supported| supported == ext)
}

pub(crate) fn validate_attachment_paths(paths: &[String]) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();
    if paths.len() > MAX_ATTACHMENT_COUNT {
        errors.push("conversation.attachment-count-exceeded".to_string());
        return errors;
    }
    let mut total_size: u64 = 0;
    let mut seen = std::collections::HashSet::new();
    for p in paths {
        if !seen.insert(p) {
            continue;
        }
        let path = Path::new(p);
        if !path.exists() {
            errors.push("conversation.attachment-not-found".to_string());
            continue;
        }
        if path.is_dir() {
            errors.push("conversation.attachment-unsupported-type".to_string());
            continue;
        }
        let meta = match path.metadata() {
            Ok(m) => m,
            Err(_) => {
                errors.push("conversation.attachment-unreadable".to_string());
                continue;
            }
        };
        if meta.len() == 0 {
            errors.push("conversation.attachment-unreadable".to_string());
            continue;
        }
        if meta.len() > MAX_ATTACHMENT_PER_FILE {
            errors.push("conversation.attachment-too-large".to_string());
            continue;
        }
        total_size += meta.len();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !allowed_attachment_ext(ext.to_lowercase().as_str()) {
            errors.push("conversation.attachment-unsupported-type".to_string());
        }
    }
    if total_size > MAX_ATTACHMENT_TOTAL {
        errors.push("conversation.attachment-total-too-large".to_string());
    }
    errors
}

fn missing_item(code: &str, label: &str, recovery_path: &str) -> ConversationMissingItemVm {
    ConversationMissingItemVm {
        code: code.to_string(),
        label: label.to_string(),
        recovery_path: recovery_path.to_string(),
    }
}

// ── Input attachments (task-level authoring) ──

fn input_attachments_vm(app: &App, task_id: &str) -> Vec<AssetItemVm> {
    let dir = app.paths.task_dir(task_id).join("authoring").join("inputs");
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<AssetItemVm> = std::fs::read_dir(dir.as_std_path())
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let size = entry.metadata().ok()?.len();
                    Some(AssetItemVm {
                        kind: "input-attachment".to_string(),
                        title: format!("{} ({} KB)", name, size / 1024),
                        preview: name.clone(),
                        tone: "info".to_string(),
                        round_id: String::new(),
                        node_id: String::new(),
                        attempt_id: String::new(),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

// ── Validated create ──

pub fn validate_conversation_create_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationValidationResultVm> {
    let mut missing: Vec<ConversationMissingItemVm> = Vec::new();

    if input.content.trim().is_empty() {
        missing.push(missing_item(
            "content.required",
            "Content is required",
            "/chat",
        ));
    }

    if input.run_mode == "auto" {
        let config = input.auto_config.as_ref();
        let strategy = config
            .and_then(|c| c.agent_strategy.as_deref())
            .unwrap_or("fixed");
        if strategy == "dynamic" {
            if config
                .and_then(|c| c.bootstrap_agent_type.as_deref())
                .or_else(|| config.map(|c| c.agent_type.as_str()))
                .map(|agent| agent.trim().is_empty())
                .unwrap_or(true)
            {
                missing.push(missing_item(
                    "agent.required",
                    "Agent is required for AUTO mode",
                    "/chat/agents",
                ));
            }
            if config
                .and_then(|c| c.available_agents.as_ref())
                .map(|agents| agents.iter().all(|agent| agent.provider.trim().is_empty()))
                .unwrap_or(true)
            {
                missing.push(missing_item(
                    "agent.required",
                    "Agent is required for AUTO mode",
                    "/chat/agents",
                ));
            }
        } else if config
            .map(|c| c.agent_type.trim().is_empty())
            .unwrap_or(true)
        {
            missing.push(missing_item(
                "agent.required",
                "Agent is required for AUTO mode",
                "/chat/agents",
            ));
        }
    } else if input.run_mode == "workflow" {
        if input
            .workflow_template_id
            .as_ref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
        {
            missing.push(missing_item(
                "workflow.required",
                "Workflow template is required",
                "/chat/run-modes",
            ));
        } else if let Some(ref tid) = input.workflow_template_id {
            let store = app.workflow_templates().ok();
            let found = store
                .as_ref()
                .and_then(|s| s.templates.iter().find(|t| t.id == *tid));
            if found.is_none() {
                missing.push(missing_item(
                    "workflow.not-found",
                    "Selected workflow template not found",
                    "/chat/run-modes",
                ));
            }
        }
    }

    // Validate attachments
    if let Some(ref paths) = input.attachment_paths {
        let errors = validate_attachment_paths(paths);
        for code in &errors {
            missing.push(missing_item(code, code, "/chat"));
        }
    }

    Ok(ConversationValidationResultVm {
        valid: missing.is_empty(),
        missing_items: missing,
    })
}

// ── Real create ──

fn dynamic_control_from_vm(control: Option<&ConversationDynamicControlVm>) -> DynamicControlDsl {
    control
        .map(|control| DynamicControlDsl {
            max_dynamic_nodes: control.max_dynamic_nodes,
            max_fanout: control.max_fanout,
            max_depth: control.max_depth,
            max_parallel: control.max_parallel,
            max_group_depth: control.max_group_depth,
            max_workflow_invocations: control.max_workflow_invocations,
            allow_nested_dynamic: control.allow_nested_dynamic,
        })
        .unwrap_or_default()
}

fn build_auto_workflow(config: Option<&ConversationAutoConfigVm>) -> WorkflowDsl {
    let agent_type = config.map(|c| c.agent_type.as_str()).unwrap_or("");
    let model_id = config
        .and_then(|c| c.model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let bootstrap_model_id = config
        .and_then(|c| c.bootstrap_model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let acceptance_model_id = config
        .and_then(|c| c.acceptance_model_id.as_deref())
        .filter(|v| !v.trim().is_empty());
    let permission_mode = config
        .and_then(|c| c.permission_mode.as_deref())
        .filter(|v| !v.trim().is_empty());
    let global_goal = config
        .and_then(|c| c.global_goal.as_deref())
        .filter(|v| !v.trim().is_empty());
    let agent_strategy_mode = config
        .and_then(|c| c.agent_strategy.as_deref())
        .unwrap_or("fixed");

    let agent_strategy = if agent_strategy_mode == "dynamic" {
        let bootstrap_provider = config
            .and_then(|c| c.bootstrap_agent_type.as_deref())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(agent_type)
            .to_string();
        let available_agents = config
            .and_then(|c| c.available_agents.as_ref())
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(|agent| {
                        let provider = agent.provider.trim();
                        if provider.is_empty() {
                            return None;
                        }
                        Some(DynamicAgentRef {
                            provider: provider.to_string(),
                            model: agent
                                .model
                                .as_deref()
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_string),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|agents| !agents.is_empty())
            .unwrap_or_else(|| {
                vec![DynamicAgentRef {
                    provider: bootstrap_provider.clone(),
                    model: model_id.map(str::to_string),
                }]
            });
        AiDynamicAgentStrategy::Dynamic {
            bootstrap_provider,
            bootstrap_model: bootstrap_model_id.map(str::to_string),
            acceptance_model: acceptance_model_id.map(str::to_string),
            routing_prompt: config
                .and_then(|c| c.routing_prompt.as_deref())
                .map(str::trim)
                .unwrap_or("")
                .to_string(),
            available_agents,
        }
    } else {
        AiDynamicAgentStrategy::Fixed {
            provider: agent_type.to_string(),
            model: model_id.map(str::to_string),
        }
    };

    WorkflowDsl {
        version: "0.1".to_string(),
        id: "auto-workflow".to_string(),
        entry: "ai-dynamic".to_string(),
        control: Default::default(),
        nodes: vec![NodeDsl::AiDynamic(AiDynamicNode {
            id: "ai-dynamic".to_string(),
            agent_strategy,
            permission_mode: permission_mode.map(|s| s.to_string()),
            allowed_profiles: config
                .and_then(|c| c.allowed_profiles.clone())
                .unwrap_or_default(),
            global_goal: global_goal.map(|s| s.to_string()),
            control: dynamic_control_from_vm(config.and_then(|c| c.control.as_ref())),
            allowed_workflows: config
                .and_then(|c| c.allowed_workflows.as_ref())
                .map(|workflows| {
                    workflows
                        .iter()
                        .filter_map(|workflow| {
                            let workflow_id = workflow.workflow_id.trim();
                            (!workflow_id.is_empty()).then(|| {
                                gold_band::dsl::AllowedWorkflowRefDsl {
                                    workflow_id: workflow_id.to_string(),
                                }
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })],
        edges: vec![EdgeDsl {
            from: "ai-dynamic".to_string(),
            to: END_NODE.to_string(),
            on: EdgeOutcome::Success,
            session: None,
        }],
    }
}

pub fn create_conversation_run_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationRunVm> {
    let title = if input.content.is_empty() {
        "New Task".to_string()
    } else {
        input
            .content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(12)
            .collect()
    };

    // Build workflow
    let workflow = if input.run_mode == "auto" {
        build_auto_workflow(input.auto_config.as_ref())
    } else {
        // Load from template
        let store = app.workflow_templates()?;
        let template_id = input.workflow_template_id.as_deref().unwrap_or("default");
        store
            .templates
            .iter()
            .find(|t| t.id == template_id)
            .map(|t| t.workflow.clone())
            .ok_or_else(|| anyhow::anyhow!("workflow template not found: {template_id}"))?
    };

    // Create task
    let summary = app.create_task_from_requirement(CreateTaskInput {
        title: Some(title.clone()),
        description: None,
        requirement_file_name: None,
        requirement_content: input.content.clone(),
        workflow,
        workflow_template_id: input.workflow_template_id.clone(),
    })?;

    let task_id = summary.task.id.clone();

    // Save conversation metadata
    let authoring_dir = app.paths.task_dir(&task_id).join("authoring");
    fs::create_dir_all(authoring_dir.as_std_path())?;

    let meta = serde_json::json!({
        "version": "1",
        "source": "conversation-ui",
        "runMode": input.run_mode,
        "workflowTemplateId": input.workflow_template_id,
        "titleAutoGenerated": true,
        "initialAttachmentNames": input.attachment_paths.as_ref().map(|paths| {
            paths.iter().map(|p| Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string()).collect::<Vec<_>>()
        }),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    write_json(&authoring_dir.join("conversation.json"), &meta)?;

    // Copy attachments to authoring dir
    if let Some(ref paths) = input.attachment_paths {
        let attach_dir = authoring_dir.join("inputs");
        fs::create_dir_all(attach_dir.as_std_path())?;
        for src in paths {
            let src_path = Path::new(src);
            if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                let dest = attach_dir.join(name);
                let _ = fs::copy(src_path, &dest);
            }
        }
    }

    // Start the workflow in the background so the conversation surface can
    // display the session as soon as the first ACP events arrive.
    let run = app.run_start_background(&task_id, None)?;

    // Return early VM from the run
    conversation_run_vm(app, &input.project_id, &task_id, &run.id, None).or_else(|_| {
        Ok(ConversationRunVm {
            project_id: input.project_id.clone(),
            task_id: task_id.clone(),
            run_id: run.id,
            title,
            auto_title: true,
            run_mode: input.run_mode.clone(),
            workflow_template_id: input.workflow_template_id.clone(),
            run_status: enum_label(&run.status),
            run_outcome: None,
            session_tree: ConversationSessionTreeVm {
                rounds: Vec::new(),
                selected_session_key: None,
            },
            selected_session: None,
            active_sessions: Vec::new(),
            artifacts: Vec::new(),
            attachments: Vec::new(),
            input_attachments: Vec::new(),
            workflow_status: "valid".to_string(),
            workflow_valid: true,
            workflow_error: None,
            workflow_json: None,
            workflow_graph: GraphVm {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            resumable: false,
            pause_reason: None,
        })
    })
}

pub fn rerun_conversation_task_vm(
    app: &App,
    project_id: &str,
    task_id: &str,
) -> anyhow::Result<ConversationRunVm> {
    // Kill running run if any
    if let Ok(summaries) = app.task_summaries() {
        if let Some(ts) = summaries.iter().find(|s| s.task.id == task_id) {
            if let Some(ref latest) = ts.latest_run {
                if latest.status == RunStatus::Running {
                    let _ = app.run_kill(task_id, &latest.id);
                }
            }
        }
    }
    // Start new run in the background; live ACP events drive the UI refresh.
    let run = app.run_start_background(task_id, None)?;
    conversation_run_vm(app, project_id, task_id, &run.id, None).or_else(|_| {
        Ok(ConversationRunVm {
            project_id: project_id.to_string(),
            task_id: task_id.to_string(),
            run_id: run.id,
            title: String::new(),
            auto_title: false,
            run_mode: "workflow".to_string(),
            workflow_template_id: None,
            run_status: enum_label(&run.status),
            run_outcome: None,
            session_tree: ConversationSessionTreeVm {
                rounds: Vec::new(),
                selected_session_key: None,
            },
            selected_session: None,
            active_sessions: Vec::new(),
            artifacts: Vec::new(),
            attachments: Vec::new(),
            input_attachments: Vec::new(),
            workflow_status: "valid".to_string(),
            workflow_valid: true,
            workflow_error: None,
            workflow_json: None,
            workflow_graph: GraphVm {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            resumable: false,
            pause_reason: None,
        })
    })
}

pub fn switch_conversation_session_vm(
    app: &App,
    task_id: &str,
    run_id: &str,
    round_id: &str,
    node_id: &str,
    attempt_id: &str,
    outer_node_id: Option<&str>,
    outer_attempt_id: Option<&str>,
) -> anyhow::Result<ConversationSessionSwitchVm> {
    let selected_session =
        if let (Some(outer_id), Some(outer_attempt)) = (outer_node_id, outer_attempt_id) {
            crate::view_models::dynamic_acp_session_vm(
                app,
                task_id,
                run_id,
                round_id,
                outer_id,
                outer_attempt,
                node_id,
                attempt_id,
                None,
                None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                app, task_id, run_id, round_id, node_id, attempt_id, None, None,
            )
            .ok()
            .flatten()
        };

    let (artifacts, attachments) = conversation_session_assets(
        app,
        task_id,
        run_id,
        round_id,
        node_id,
        attempt_id,
        outer_node_id,
        outer_attempt_id,
    )?;

    let result = ConversationSessionSwitchVm {
        selected_session,
        artifacts,
        attachments,
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        ConversationAutoConfigVm, ConversationDynamicAgentRefVm, build_auto_workflow,
        conversation_run_vm, conversation_status_from_session,
        derive_conversation_attempt_lifecycle, lifecycle_is_active, switch_conversation_session_vm,
    };
    use camino::Utf8PathBuf;
    use gold_band::app::App;
    use gold_band::dsl::{AiDynamicAgentStrategy, NodeDsl};
    use serde_json::json;

    #[test]
    fn paused_runtime_keeps_paused_status_after_process_interrupt() {
        let status = conversation_status_from_session(
            Some("cancelled"),
            "paused",
            Some("process-interrupted"),
            true,
        );

        assert_eq!(status, "paused");
    }

    #[test]
    fn running_runtime_overrides_stale_session_terminal_status() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelled"),
            "running",
            None,
            true,
            None,
            false,
        );

        assert_eq!(lifecycle.display_status, "running");
        assert!(lifecycle.runtime.active);
        assert!(!lifecycle.acp.active);
        assert!(lifecycle_is_active(&lifecycle));
    }

    #[test]
    fn non_resumable_runtime_still_uses_session_terminal_status() {
        let status = conversation_status_from_session(
            Some("cancelled"),
            "paused",
            Some("process-interrupted"),
            false,
        );

        assert_eq!(status, "cancelled");
    }

    #[test]
    fn acp_cancelling_keeps_leaf_active_and_stopping() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelling"),
            "paused",
            None,
            true,
            Some("process-interrupted"),
            true,
        );

        assert_eq!(lifecycle.display_status, "cancelling");
        assert!(lifecycle.acp.active);
        assert!(lifecycle.acp.stopping);
        assert!(lifecycle_is_active(&lifecycle));
    }

    #[test]
    fn completed_runtime_suppresses_stale_acp_running() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            "completed",
            Some("success"),
            false,
            None,
            false,
        );

        assert_eq!(lifecycle.display_status, "completed");
        assert!(!lifecycle.runtime.active);
        assert!(!lifecycle.acp.active);
        assert!(lifecycle.acp.terminal);
        assert!(!lifecycle_is_active(&lifecycle));
    }

    #[test]
    fn workflow_failure_runtime_suppresses_stale_acp_running() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("running"),
            "completed",
            Some("failure"),
            false,
            None,
            false,
        );

        assert_eq!(lifecycle.display_status, "completed");
        assert_eq!(lifecycle.runtime_display.tone, "danger");
        assert!(!lifecycle.runtime_display.blocking_error);
        assert!(!lifecycle.acp.active);
        assert!(!lifecycle_is_active(&lifecycle));
    }

    #[test]
    fn interrupted_runtime_pause_is_input_continue() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            Some("cancelled"),
            "paused",
            None,
            true,
            Some("process-interrupted"),
            true,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("input"));
        assert!(lifecycle.runtime.continuable);
    }

    #[test]
    fn waiting_for_user_input_pause_is_action_continue() {
        let lifecycle = derive_conversation_attempt_lifecycle(
            None,
            "paused",
            None,
            true,
            Some("waiting-for-user-input"),
            true,
        );

        assert_eq!(lifecycle.display_status, "paused");
        assert_eq!(lifecycle.continue_kind.as_deref(), Some("action"));
        assert!(lifecycle.runtime.continuable);
    }

    #[test]
    fn build_auto_workflow_preserves_dynamic_acceptance_model() {
        let workflow = build_auto_workflow(Some(&ConversationAutoConfigVm {
            agent_strategy: Some("dynamic".to_string()),
            agent_type: "claude-acp".to_string(),
            bootstrap_agent_type: Some("claude-acp".to_string()),
            bootstrap_model_id: Some("bootstrap-model".to_string()),
            acceptance_model_id: Some("accept-model".to_string()),
            model_id: None,
            permission_mode: None,
            available_agents: Some(vec![ConversationDynamicAgentRefVm {
                provider: "claude-acp".to_string(),
                model: Some("worker-model".to_string()),
            }]),
            routing_prompt: Some("Pick worker models explicitly".to_string()),
            allowed_workflows: None,
            allowed_profiles: None,
            global_goal: None,
            control: None,
            active_template_id: None,
            active_template_name: None,
        }));

        let NodeDsl::AiDynamic(node) = &workflow.nodes[0] else {
            panic!("expected ai-dynamic node");
        };
        match &node.agent_strategy {
            AiDynamicAgentStrategy::Dynamic {
                bootstrap_model,
                acceptance_model,
                available_agents,
                ..
            } => {
                assert_eq!(bootstrap_model.as_deref(), Some("bootstrap-model"));
                assert_eq!(acceptance_model.as_deref(), Some("accept-model"));
                assert_eq!(available_agents[0].model.as_deref(), Some("worker-model"));
            }
            other => panic!("expected dynamic strategy, got {other:?}"),
        }
    }

    #[test]
    fn conversation_run_vm_exposes_selected_session_assets_and_leaf_counts() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);

        let vm = conversation_run_vm(
            &app,
            "project-001",
            "task-046",
            "run-060",
            Some("round-001/测试/attempt-002"),
        )
        .unwrap();

        assert_eq!(vm.artifacts.len(), 1);
        assert_eq!(vm.artifacts[0].name, "测试-result");
        assert_eq!(vm.attachments.len(), 1);
        assert_eq!(vm.attachments[0].name, "test-report.md");

        let leaf = vm.session_tree.rounds[0].nodes[0]
            .attempts
            .iter()
            .find(|leaf| leaf.attempt_id == "attempt-002")
            .unwrap();
        assert_eq!(leaf.artifact_count, 1);
        assert_eq!(leaf.attachment_count, 1);
    }

    #[test]
    fn switch_conversation_session_vm_uses_same_asset_contract() {
        let repo_root = temp_repo_root();
        let app = App::new(repo_root);
        write_conversation_assets_fixture(&app);

        let switched = switch_conversation_session_vm(
            &app,
            "task-046",
            "run-060",
            "round-001",
            "测试",
            "attempt-002",
            None,
            None,
        )
        .unwrap();

        assert_eq!(switched.artifacts.len(), 1);
        assert_eq!(switched.artifacts[0].name, "测试-result");
        assert_eq!(switched.attachments.len(), 1);
        assert_eq!(switched.attachments[0].name, "test-report.md");
    }

    fn temp_repo_root() -> Utf8PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "gold-band-conversation-assets-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Utf8PathBuf::from_path_buf(root).unwrap()
    }

    fn write_conversation_assets_fixture(app: &App) {
        let task_id = "task-046";
        let run_id = "run-060";
        let round_id = "round-001";
        let node_id = "测试";
        let attempt_id = "attempt-002";

        std::fs::create_dir_all(app.paths.task_dir(task_id).as_std_path()).unwrap();
        gold_band::storage::write_json(
            &app.paths.task_file(task_id),
            &json!({
                "version": "0.1",
                "id": task_id,
                "title": "中文节点资源回归",
                "description": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.run_file(task_id, run_id),
            &json!({
                "version": "0.1",
                "id": run_id,
                "task_id": task_id,
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:00:02Z",
                "workflow_snapshot": "workflow.snapshot.json",
                "current_round": round_id,
                "current_node": node_id,
                "current_attempt": attempt_id,
                "new_rounds_opened": 0,
                "pause_reason": null
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths.round_file(task_id, run_id, round_id),
            &json!({
                "version": "0.1",
                "id": round_id,
                "run_id": run_id,
                "index": 1,
                "status": "completed",
                "outcome": "success",
                "trigger": "initial",
                "started_at": "2026-06-15T00:00:00Z",
                "trace": [
                    {
                        "sequence": 1,
                        "node_id": node_id,
                        "attempt_id": attempt_id,
                        "from_node_id": null,
                        "edge_outcome": null,
                        "entered_at": "2026-06-15T00:00:00Z"
                    }
                ]
            }),
        )
        .unwrap();
        gold_band::storage::write_json(
            &app.paths
                .node_file(task_id, run_id, round_id, node_id, attempt_id),
            &json!({
                "version": "0.1",
                "node_id": node_id,
                "node_type": "worker",
                "run_id": run_id,
                "round_id": round_id,
                "attempt_id": attempt_id,
                "status": "completed",
                "outcome": "success",
                "started_at": "2026-06-15T00:00:00Z",
                "finished_at": "2026-06-15T00:00:02Z",
                "manual_check_pending": false,
                "resolved_config": {}
            }),
        )
        .unwrap();

        let artifacts_dir = app
            .paths
            .artifacts_dir(task_id, run_id, round_id, node_id, attempt_id);
        std::fs::create_dir_all(artifacts_dir.as_std_path()).unwrap();
        std::fs::write(
            artifacts_dir.join("测试-result.json").as_std_path(),
            r#"{"result":true}"#,
        )
        .unwrap();

        let attachments_dir = app
            .paths
            .attachments_dir(task_id, run_id, round_id, node_id, attempt_id);
        std::fs::create_dir_all(attachments_dir.as_std_path()).unwrap();
        std::fs::write(attachments_dir.join("test-report.md").as_std_path(), "ok").unwrap();
    }
}

pub fn update_task_metadata_vm(
    app: &App,
    _project_id: &str,
    task_id: &str,
    title: &str,
    description: Option<&str>,
) -> anyhow::Result<()> {
    let mut task = app.task_show(task_id)?;
    task.title = Some(title.to_string());
    if let Some(desc) = description {
        task.description = Some(desc.to_string());
    }
    gold_band::storage::write_json(&app.paths.task_file(task_id), &task)?;
    Ok(())
}
