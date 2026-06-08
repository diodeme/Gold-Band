use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use gold_band::config::StateConfig;
use gold_band::app::App;
use gold_band::app::is_run_continuable;
use gold_band::app::CreateTaskInput;
use gold_band::domain::RunStatus;
use gold_band::domain::NodeType;
use gold_band::dynamic::DynamicGraphState;
use gold_band::dsl::{
    AiDynamicAgentStrategy, AiDynamicNode, EdgeDsl, EdgeOutcome, NodeDsl, WorkflowDsl,
    END_NODE,
};
use gold_band::storage::{read_json, write_json};
use crate::view_models::{round_detail_vm, workflow_graph_vm, AssetItemVm, GraphVm};

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
    pub nodes: Vec<ConversationTreeNodeVm>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTreeNodeVm {
    pub node_id: String,
    pub label: String,
    pub node_type: String,
    pub status: String,
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
    pub current: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub session_id: Option<String>,
    pub artifact_count: usize,
    pub attachment_count: usize,
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
    pub agent_type: String,
    pub model_id: Option<String>,
    pub permission_mode: Option<String>,
    pub allowed_profiles: Option<Vec<String>>,
    pub global_goal: Option<String>,
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

pub fn conversation_sidebar_vm(
    app: &App,
    state: &StateConfig,
) -> ConversationSidebarVm {
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

    let default_project_id = workspaces.first().map(|w| w.project_id.clone()).unwrap_or_default();

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
            let pin_order = state.conversation_pins.iter()
                .find(|p| p.project_id == *project_id && p.task_id == *task_id)
                .map(|p| p.order);

            // Read conversation metadata if exists
            let conversation_json_path = app.paths.tasks_dir()
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
            let runs: Vec<ConversationRunSummaryVm> = app.run_list(task_id)
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
                title: summary.task.title.clone().unwrap_or_else(|| task_id.clone()),
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
            let a_time = a.latest_run.as_ref().map(|r| r.started_at.as_str()).unwrap_or("");
            let b_time = b.latest_run.as_ref().map(|r| r.started_at.as_str()).unwrap_or("");
            b_time.cmp(&a_time) // newest first
        });
    }

    let last_active_workspace_id = state.last_conversation_workspace.clone()
        .or_else(|| workspaces.first().map(|w| w.project_id.clone()));

    ConversationSidebarVm {
        workspaces,
        pinned_tasks,
        tasks_by_workspace,
        last_active_workspace_id,
        preferences: app.load_state().map(|s| s.preferences).unwrap_or_default(),
    }
}

fn enum_label<T: std::fmt::Debug>(value: &T) -> String {
    format!("{:?}", value).to_lowercase()
}

