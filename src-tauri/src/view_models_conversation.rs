use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use gold_band::config::ConversationState;
use gold_band::app::App;
use gold_band::app::is_run_continuable;
use gold_band::domain::RunStatus;
use gold_band::domain::NodeType;
use gold_band::dynamic::DynamicGraphState;
use gold_band::dsl::WorkflowDsl;
use gold_band::storage::read_json;
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
    conv_state: &ConversationState,
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

    for w in &conv_state.conversation_workspaces {
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
    let pinned_set: std::collections::HashSet<(String, String)> = conv_state
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
            let pin_order = conv_state.conversation_pins.iter()
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

    let last_active_workspace_id = conv_state.last_conversation_workspace.clone()
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
        workflow_status: "valid".to_string(),
        workflow_valid,
        workflow_error: None,
        workflow_json,
        resumable,
        pause_reason: run.pause_reason.map(|r| format!("{:?}", r).to_lowercase()),
    })
}

pub fn validate_conversation_create_vm(
    _app: &App,
    _input: &ConversationCreateInputVm,
) -> anyhow::Result<ConversationValidationResultVm> {
    Ok(ConversationValidationResultVm {
        valid: true,
        missing_items: Vec::new(),
    })
}

pub fn create_conversation_run_vm(
    _app: &App,
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
    Ok(ConversationRunVm {
        project_id: input.project_id.clone(),
        task_id: format!("task-{}", chrono::Utc::now().timestamp_millis()),
        run_id: format!("run-{}", chrono::Utc::now().timestamp_millis()),
        title,
        auto_title: true,
        run_mode: input.run_mode.clone(),
        workflow_template_id: input.workflow_template_id.clone(),
        run_status: "running".to_string(),
        run_outcome: None,
        session_tree: ConversationSessionTreeVm {
            rounds: Vec::new(),
            selected_session_key: None,
        },
        selected_session: None,
        active_sessions: Vec::new(),
        artifacts: Vec::new(),
        attachments: Vec::new(),
        workflow_status: "valid".to_string(),
        workflow_valid: true,
        workflow_error: None,
        workflow_json: None,
        workflow_graph: GraphVm { nodes: Vec::new(), edges: Vec::new() },
        resumable: false,
        pause_reason: None,
    })
}

pub fn rerun_conversation_task_vm(
    app: &App,
    project_id: &str,
    task_id: &str,
) -> anyhow::Result<ConversationRunVm> {
    let new_run_id = format!("run-{}", chrono::Utc::now().timestamp_millis());
    conversation_run_vm(app, project_id, task_id, &new_run_id, None)
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
