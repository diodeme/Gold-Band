pub mod symlink;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use tracing::debug;

use crate::config::{
    GOLD_BAND_DIR_NAME, MAX_SKILL_DESCRIPTION_LEN, ManagedAgentConfig, ManagedAgentId,
    SKILL_FILE_NAME, SKILLS_DIR_NAME, SkillMeta, SkillSource,
};
use crate::frontmatter::{
    FrontmatterUpdate, parse_optional_frontmatter_document, update_frontmatter_document,
};
use crate::storage::GoldBandPaths;

#[derive(Debug, Clone)]
pub struct AgentSkillDir {
    pub agent_id: ManagedAgentId,
    pub dir_name: String,
    pub skills_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSkillReadDir {
    dir_name: String,
    skills_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListResult {
    pub global: Vec<SkillMeta>,
    pub project: Vec<SkillMeta>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillContent {
    pub meta: SkillMeta,
    pub description_source: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct SkillWriteResult {
    pub directory_path: Utf8PathBuf,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SkillCommandError {
    #[error("skill.already-exists")]
    AlreadyExists {
        skill_name: String,
        directory_path: String,
    },
    #[error("skill.sync-conflict")]
    SyncConflict {
        skill_name: String,
        conflicts: Vec<String>,
    },
}

impl SkillCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AlreadyExists { .. } => "skill.already-exists",
            Self::SyncConflict { .. } => "skill.sync-conflict",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::AlreadyExists {
                skill_name,
                directory_path,
            } => serde_json::json!({
                "skillName": skill_name,
                "directoryPath": directory_path,
            }),
            Self::SyncConflict {
                skill_name,
                conflicts,
            } => serde_json::json!({
                "skillName": skill_name,
                "conflicts": conflicts,
            }),
        }
    }
}

fn configured_agent_skill_read_dirs(
    agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
) -> Vec<AgentSkillReadDir> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    configured_agent_skill_read_dirs_at_root(&home, agents)
}

fn configured_agent_skill_read_dirs_at_root(
    root: &Path,
    agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
) -> Vec<AgentSkillReadDir> {
    let mut dirs = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for config in agents.values() {
        for dir_name in config.skill_directory_policy().read_dir_names {
            let skills_dir = resolve_agent_skills_dir(root, &dir_name);
            if skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir() {
                let canonical_path = canonicalize_lossy(skills_dir.as_std_path());
                if seen_paths.insert(canonical_path) {
                    dirs.push(AgentSkillReadDir {
                        dir_name,
                        skills_dir,
                    });
                }
            }
        }
    }
    dirs
}

pub struct SkillManager {
    paths: GoldBandPaths,
    agents_config: BTreeMap<ManagedAgentId, ManagedAgentConfig>,
}

impl SkillManager {
    pub fn new(
        paths: GoldBandPaths,
        agents_config: BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    ) -> Self {
        Self {
            paths,
            agents_config,
        }
    }

    pub fn workspace_skills_dir(workspace_path: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(workspace_path)
            .join(GOLD_BAND_DIR_NAME)
            .join(SKILLS_DIR_NAME)
    }

    pub fn list(&self) -> Result<SkillListResult> {
        let mut global = scan_skills_dir(
            &GoldBandPaths::global_skills_dir(),
            SkillSource::Global,
            ".gold-band",
        );
        let mut project = scan_skills_dir(
            &self.paths.project_skills_dir(),
            SkillSource::Project,
            ".gold-band",
        );

        let agent_dirs = configured_agent_skill_read_dirs(&self.agents_config);
        debug!(
            agents_count = self.agents_config.len(),
            found_agent_dirs = agent_dirs.len(),
            "scanning global agent skills dirs"
        );
        for agent_dir in &agent_dirs {
            let agent_skills = scan_skills_dir(
                &agent_dir.skills_dir,
                SkillSource::Global,
                agent_dir.dir_name.as_str(),
            );
            debug!(
                agent_source = agent_dir.dir_name.as_str(),
                found = agent_skills.len(),
                "scanned global agent skills dir"
            );
            global.extend(agent_skills);
        }

        let project_agent_dirs = configured_agent_skill_read_dirs_at_root(
            self.paths.repo_root.as_std_path(),
            &self.agents_config,
        );
        for agent_dir in &project_agent_dirs {
            let agent_skills = scan_skills_dir(
                &agent_dir.skills_dir,
                SkillSource::Project,
                agent_dir.dir_name.as_str(),
            );
            project.extend(agent_skills);
        }

        populate_synced_agent_types(&mut global, &self.agents_config, SkillSource::Global, None);
        populate_synced_agent_types(
            &mut project,
            &self.agents_config,
            SkillSource::Project,
            Some(self.paths.repo_root.as_str()),
        );

        global.sort_by(skill_sort_key);
        project.sort_by(skill_sort_key);
        Ok(SkillListResult { global, project })
    }

    pub fn list_by_workspace(&self, workspace_path: &str) -> Result<Vec<SkillMeta>> {
        let workspace_root = Utf8PathBuf::from(workspace_path);
        let mut skills = scan_skills_dir(
            &Self::workspace_skills_dir(workspace_path),
            SkillSource::Project,
            ".gold-band",
        );
        let agent_dirs = configured_agent_skill_read_dirs_at_root(
            workspace_root.as_std_path(),
            &self.agents_config,
        );
        for agent_dir in &agent_dirs {
            let agent_skills = scan_skills_dir(
                &agent_dir.skills_dir,
                SkillSource::Project,
                agent_dir.dir_name.as_str(),
            );
            skills.extend(agent_skills);
        }
        populate_synced_agent_types(
            &mut skills,
            &self.agents_config,
            SkillSource::Project,
            Some(workspace_path),
        );
        skills.sort_by(skill_sort_key);
        Ok(skills)
    }