fn asset_item_vm(kind: &str, round_id: &str, node_id: &str, attempt_id: &str, name: String) -> AssetItemVm {
    AssetItemVm {
        kind: kind.to_string(),
        title: name.clone(),
        preview: name.clone(),
        tone: if kind == "artifact" { "accent" } else { "neutral" }.to_string(),
        round_id: round_id.to_string(),
        node_id: node_id.to_string(),
        attempt_id: attempt_id.to_string(),
        name,
    }
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
                        if let (Some(outer_id), Some(outer_attempt)) =
                            (leaf.outer_node_id.as_deref(), leaf.outer_attempt_id.as_deref())
                        {
                            let dyn_key = format!(
                                "{}/{}/{}/{}/{}",
                                leaf.round_id, outer_id, outer_attempt, leaf.node_id, leaf.attempt_id,
                            );
                            if dyn_key == key {
                                return Some(leaf.clone());
                            }
                        }
                        if format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id) == key {
                            return Some(leaf.clone());
                        }
                    }
                }
            }
        }
    }
    None
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
    let task_state = app.task_show(task_id)
        .map_err(|e| anyhow::anyhow!("task not found: {task_id}: {e}"))?;
    let title = task_state.title.unwrap_or_else(|| task_id.to_string());

    // Read conversation metadata if exists
    let conversation_json_path = app.paths.task_dir(task_id)
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
    let workflow_snapshot: Option<WorkflowDsl> =
        gold_band::storage::read_json::<WorkflowDsl>(
            &app.paths.workflow_snapshot_file(task_id, run_id),
        ).ok();
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
            let all_attempts = app.attempt_list(
                task_id, run_id, &round.id, &node.node_id,
            )?;

            // Build child nodes for AI-DYNAMIC
            let mut outer_nodes: Option<Vec<ConversationTreeNodeVm>> = None;
            if is_ai_dynamic {
                if let Some(latest_attempt) = all_attempts.last() {
                    let dynamic_path = app.paths.dynamic_graph_file(
                        task_id, run_id, &round.id, &node.node_id, &latest_attempt.attempt_id,
                    );
                    if let Ok(dynamic_graph) = read_json::<DynamicGraphState>(&dynamic_path) {
                        let mut dynamic_tree_nodes: Vec<ConversationTreeNodeVm> = Vec::new();
                        for dyn_node in &dynamic_graph.nodes {
                            // Find the latest attempt for this dynamic child node
                            let dyn_node_dir = app.paths.dynamic_node_dir(
                                task_id, run_id, &round.id, &node.node_id, &latest_attempt.attempt_id, &dyn_node.id,
                            );
                            let mut dyn_attempt_ids = std::fs::read_dir(dyn_node_dir.as_std_path())
                                .map(|entries| {
                                    entries
                                        .filter_map(|e| e.ok())
                                        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                                        .filter_map(|e| e.file_name().into_string().ok())
                                        .filter(|n| n.starts_with("attempt-"))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            dyn_attempt_ids.sort();

                            let mut dyn_leafs: Vec<ConversationSessionLeafVm> = Vec::new();
                            for dyn_attempt_id in &dyn_attempt_ids {
                                let dyn_attempt_dir = app.paths.dynamic_node_attempt_dir(
                                    task_id, run_id, &round.id, &node.node_id, &latest_attempt.attempt_id, &dyn_node.id, dyn_attempt_id,
                                );
                                // Read node.json from the dynamic attempt to get status
                                let node_file = dyn_attempt_dir.join("node.json");
                                let (dyn_status, dyn_outcome) = if node_file.exists() {
                                    read_json::<serde_json::Value>(&node_file)
                                        .ok()
                                        .map(|v| {
                                            let status = v.get("status")
                                                .and_then(|s| s.as_str())
                                                .map(|s| s.to_string())
                                                .unwrap_or_else(|| "completed".to_string());
                                            let outcome = v.get("outcome")
                                                .and_then(|o| o.as_str())
                                                .map(|s| s.to_string());
                                            (status, outcome)
                                        })
                                        .unwrap_or_else(|| ("completed".to_string(), None))
                                } else {
                                    ("completed".to_string(), None)
                                };
                                let is_active = dyn_status == "running";

                                dyn_leafs.push(ConversationSessionLeafVm {
                                    round_id: round.id.clone(),
                                    node_id: dyn_node.id.clone(),
                                    attempt_id: dyn_attempt_id.clone(),
                                    outer_node_id: Some(node.node_id.clone()),
                                    outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                    path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                    status: dyn_status.clone(),
                                    outcome: dyn_outcome,
                                    current: is_active,
                                    started_at: None,
                                    finished_at: None,
                                    session_id: None,
                                    artifact_count: 0,
                                    attachment_count: 0,
                                });

                                if is_active {
                                    active_sessions.push(ConversationActiveSessionVm {
                                        round_id: round.id.clone(),
                                        node_id: dyn_node.id.clone(),
                                        attempt_id: dyn_attempt_id.clone(),
                                        outer_node_id: Some(node.node_id.clone()),
                                        outer_attempt_id: Some(latest_attempt.attempt_id.clone()),
                                        path_label: format!("{}/{}", dyn_node.id, dyn_attempt_id),
                                        status: dyn_status,
                                        session_id: None,
                                        started_at: None,
                                    });
                                }
                            }

                            let dyn_node_status = dyn_leafs.last()
                                .map(|l| l.status.clone())
                                .unwrap_or_else(|| "completed".to_string());

                            dynamic_tree_nodes.push(ConversationTreeNodeVm {
                                node_id: dyn_node.id.clone(),
                                label: dyn_node.title.clone(),
                                node_type: format!("dynamic-{}", enum_label(&dyn_node.kind)),
                                status: dyn_node_status,
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
                    let is_active = attempt.status == RunStatus::Running;
                    leafs.push(ConversationSessionLeafVm {
                        round_id: round.id.clone(),
                        node_id: node.node_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        outer_node_id: None,
                        outer_attempt_id: None,
                        path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                        status: enum_label(&attempt.status),
                        outcome: attempt.outcome.as_ref().map(|o| enum_label(o)),
                        current: is_active,
                        started_at: Some(attempt.started_at.clone()),
                        finished_at: attempt.finished_at.clone(),
                        session_id: None,
                        artifact_count: 0,
                        attachment_count: 0,
                    });

                    if is_active {
                        active_sessions.push(ConversationActiveSessionVm {
                            round_id: round.id.clone(),
                            node_id: node.node_id.clone(),
                            attempt_id: attempt.attempt_id.clone(),
                            outer_node_id: None,
                            outer_attempt_id: None,
                            path_label: format!("{}/{}", node.node_id, attempt.attempt_id),
                            status: enum_label(&attempt.status),
                            session_id: None,
                            started_at: Some(attempt.started_at.clone()),
                        });
                    }
                }
            }

            let node_status = if is_ai_dynamic {
                // Derive status from dynamic child nodes
                outer_nodes.as_ref()
                    .and_then(|ons| ons.last())
                    .map(|on| on.status.clone())
                    .unwrap_or_else(|| "completed".to_string())
            } else {
                all_attempts.last()
                    .map(|a| enum_label(&a.status))
                    .unwrap_or_else(|| "pending".to_string())
            };

            tree_nodes.push(ConversationTreeNodeVm {
                node_id: node.node_id.clone(),
                label: node.node_id.clone(),
                node_type: enum_label(&node.node_type),
                status: node_status,
                attempts: leafs,
                outer_nodes,
            });
        }

        tree_rounds.push(ConversationRoundNodeVm {
            round_id: round.id.clone(),
            index: round.index,
            label: format!("round-{:03}", round.index),
            status: enum_label(&round.status),
            nodes: tree_nodes,
        });
    }

    // Determine which session leaf to load (prefer last attempt of last dynamic child, then last top-level attempt)
    let selected_leaf: Option<ConversationSessionLeafVm> = if let Some(key) = selected_session_key {
        // Find the leaf matching the key by searching the tree
        find_leaf_by_key(&tree_rounds, key)
    } else {
        // Default: last attempt of last dynamic child of last node, or last top-level attempt
        tree_rounds.last().and_then(|r| {
            r.nodes.last().and_then(|n| {
                n.outer_nodes.as_ref().and_then(|o| o.last()).and_then(|on| on.attempts.last())
                    .or_else(|| n.attempts.last())
                    .cloned()
            })
        })
    };

    let effective_key: Option<String> = selected_leaf.as_ref().map(|leaf| {
        if leaf.outer_node_id.is_some() {
            format!("{}/{}/{}/{}/{}", leaf.round_id, leaf.outer_node_id.as_deref().unwrap_or(""), leaf.outer_attempt_id.as_deref().unwrap_or(""), leaf.node_id, leaf.attempt_id)
        } else {
            format!("{}/{}/{}", leaf.round_id, leaf.node_id, leaf.attempt_id)
        }
    });

    // Load the selected ACP session
    let selected_session = if let Some(ref leaf) = selected_leaf {
        if let (Some(outer_id), Some(outer_attempt)) = (leaf.outer_node_id.as_deref(), leaf.outer_attempt_id.as_deref()) {
            crate::view_models::dynamic_acp_session_vm(
                app, task_id, run_id, &leaf.round_id,
                outer_id, outer_attempt,
                &leaf.node_id, &leaf.attempt_id, None,
            )
            .ok()
            .flatten()
        } else {
            crate::view_models::acp_session_vm(
                app, task_id, run_id, &leaf.round_id, &leaf.node_id, &leaf.attempt_id, None,
            )
            .ok()
            .flatten()
        }
    } else {
        None
    };

    let (artifacts, attachments) = if let Some(ref leaf) = selected_leaf {
        if let (Some(outer_node_id), Some(outer_attempt_id)) = (
            leaf.outer_node_id.as_deref(),
            leaf.outer_attempt_id.as_deref(),
        ) {
            let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
                task_id,
                run_id,
                &leaf.round_id,
                outer_node_id,
                outer_attempt_id,
                &leaf.node_id,
                &leaf.attempt_id,
            );
            let attachments_dir = app.paths.dynamic_node_attachments_dir(
                task_id,
                run_id,
                &leaf.round_id,
                outer_node_id,
                outer_attempt_id,
                &leaf.node_id,
                &leaf.attempt_id,
            );
            let artifacts = std::fs::read_dir(artifacts_dir.as_std_path())
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .filter_map(|entry| entry.file_name().into_string().ok())
                        .map(|name| asset_item_vm("artifact", &leaf.round_id, &leaf.node_id, &leaf.attempt_id, name.strip_suffix(".json").unwrap_or(&name).to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let attachments = std::fs::read_dir(attachments_dir.as_std_path())
                .map(|entries| {
                    entries
                        .filter_map(|entry| entry.ok())
                        .filter_map(|entry| entry.file_name().into_string().ok())
                        .map(|name| asset_item_vm("attachment", &leaf.round_id, &leaf.node_id, &leaf.attempt_id, name))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (artifacts, attachments)
        } else {
            let artifacts = app
                .artifact_list(task_id, run_id, &leaf.round_id, &leaf.node_id, &leaf.attempt_id)
                .unwrap_or_default()
                .into_iter()
                .map(|name| asset_item_vm("artifact", &leaf.round_id, &leaf.node_id, &leaf.attempt_id, name))
                .collect::<Vec<_>>();
            let attachments = app
                .attachment_list(task_id, run_id, &leaf.round_id, &leaf.node_id, &leaf.attempt_id)
                .unwrap_or_default()
                .into_iter()
                .map(|name| asset_item_vm("attachment", &leaf.round_id, &leaf.node_id, &leaf.attempt_id, name))
                .collect::<Vec<_>>();
            (artifacts, attachments)
        }
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
        .or_else(|| {
            workflow_snapshot
                .as_ref()
                .map(|dsl| workflow_graph_vm(dsl))
        })
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
        pause_reason: run.pause_reason.map(|r| format!("{:?}", r).to_lowercase()),
    })
}

// ── Attachment validation helpers ──

const MAX_ATTACHMENT_COUNT: usize = 10;
const MAX_ATTACHMENT_PER_FILE: u64 = 25 * 1024 * 1024; // 25 MB
const MAX_ATTACHMENT_TOTAL: u64 = 100 * 1024 * 1024; // 100 MB

fn allowed_attachment_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "md" | "json" | "jsonl" | "csv"
            | "png" | "jpg" | "jpeg" | "webp"
            | "rs" | "ts" | "tsx" | "js" | "jsx" | "py"
            | "go" | "java" | "c" | "cpp" | "h" | "hpp"
            | "html" | "css" | "xml" | "yaml" | "yml" | "toml"
    )
}

fn validate_attachment_paths(paths: &[String]) -> Vec<String> {
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
        let meta = match path.metadata() { Ok(m) => m, Err(_) => { errors.push("conversation.attachment-unreadable".to_string()); continue; } };
        if meta.len() == 0 { errors.push("conversation.attachment-unreadable".to_string()); continue; }
        if meta.len() > MAX_ATTACHMENT_PER_FILE { errors.push("conversation.attachment-too-large".to_string()); continue; }
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
    ConversationMissingItemVm { code: code.to_string(), label: label.to_string(), recovery_path: recovery_path.to_string() }
}

// ── Input attachments (task-level authoring) ──

fn input_attachments_vm(app: &App, task_id: &str) -> Vec<AssetItemVm> {
    let dir = app.paths.task_dir(task_id).join("authoring").join("attachments");
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
        missing.push(missing_item("content.required", "Content is required", "/chat"));
    }

    if input.run_mode == "auto" {
        let config = input.auto_config.as_ref();
        if config.map(|c| c.agent_type.trim().is_empty()).unwrap_or(true) {
            missing.push(missing_item("agent.required", "Agent is required for AUTO mode", "/chat/agents"));
        }
    } else if input.run_mode == "workflow" {
        if input.workflow_template_id.as_ref().map(|t| t.trim().is_empty()).unwrap_or(true) {
            missing.push(missing_item("workflow.required", "Workflow template is required", "/chat/run-modes"));
        } else if let Some(ref tid) = input.workflow_template_id {
            let store = app.workflow_templates().ok();
            let found = store.as_ref().and_then(|s| s.templates.iter().find(|t| t.id == *tid));
            if found.is_none() {
                missing.push(missing_item("workflow.not-found", "Selected workflow template not found", "/chat/run-modes"));
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

fn build_auto_workflow(agent_type: &str, _model_id: Option<&str>, permission_mode: Option<&str>, global_goal: Option<&str>) -> WorkflowDsl {
    let provider = agent_type.to_string();
    WorkflowDsl {
        version: "0.1".to_string(),
        id: "auto-workflow".to_string(),
        entry: "ai-dynamic".to_string(),
        control: Default::default(),
        nodes: vec![
            NodeDsl::AiDynamic(AiDynamicNode {
                id: "ai-dynamic".to_string(),
                agent_strategy: AiDynamicAgentStrategy::Fixed { provider },
                permission_mode: permission_mode.map(|s| s.to_string()),
                allowed_profiles: Vec::new(),
                global_goal: global_goal.map(|s| s.to_string()),
                control: Default::default(),
                allowed_workflows: Vec::new(),
            }),
        ],
        edges: vec![
            EdgeDsl {
                from: "ai-dynamic".to_string(),
                to: END_NODE.to_string(),
                on: EdgeOutcome::Success,
                session: None,
            },
        ],
    }
}

pub fn create_conversation_run_vm(
    app: &App,
    input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationRunVm> {
    let title = if input.content.is_empty() {
        "New Task".to_string()
    } else {
        input.content.lines().next().unwrap_or("").chars().take(12).collect()
    };

    // Build workflow
    let workflow = if input.run_mode == "auto" {
        let config = input.auto_config.as_ref();
        let agent = config.map(|c| c.agent_type.as_str()).unwrap_or("");
        let model = config.and_then(|c| c.model_id.as_deref());
        let perm = config.and_then(|c| c.permission_mode.as_deref());
        let goal = config.and_then(|c| c.global_goal.as_deref());
        build_auto_workflow(agent, model, perm, goal)
    } else {
        // Load from template
        let store = app.workflow_templates()?;
        let template_id = input.workflow_template_id.as_deref().unwrap_or("default");
        store.templates.iter()
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
        let attach_dir = authoring_dir.join("attachments");
        fs::create_dir_all(attach_dir.as_std_path())?;
        for src in paths {
            let src_path = Path::new(src);
            if let Some(name) = src_path.file_name().and_then(|n| n.to_str()) {
                let dest = attach_dir.join(name);
                let _ = fs::copy(src_path, &dest);
            }
        }
    }

    // Start run
    let run = app.run_start(&task_id, None)?;

    // Return early VM from the run
    conversation_run_vm(app, &input.project_id, &task_id, &run.id, None)
        .or_else(|_| {
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
                session_tree: ConversationSessionTreeVm { rounds: Vec::new(), selected_session_key: None },
                selected_session: None,
                active_sessions: Vec::new(),
                artifacts: Vec::new(),
                attachments: Vec::new(),
                input_attachments: Vec::new(),
                workflow_status: "valid".to_string(),
                workflow_valid: true,
                workflow_error: None,
                workflow_json: None,
                workflow_graph: GraphVm { nodes: Vec::new(), edges: Vec::new() },
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
    // Start new run
    let run = app.run_start(task_id, None)?;
    conversation_run_vm(app, project_id, task_id, &run.id, None)
        .or_else(|_| {
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
                session_tree: ConversationSessionTreeVm { rounds: Vec::new(), selected_session_key: None },
                selected_session: None,
                active_sessions: Vec::new(),
                artifacts: Vec::new(),
                attachments: Vec::new(),
                input_attachments: Vec::new(),
                workflow_status: "valid".to_string(),
                workflow_valid: true,
                workflow_error: None,
                workflow_json: None,
                workflow_graph: GraphVm { nodes: Vec::new(), edges: Vec::new() },
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
    let selected_session = if let (Some(outer_id), Some(outer_attempt)) = (outer_node_id, outer_attempt_id) {
        crate::view_models::dynamic_acp_session_vm(
            app, task_id, run_id, round_id,
            outer_id, outer_attempt,
            node_id, attempt_id, None,
        )
        .ok()
        .flatten()
    } else {
        crate::view_models::acp_session_vm(
            app, task_id, run_id, round_id, node_id, attempt_id, None,
        )
        .ok()
        .flatten()
    };


    let (artifacts, attachments) = if let (Some(outer_node_id), Some(outer_attempt_id)) = (outer_node_id, outer_attempt_id) {
        let artifacts_dir = app.paths.dynamic_node_artifacts_dir(
            task_id, run_id, round_id,
            outer_node_id, outer_attempt_id,
            node_id, attempt_id,
        );
        let attachments_dir = app.paths.dynamic_node_attachments_dir(
            task_id, run_id, round_id,
            outer_node_id, outer_attempt_id,
            node_id, attempt_id,
        );
        let artifacts = std::fs::read_dir(artifacts_dir.as_std_path())
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .map(|name| asset_item_vm("artifact", round_id, node_id, attempt_id, name.strip_suffix(".json").unwrap_or(&name).to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let attachments = std::fs::read_dir(attachments_dir.as_std_path())
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .map(|name| asset_item_vm("attachment", round_id, node_id, attempt_id, name))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        (artifacts, attachments)
    } else {
        let artifacts = app
            .artifact_list(task_id, run_id, round_id, node_id, attempt_id)
            .unwrap_or_default()
            .into_iter()
            .map(|name| asset_item_vm("artifact", round_id, node_id, attempt_id, name))
            .collect::<Vec<_>>();
        let attachments = app
            .attachment_list(task_id, run_id, round_id, node_id, attempt_id)
            .unwrap_or_default()
            .into_iter()
            .map(|name| asset_item_vm("attachment", round_id, node_id, attempt_id, name))
            .collect::<Vec<_>>();
        (artifacts, attachments)
    };

    Ok(ConversationSessionSwitchVm {
        selected_session,
        artifacts,
        attachments,
    })
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
