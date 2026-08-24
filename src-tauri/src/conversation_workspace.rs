use std::collections::{HashMap, HashSet};
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use gold_band::app::App;
use gold_band::config::{ConversationWorkspaceEntry, StateConfig};
use gold_band::storage::core_state::{CoreStateDatabase, WORKSPACE_IDENTITY_SCHEMA_VERSION};
use gold_band::storage::sqlite::SearchIndex;
use gold_band::storage::{
    GoldBandPaths, ProjectManifest, legacy_project_id_for_workspace, read_json, write_json,
};

use crate::state::DesktopContext;

pub(crate) const CURRENT_STATE_SCHEMA_VERSION: u32 = 2;

pub(crate) fn project_ids_match(left: &str, right: &str) -> bool {
    left == right
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
    let project_id = workspace.project_id.as_str();
    state
        .conversation_workspaces
        .retain(|candidate| candidate.project_id != project_id);
    state
        .conversation_pins
        .retain(|pin| pin.project_id != project_id);
    state
        .conversation_run_modes
        .retain(|candidate, _| candidate != project_id);
    if state
        .last_conversation_workspace
        .as_deref()
        .is_some_and(|candidate| candidate == project_id)
    {
        state.last_conversation_workspace = state
            .conversation_workspaces
            .first()
            .map(|workspace| workspace.project_id.clone());
    }

    Some(workspace)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspaceIdentityMigrationReport {
    pub migrated_directory_count: usize,
    pub already_migrated_directory_count: usize,
    pub state_changed: bool,
}

#[derive(Debug)]
pub(crate) enum WorkspaceIdentityMigrationError {
    CoreState(gold_band::storage::core_state::CoreStateError),
    Io(std::io::Error),
    Conflict {
        source: Utf8PathBuf,
        target: Utf8PathBuf,
    },
    ManifestInvalid {
        path: Utf8PathBuf,
    },
    RuntimeStateInvalid {
        path: Utf8PathBuf,
    },
    UnresolvedDirectory {
        path: Utf8PathBuf,
    },
    Storage(anyhow::Error),
    Search(anyhow::Error),
}

impl From<gold_band::storage::core_state::CoreStateError> for WorkspaceIdentityMigrationError {
    fn from(error: gold_band::storage::core_state::CoreStateError) -> Self {
        Self::CoreState(error)
    }
}

impl From<std::io::Error> for WorkspaceIdentityMigrationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for WorkspaceIdentityMigrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CoreState(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Conflict { source, target } => write!(
                formatter,
                "workspace identity migration conflict between {source} and {target}"
            ),
            Self::ManifestInvalid { path } => write!(
                formatter,
                "workspace identity migration manifest is invalid at {path}"
            ),
            Self::RuntimeStateInvalid { path } => write!(
                formatter,
                "workspace identity migration runtime state is invalid at {path}"
            ),
            Self::UnresolvedDirectory { path } => write!(
                formatter,
                "workspace identity migration cannot resolve project directory {path}"
            ),
            Self::Storage(error) => write!(
                formatter,
                "workspace identity migration storage write failed: {error}"
            ),
            Self::Search(error) => {
                write!(
                    formatter,
                    "workspace search projection rebuild failed: {error}"
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceIdentityMigrationError {}

impl WorkspaceIdentityMigrationError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Conflict { .. } => "workspace.identity-migration-conflict",
            Self::ManifestInvalid { .. } => "workspace.identity-migration-manifest-invalid",
            Self::RuntimeStateInvalid { .. } => {
                "workspace.identity-migration-runtime-state-invalid"
            }
            Self::UnresolvedDirectory { .. } => "workspace.identity-migration-source-missing",
            Self::CoreState(_) | Self::Io(_) | Self::Storage(_) | Self::Search(_) => {
                "workspace.identity-migration-failed"
            }
        }
    }
}

#[derive(Debug, Clone)]
struct WorkspaceIdentityPlanItem {
    workspace_path: Utf8PathBuf,
    normalized_workspace_path: String,
    old_project_id: String,
    new_project_id: String,
    old_runtime_root: Utf8PathBuf,
    new_runtime_root: Utf8PathBuf,
}

#[derive(Debug, Clone)]
struct DiscoveredProjectDirectory {
    runtime_root: Utf8PathBuf,
    manifest_project_id: String,
    is_new_layout: bool,
}

pub(crate) struct WorkspaceIdentityMigrator<'a> {
    base_paths: &'a GoldBandPaths,
    core_state: CoreStateDatabase,
}

impl<'a> WorkspaceIdentityMigrator<'a> {
    pub(crate) fn new(base_paths: &'a GoldBandPaths) -> Self {
        Self {
            base_paths,
            core_state: CoreStateDatabase::new(base_paths.core_db_path()),
        }
    }