    pub fn read(&self, name: &str, source: SkillSource) -> Result<SkillContent> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_path = dir.join(name).join(SKILL_FILE_NAME);
        self.read_at_path(&skill_path, name, source, ".gold-band")
    }

    pub fn read_by_path(
        &self,
        skill_dir: &Utf8PathBuf,
        name: &str,
        source: SkillSource,
        agent_source: &str,
    ) -> Result<SkillContent> {
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        self.read_at_path(&skill_path, name, source, agent_source)
    }

    fn read_at_path(
        &self,
        skill_path: &Utf8PathBuf,
        name: &str,
        source: SkillSource,
        agent_source: &str,
    ) -> Result<SkillContent> {
        if !skill_path.exists() {
            bail!("SKILL `{name}` not found at {:?}", skill_path);
        }
        let raw = fs::read_to_string(skill_path.as_std_path())?;
        let directory_path = skill_path
            .parent()
            .map(|path| path.as_str().to_string())
            .unwrap_or_else(|| skill_path.as_str().to_string());
        let (meta, body) =
            parse_skill_md(&raw, name, source, directory_path.as_str(), agent_source)?;
        let description_source =
            skill_description_source(&raw).unwrap_or_else(|| meta.description.clone());
        Ok(SkillContent {
            meta,
            description_source,
            body,
        })
    }

