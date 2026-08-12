use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{Context, Result, anyhow, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use serde_json::{Map, Value};

use crate::domain::VERSION;
use crate::dynamic::{
    DynamicGraphState, WorkspaceKind, WorkspaceOwnership, WorkspaceState, WorkspaceStatus,
    validate_dynamic_group_state, validate_dynamic_node_state, validate_dynamic_run_state,
    validate_workspace_state, validate_workspace_topology,
};
use crate::git::{GitCommandRunner, GitRepositoryService};
use crate::storage::{read_json, write_json};

pub const CURRENT_DYNAMIC_GRAPH_VERSION: &str = "0.2";
const LEGACY_DYNAMIC_GRAPH_VERSION: &str = "0.1";
const MAIN_WORKSPACE_ID: &str = "workspace-main";
static DYNAMIC_GRAPH_LOAD_LOCKS: OnceLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
struct LegacyWorkspaceDescriptor {
    id: String,
    path: Utf8PathBuf,
    parent_workspace_id: String,
    created_by_group_id: String,
    force_released: bool,
}

/// Loads a dynamic graph through its versioned storage boundary.
///
/// Graph v0.1 stored workspace policy and paths on nodes. The v0.2 canonical
/// model stores workspace identity and lifecycle in a graph-owned catalog.
/// Migration is deterministic, validated before commit, atomically persisted,
/// and therefore becomes a no-op after the first successful load.
pub fn load_dynamic_graph(path: &Utf8Path, repo_root: &Utf8Path) -> Result<DynamicGraphState> {
    let lock = dynamic_graph_load_lock(path)?;
    let _guard = lock
        .lock()
        .map_err(|_| anyhow!("dynamic graph load lock poisoned"))?;
    let value: Value = read_json(path)?;
    let (graph, migrated) = dynamic_graph_from_value_with_migration(value, repo_root)?;
    if migrated {
        write_json(path, &graph)?;
    }
    Ok(graph)
}

fn dynamic_graph_load_lock(path: &Utf8Path) -> Result<Arc<Mutex<()>>> {
    let key = normalized_path(path);
    let mut locks = DYNAMIC_GRAPH_LOAD_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("dynamic graph load lock registry poisoned"))?;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    Ok(lock)
}

pub fn dynamic_graph_from_value_with_migration(
    mut value: Value,
    repo_root: &Utf8Path,
) -> Result<(DynamicGraphState, bool)> {
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("dynamic graph version is missing"))?;
    match version {
        CURRENT_DYNAMIC_GRAPH_VERSION => {
            let graph = serde_json::from_value(value)?;
            validate_dynamic_graph(&graph)?;
            Ok((graph, false))
        }
        LEGACY_DYNAMIC_GRAPH_VERSION => {
            if dynamic_graph_has_workspace_catalog(&value) {
                value["version"] = Value::String(CURRENT_DYNAMIC_GRAPH_VERSION.to_string());
            } else {
                migrate_v01_workspace_policy_to_catalog(&mut value, repo_root)?;
            }
            let graph: DynamicGraphState = serde_json::from_value(value)?;
            validate_dynamic_graph(&graph)?;
            Ok((graph, true))
        }
        other => Err(anyhow!(
            "dynamic graph version `{other}` is newer than or incompatible with supported version `{CURRENT_DYNAMIC_GRAPH_VERSION}`"
        )),
    }
}

pub fn validate_dynamic_graph(graph: &DynamicGraphState) -> Result<()> {
    ensure!(
        graph.version == CURRENT_DYNAMIC_GRAPH_VERSION,
        "unsupported dynamic graph version"
    );
    validate_dynamic_run_state(&graph.run)?;
    for node in &graph.nodes {
        validate_dynamic_node_state(node)?;
    }
    for group in &graph.groups {
        validate_dynamic_group_state(group)?;
    }
    for workspace in &graph.workspaces {
        validate_workspace_state(workspace)?;
    }
    validate_workspace_topology(graph)
}