    pub(crate) fn execute(
        &self,
        default_workspace: Option<&Utf8Path>,
        state: &mut StateConfig,
    ) -> Result<WorkspaceIdentityMigrationReport, WorkspaceIdentityMigrationError> {
        if self.core_state.workspace_identity_version()? == Some(WORKSPACE_IDENTITY_SCHEMA_VERSION)
        {
            return Ok(WorkspaceIdentityMigrationReport::default());
        }

        let plan = self.plan(default_workspace, state)?;
        let mut report = WorkspaceIdentityMigrationReport::default();
        for item in &plan {
            if item.old_runtime_root.is_dir() {
                if let Some(parent) = item.new_runtime_root.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                match std::fs::rename(
                    item.old_runtime_root.as_std_path(),
                    item.new_runtime_root.as_std_path(),
                ) {
                    Ok(()) => report.migrated_directory_count += 1,
                    Err(_) if !item.old_runtime_root.exists() && item.new_runtime_root.is_dir() => {
                        validate_existing_manifest(item)?;
                        report.already_migrated_directory_count += 1;
                    }
                    Err(error) => return Err(error.into()),
                }
            } else if item.new_runtime_root.is_dir() {
                report.already_migrated_directory_count += 1;
            }
            if item.new_runtime_root.is_dir() {
                let paths = GoldBandPaths::new_with_path_config(
                    item.workspace_path.clone(),
                    self.base_paths.storage_path_config(),
                );
                paths
                    .replace_project_manifest_for_migration()
                    .map_err(WorkspaceIdentityMigrationError::Storage)?;
                rewrite_runtime_locators(item)?;
            }
        }

        report.state_changed = migrate_state_references(
            default_workspace,
            state,
            self.base_paths.storage_path_config(),
        );
        write_json(&self.base_paths.user_state_file(), state)
            .map_err(WorkspaceIdentityMigrationError::Storage)?;

        let search = SearchIndex::open(&self.base_paths.sqlite_db_path())
            .map_err(|error| WorkspaceIdentityMigrationError::Search(error.into()))?;
        search
            .rebuild_from_disk(&self.base_paths.projects_dir())
            .map_err(WorkspaceIdentityMigrationError::Search)?;
        self.core_state.mark_workspace_identity_migrated()?;
        Ok(report)
    }

    fn plan(
        &self,
        default_workspace: Option<&Utf8Path>,
        state: &StateConfig,
    ) -> Result<Vec<WorkspaceIdentityPlanItem>, WorkspaceIdentityMigrationError> {
        let mut workspace_paths = HashMap::<String, Utf8PathBuf>::new();
        let mut discovered_projects = HashMap::<String, DiscoveredProjectDirectory>::new();
        for path in state
            .conversation_workspaces
            .iter()
            .map(|workspace| workspace.workspace_path.as_str())
            .chain(state.recent_desktop_workspaces.iter().map(String::as_str))
            .chain(default_workspace.into_iter().map(Utf8Path::as_str))
        {
            let paths = GoldBandPaths::new_with_path_config(
                Utf8PathBuf::from(path),
                self.base_paths.storage_path_config(),
            );
            workspace_paths
                .entry(paths.normalized_repo_root)
                .or_insert(paths.repo_root);
        }

        let projects_dir = self.base_paths.projects_dir();
        if projects_dir.is_dir() {
            for entry in std::fs::read_dir(projects_dir.as_std_path())? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let directory = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                    WorkspaceIdentityMigrationError::UnresolvedDirectory {
                        path: Utf8PathBuf::from(path.to_string_lossy().into_owned()),
                    }
                })?;
                let manifest_path = directory.join("project.json");
                if manifest_path.is_file() {
                    let manifest: ProjectManifest = read_json(&manifest_path).map_err(|_| {
                        WorkspaceIdentityMigrationError::ManifestInvalid {
                            path: manifest_path.clone(),
                        }
                    })?;
                    let repo_root = Utf8PathBuf::from(&manifest.repo_root);
                    let paths = GoldBandPaths::new_with_path_config(
                        repo_root.clone(),
                        self.base_paths.storage_path_config(),
                    );
                    if paths.normalized_repo_root != manifest.normalized_repo_root {
                        return Err(WorkspaceIdentityMigrationError::ManifestInvalid {
                            path: manifest_path,
                        });
                    }
                    let directory_name = directory.file_name().unwrap_or_default();
                    let is_new_layout = directory_name == paths.project_id;
                    if !is_new_layout && directory_name != manifest.project_id {
                        return Err(WorkspaceIdentityMigrationError::ManifestInvalid {
                            path: manifest_path,
                        });
                    }
                    let discovered = DiscoveredProjectDirectory {
                        runtime_root: directory.clone(),
                        manifest_project_id: manifest.project_id,
                        is_new_layout,
                    };
                    if let Some(existing) =
                        discovered_projects.insert(paths.normalized_repo_root.clone(), discovered)
                    {
                        return Err(WorkspaceIdentityMigrationError::Conflict {
                            source: existing.runtime_root,
                            target: directory,
                        });
                    }
                    workspace_paths
                        .entry(paths.normalized_repo_root)
                        .or_insert(repo_root);
                }
            }
        }

        let mut plan = workspace_paths
            .into_iter()
            .map(|(normalized_workspace_path, workspace_path)| {
                let paths = GoldBandPaths::new_with_path_config(
                    workspace_path.clone(),
                    self.base_paths.storage_path_config(),
                );
                let discovered = discovered_projects.remove(&normalized_workspace_path);
                let old_project_id = discovered
                    .as_ref()
                    .filter(|project| project.manifest_project_id != paths.project_id)
                    .map(|project| project.manifest_project_id.clone())
                    .unwrap_or_else(|| legacy_project_id_for_workspace(&workspace_path));
                let old_runtime_root = discovered
                    .filter(|project| !project.is_new_layout)
                    .map(|project| project.runtime_root)
                    .unwrap_or_else(|| paths.projects_dir().join(&old_project_id));
                WorkspaceIdentityPlanItem {
                    workspace_path,
                    normalized_workspace_path,
                    old_runtime_root,
                    new_runtime_root: paths.runtime_root,
                    old_project_id,
                    new_project_id: paths.project_id,
                }
            })
            .collect::<Vec<_>>();
        plan.sort_by(|left, right| left.new_project_id.cmp(&right.new_project_id));

        let mut new_owners = HashMap::<String, String>::new();
        for item in &plan {
            if let Some(owner) = new_owners.insert(
                item.new_project_id.clone(),
                item.normalized_workspace_path.clone(),
            ) && owner != item.normalized_workspace_path
            {
                return Err(WorkspaceIdentityMigrationError::Conflict {
                    source: item.old_runtime_root.clone(),
                    target: item.new_runtime_root.clone(),
                });
            }
            if item.old_runtime_root.is_dir() && item.new_runtime_root.is_dir() {
                return Err(WorkspaceIdentityMigrationError::Conflict {
                    source: item.old_runtime_root.clone(),
                    target: item.new_runtime_root.clone(),
                });
            }
            validate_existing_manifest(item)?;
        }

        Ok(plan)
    }
}