    pub fn write(&self, name: &str, source: SkillSource, content: &str) -> Result<SkillMeta> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_dir = dir.join(name);
        if skill_dir.exists() {
            return Err(SkillCommandError::AlreadyExists {
                skill_name: name.to_string(),
                directory_path: skill_dir.as_str().to_string(),
            }
            .into());
        }
        fs::create_dir_all(skill_dir.as_std_path())?;
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let (meta, _) = parse_skill_md(content, name, source, skill_dir.as_str(), ".gold-band")?;
        Ok(meta)
    }

    pub fn write_to_workspace(
        &self,
        name: &str,
        workspace_path: &str,
        content: &str,
    ) -> Result<SkillMeta> {
        let dir = Self::workspace_skills_dir(workspace_path);
        let skill_dir = dir.join(name);
        if skill_dir.exists() {
            return Err(SkillCommandError::AlreadyExists {
                skill_name: name.to_string(),
                directory_path: skill_dir.as_str().to_string(),
            }
            .into());
        }
        fs::create_dir_all(skill_dir.as_std_path())?;
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let (meta, _) = parse_skill_md(
            content,
            name,
            SkillSource::Project,
            skill_dir.as_str(),
            ".gold-band",
        )?;
        Ok(meta)
    }

    pub fn write_at_path(
        &self,
        skill_dir: &Utf8PathBuf,
        name: &str,
        source: SkillSource,
        content: &str,
    ) -> Result<SkillMeta> {
        fs::create_dir_all(skill_dir.as_std_path())?;
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let agent_source = infer_agent_source(skill_dir.as_std_path());
        let (meta, _) = parse_skill_md(content, name, source, skill_dir.as_str(), &agent_source)?;
        Ok(meta)
    }

    pub fn write_instance(
        &self,
        name: &str,
        source: SkillSource,
        content: &str,
        workspace_path: Option<&str>,
        old_name: Option<&str>,
        current_directory_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Result<SkillWriteResult> {
        let target_dir = self.save_target_dir(
            name,
            source,
            workspace_path,
            old_name,
            current_directory_path,
        )?;
        self.ensure_save_target_available(name, &target_dir, current_directory_path)?;

        let skill_dir_name = skill_dir_name_from_str(target_dir.as_str())
            .ok_or_else(|| anyhow::anyhow!("invalid skill directory: {}", target_dir.as_str()))?;
        let sync_conflicts = self.check_dir_name_conflict(
            skill_dir_name,
            source,
            workspace_path,
            sync_targets,
            current_directory_path,
        );
        if !sync_conflicts.is_empty() {
            return Err(SkillCommandError::SyncConflict {
                skill_name: skill_dir_name.to_string(),
                conflicts: sync_conflicts,
            }
            .into());
        }

        let current_dir = current_directory_path.map(Utf8PathBuf::from);
        let is_rename = current_dir
            .as_ref()
            .map(|dir| {
                canonicalize_lossy(dir.as_std_path())
                    != canonicalize_lossy(target_dir.as_std_path())
            })
            .unwrap_or(false);

        if let Some(ref current_dir) = current_dir {
            if !current_dir.exists() {
                bail!("SKILL dir not found: {:?}", current_dir);
            }
        }

        let old_content = current_dir
            .as_ref()
            .and_then(|dir| fs::read_to_string(dir.join(SKILL_FILE_NAME).as_std_path()).ok());
        let content_to_write = if current_dir.is_some() {
            merge_skill_edit_content(old_content.as_deref(), content, name)?
        } else {
            content.to_string()
        };
        let previous_sync_targets = current_dir
            .as_ref()
            .map(|dir| self.synced_agent_types_for_directory(dir.as_str(), source, workspace_path));

        if is_rename {
            if let Some(ref current_dir) = current_dir {
                self.cleanup_skill_instance_links(
                    name,
                    current_dir.as_str(),
                    source,
                    workspace_path,
                    None,
                );
                if let Err(error) = fs::rename(current_dir.as_std_path(), target_dir.as_std_path())
                {
                    self.restore_skill_links(
                        current_dir.as_str(),
                        source,
                        workspace_path,
                        previous_sync_targets.as_deref(),
                    );
                    return Err(error.into());
                }
            }
        } else if current_dir.is_none() {
            fs::create_dir_all(target_dir.as_std_path())?;
        }

        let skill_path = target_dir.join(SKILL_FILE_NAME);
        if let Err(error) = fs::write(skill_path.as_std_path(), &content_to_write) {
            self.rollback_instance_write(
                &target_dir,
                current_dir.as_ref(),
                is_rename,
                old_content.as_deref(),
                source,
                workspace_path,
                previous_sync_targets.as_deref(),
            );
            return Err(error.into());
        }

        if let Err(error) = self.reconcile_skill_instance_links(
            name,
            target_dir.as_str(),
            source,
            workspace_path,
            sync_targets,
        ) {
            self.rollback_instance_write(
                &target_dir,
                current_dir.as_ref(),
                is_rename,
                old_content.as_deref(),
                source,
                workspace_path,
                previous_sync_targets.as_deref(),
            );
            return Err(error);
        }

        Ok(SkillWriteResult {
            directory_path: target_dir,
        })
    }

    pub fn delete(&self, name: &str, source: SkillSource) -> Result<()> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_dir = dir.join(name);
        if !skill_dir.exists() {
            bail!("SKILL `{name}` not found");
        }
        fs::remove_dir_all(skill_dir.as_std_path())?;
        Ok(())
    }

    pub fn delete_at_path(&self, skill_dir: &Utf8PathBuf) -> Result<()> {
        if !skill_dir.exists() {
            bail!("SKILL dir not found: {:?}", skill_dir);
        }
        fs::remove_dir_all(skill_dir.as_std_path())?;
        Ok(())
    }

    pub fn configured_agent_dirs_for_scope(
        &self,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Vec<AgentSkillDir> {
        resolve_skill_dirs(
            &self.agents_config,
            source,
            workspace_path,
            sync_targets,
            true,
        )
    }

    pub fn check_name_conflict(
        &self,
        name: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
        current_directory_path: Option<&str>,
    ) -> Vec<String> {
        let current_canonical =
            current_directory_path.map(|value| canonicalize_lossy(Path::new(value)));
        let target_dir_name = current_directory_path
            .and_then(skill_dir_name_from_str)
            .unwrap_or(name);
        self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
            .into_iter()
            .filter_map(|agent_dir| {
                let skill_dir = agent_dir.skills_dir.join(target_dir_name);
                if !skill_dir.exists() {
                    return None;
                }
                if skill_dir.as_std_path().read_link().is_ok()
                    || skill_dir.as_std_path().is_symlink()
                {
                    return None;
                }
                let target_canonical = canonicalize_lossy(skill_dir.as_std_path());
                if current_canonical
                    .as_ref()
                    .map(|current| current == &target_canonical)
                    .unwrap_or(false)
                {
                    None
                } else {
                    Some(skill_dir.as_str().to_string())
                }
            })
            .collect()
    }

    pub fn check_save_conflict(
        &self,
        name: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        old_name: Option<&str>,
        current_directory_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Result<Vec<String>> {
        let target_dir = self.save_target_dir(
            name,
            source,
            workspace_path,
            old_name,
            current_directory_path,
        )?;
        let mut conflicts = Vec::new();
        if self.target_conflicts_with_existing_directory(&target_dir, current_directory_path) {
            conflicts.push(target_dir.as_str().to_string());
        }
        let Some(skill_dir_name) = skill_dir_name_from_str(target_dir.as_str()) else {
            return Ok(conflicts);
        };
        conflicts.extend(self.check_dir_name_conflict(
            skill_dir_name,
            source,
            workspace_path,
            sync_targets,
            current_directory_path,
        ));
        Ok(conflicts)
    }

    pub fn sync_skill_instance(
        &self,
        _skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Result<()> {
        let skill_dir_name = skill_dir_name_from_str(source_directory_path)
            .ok_or_else(|| anyhow::anyhow!("invalid skill directory: {source_directory_path}"))?;
        let conflicts = self.check_name_conflict(
            skill_dir_name,
            source,
            workspace_path,
            sync_targets,
            Some(source_directory_path),
        );
        if !conflicts.is_empty() {
            return Err(SkillCommandError::SyncConflict {
                skill_name: skill_dir_name.to_string(),
                conflicts,
            }
            .into());
        }

        let source_path = Path::new(source_directory_path);
        let source_canonical = canonicalize_lossy(source_path);
        for agent_dir in self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
        {
            if fs::create_dir_all(agent_dir.skills_dir.as_std_path()).is_err() {
                continue;
            }
            let target_skill_dir = agent_dir.skills_dir.join(skill_dir_name);
            if target_skill_dir.exists() {
                let target_canonical = canonicalize_lossy(target_skill_dir.as_std_path());
                if target_canonical == source_canonical {
                    continue;
                }
                if target_skill_dir.as_std_path().read_link().is_ok()
                    || target_skill_dir.as_std_path().is_symlink()
                {
                    if fs::remove_file(target_skill_dir.as_std_path()).is_err() {
                        let _ = fs::remove_dir(target_skill_dir.as_std_path());
                    }
                } else {
                    continue;
                }
            }
            symlink::create_link(source_path, target_skill_dir.as_std_path());
        }
        Ok(())
    }

    pub fn reconcile_skill_instance_links(
        &self,
        skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Result<()> {
        let conflicts = self.check_name_conflict(
            skill_name,
            source,
            workspace_path,
            sync_targets,
            Some(source_directory_path),
        );
        if !conflicts.is_empty() {
            return Err(SkillCommandError::SyncConflict {
                skill_name: skill_name.to_string(),
                conflicts,
            }
            .into());
        }

        self.cleanup_skill_instance_links(
            skill_name,
            source_directory_path,
            source,
            workspace_path,
            None,
        );
        self.sync_skill_instance(
            skill_name,
            source_directory_path,
            source,
            workspace_path,
            sync_targets,
        )
    }

    pub fn cleanup_skill_instance_links(
        &self,
        _skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) {
        let Some(skill_dir_name) = skill_dir_name_from_str(source_directory_path) else {
            return;
        };
        for agent_dir in self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
        {
            let target_skill_dir = agent_dir.skills_dir.join(skill_dir_name);
            symlink::remove_link_if_points_to(
                target_skill_dir.as_std_path(),
                Path::new(source_directory_path),
            );
        }
    }

    fn save_target_dir(
        &self,
        name: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        old_name: Option<&str>,
        current_directory_path: Option<&str>,
    ) -> Result<Utf8PathBuf> {
        if let Some(current_directory_path) = current_directory_path {
            let current_dir = Utf8PathBuf::from(current_directory_path);
            let should_rename = old_name.map(|old| old != name).unwrap_or(false);
            if !should_rename {
                return Ok(current_dir);
            }
            let parent = current_dir.parent().ok_or_else(|| {
                anyhow::anyhow!("invalid skill directory: {current_directory_path}")
            })?;
            return Ok(parent.join(name));
        }

        if source == SkillSource::Project {
            if let Some(workspace_path) = workspace_path {
                return Ok(Self::workspace_skills_dir(workspace_path).join(name));
            }
        }

        Ok(skills_dir_for_source(source, &self.paths)?.join(name))
    }

    fn ensure_save_target_available(
        &self,
        name: &str,
        target_dir: &Utf8PathBuf,
        current_directory_path: Option<&str>,
    ) -> Result<()> {
        if !self.target_conflicts_with_existing_directory(target_dir, current_directory_path) {
            return Ok(());
        }
        Err(SkillCommandError::AlreadyExists {
            skill_name: name.to_string(),
            directory_path: target_dir.as_str().to_string(),
        }
        .into())
    }

    fn target_conflicts_with_existing_directory(
        &self,
        target_dir: &Utf8PathBuf,
        current_directory_path: Option<&str>,
    ) -> bool {
        if !target_dir.exists() {
            return false;
        }
        let Some(current_directory_path) = current_directory_path else {
            return true;
        };
        canonicalize_lossy(target_dir.as_std_path())
            != canonicalize_lossy(Path::new(current_directory_path))
    }

    fn check_dir_name_conflict(
        &self,
        skill_dir_name: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
        current_directory_path: Option<&str>,
    ) -> Vec<String> {
        let current_canonical =
            current_directory_path.map(|value| canonicalize_lossy(Path::new(value)));
        self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
            .into_iter()
            .filter_map(|agent_dir| {
                let skill_dir = agent_dir.skills_dir.join(skill_dir_name);
                if !skill_dir.exists() {
                    return None;
                }
                if skill_dir.as_std_path().read_link().is_ok()
                    || skill_dir.as_std_path().is_symlink()
                {
                    return None;
                }
                let target_canonical = canonicalize_lossy(skill_dir.as_std_path());
                if current_canonical
                    .as_ref()
                    .map(|current| current == &target_canonical)
                    .unwrap_or(false)
                {
                    None
                } else {
                    Some(skill_dir.as_str().to_string())
                }
            })
            .collect()
    }

    fn synced_agent_types_for_directory(
        &self,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
    ) -> Vec<String> {
        let source_canonical = canonicalize_lossy(Path::new(source_directory_path));
        let Some(skill_dir_name) = skill_dir_name_from_str(source_directory_path) else {
            return Vec::new();
        };
        resolve_skill_dirs(&self.agents_config, source, workspace_path, None, false)
            .into_iter()
            .filter_map(|agent_dir| {
                let candidate = agent_dir.skills_dir.join(skill_dir_name);
                is_link_pointing_to(candidate.as_std_path(), &source_canonical)
                    .then(|| agent_dir.agent_id.as_str().to_string())
            })
            .collect()
    }

    fn restore_skill_links(
        &self,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) {
        if let Some(targets) = sync_targets {
            let _ = self.sync_skill_instance(
                skill_dir_name_from_str(source_directory_path).unwrap_or_default(),
                source_directory_path,
                source,
                workspace_path,
                Some(targets),
            );
        }
    }

    fn rollback_instance_write(
        &self,
        target_dir: &Utf8PathBuf,
        current_dir: Option<&Utf8PathBuf>,
        is_rename: bool,
        old_content: Option<&str>,
        source: SkillSource,
        workspace_path: Option<&str>,
        previous_sync_targets: Option<&[String]>,
    ) {
        self.cleanup_skill_instance_links(
            skill_dir_name_from_str(target_dir.as_str()).unwrap_or_default(),
            target_dir.as_str(),
            source,
            workspace_path,
            None,
        );

        if is_rename {
            if let Some(current_dir) = current_dir {
                let _ = fs::remove_dir_all(current_dir.as_std_path());
                if target_dir.exists() {
                    let _ = fs::rename(target_dir.as_std_path(), current_dir.as_std_path());
                }
                if let Some(old_content) = old_content {
                    let _ = fs::write(current_dir.join(SKILL_FILE_NAME).as_std_path(), old_content);
                }
                self.restore_skill_links(
                    current_dir.as_str(),
                    source,
                    workspace_path,
                    previous_sync_targets,
                );
            }
        } else if current_dir.is_none() {
            let _ = fs::remove_dir_all(target_dir.as_std_path());
        } else if let Some(old_content) = old_content {
            let _ = fs::write(target_dir.join(SKILL_FILE_NAME).as_std_path(), old_content);
            self.restore_skill_links(
                target_dir.as_str(),
                source,
                workspace_path,
                previous_sync_targets,
            );
        }
    }
}

fn skills_dir_for_source(source: SkillSource, paths: &GoldBandPaths) -> Result<Utf8PathBuf> {
    match source {
        SkillSource::Global => Ok(GoldBandPaths::global_skills_dir()),
        SkillSource::Project => Ok(paths.project_skills_dir()),
        SkillSource::BuiltIn => bail!("built-in skills are not supported yet"),
    }
}

pub(crate) fn scan_skills_dir(
    dir: &Utf8PathBuf,
    source: SkillSource,
    agent_source: &str,
) -> Vec<SkillMeta> {
    let mut skills = Vec::new();
    let Ok(entries) = fs::read_dir(dir.as_std_path()) else {
        return skills;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() || path.read_link().is_ok() {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join(SKILL_FILE_NAME);
        if !skill_md.exists() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&skill_md) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|item| item.to_str())
            .unwrap_or("unknown");
        let dir_path = Utf8PathBuf::from_path_buf(path.clone()).unwrap_or_default();
        match parse_skill_md(&raw, name, source, dir_path.as_str(), agent_source) {
            Ok((meta, _)) => skills.push(meta),
            Err(_) => continue,
        }
    }
    skills.sort_by(skill_sort_key);
    skills
}

pub fn parse_skill_md_public(
    raw: &str,
    default_name: &str,
    source: SkillSource,
    dir_path: &str,
    agent_source: &str,
) -> (SkillMeta, String) {
    parse_skill_md(raw, default_name, source, dir_path, agent_source).unwrap_or_else(|_| {
        (
            SkillMeta {
                name: default_name.to_string(),
                description: String::new(),
                source,
                directory_path: dir_path.to_string(),
                agent_source: agent_source.to_string(),
                load_warnings: vec![],
                synced_agent_types: Vec::new(),
            },
            raw.to_string(),
        )
    })
}

fn parse_skill_md(
    raw: &str,
    default_name: &str,
    source: SkillSource,
    dir_path: &str,
    agent_source: &str,
) -> Result<(SkillMeta, String)> {
    let mut load_warnings = Vec::new();
    let document = parse_optional_frontmatter_document(raw)?;

    let mut parsed_name = default_name.to_string();
    let mut description = String::new();

    if let Some(value) = document.fields.get("name") {
        parsed_name = value.trim().to_string();
    }
    if let Some(value) = document.fields.get("description") {
        description = value.trim().to_string();
        if description.len() > MAX_SKILL_DESCRIPTION_LEN {
            load_warnings.push(format!(
                "description exceeds {MAX_SKILL_DESCRIPTION_LEN} bytes"
            ));
        }
    }

    Ok((
        SkillMeta {
            name: parsed_name,
            description,
            source,
            directory_path: dir_path.to_string(),
            agent_source: agent_source.to_string(),
            load_warnings,
            synced_agent_types: Vec::new(),
        },
        document.body.trim_start().to_string(),
    ))
}

fn skill_description_source(raw: &str) -> Option<String> {
    parse_optional_frontmatter_document(raw)
        .ok()
        .and_then(|document| {
            document
                .field_sources
                .get("description")
                .cloned()
                .or_else(|| document.fields.get("description").cloned())
        })
}

fn merge_skill_edit_content(
    old_content: Option<&str>,
    requested_content: &str,
    name: &str,
) -> Result<String> {
    let requested = parse_optional_frontmatter_document(requested_content)?;
    let description_value = requested
        .fields
        .get("description")
        .map(|value| value.trim())
        .unwrap_or_default();
    let description_source = requested
        .field_sources
        .get("description")
        .map(String::as_str)
        .unwrap_or(description_value);
    let body = requested.body.trim_start();

    if let Some(old_content) = old_content {
        return update_frontmatter_document(
            old_content,
            &[
                FrontmatterUpdate {
                    key: "name",
                    value: name,
                    source: None,
                },
                FrontmatterUpdate {
                    key: "description",
                    value: description_value,
                    source: Some(description_source),
                },
            ],
            body,
        );
    }

    Ok(requested_content.to_string())
}

fn populate_synced_agent_types(
    skills: &mut [SkillMeta],
    agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    source: SkillSource,
    workspace_path: Option<&str>,
) {
    let agent_dirs = resolve_skill_dirs(agents, source, workspace_path, None, false);
    if agent_dirs.is_empty() {
        return;
    }

    for skill in skills.iter_mut() {
        let canonical_source = canonicalize_lossy(Path::new(&skill.directory_path));
        let Some(skill_dir_name) = skill_dir_name_from_str(&skill.directory_path) else {
            skill.synced_agent_types.clear();
            continue;
        };
        skill.synced_agent_types = agent_dirs
            .iter()
            .filter_map(|agent_dir| {
                let candidate = agent_dir.skills_dir.join(skill_dir_name);
                is_link_pointing_to(candidate.as_std_path(), &canonical_source)
                    .then(|| agent_dir.agent_id.as_str().to_string())
            })
            .collect();
    }
}

fn resolve_skill_dirs(
    agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    source: SkillSource,
    workspace_path: Option<&str>,
    sync_targets: Option<&[String]>,
    include_missing: bool,
) -> Vec<AgentSkillDir> {
    let root = match source {
        SkillSource::Global => Some(dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))),
        SkillSource::Project => workspace_path.map(PathBuf::from),
        SkillSource::BuiltIn => None,
    };
    let Some(root) = root else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    for (agent_id, config) in agents {
        if sync_targets
            .map(|targets| !targets.iter().any(|target| target == agent_id.as_str()))
            .unwrap_or(false)
        {
            continue;
        }
        for dir_name in config.skill_directory_policy().write_dir_names {
            let skills_dir = resolve_agent_skills_dir(&root, &dir_name);
            if include_missing
                || (skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir())
            {
                dirs.push(AgentSkillDir {
                    agent_id: agent_id.clone(),
                    dir_name,
                    skills_dir,
                });
            }
        }
    }
    dirs
}

