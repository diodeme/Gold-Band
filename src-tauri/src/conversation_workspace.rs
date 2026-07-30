use std::collections::{HashMap, HashSet};

use camino::{Utf8Path, Utf8PathBuf};
use gold_band::app::App;
use gold_band::config::{ConversationWorkspaceEntry, StateConfig};
use gold_band::storage::GoldBandPaths;

use crate::state::DesktopContext;

pub(crate) const CURRENT_STATE_SCHEMA_VERSION: u32 = 1;

pub(crate) fn project_ids_match(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

pub(crate) fn project_id_for_workspace(workspace_path: &str) -> String {
    GoldBandPaths::new(Utf8PathBuf::from(workspace_path)).project_id
}

pub(crate) fn workspace_entry_for_project(
    state: &StateConfig,
    project_id: &str,
) -> Option<(String, String)> {
    find_workspace_entry(state, project_id).map(|workspace| {
        (
            workspace.workspace_path.clone(),
            workspace.project_id.clone(),
        )
    })
}

pub(crate) fn app_for_workspace(
    context: &DesktopContext,
    workspace_path: &str,
) -> anyhow::Result<App> {
    Ok(App::with_config(
        Utf8PathBuf::from(workspace_path),
        context.config.clone(),
    ))
}

pub(crate) fn remove_workspace_from_state(
    state: &mut StateConfig,
    requested_project_id: &str,
) -> Option<ConversationWorkspaceEntry> {
    let workspace = find_workspace_entry(state, requested_project_id)?.clone();
    let workspace_key = normalized_workspace_key(Utf8Path::new(&workspace.workspace_path));
    let resolved_project_id = workspace.project_id.clone();
    let generated_project_id = project_id_for_workspace(&workspace.workspace_path);
    let related_project_ids = [
        requested_project_id,
        resolved_project_id.as_str(),
        generated_project_id.as_str(),
    ];
    let is_related_project = |project_id: &str| {
        related_project_ids
            .iter()
            .any(|related| project_ids_match(project_id, related))
    };

    state.conversation_workspaces.retain(|candidate| {
        normalized_workspace_key(Utf8Path::new(&candidate.workspace_path)) != workspace_key
    });
    state
        .conversation_pins
        .retain(|pin| !is_related_project(&pin.project_id));
    state
        .conversation_run_modes
        .retain(|project_id, _| !is_related_project(project_id));
    if state
        .last_conversation_workspace
        .as_deref()
        .is_some_and(is_related_project)
    {
        state.last_conversation_workspace = state
            .conversation_workspaces
            .first()
            .map(|workspace| workspace.project_id.clone());
    }

    Some(workspace)
}

pub(crate) fn migrate_conversation_workspace_state(
    default_workspace: Option<&Utf8Path>,
    state: &mut StateConfig,
) -> bool {
    if state.state_schema_version >= CURRENT_STATE_SCHEMA_VERSION {
        return false;
    }

    let mut aliases = Vec::<(String, String)>::new();
    let mut canonical_by_workspace = HashMap::<String, String>::new();
    let mut migrated_workspaces = Vec::<ConversationWorkspaceEntry>::new();

    for mut workspace in std::mem::take(&mut state.conversation_workspaces) {
        let workspace_key = normalized_workspace_key(Utf8Path::new(&workspace.workspace_path));
        let generated_project_id = project_id_for_workspace(&workspace.workspace_path);
        let canonical_project_id = canonical_by_workspace
            .entry(workspace_key)
            .or_insert_with(|| generated_project_id.clone())
            .clone();
        aliases.push((workspace.project_id.clone(), canonical_project_id.clone()));
        aliases.push((generated_project_id, canonical_project_id.clone()));
        if migrated_workspaces
            .iter()
            .any(|candidate| candidate.project_id == canonical_project_id)
        {
            continue;
        }
        workspace.project_id = canonical_project_id;
        migrated_workspaces.push(workspace);
    }

    if let Some(default_workspace) = default_workspace {
        let workspace_key = normalized_workspace_key(default_workspace);
        if !canonical_by_workspace.contains_key(&workspace_key) {
            let workspace_path = default_workspace.to_string();
            let project_id = project_id_for_workspace(&workspace_path);
            let name = default_workspace
                .file_name()
                .unwrap_or("Workspace")
                .to_string();
            canonical_by_workspace.insert(workspace_key, project_id.clone());
            aliases.push((project_id.clone(), project_id.clone()));
            migrated_workspaces.push(ConversationWorkspaceEntry {
                project_id,
                workspace_path,
                name,
                added_at: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    let canonical_ids = migrated_workspaces
        .iter()
        .map(|workspace| workspace.project_id.clone())
        .collect::<HashSet<_>>();
    state.conversation_workspaces = migrated_workspaces;

    state.last_conversation_workspace = state
        .last_conversation_workspace
        .take()
        .and_then(|project_id| canonical_project_id(&project_id, &aliases));

    let mut run_modes = std::mem::take(&mut state.conversation_run_modes)
        .into_iter()
        .filter_map(|(project_id, entry)| {
            let canonical = canonical_project_id(&project_id, &aliases)?;
            let preferred = canonical_ids.contains(&project_id);
            Some((canonical, entry, preferred))
        })
        .collect::<Vec<_>>();
    run_modes.sort_by_key(|(_, _, preferred)| *preferred);
    for (project_id, entry, _) in run_modes {
        state.conversation_run_modes.insert(project_id, entry);
    }

    let mut seen_pins = HashSet::new();
    state.conversation_pins = std::mem::take(&mut state.conversation_pins)
        .into_iter()
        .filter_map(|mut pin| {
            pin.project_id = canonical_project_id(&pin.project_id, &aliases)?;
            seen_pins
                .insert((pin.project_id.clone(), pin.task_id.clone()))
                .then_some(pin)
        })
        .collect();

    if state.last_conversation_workspace.is_none() {
        state.last_conversation_workspace = state
            .conversation_workspaces
            .first()
            .map(|workspace| workspace.project_id.clone());
    }
    state.state_schema_version = CURRENT_STATE_SCHEMA_VERSION;
    true
}

fn canonical_project_id(project_id: &str, aliases: &[(String, String)]) -> Option<String> {
    aliases
        .iter()
        .find(|(alias, _)| project_ids_match(alias, project_id))
        .map(|(_, canonical)| canonical.clone())
}

fn find_workspace_entry<'a>(
    state: &'a StateConfig,
    project_id: &str,
) -> Option<&'a ConversationWorkspaceEntry> {
    state.conversation_workspaces.iter().find(|workspace| {
        project_ids_match(&workspace.project_id, project_id)
            || project_ids_match(
                &project_id_for_workspace(&workspace.workspace_path),
                project_id,
            )
    })
}

fn normalized_workspace_key(workspace_path: &Utf8Path) -> String {
    GoldBandPaths::new(workspace_path.to_path_buf()).normalized_repo_root
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::config::{
        ConversationDirectConfig, ConversationRunMode, ConversationRunModeEntry,
    };
    use tempfile::tempdir;

    fn run_mode(mode: ConversationRunMode) -> ConversationRunModeEntry {
        ConversationRunModeEntry {
            mode,
            workflow_template_id: None,
            include_interview: None,
            direct_config: None,
            direct_preferences: Default::default(),
            auto_config: None,
        }
    }

    #[test]
    fn migration_normalizes_workspace_ids_and_prefers_canonical_run_mode() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("AI-Training");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = Utf8PathBuf::from_path_buf(workspace).unwrap();
        let canonical = project_id_for_workspace(workspace.as_str());
        let legacy = canonical.to_ascii_lowercase();
        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: legacy.clone(),
                workspace_path: workspace.to_string(),
                name: "AI-Training".to_string(),
                added_at: "2026-01-01T00:00:00Z".to_string(),
            });
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: "duplicate-alias".to_string(),
                workspace_path: workspace.to_string(),
                name: "Duplicate AI-Training".to_string(),
                added_at: "2026-01-02T00:00:00Z".to_string(),
            });
        state.last_conversation_workspace = Some(canonical.clone());
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: canonical.clone(),
                task_id: "task-001".to_string(),
                order: 0,
            });
        state
            .conversation_run_modes
            .insert(legacy, run_mode(ConversationRunMode::Workflow));
        let mut direct = run_mode(ConversationRunMode::Direct);
        direct.direct_config = Some(ConversationDirectConfig {
            agent_type: "claude-acp".to_string(),
            model_id: Some("model-a".to_string()),
            permission_mode: None,
            config_options: Default::default(),
        });
        state
            .conversation_run_modes
            .insert(canonical.clone(), direct);

        assert!(migrate_conversation_workspace_state(
            Some(workspace.as_path()),
            &mut state,
        ));

        assert_eq!(state.state_schema_version, CURRENT_STATE_SCHEMA_VERSION);
        assert_eq!(state.conversation_workspaces.len(), 1);
        assert_eq!(state.conversation_workspaces[0].project_id, canonical);
        assert_eq!(
            state.last_conversation_workspace.as_deref(),
            Some(canonical.as_str())
        );
        assert_eq!(state.conversation_run_modes.len(), 1);
        assert_eq!(
            state.conversation_run_modes[&canonical].mode,
            ConversationRunMode::Direct,
        );
        assert_eq!(
            state.conversation_run_modes[&canonical]
                .direct_config
                .as_ref()
                .and_then(|config| config.model_id.as_deref()),
            Some("model-a"),
        );
        assert_eq!(state.conversation_pins.len(), 1);
        assert_eq!(state.conversation_pins[0].project_id, canonical);
    }

    #[test]
    fn migration_is_skipped_after_schema_version_is_current() {
        let mut state = StateConfig::default();
        state.state_schema_version = CURRENT_STATE_SCHEMA_VERSION;
        state.last_conversation_workspace = Some("leave-this-unchanged".to_string());
        let before = serde_json::to_value(&state).unwrap();

        assert!(!migrate_conversation_workspace_state(None, &mut state));
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }

    #[test]
    fn migration_seeds_default_workspace_once() {
        let dir = tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(dir.path().join("SeededWorkspace")).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let expected_project_id = project_id_for_workspace(workspace.as_str());
        let mut state = StateConfig::default();

        assert!(migrate_conversation_workspace_state(
            Some(workspace.as_path()),
            &mut state,
        ));
        assert_eq!(state.conversation_workspaces.len(), 1);
        assert_eq!(
            state.conversation_workspaces[0].project_id,
            expected_project_id
        );

        assert!(!migrate_conversation_workspace_state(
            Some(workspace.as_path()),
            &mut state,
        ));
        assert_eq!(state.conversation_workspaces.len(), 1);
    }

    #[test]
    fn remove_workspace_uses_resolved_identity_and_cleans_related_state() {
        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: "f--file-ai-training".to_string(),
                workspace_path: "F:/file/ai-training".to_string(),
                name: "ai-training".to_string(),
                added_at: "2026-01-01T00:00:00Z".to_string(),
            });
        state.last_conversation_workspace = Some("F--file-ai-training".to_string());
        state.conversation_run_modes.insert(
            "F--file-ai-training".to_string(),
            run_mode(ConversationRunMode::Direct),
        );
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: "F--file-ai-training".to_string(),
                task_id: "task-001".to_string(),
                order: 0,
            });

        let removed = remove_workspace_from_state(&mut state, "F--file-ai-training").unwrap();

        assert_eq!(removed.workspace_path, "F:/file/ai-training");
        assert!(state.conversation_workspaces.is_empty());
        assert!(state.conversation_run_modes.is_empty());
        assert!(state.conversation_pins.is_empty());
        assert!(state.last_conversation_workspace.is_none());
    }
}