fn validate_existing_manifest(
    item: &WorkspaceIdentityPlanItem,
) -> Result<(), WorkspaceIdentityMigrationError> {
    let runtime_root = if item.old_runtime_root.is_dir() {
        &item.old_runtime_root
    } else if item.new_runtime_root.is_dir() {
        &item.new_runtime_root
    } else {
        return Ok(());
    };
    let path = runtime_root.join("project.json");
    let manifest: ProjectManifest = read_json(&path)
        .map_err(|_| WorkspaceIdentityMigrationError::ManifestInvalid { path: path.clone() })?;
    let valid_project_id =
        manifest.project_id == item.old_project_id || manifest.project_id == item.new_project_id;
    if !valid_project_id || manifest.normalized_repo_root != item.normalized_workspace_path {
        return Err(WorkspaceIdentityMigrationError::ManifestInvalid { path });
    }
    Ok(())
}

fn migrate_state_references(
    default_workspace: Option<&Utf8Path>,
    state: &mut StateConfig,
    path_config: gold_band::storage::StoragePathConfig,
) -> bool {
    if state.state_schema_version >= CURRENT_STATE_SCHEMA_VERSION {
        return false;
    }
    let mut aliases = Vec::<(String, String)>::new();
    let mut migrated_workspaces = Vec::<ConversationWorkspaceEntry>::new();
    let mut normalized_paths = HashSet::new();
    for mut workspace in std::mem::take(&mut state.conversation_workspaces) {
        let paths = GoldBandPaths::new_with_path_config(
            Utf8PathBuf::from(&workspace.workspace_path),
            path_config,
        );
        aliases.push((workspace.project_id.clone(), paths.project_id.clone()));
        aliases.push((
            legacy_project_id_for_workspace(&paths.repo_root),
            paths.project_id.clone(),
        ));
        aliases.push((paths.project_id.clone(), paths.project_id.clone()));
        if normalized_paths.insert(paths.normalized_repo_root) {
            workspace.project_id = paths.project_id;
            migrated_workspaces.push(workspace);
        }
    }
    if let Some(default_workspace) = default_workspace {
        let paths =
            GoldBandPaths::new_with_path_config(default_workspace.to_path_buf(), path_config);
        if normalized_paths.insert(paths.normalized_repo_root) {
            aliases.push((paths.project_id.clone(), paths.project_id.clone()));
            migrated_workspaces.push(ConversationWorkspaceEntry {
                project_id: paths.project_id,
                workspace_path: paths.repo_root.to_string(),
                name: default_workspace
                    .file_name()
                    .unwrap_or("Workspace")
                    .to_string(),
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
        .and_then(|project_id| canonical_project_id_for_migration(&project_id, &aliases));

    let mut run_modes = std::mem::take(&mut state.conversation_run_modes)
        .into_iter()
        .filter_map(|(project_id, entry)| {
            let canonical = canonical_project_id_for_migration(&project_id, &aliases)?;
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
            pin.project_id = canonical_project_id_for_migration(&pin.project_id, &aliases)?;
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

fn canonical_project_id_for_migration(
    project_id: &str,
    aliases: &[(String, String)],
) -> Option<String> {
    aliases
        .iter()
        .find(|(alias, _)| migration_alias_matches(alias, project_id))
        .map(|(_, canonical)| canonical.clone())
}

fn migration_alias_matches(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn rewrite_runtime_locators(
    item: &WorkspaceIdentityPlanItem,
) -> Result<(), WorkspaceIdentityMigrationError> {
    let mut pending = vec![item.new_runtime_root.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory.as_std_path())? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                WorkspaceIdentityMigrationError::UnresolvedDirectory {
                    path: Utf8PathBuf::from(path.to_string_lossy().into_owned()),
                }
            })?;
            if file_type.is_dir() && !file_type.is_symlink() {
                pending.push(path);
            } else if file_type.is_file() {
                match path.file_name().unwrap_or_default() {
                    "run.json" => rewrite_json_locator_fields(
                        &path,
                        item,
                        &[&["worktree", "path"], &["last_executed_node", "attemptDir"]],
                    )?,
                    "worker-ref.json" => rewrite_json_locator_fields(
                        &path,
                        item,
                        &[&["continue_ref", "cwd"], &["continue_ref", "snapshotFile"]],
                    )?,
                    "acp.snapshot.json" | "acp.session.json" => {
                        rewrite_json_locator_fields(&path, item, &[&["cwd"]])?
                    }
                    "acp.turn-file-mutations.jsonl" => {
                        rewrite_turn_file_mutation_journal(&path, item)?
                    }
                    file_name
                        if file_name.starts_with("turn-files-") && file_name.ends_with(".json") =>
                    {
                        rewrite_turn_file_change_set(&path, item)?
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn rewrite_json_locator_fields(
    path: &Utf8Path,
    item: &WorkspaceIdentityPlanItem,
    fields: &[&[&str]],
) -> Result<(), WorkspaceIdentityMigrationError> {
    let mut value: serde_json::Value =
        read_json(path).map_err(|_| WorkspaceIdentityMigrationError::RuntimeStateInvalid {
            path: path.to_path_buf(),
        })?;
    let changed = fields.iter().fold(false, |changed, field| {
        rewrite_json_locator(&mut value, field, item) || changed
    });
    if changed {
        write_json(path, &value).map_err(WorkspaceIdentityMigrationError::Storage)?;
    }
    Ok(())
}

fn rewrite_turn_file_change_set(
    path: &Utf8Path,
    item: &WorkspaceIdentityPlanItem,
) -> Result<(), WorkspaceIdentityMigrationError> {
    let mut value: serde_json::Value =
        read_json(path).map_err(|_| WorkspaceIdentityMigrationError::RuntimeStateInvalid {
            path: path.to_path_buf(),
        })?;
    let mut changed = false;
    if let Some(changes) = value
        .get_mut("changes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for change in changes {
            changed = rewrite_json_locator(change, &["logicalPath"], item) || changed;
            changed = rewrite_json_locator(change, &["previousLogicalPath"], item) || changed;
        }
    }
    if changed {
        write_json(path, &value).map_err(WorkspaceIdentityMigrationError::Storage)?;
    }
    Ok(())
}

fn rewrite_turn_file_mutation_journal(
    path: &Utf8Path,
    item: &WorkspaceIdentityPlanItem,
) -> Result<(), WorkspaceIdentityMigrationError> {
    let content = std::fs::read_to_string(path.as_std_path())?;
    let trailing_newline = content.ends_with('\n');
    let mut changed = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            lines.push(line.to_string());
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(line).map_err(|_| {
            WorkspaceIdentityMigrationError::RuntimeStateInvalid {
                path: path.to_path_buf(),
            }
        })?;
        if rewrite_json_locator(&mut value, &["logicalPath"], item) {
            changed = true;
            lines.push(serde_json::to_string(&value).map_err(|_| {
                WorkspaceIdentityMigrationError::RuntimeStateInvalid {
                    path: path.to_path_buf(),
                }
            })?);
        } else {
            lines.push(line.to_string());
        }
    }
    if changed {
        let mut rewritten = lines.join("\n");
        if trailing_newline {
            rewritten.push('\n');
        }
        gold_band::storage::atomic_write_file(path.as_std_path(), |file| -> anyhow::Result<()> {
            file.write_all(rewritten.as_bytes())?;
            Ok(())
        })
        .map_err(WorkspaceIdentityMigrationError::Storage)?;
    }
    Ok(())
}

fn rewrite_json_locator(
    value: &mut serde_json::Value,
    field: &[&str],
    item: &WorkspaceIdentityPlanItem,
) -> bool {
    let mut current = value;
    for segment in field {
        let Some(next) = current.get_mut(*segment) else {
            return false;
        };
        current = next;
    }
    let Some(locator) = current.as_str() else {
        return false;
    };
    let Some(replacement) =
        replace_runtime_root(locator, &item.old_runtime_root, &item.new_runtime_root)
    else {
        return false;
    };
    *current = serde_json::Value::String(replacement);
    true
}

fn replace_runtime_root(cwd: &str, old_root: &Utf8Path, new_root: &Utf8Path) -> Option<String> {
    let cwd = cwd.replace('\\', "/");
    let old = old_root.as_str().replace('\\', "/");
    let matches = if cfg!(windows) {
        cwd.get(..old.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&old))
    } else {
        cwd.starts_with(&old)
    };
    if !matches
        || cwd
            .as_bytes()
            .get(old.len())
            .is_some_and(|separator| *separator != b'/')
    {
        return None;
    }
    Some(format!(
        "{}{}",
        new_root.as_str().replace('\\', "/"),
        &cwd[old.len()..]
    ))
}

fn find_workspace_entry<'a>(
    state: &'a StateConfig,
    project_id: &str,
) -> Option<&'a ConversationWorkspaceEntry> {
    state
        .conversation_workspaces
        .iter()
        .find(|workspace| workspace.project_id == project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::config::{
        ConversationDirectConfig, ConversationRunMode, ConversationRunModeEntry,
    };
    use gold_band::storage::StoragePathConfig;
    use tempfile::tempdir;

    fn run_mode(mode: ConversationRunMode) -> ConversationRunModeEntry {
        ConversationRunModeEntry {
            mode,
            workflow_template_id: None,
            optional_entry_preferences: Default::default(),
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
        let legacy = legacy_project_id_for_workspace(&workspace);
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

        assert!(migrate_state_references(
            Some(workspace.as_path()),
            &mut state,
            gold_band::storage::active_storage_path_config(),
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

        assert!(!migrate_state_references(
            None,
            &mut state,
            gold_band::storage::active_storage_path_config(),
        ));
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }

    #[test]
    fn migration_seeds_default_workspace_once() {
        let dir = tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(dir.path().join("SeededWorkspace")).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        let expected_project_id = project_id_for_workspace(workspace.as_str());
        let mut state = StateConfig::default();

        assert!(migrate_state_references(
            Some(workspace.as_path()),
            &mut state,
            gold_band::storage::active_storage_path_config(),
        ));
        assert_eq!(state.conversation_workspaces.len(), 1);
        assert_eq!(
            state.conversation_workspaces[0].project_id,
            expected_project_id
        );

        assert!(!migrate_state_references(
            Some(workspace.as_path()),
            &mut state,
            gold_band::storage::active_storage_path_config(),
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
        state.last_conversation_workspace = Some("f--file-ai-training".to_string());
        state.conversation_run_modes.insert(
            "f--file-ai-training".to_string(),
            run_mode(ConversationRunMode::Direct),
        );
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: "f--file-ai-training".to_string(),
                task_id: "task-001".to_string(),
                order: 0,
            });

        let removed = remove_workspace_from_state(&mut state, "f--file-ai-training").unwrap();

        assert_eq!(removed.workspace_path, "F:/file/ai-training");
        assert!(state.conversation_workspaces.is_empty());
        assert!(state.conversation_run_modes.is_empty());
        assert!(state.conversation_pins.is_empty());
        assert!(state.last_conversation_workspace.is_none());
    }

    #[test]
    fn workspace_lookup_requires_the_exact_canonical_project_id() {
        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: "workspace--a1b2c3d4".to_string(),
                workspace_path: "C:/workspace".to_string(),
                name: "workspace".to_string(),
                added_at: "2026-01-01T00:00:00Z".to_string(),
            });

        assert!(workspace_entry_for_project(&state, "workspace--a1b2c3d4").is_some());
        assert!(workspace_entry_for_project(&state, "WORKSPACE--A1B2C3D4").is_none());
    }

    #[test]
    fn migrator_moves_runtime_data_rewrites_references_and_is_idempotent() {
        let directory = tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("workspace")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        let test_home = directory.path().join("user-home");
        let path_config = StoragePathConfig {
            app_key: "gold-band-identity-move-test",
            config_dir_name: ".gold-band-identity-move-test",
            home_env_var: "GOLD_BAND_IDENTITY_MOVE_TEST_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, &test_home) };
        let paths = GoldBandPaths::new_with_path_config(workspace.clone(), path_config);
        assert!(
            paths
                .user_state_file()
                .as_std_path()
                .starts_with(&test_home)
        );
        let old_project_id = legacy_project_id_for_workspace(&workspace);
        let old_root = paths.projects_dir().join(&old_project_id);
        let task_dir = old_root.join("tasks/task-001");
        let attempt_dir = task_dir.join("runs/run-001/rounds/round-001/nodes/node-001/attempt-001");
        std::fs::create_dir_all(attempt_dir.as_std_path()).unwrap();
        std::fs::create_dir_all(task_dir.join("authoring").as_std_path()).unwrap();
        std::fs::write(
            task_dir.join("authoring/requirement.md").as_std_path(),
            "migrationneedle",
        )
        .unwrap();
        let scheduler_bytes = b"scheduler-data-must-remain-byte-identical";
        std::fs::write(
            old_root.join("scheduled-tasks.db").as_std_path(),
            scheduler_bytes,
        )
        .unwrap();
        write_json(
            &old_root.join("project.json"),
            &ProjectManifest {
                version: gold_band::domain::VERSION.to_string(),
                project_id: old_project_id.clone(),
                repo_root: workspace.to_string(),
                normalized_repo_root: paths.normalized_repo_root.clone(),
            },
        )
        .unwrap();
        write_json(
            &attempt_dir.join("acp.session.json"),
            &serde_json::json!({
                "cwd": attempt_dir,
                "unrelated": old_root,
            }),
        )
        .unwrap();
        let old_worktree = old_root.join("worktrees/worktree-001");
        std::fs::create_dir_all(old_worktree.as_std_path()).unwrap();
        write_json(
            &task_dir.join("runs/run-001/run.json"),
            &serde_json::json!({
                "last_executed_node": {
                    "attemptDir": attempt_dir
                },
                "worktree": {
                    "path": old_worktree,
                    "branch": "gold-band/test",
                    "forkCommit": "abc123"
                }
            }),
        )
        .unwrap();
        write_json(
            &attempt_dir.join("worker-ref.json"),
            &serde_json::json!({
                "continue_ref": {
                    "cwd": old_worktree,
                    "snapshotFile": attempt_dir.join("acp.snapshot.json")
                }
            }),
        )
        .unwrap();
        write_json(
            &attempt_dir.join("acp.snapshot.json"),
            &serde_json::json!({ "cwd": attempt_dir }),
        )
        .unwrap();
        let mutation_journal = attempt_dir.join("acp.turn-file-mutations.jsonl");
        std::fs::write(
            mutation_journal.as_std_path(),
            format!(
                "{}\n",
                serde_json::json!({
                    "logicalPath": attempt_dir.join("attachments/report.md")
                })
            ),
        )
        .unwrap();
        let change_set_dir = attempt_dir.join("turn-file-change-sets");
        std::fs::create_dir_all(change_set_dir.as_std_path()).unwrap();
        write_json(
            &change_set_dir.join("turn-files-test.json"),
            &serde_json::json!({
                "changes": [{
                    "logicalPath": attempt_dir.join("attachments/report.md"),
                    "previousLogicalPath": attempt_dir.join("attachments/old-report.md")
                }]
            }),
        )
        .unwrap();
        let raw_audit = format!(
            "{{\"cwd\":{}}}\n",
            serde_json::to_string(old_worktree.as_str()).unwrap()
        );
        std::fs::write(attempt_dir.join("acp.raw.jsonl").as_std_path(), &raw_audit).unwrap();

        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: old_project_id.clone(),
                workspace_path: workspace.to_string(),
                name: "workspace".to_string(),
                added_at: "2026-01-01T00:00:00Z".to_string(),
            });
        state.last_conversation_workspace = Some(old_project_id.clone());
        state
            .conversation_pins
            .push(gold_band::config::ConversationPin {
                project_id: old_project_id.clone(),
                task_id: "task-001".to_string(),
                order: 0,
            });
        state
            .conversation_run_modes
            .insert(old_project_id, run_mode(ConversationRunMode::Direct));

        let migrator = WorkspaceIdentityMigrator::new(&paths);
        let report = migrator
            .execute(Some(workspace.as_path()), &mut state)
            .unwrap();

        assert_eq!(report.migrated_directory_count, 1);
        assert!(!old_root.exists());
        assert!(paths.runtime_root.is_dir());
        assert_eq!(
            std::fs::read(paths.runtime_root.join("scheduled-tasks.db").as_std_path()).unwrap(),
            scheduler_bytes
        );
        assert_eq!(
            state.conversation_workspaces[0].project_id,
            paths.project_id
        );
        assert_eq!(
            state.last_conversation_workspace.as_deref(),
            Some(paths.project_id.as_str())
        );
        assert_eq!(state.conversation_pins[0].project_id, paths.project_id);
        assert!(state.conversation_run_modes.contains_key(&paths.project_id));
        let migrated_attempt = paths
            .runtime_root
            .join("tasks/task-001/runs/run-001/rounds/round-001/nodes/node-001/attempt-001");
        let session: serde_json::Value =
            read_json(&migrated_attempt.join("acp.session.json")).unwrap();
        assert_eq!(session["cwd"], migrated_attempt.as_str().replace('\\', "/"));
        assert_eq!(session["unrelated"], old_root.as_str());
        let migrated_worktree = paths.runtime_root.join("worktrees/worktree-001");
        let run: serde_json::Value = read_json(
            &paths
                .runtime_root
                .join("tasks/task-001/runs/run-001/run.json"),
        )
        .unwrap();
        assert_eq!(
            run["worktree"]["path"],
            migrated_worktree.as_str().replace('\\', "/")
        );
        assert_eq!(
            run["last_executed_node"]["attemptDir"],
            migrated_attempt.as_str().replace('\\', "/")
        );
        let worker_ref: serde_json::Value =
            read_json(&migrated_attempt.join("worker-ref.json")).unwrap();
        assert_eq!(
            worker_ref["continue_ref"]["cwd"],
            migrated_worktree.as_str().replace('\\', "/")
        );
        assert_eq!(
            worker_ref["continue_ref"]["snapshotFile"],
            migrated_attempt
                .join("acp.snapshot.json")
                .as_str()
                .replace('\\', "/")
        );
        let snapshot: serde_json::Value =
            read_json(&migrated_attempt.join("acp.snapshot.json")).unwrap();
        assert_eq!(
            snapshot["cwd"],
            migrated_attempt.as_str().replace('\\', "/")
        );
        let mutation: serde_json::Value = serde_json::from_str(
            std::fs::read_to_string(
                migrated_attempt
                    .join("acp.turn-file-mutations.jsonl")
                    .as_std_path(),
            )
            .unwrap()
            .trim(),
        )
        .unwrap();
        assert_eq!(
            mutation["logicalPath"],
            migrated_attempt
                .join("attachments/report.md")
                .as_str()
                .replace('\\', "/")
        );
        let change_set: serde_json::Value =
            read_json(&migrated_attempt.join("turn-file-change-sets/turn-files-test.json"))
                .unwrap();
        assert_eq!(
            change_set["changes"][0]["logicalPath"],
            migrated_attempt
                .join("attachments/report.md")
                .as_str()
                .replace('\\', "/")
        );
        assert_eq!(
            change_set["changes"][0]["previousLogicalPath"],
            migrated_attempt
                .join("attachments/old-report.md")
                .as_str()
                .replace('\\', "/")
        );
        assert_eq!(
            std::fs::read_to_string(migrated_attempt.join("acp.raw.jsonl").as_std_path()).unwrap(),
            raw_audit
        );
        let manifest: ProjectManifest = read_json(&paths.project_manifest_file()).unwrap();
        assert_eq!(manifest.project_id, paths.project_id);

        let search = SearchIndex::open(&paths.sqlite_db_path()).unwrap();
        let results = search.search_tasks("migrationneedle", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(
            results[0]
                .task_path
                .starts_with(paths.runtime_root.as_str())
        );
        assert!(
            !Utf8Path::new(&results[0].task_path).starts_with(&old_root),
            "canonical slug--hash directory must not be mistaken for the legacy slug directory"
        );

        let second = migrator
            .execute(Some(workspace.as_path()), &mut state)
            .unwrap();
        assert_eq!(second, WorkspaceIdentityMigrationReport::default());
        unsafe { std::env::remove_var(path_config.home_env_var) };
    }

    #[test]
    fn migrator_resumes_after_directory_rename_before_manifest_rewrite() {
        let directory = tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("workspace")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        let test_home = directory.path().join("user-home");
        let path_config = StoragePathConfig {
            app_key: "gold-band-identity-resume-test",
            config_dir_name: ".gold-band-identity-resume-test",
            home_env_var: "GOLD_BAND_IDENTITY_RESUME_TEST_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, &test_home) };
        let paths = GoldBandPaths::new_with_path_config(workspace.clone(), path_config);
        assert!(
            paths
                .user_state_file()
                .as_std_path()
                .starts_with(&test_home)
        );
        let old_project_id = legacy_project_id_for_workspace(&workspace);
        std::fs::create_dir_all(paths.runtime_root.as_std_path()).unwrap();
        write_json(
            &paths.project_manifest_file(),
            &ProjectManifest {
                version: gold_band::domain::VERSION.to_string(),
                project_id: old_project_id.clone(),
                repo_root: workspace.to_string(),
                normalized_repo_root: paths.normalized_repo_root.clone(),
            },
        )
        .unwrap();
        let migrated_attempt = paths
            .runtime_root
            .join("tasks/task-001/runs/run-001/rounds/round-001/nodes/node-001/attempt-001");
        std::fs::create_dir_all(migrated_attempt.as_std_path()).unwrap();
        let old_attempt = paths
            .projects_dir()
            .join(&old_project_id)
            .join("tasks/task-001/runs/run-001/rounds/round-001/nodes/node-001/attempt-001");
        let old_worktree = paths
            .projects_dir()
            .join(&old_project_id)
            .join("worktrees/worktree-001");
        write_json(
            &paths
                .runtime_root
                .join("tasks/task-001/runs/run-001/run.json"),
            &serde_json::json!({
                "last_executed_node": {
                    "attemptDir": old_attempt
                },
                "worktree": {
                    "path": old_worktree
                }
            }),
        )
        .unwrap();
        write_json(
            &migrated_attempt.join("worker-ref.json"),
            &serde_json::json!({
                "continue_ref": {
                    "cwd": old_worktree,
                    "snapshotFile": old_attempt.join("acp.snapshot.json")
                }
            }),
        )
        .unwrap();
        write_json(
            &migrated_attempt.join("acp.snapshot.json"),
            &serde_json::json!({ "cwd": old_attempt }),
        )
        .unwrap();
        let connection = rusqlite::Connection::open(paths.core_db_path().as_std_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE core_schema (
                    component TEXT PRIMARY KEY NOT NULL,
                    version INTEGER NOT NULL
                 );
                 INSERT INTO core_schema(component, version)
                 VALUES ('workspace_identity', 1);",
            )
            .unwrap();
        drop(connection);
        let mut state = StateConfig::default();
        state
            .conversation_workspaces
            .push(ConversationWorkspaceEntry {
                project_id: old_project_id,
                workspace_path: workspace.to_string(),
                name: "workspace".to_string(),
                added_at: "2026-01-01T00:00:00Z".to_string(),
            });

        let report = WorkspaceIdentityMigrator::new(&paths)
            .execute(Some(workspace.as_path()), &mut state)
            .unwrap();

        assert_eq!(report.migrated_directory_count, 0);
        assert_eq!(report.already_migrated_directory_count, 1);
        assert_eq!(
            state.conversation_workspaces[0].project_id,
            paths.project_id
        );
        let manifest: ProjectManifest = read_json(&paths.project_manifest_file()).unwrap();
        assert_eq!(manifest.project_id, paths.project_id);
        let snapshot: serde_json::Value =
            read_json(&migrated_attempt.join("acp.snapshot.json")).unwrap();
        assert_eq!(
            snapshot["cwd"],
            migrated_attempt.as_str().replace('\\', "/")
        );
        let migrated_worktree = paths.runtime_root.join("worktrees/worktree-001");
        let run: serde_json::Value = read_json(
            &paths
                .runtime_root
                .join("tasks/task-001/runs/run-001/run.json"),
        )
        .unwrap();
        assert_eq!(
            run["worktree"]["path"],
            migrated_worktree.as_str().replace('\\', "/")
        );
        assert_eq!(
            run["last_executed_node"]["attemptDir"],
            migrated_attempt.as_str().replace('\\', "/")
        );
        let worker_ref: serde_json::Value =
            read_json(&migrated_attempt.join("worker-ref.json")).unwrap();
        assert_eq!(
            worker_ref["continue_ref"]["cwd"],
            migrated_worktree.as_str().replace('\\', "/")
        );
        assert_eq!(
            worker_ref["continue_ref"]["snapshotFile"],
            migrated_attempt
                .join("acp.snapshot.json")
                .as_str()
                .replace('\\', "/")
        );
        assert_eq!(
            CoreStateDatabase::new(paths.core_db_path())
                .workspace_identity_version()
                .unwrap(),
            Some(WORKSPACE_IDENTITY_SCHEMA_VERSION)
        );
        unsafe { std::env::remove_var(path_config.home_env_var) };
    }

    #[test]
    fn migrator_uses_manifest_directory_when_workspace_no_longer_exists() {
        let directory = tempdir().unwrap();
        let workspace =
            Utf8PathBuf::from_path_buf(directory.path().join("deleted-workspace")).unwrap();
        assert!(!workspace.exists());
        let test_home = directory.path().join("user-home");
        let path_config = StoragePathConfig {
            app_key: "gold-band-identity-orphan-test",
            config_dir_name: ".gold-band-identity-orphan-test",
            home_env_var: "GOLD_BAND_IDENTITY_ORPHAN_TEST_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, &test_home) };
        let paths = GoldBandPaths::new_with_path_config(workspace.clone(), path_config);
        assert!(
            paths
                .user_state_file()
                .as_std_path()
                .starts_with(&test_home)
        );
        let manifest_project_id = "legacy-directory-recorded-by-manifest";
        let old_root = paths.projects_dir().join(manifest_project_id);
        std::fs::create_dir_all(old_root.join("tasks/task-001").as_std_path()).unwrap();
        write_json(
            &old_root.join("project.json"),
            &ProjectManifest {
                version: gold_band::domain::VERSION.to_string(),
                project_id: manifest_project_id.to_string(),
                repo_root: workspace.to_string(),
                normalized_repo_root: paths.normalized_repo_root.clone(),
            },
        )
        .unwrap();

        let mut state = StateConfig::default();
        let report = WorkspaceIdentityMigrator::new(&paths)
            .execute(None, &mut state)
            .unwrap();

        assert_eq!(report.migrated_directory_count, 1);
        assert!(!old_root.exists());
        assert!(paths.runtime_root.join("tasks/task-001").is_dir());
        let manifest: ProjectManifest = read_json(&paths.project_manifest_file()).unwrap();
        assert_eq!(manifest.project_id, paths.project_id);
        assert_eq!(manifest.repo_root, workspace);
        unsafe { std::env::remove_var(path_config.home_env_var) };
    }

    #[test]
    fn migrator_preserves_unowned_directory_without_blocking_startup() {
        let directory = tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(directory.path().join("workspace")).unwrap();
        std::fs::create_dir_all(workspace.as_std_path()).unwrap();
        let test_home = directory.path().join("user-home");
        let path_config = StoragePathConfig {
            app_key: "gold-band-identity-unowned-test",
            config_dir_name: ".gold-band-identity-unowned-test",
            home_env_var: "GOLD_BAND_IDENTITY_UNOWNED_TEST_HOME",
        };
        unsafe { std::env::set_var(path_config.home_env_var, &test_home) };
        let paths = GoldBandPaths::new_with_path_config(workspace.clone(), path_config);
        let unowned = paths.projects_dir().join("unowned-legacy-directory");
        std::fs::create_dir_all(unowned.join("nested/tasks/task-001").as_std_path()).unwrap();

        let mut state = StateConfig::default();
        let report = WorkspaceIdentityMigrator::new(&paths)
            .execute(None, &mut state)
            .unwrap();

        assert_eq!(report.migrated_directory_count, 0);
        assert_eq!(report.already_migrated_directory_count, 0);
        assert!(unowned.join("nested/tasks/task-001").is_dir());
        assert!(!unowned.join("project.json").exists());
        unsafe { std::env::remove_var(path_config.home_env_var) };
    }
}