fn dynamic_graph_has_workspace_catalog(value: &Value) -> bool {
    value.get("workspaces").is_some_and(Value::is_array)
        && value
            .get("nodes")
            .and_then(Value::as_array)
            .is_some_and(|nodes| nodes.iter().all(|node| node.get("workspaceId").is_some()))
        && value
            .get("groups")
            .and_then(Value::as_array)
            .is_some_and(|groups| {
                groups.iter().all(|group| {
                    group.get("targetWorkspaceId").is_some()
                        && group.get("childWorkspaceIds").is_some()
                })
            })
}

fn migrate_v01_workspace_policy_to_catalog(value: &mut Value, repo_root: &Utf8Path) -> Result<()> {
    let root = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("dynamic graph must be a JSON object"))?;
    let run = root
        .get("run")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("dynamic graph run is missing"))?;
    let dynamic_run_id = required_string(run, "id")?.to_string();
    let created_at = required_string(run, "startedAt")?.to_string();
    let updated_at = required_string(run, "updatedAt")?.to_string();
    let main_head = GitRepositoryService::default()
        .head(repo_root)
        .unwrap_or_else(|| "legacy-unknown".to_string());

    let groups = root
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("dynamic graph groups are missing"))?;
    let group_statuses = groups
        .iter()
        .filter_map(|group| {
            Some((
                group.get("id")?.as_str()?.to_string(),
                group.get("status")?.as_str()?.to_string(),
            ))
        })
        .collect::<HashMap<_, _>>();

    let nodes = root
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("dynamic graph nodes are missing"))?;
    let mut node_workspace_ids = HashMap::<String, String>::new();
    let mut legacy_descriptors = HashMap::<String, LegacyWorkspaceDescriptor>::new();

    for node in nodes.iter_mut() {
        let object = node
            .as_object_mut()
            .ok_or_else(|| anyhow!("dynamic graph node must be an object"))?;
        let node_id = required_string(object, "id")?.to_string();
        let group_id = optional_string(object, "groupId").map(ToOwned::to_owned);
        let mode = object
            .get("workspace")
            .and_then(Value::as_object)
            .and_then(|workspace| workspace.get("mode"))
            .and_then(Value::as_str)
            .unwrap_or("readonly");
        let workspace_path = optional_string(object, "workspacePath")
            .filter(|path| !path.trim().is_empty())
            .map(Utf8PathBuf::from);
        let workspace_id = if mode == "worktree" {
            let identity = workspace_path
                .as_ref()
                .map(|path| format!("path:{}", normalized_path(path)))
                .unwrap_or_else(|| format!("node:{node_id}"));
            legacy_workspace_id(&dynamic_run_id, &identity)
        } else {
            MAIN_WORKSPACE_ID.to_string()
        };
        if mode == "worktree" {
            let group_id = group_id
                .clone()
                .unwrap_or_else(|| format!("legacy-{node_id}"));
            let path = workspace_path.unwrap_or_else(|| {
                repo_root
                    .join(".gold-band/legacy-workspaces")
                    .join(&workspace_id)
            });
            legacy_descriptors
                .entry(workspace_id.clone())
                .or_insert(LegacyWorkspaceDescriptor {
                    id: workspace_id.clone(),
                    path,
                    parent_workspace_id: MAIN_WORKSPACE_ID.to_string(),
                    created_by_group_id: group_id,
                    force_released: false,
                });
        }
        node_workspace_ids.insert(node_id, workspace_id.clone());
        object.insert("workspaceId".to_string(), Value::String(workspace_id));
        object.remove("workspace");
        object.remove("workspacePath");
    }

    // Legacy readonly/main fanout branches physically shared the main checkout.
    // They cannot be resumed safely under the isolated v0.2 model, so migration
    // records deterministic released historical workspaces while preserving the
    // branch topology and every session locator.
    for group in &groups {
        let group_object = group
            .as_object()
            .ok_or_else(|| anyhow!("dynamic group must be an object"))?;
        let group_id = required_string(group_object, "id")?.to_string();
        let root_node_ids = required_string_array(group_object, "rootNodeIds")?;
        let target_workspace_id = optional_string(group_object, "createdByNodeId")
            .and_then(|node_id| node_workspace_ids.get(node_id))
            .cloned()
            .unwrap_or_else(|| MAIN_WORKSPACE_ID.to_string());
        for root_node_id in &root_node_ids {
            let Some(workspace_id) = node_workspace_ids.get(root_node_id).cloned() else {
                continue;
            };
            if workspace_id == MAIN_WORKSPACE_ID {
                let synthetic_id = legacy_workspace_id(
                    &dynamic_run_id,
                    &format!("group:{group_id}:root:{root_node_id}"),
                );
                legacy_descriptors.insert(
                    synthetic_id.clone(),
                    LegacyWorkspaceDescriptor {
                        id: synthetic_id.clone(),
                        path: repo_root
                            .join(".gold-band/legacy-workspaces")
                            .join(&synthetic_id),
                        parent_workspace_id: target_workspace_id.clone(),
                        created_by_group_id: group_id.clone(),
                        force_released: true,
                    },
                );
                remap_legacy_group_chain(
                    nodes,
                    &group_id,
                    root_node_id,
                    &synthetic_id,
                    &mut node_workspace_ids,
                );
            } else if let Some(descriptor) = legacy_descriptors.get_mut(&workspace_id) {
                descriptor.parent_workspace_id = target_workspace_id.clone();
                descriptor.created_by_group_id = group_id.clone();
            }
        }
    }

    let mut workspace_values = vec![serde_json::to_value(WorkspaceState {
        version: VERSION.to_string(),
        id: MAIN_WORKSPACE_ID.to_string(),
        dynamic_run_id: dynamic_run_id.clone(),
        kind: WorkspaceKind::Main,
        ownership: WorkspaceOwnership::User,
        repo_root: repo_root.to_path_buf(),
        path: repo_root.to_path_buf(),
        branch: None,
        parent_workspace_id: None,
        created_by_group_id: None,
        fork_commit: main_head.clone(),
        checkpoint_commit: None,
        status: WorkspaceStatus::Active,
        created_at: created_at.clone(),
        updated_at: updated_at.clone(),
    })?];
    let mut descriptors = legacy_descriptors.into_values().collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.id.cmp(&right.id));
    for descriptor in descriptors {
        let group_closed = group_statuses
            .get(&descriptor.created_by_group_id)
            .is_some_and(|status| status == "closed");
        workspace_values.push(serde_json::to_value(legacy_workspace_state(
            descriptor,
            repo_root,
            &dynamic_run_id,
            &main_head,
            &created_at,
            &updated_at,
            group_closed,
        ))?);
    }

    let groups = root
        .get_mut("groups")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("dynamic graph groups are missing"))?;
    for group in groups {
        let object = group
            .as_object_mut()
            .ok_or_else(|| anyhow!("dynamic group must be an object"))?;
        let target_workspace_id = optional_string(object, "createdByNodeId")
            .and_then(|node_id| node_workspace_ids.get(node_id))
            .cloned()
            .unwrap_or_else(|| MAIN_WORKSPACE_ID.to_string());
        let child_workspace_ids = required_string_array(object, "rootNodeIds")?
            .into_iter()
            .map(|node_id| {
                node_workspace_ids
                    .get(&node_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("dynamic group root node `{node_id}` is missing"))
            })
            .collect::<Result<Vec<_>>>()?;
        object.insert(
            "targetWorkspaceId".to_string(),
            Value::String(target_workspace_id),
        );
        object.insert(
            "childWorkspaceIds".to_string(),
            serde_json::to_value(child_workspace_ids)?,
        );
    }
    root.insert(
        "version".to_string(),
        Value::String(CURRENT_DYNAMIC_GRAPH_VERSION.to_string()),
    );
    root.insert("workspaces".to_string(), Value::Array(workspace_values));
    Ok(())
}