fn resolve_agent_root(root: &Path, dir_name: &str) -> PathBuf {
    let configured = PathBuf::from(dir_name);
    if configured.is_absolute() {
        configured
    } else {
        root.join(configured)
    }
}

pub fn resolve_agent_skills_dir(root: &Path, agent_dir: &str) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(resolve_agent_root(root, agent_dir).join(SKILLS_DIR_NAME))
        .unwrap_or_default()
}

fn is_link_pointing_to(link_path: &Path, expected: &Path) -> bool {
    if !link_path.exists() {
        return false;
    }
    let Ok(target) = link_path.read_link() else {
        return false;
    };
    canonicalize_lossy(&target) == expected
}

fn infer_agent_source(skill_dir: &Path) -> String {
    skill_dir
        .parent()
        .and_then(|parent| parent.parent())
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| ".gold-band".to_string())
}

pub fn skill_dir_name_from_str(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|name| name.to_str())
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn skill_sort_key(left: &SkillMeta, right: &SkillMeta) -> std::cmp::Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.agent_source.cmp(&right.agent_source))
        .then_with(|| left.directory_path.cmp(&right.directory_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ManagedAgentConfig, ManagedAgentId, catalog_agent_default_config};
    use std::fs;
    use std::str::FromStr;

    fn tmp_skill_dir(base: &Path, name: &str) -> PathBuf {
        let skill_dir = base.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\ncontent"),
        )
        .unwrap();
        skill_dir
    }

    fn claude_acp_config() -> ManagedAgentConfig {
        catalog_agent_default_config("claude-acp").unwrap()
    }

    fn codex_acp_config() -> ManagedAgentConfig {
        catalog_agent_default_config("codex-acp").unwrap()
    }

    fn agent_id(value: &str) -> ManagedAgentId {
        ManagedAgentId::from_str(value).unwrap()
    }

    #[test]
    fn scan_skills_dir_sets_agent_source_and_skips_symlink() {
        let tmp = std::env::temp_dir().join(format!("gb-scan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let source_dir = tmp_skill_dir(&tmp, "my-skill");
        let link_dir = tmp.join("linked-skill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source_dir, &link_dir).unwrap();
        #[cfg(windows)]
        {
            let _ = std::os::windows::fs::symlink_dir(&source_dir, &link_dir);
        }

        let skills_dir = Utf8PathBuf::from_path_buf(tmp.clone()).unwrap();
        let results = scan_skills_dir(&skills_dir, SkillSource::Global, ".claude");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "my-skill");
        assert_eq!(results[0].agent_source, ".claude");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn configured_agent_skill_read_dirs_include_compatible_directory_once() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".codex").join("skills")).unwrap();
        fs::create_dir_all(temp.path().join(".agents").join("skills")).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("codex-acp"), codex_acp_config());
        agents.insert(
            agent_id("cursor"),
            catalog_agent_default_config("cursor").unwrap(),
        );

        let dirs = configured_agent_skill_read_dirs_at_root(temp.path(), &agents);

        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0].dir_name, ".codex");
        assert_eq!(dirs[1].dir_name, ".agents");
    }

    #[test]
    fn list_by_workspace_reads_skills_from_compatible_agent_directory() {
        let temp = tempfile::tempdir().unwrap();
        tmp_skill_dir(
            &temp.path().join(".agents").join("skills"),
            "compatible-skill",
        );
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let mut agents = BTreeMap::new();
        agents.insert(agent_id("codex-acp"), codex_acp_config());
        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);

        let skills = manager.list_by_workspace(repo_root.as_str()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "compatible-skill");
        assert_eq!(skills[0].agent_source, ".agents");
    }

    #[test]
    fn parse_skill_md_supports_folded_description_frontmatter() {
        let raw = r#"---
name: tailwind-theme-builder
description: >
  Set up Tailwind v4 with shadcn/ui themed UI. Workflow: install dependencies,
  configure CSS variables with @theme inline, set up dark mode, verify.
  Use when initialising React projects with Tailwind v4, setting up shadcn/ui theming,
  or fixing colors not working.
compatibility: claude-code-only
---

# Tailwind Theme Builder
"#;
        let raw = raw.replace('\n', "\r\n");

        let (meta, body) = parse_skill_md(
            &raw,
            "fallback-name",
            SkillSource::Project,
            "/tmp/tailwind-theme-builder",
            ".claude",
        )
        .unwrap();

        assert_eq!(meta.name, "tailwind-theme-builder");
        assert_eq!(
            meta.description,
            "Set up Tailwind v4 with shadcn/ui themed UI. Workflow: install dependencies, configure CSS variables with @theme inline, set up dark mode, verify. Use when initialising React projects with Tailwind v4, setting up shadcn/ui theming, or fixing colors not working."
        );
        assert!(!meta.description.contains("compatibility"));
        assert_eq!(body, "# Tailwind Theme Builder\r\n");
    }

    #[test]
    fn merge_skill_edit_content_preserves_unknown_frontmatter_fields() {
        let old = "---\r\nname: tailwind-theme-builder\r\ndescription: >\r\n  Set up Tailwind v4 with shadcn/ui themed UI.\r\ncompatibility: claude-code-only\r\n---\r\n# Old\r\n";
        let requested = "---\nname: tailwind-theme-builder\ndescription: |\n  Set up Tailwind v4 with shadcn/ui themed UI.\n  Verify dark mode.\n---\n\n# New\n";

        let merged =
            merge_skill_edit_content(Some(old), requested, "tailwind-theme-builder").unwrap();

        assert!(merged.contains("compatibility: claude-code-only"));
        assert!(merged.contains(
            "description: >\r\n  Set up Tailwind v4 with shadcn/ui themed UI.\r\n  Verify dark mode.\r\n"
        ));
        assert!(merged.ends_with("---\r\n# New\n"));
    }

    #[test]
    fn check_name_conflict_detects_existing_native_target() {
        let tmp = std::env::temp_dir().join(format!("gb-conflict-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let claude_skills = tmp.join(".claude").join("skills");
        tmp_skill_dir(&claude_skills, "my-skill");

        let mut agents = BTreeMap::new();
        let mut claude_config = claude_acp_config();
        claude_config.primary_agent_dir = Some(tmp.join(".claude").to_string_lossy().to_string());
        agents.insert(agent_id("claude-acp"), claude_config);

        let manager = SkillManager::new(GoldBandPaths::new("."), agents);
        let conflicts = manager.check_name_conflict(
            "my-skill",
            SkillSource::Global,
            None,
            Some(&["claude-acp".to_string()]),
            None,
        );
        assert_eq!(conflicts.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_by_workspace_keeps_same_name_native_skills_from_multiple_dirs() {
        let tmp =
            std::env::temp_dir().join(format!("gb-list-workspace-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        tmp_skill_dir(&tmp.join(".gold-band").join("skills"), "shared-skill");
        tmp_skill_dir(&tmp.join(".claude").join("skills"), "shared-skill");

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        let manager = SkillManager::new(
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.clone()).unwrap()),
            agents,
        );
        let skills = manager
            .list_by_workspace(tmp.to_string_lossy().as_ref())
            .unwrap();
        assert_eq!(skills.len(), 2);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_rejects_duplicate_skill_in_same_directory() {
        let tmp =
            std::env::temp_dir().join(format!("gb-write-duplicate-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let manager = SkillManager::new(GoldBandPaths::new(repo_root), BTreeMap::new());
        manager
            .write_to_workspace(
                "duplicate-skill",
                manager.paths.repo_root.as_str(),
                "---\nname: duplicate-skill\ndescription: test\n---\ncontent",
            )
            .unwrap();

        let error = manager
            .write_to_workspace(
                "duplicate-skill",
                manager.paths.repo_root.as_str(),
                "---\nname: duplicate-skill\ndescription: test\n---\ncontent",
            )
            .unwrap_err();
        let skill_error = error.downcast_ref::<SkillCommandError>().unwrap();
        assert!(matches!(
            skill_error,
            SkillCommandError::AlreadyExists { .. }
        ));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn reconcile_skill_instance_links_removes_unselected_targets() {
        let tmp =
            std::env::temp_dir().join(format!("gb-reconcile-skill-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let source_dir = tmp.join(".gold-band").join("skills").join("my-skill");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        let mut claude_config = claude_acp_config();
        claude_config.primary_agent_dir = Some(tmp.join(".claude").to_string_lossy().to_string());
        agents.insert(agent_id("claude-acp"), claude_config);
        let mut codex_config = codex_acp_config();
        codex_config.primary_agent_dir = Some(tmp.join(".codex").to_string_lossy().to_string());
        agents.insert(agent_id("codex-acp"), codex_config);

        fs::create_dir_all(tmp.join(".claude").join("skills")).unwrap();
        fs::create_dir_all(tmp.join(".codex").join("skills")).unwrap();

        let manager = SkillManager::new(GoldBandPaths::new(repo_root), agents);
        manager
            .sync_skill_instance(
                "my-skill",
                source_dir.to_string_lossy().as_ref(),
                SkillSource::Global,
                None,
                Some(&["claude-acp".to_string(), "codex-acp".to_string()]),
            )
            .unwrap();

        manager
            .reconcile_skill_instance_links(
                "my-skill",
                source_dir.to_string_lossy().as_ref(),
                SkillSource::Global,
                None,
                Some(&["claude-acp".to_string()]),
            )
            .unwrap();

        assert!(tmp.join(".claude").join("skills").join("my-skill").exists());
        assert!(!tmp.join(".codex").join("skills").join("my-skill").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn project_sync_creates_missing_configured_agent_dirs() {
        let tmp = std::env::temp_dir().join(format!(
            "gb-project-sync-create-missing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let source_dir = repo_root.join(".gold-band").join("skills").join("my-skill");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        agents.insert(agent_id("codex-acp"), codex_acp_config());

        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);
        manager
            .sync_skill_instance(
                "my-skill",
                source_dir.as_str(),
                SkillSource::Project,
                Some(repo_root.as_str()),
                Some(&["claude-acp".to_string(), "codex-acp".to_string()]),
            )
            .unwrap();

        assert!(
            repo_root
                .join(".claude")
                .join("skills")
                .join("my-skill")
                .exists()
        );
        assert!(
            repo_root
                .join(".codex")
                .join("skills")
                .join("my-skill")
                .exists()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_by_workspace_reports_synced_agent_types() {
        let tmp =
            std::env::temp_dir().join(format!("gb-list-synced-agents-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let source_dir = repo_root.join(".gold-band").join("skills").join("my-skill");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        agents.insert(agent_id("codex-acp"), codex_acp_config());

        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);
        manager
            .sync_skill_instance(
                "my-skill",
                source_dir.as_str(),
                SkillSource::Project,
                Some(repo_root.as_str()),
                Some(&["claude-acp".to_string(), "codex-acp".to_string()]),
            )
            .unwrap();

        let skills = manager.list_by_workspace(repo_root.as_str()).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.directory_path == source_dir.as_str())
            .unwrap();
        assert_eq!(
            skill.synced_agent_types,
            vec!["claude-acp".to_string(), "codex-acp".to_string()]
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sync_uses_directory_name_instead_of_frontmatter_name() {
        let tmp =
            std::env::temp_dir().join(format!("gb-sync-directory-name-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let source_dir = repo_root.join(".claude").join("skills").join("ckm-design");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: ckm:design\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        let mut codex_config = codex_acp_config();
        codex_config.primary_agent_dir = Some(tmp.join(".codex").to_string_lossy().to_string());
        agents.insert(agent_id("codex-acp"), codex_config);

        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);
        manager
            .sync_skill_instance(
                "ckm:design",
                source_dir.as_str(),
                SkillSource::Project,
                Some(repo_root.as_str()),
                Some(&["codex-acp".to_string()]),
            )
            .unwrap();

        assert!(
            tmp.join(".codex")
                .join("skills")
                .join("ckm-design")
                .exists()
        );
        assert!(
            !tmp.join(".codex")
                .join("skills")
                .join("ckm:design")
                .exists()
        );

        let skills = manager.list_by_workspace(repo_root.as_str()).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.directory_path == source_dir.as_str())
            .unwrap();
        assert_eq!(skill.synced_agent_types, vec!["codex-acp".to_string()]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_instance_rejects_sync_conflict_before_creating_source() {
        let tmp = std::env::temp_dir().join(format!(
            "gb-write-atomic-sync-conflict-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let conflict_dir = repo_root
            .join(".codex")
            .join("skills")
            .join("conflicted-skill");
        fs::create_dir_all(conflict_dir.as_std_path()).unwrap();
        fs::write(
            conflict_dir.join("SKILL.md").as_std_path(),
            "---\nname: conflicted-skill\ndescription: native\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        agents.insert(agent_id("codex-acp"), codex_acp_config());
        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);

        let error = manager
            .write_instance(
                "conflicted-skill",
                SkillSource::Project,
                "---\nname: conflicted-skill\ndescription: test\n---\ncontent",
                Some(repo_root.as_str()),
                None,
                None,
                Some(&["codex-acp".to_string()]),
            )
            .unwrap_err();
        let skill_error = error.downcast_ref::<SkillCommandError>().unwrap();
        assert!(matches!(
            skill_error,
            SkillCommandError::SyncConflict { .. }
        ));
        assert!(
            !repo_root
                .join(".gold-band")
                .join("skills")
                .join("conflicted-skill")
                .exists()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn write_instance_renames_directory_and_reconciles_links() {
        let tmp = std::env::temp_dir().join(format!("gb-write-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();

        let source_dir = repo_root
            .join(".gold-band")
            .join("skills")
            .join("old-skill");
        fs::create_dir_all(source_dir.as_std_path()).unwrap();
        fs::write(
            source_dir.join("SKILL.md").as_std_path(),
            "---\nname: old-skill\ndescription: test\n---\nold content",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        agents.insert(agent_id("codex-acp"), codex_acp_config());
        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);
        manager
            .sync_skill_instance(
                "old-skill",
                source_dir.as_str(),
                SkillSource::Project,
                Some(repo_root.as_str()),
                Some(&["claude-acp".to_string(), "codex-acp".to_string()]),
            )
            .unwrap();

        let result = manager
            .write_instance(
                "new-skill",
                SkillSource::Project,
                "---\nname: new-skill\ndescription: test\n---\nnew content",
                Some(repo_root.as_str()),
                Some("old-skill"),
                Some(source_dir.as_str()),
                Some(&["claude-acp".to_string(), "codex-acp".to_string()]),
            )
            .unwrap();

        let new_source_dir = repo_root
            .join(".gold-band")
            .join("skills")
            .join("new-skill");
        assert_eq!(result.directory_path, new_source_dir);
        assert!(!source_dir.exists());
        assert!(new_source_dir.exists());
        for agent_dir in [".claude", ".codex"] {
            let old_link = repo_root.join(agent_dir).join("skills").join("old-skill");
            let new_link = repo_root.join(agent_dir).join("skills").join("new-skill");
            assert!(!old_link.exists());
            assert!(is_link_pointing_to(
                new_link.as_std_path(),
                &canonicalize_lossy(new_source_dir.as_std_path())
            ));
        }
        let saved = fs::read_to_string(new_source_dir.join("SKILL.md").as_std_path()).unwrap();
        assert!(saved.contains("new content"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