fn remap_legacy_group_chain(
    nodes: &mut [Value],
    group_id: &str,
    root_node_id: &str,
    workspace_id: &str,
    node_workspace_ids: &mut HashMap<String, String>,
) {
    for node in nodes {
        let Some(object) = node.as_object_mut() else {
            continue;
        };
        let same_group = optional_string(object, "groupId") == Some(group_id);
        let same_chain = optional_string(object, "chainId") == Some(root_node_id);
        if !same_group || !same_chain {
            continue;
        }
        let Some(node_id) = optional_string(object, "id").map(ToOwned::to_owned) else {
            continue;
        };
        object.insert(
            "workspaceId".to_string(),
            Value::String(workspace_id.to_string()),
        );
        node_workspace_ids.insert(node_id, workspace_id.to_string());
    }
}

fn legacy_workspace_state(
    descriptor: LegacyWorkspaceDescriptor,
    repo_root: &Utf8Path,
    dynamic_run_id: &str,
    main_head: &str,
    created_at: &str,
    updated_at: &str,
    group_closed: bool,
) -> WorkspaceState {
    let runner = GitCommandRunner::default();
    let branch = (!descriptor.force_released && descriptor.path.exists())
        .then(|| {
            runner
                .run(&descriptor.path, &["rev-parse", "--abbrev-ref", "HEAD"])
                .ok()
        })
        .flatten()
        .filter(|output| output.success && !output.stdout.is_empty() && output.stdout != "HEAD")
        .map(|output| output.stdout);
    let fork_commit = (!descriptor.force_released && descriptor.path.exists())
        .then(|| GitRepositoryService::default().head(&descriptor.path).ok())
        .flatten()
        .unwrap_or_else(|| main_head.to_string());
    let released = descriptor.force_released || group_closed || branch.is_none();
    WorkspaceState {
        version: VERSION.to_string(),
        id: descriptor.id.clone(),
        dynamic_run_id: dynamic_run_id.to_string(),
        kind: WorkspaceKind::Worktree,
        ownership: WorkspaceOwnership::Runtime,
        repo_root: repo_root.to_path_buf(),
        path: descriptor.path,
        branch: Some(branch.unwrap_or_else(|| format!("legacy/{}", descriptor.id))),
        parent_workspace_id: Some(descriptor.parent_workspace_id),
        created_by_group_id: Some(descriptor.created_by_group_id),
        fork_commit,
        checkpoint_commit: None,
        status: if released {
            WorkspaceStatus::Released
        } else {
            WorkspaceStatus::Active
        },
        created_at: created_at.to_string(),
        updated_at: updated_at.to_string(),
    }
}

fn legacy_workspace_id(dynamic_run_id: &str, identity: &str) -> String {
    let digest = blake3::hash(format!("{dynamic_run_id}:{identity}").as_bytes()).to_hex();
    format!("workspace-legacy-{}", &digest[..16])
}

fn normalized_path(path: &Utf8Path) -> String {
    let normalized = path.as_str().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    optional_string(object, field)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("dynamic graph field `{field}` is missing"))
}

fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str)
}

fn required_string_array(object: &Map<String, Value>, field: &str) -> Result<Vec<String>> {
    object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("dynamic graph field `{field}` is missing"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| anyhow!("dynamic graph field `{field}` must contain strings"))
        })
        .collect::<Result<Vec<_>>>()
        .with_context(|| format!("failed to read dynamic graph field `{field}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn legacy_graph() -> Value {
        json!({
            "version": "0.1",
            "run": {
                "version": "0.1",
                "id": "dynamic-run-001",
                "parentRunId": "run-001",
                "parentRoundId": "round-001",
                "parentNodeId": "ai-dynamic",
                "parentAttemptId": "attempt-001",
                "status": "paused",
                "outcome": null,
                "pauseReason": "process-interrupted",
                "startedAt": "2026-08-01T00:00:00Z",
                "updatedAt": "2026-08-01T00:01:00Z",
                "control": {},
                "allowedWorkflowSnapshots": [],
                "currentNodeIds": ["bootstrap"]
            },
            "nodes": [{
                "version": "0.1",
                "id": "bootstrap",
                "dynamicRunId": "dynamic-run-001",
                "kind": "worker",
                "title": "Bootstrap",
                "task": "Plan",
                "status": "running",
                "outcome": null,
                "groupId": null,
                "chainId": "bootstrap",
                "depth": 0,
                "dependsOn": [],
                "workspace": { "mode": "readonly" },
                "workspacePath": "D:/repo",
                "provider": "codex-acp",
                "profile": null,
                "permissionMode": null,
                "model": null,
                "sessionMode": "new",
                "continueFromNodeId": null,
                "workflowId": null,
                "workflowSnapshotId": null,
                "childRunId": null,
                "startedAt": "2026-08-01T00:00:00Z",
                "finishedAt": null
            }],
            "groups": [],
            "proposals": []
        })
    }

    #[test]
    fn v01_single_workspace_migrates_to_catalog_once() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let (graph, migrated) =
            dynamic_graph_from_value_with_migration(legacy_graph(), &repo).unwrap();
        assert!(migrated);
        assert_eq!(graph.version, CURRENT_DYNAMIC_GRAPH_VERSION);
        assert_eq!(graph.workspaces.len(), 1);
        assert_eq!(graph.nodes[0].workspace_id, MAIN_WORKSPACE_ID);

        let (again, migrated_again) =
            dynamic_graph_from_value_with_migration(serde_json::to_value(&graph).unwrap(), &repo)
                .unwrap();
        assert!(!migrated_again);
        assert_eq!(
            serde_json::to_value(again).unwrap(),
            serde_json::to_value(graph).unwrap()
        );
    }

    #[test]
    fn load_v01_graph_persists_once_and_second_load_is_a_noop() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let graph_path = repo.join("graph.json");
        write_json(&graph_path, &legacy_graph()).unwrap();

        let migrated = load_dynamic_graph(&graph_path, &repo).unwrap();
        assert_eq!(migrated.version, CURRENT_DYNAMIC_GRAPH_VERSION);
        let first_persisted = std::fs::read(graph_path.as_std_path()).unwrap();

        let loaded_again = load_dynamic_graph(&graph_path, &repo).unwrap();
        let second_persisted = std::fs::read(graph_path.as_std_path()).unwrap();
        assert_eq!(loaded_again.version, CURRENT_DYNAMIC_GRAPH_VERSION);
        assert_eq!(second_persisted, first_persisted);
    }

    #[test]
    fn current_graph_load_does_not_rewrite_disk() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let graph_path = repo.join("graph.json");
        let (graph, _) = dynamic_graph_from_value_with_migration(legacy_graph(), &repo).unwrap();
        write_json(&graph_path, &graph).unwrap();
        let before = std::fs::read(graph_path.as_std_path()).unwrap();

        let loaded = load_dynamic_graph(&graph_path, &repo).unwrap();

        assert_eq!(loaded.version, CURRENT_DYNAMIC_GRAPH_VERSION);
        assert_eq!(std::fs::read(graph_path.as_std_path()).unwrap(), before);
    }

    #[test]
    fn future_graph_version_is_rejected_without_touching_disk() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let graph_path = repo.join("graph.json");
        let mut value = legacy_graph();
        value["version"] = json!("9.0");
        write_json(&graph_path, &value).unwrap();
        let before = std::fs::read(graph_path.as_std_path()).unwrap();

        let error = load_dynamic_graph(&graph_path, &repo).unwrap_err();

        assert!(error.to_string().contains("9.0"));
        assert_eq!(std::fs::read(graph_path.as_std_path()).unwrap(), before);
    }

    #[test]
    fn v01_readonly_fanout_preserves_group_topology_as_historical_workspaces() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let mut value = legacy_graph();
        value["nodes"] = json!([
            value["nodes"][0].clone(),
            legacy_node("left", "left", Some("group-1"), "readonly"),
            legacy_node("right", "right", Some("group-1"), "readonly")
        ]);
        value["groups"] = json!([{
            "version": "0.1",
            "id": "group-1",
            "dynamicRunId": "dynamic-run-001",
            "status": "open",
            "depth": 1,
            "parentGroupId": null,
            "rootNodeIds": ["left", "right"],
            "terminalNodeIds": [],
            "mergeNodeId": null,
            "acceptanceNodeId": null,
            "createdByNodeId": "bootstrap",
            "merge": { "title": "Merge", "task": "Merge" },
            "acceptance": { "title": "Accept", "task": "Accept" },
            "createdAt": "2026-08-01T00:00:00Z",
            "updatedAt": "2026-08-01T00:01:00Z"
        }]);

        let (graph, migrated) = dynamic_graph_from_value_with_migration(value, &repo).unwrap();
        assert!(migrated);
        let group = &graph.groups[0];
        assert_eq!(group.target_workspace_id, MAIN_WORKSPACE_ID);
        assert_eq!(group.child_workspace_ids.len(), 2);
        assert_ne!(group.child_workspace_ids[0], group.child_workspace_ids[1]);
        assert!(group.child_workspace_ids.iter().all(|id| {
            graph.workspaces.iter().any(|workspace| {
                workspace.id == *id && workspace.status == WorkspaceStatus::Released
            })
        }));
    }

    #[test]
    fn v01_missing_worktree_fanout_is_preserved_as_released() {
        let repo = Utf8PathBuf::from_path_buf(tempdir().unwrap().keep()).unwrap();
        let mut value = legacy_graph();
        let mut left = legacy_node("left", "left", Some("group-1"), "worktree");
        let mut right = legacy_node("right", "right", Some("group-1"), "worktree");
        left["workspacePath"] = json!(repo.join("missing-left"));
        right["workspacePath"] = json!(repo.join("missing-right"));
        value["nodes"] = json!([value["nodes"][0].clone(), left, right]);
        value["groups"] = json!([legacy_group()]);

        let (graph, migrated) = dynamic_graph_from_value_with_migration(value, &repo).unwrap();

        assert!(migrated);
        let group = &graph.groups[0];
        assert_eq!(group.target_workspace_id, MAIN_WORKSPACE_ID);
        assert_eq!(group.child_workspace_ids.len(), 2);
        assert_ne!(group.child_workspace_ids[0], group.child_workspace_ids[1]);
        for (node_id, workspace_id) in ["left", "right"]
            .into_iter()
            .zip(&group.child_workspace_ids)
        {
            assert_eq!(
                graph
                    .nodes
                    .iter()
                    .find(|node| node.id == node_id)
                    .unwrap()
                    .workspace_id,
                *workspace_id
            );
            let workspace = graph
                .workspaces
                .iter()
                .find(|workspace| workspace.id == *workspace_id)
                .unwrap();
            assert_eq!(workspace.status, WorkspaceStatus::Released);
            assert_eq!(
                workspace.parent_workspace_id.as_deref(),
                Some(MAIN_WORKSPACE_ID)
            );
            assert_eq!(workspace.created_by_group_id.as_deref(), Some("group-1"));
        }
    }

    fn legacy_group() -> Value {
        json!({
            "version": "0.1",
            "id": "group-1",
            "dynamicRunId": "dynamic-run-001",
            "status": "open",
            "depth": 1,
            "parentGroupId": null,
            "rootNodeIds": ["left", "right"],
            "terminalNodeIds": [],
            "mergeNodeId": null,
            "acceptanceNodeId": null,
            "createdByNodeId": "bootstrap",
            "merge": { "title": "Merge", "task": "Merge" },
            "acceptance": { "title": "Accept", "task": "Accept" },
            "createdAt": "2026-08-01T00:00:00Z",
            "updatedAt": "2026-08-01T00:01:00Z"
        })
    }

    fn legacy_node(id: &str, chain_id: &str, group_id: Option<&str>, mode: &str) -> Value {
        json!({
            "version": "0.1",
            "id": id,
            "dynamicRunId": "dynamic-run-001",
            "kind": "worker",
            "title": id,
            "task": id,
            "status": "running",
            "outcome": null,
            "groupId": group_id,
            "chainId": chain_id,
            "depth": 1,
            "dependsOn": [],
            "workspace": { "mode": mode },
            "workspacePath": null,
            "provider": "codex-acp",
            "profile": null,
            "permissionMode": null,
            "model": null,
            "sessionMode": "new",
            "continueFromNodeId": null,
            "workflowId": null,
            "workflowSnapshotId": null,
            "childRunId": null,
            "startedAt": "2026-08-01T00:00:00Z",
            "finishedAt": null
        })
    }
}
