pub mod symlink;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use camino::Utf8PathBuf;
use tracing::debug;

use crate::config::{
    AGENTS_DIR_NAME, MAX_SKILL_DESCRIPTION_LEN, ManagedAgentConfig, ManagedAgentType,
    SKILL_FILE_NAME, SKILLS_DIR_NAME, SkillMeta, SkillSource,
};
use crate::storage::GoldBandPaths;

#[derive(Debug, Clone)]
pub struct AgentSkillDir {
    pub agent_type: ManagedAgentType,
    pub dir_name: String,
    pub skills_dir: Utf8PathBuf,
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
    pub body: String,
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

pub fn configured_agent_skills_dirs(
    agents: &BTreeMap<ManagedAgentType, ManagedAgentConfig>,
) -> Vec<AgentSkillDir> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    configured_agent_skills_dirs_at_root(&home, agents)
}

fn configured_agent_skills_dirs_at_root(
    root: &Path,
    agents: &BTreeMap<ManagedAgentType, ManagedAgentConfig>,
) -> Vec<AgentSkillDir> {
    let mut dirs = Vec::new();
    for (agent_type, config) in agents {
        let dir_name = config.skills_dir_name(*agent_type).to_string();
        let agent_root = resolve_agent_root(root, &dir_name);
        let skills_dir =
            Utf8PathBuf::from_path_buf(agent_root.join(SKILLS_DIR_NAME)).unwrap_or_default();
        if skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir() {
            dirs.push(AgentSkillDir {
                agent_type: *agent_type,
                dir_name,
                skills_dir,
            });
        }
    }
    dirs
}

pub struct SkillManager {
    paths: GoldBandPaths,
    agents_config: BTreeMap<ManagedAgentType, ManagedAgentConfig>,
}

impl SkillManager {
    pub fn new(
        paths: GoldBandPaths,
        agents_config: BTreeMap<ManagedAgentType, ManagedAgentConfig>,
    ) -> Self {
        Self {
            paths,
            agents_config,
        }
    }

    pub fn workspace_skills_dir(workspace_path: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(workspace_path)
            .join(AGENTS_DIR_NAME)
            .join(SKILLS_DIR_NAME)
    }

    pub fn list(&self) -> Result<SkillListResult> {
        let mut global = scan_skills_dir(
            &GoldBandPaths::global_skills_dir(),
            SkillSource::Global,
            ".agents",
        );
        let project = scan_skills_dir(
            &self.paths.project_skills_dir(),
            SkillSource::Project,
            ".agents",
        );

        let agent_dirs = configured_agent_skills_dirs(&self.agents_config);
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

        global.sort_by(skill_sort_key);
        let mut project_sorted = project;
        project_sorted.sort_by(skill_sort_key);
        Ok(SkillListResult {
            global,
            project: project_sorted,
        })
    }

    pub fn list_by_workspace(&self, workspace_path: &str) -> Result<Vec<SkillMeta>> {
        let workspace_root = Utf8PathBuf::from(workspace_path);
        let mut skills = scan_skills_dir(
            &Self::workspace_skills_dir(workspace_path),
            SkillSource::Project,
            ".agents",
        );
        let agent_dirs =
            configured_agent_skills_dirs_at_root(workspace_root.as_std_path(), &self.agents_config);
        for agent_dir in &agent_dirs {
            let agent_skills = scan_skills_dir(
                &agent_dir.skills_dir,
                SkillSource::Project,
                agent_dir.dir_name.as_str(),
            );
            skills.extend(agent_skills);
        }
        skills.sort_by(skill_sort_key);
        Ok(skills)
    }

    pub fn read(&self, name: &str, source: SkillSource) -> Result<SkillContent> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_path = dir.join(name).join(SKILL_FILE_NAME);
        self.read_at_path(&skill_path, name, source, ".agents")
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
        Ok(SkillContent { meta, body })
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
        let (meta, _) = parse_skill_md(content, name, source, skill_dir.as_str(), ".agents")?;
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
            ".agents",
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
        let current_name = skill_dir
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string());
        let target_dir = if current_name == name {
            skill_dir.clone()
        } else {
            let parent = skill_dir
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid skill directory: {:?}", skill_dir))?;
            let renamed = parent.join(name);
            if renamed.exists() {
                return Err(SkillCommandError::AlreadyExists {
                    skill_name: name.to_string(),
                    directory_path: renamed.as_str().to_string(),
                }
                .into());
            }
            fs::rename(skill_dir.as_std_path(), renamed.as_std_path())?;
            renamed
        };

        fs::create_dir_all(target_dir.as_std_path())?;
        let skill_path = target_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let agent_source = infer_agent_source(target_dir.as_std_path());
        let (meta, _) = parse_skill_md(content, name, source, target_dir.as_str(), &agent_source)?;
        Ok(meta)
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
        resolve_skill_dirs(&self.agents_config, source, workspace_path, sync_targets)
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
        self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
            .into_iter()
            .filter_map(|agent_dir| {
                let skill_dir = agent_dir.skills_dir.join(name);
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

    pub fn sync_skill_instance(
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

        let source_path = Path::new(source_directory_path);
        let source_canonical = canonicalize_lossy(source_path);
        for agent_dir in self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
        {
            if fs::create_dir_all(agent_dir.skills_dir.as_std_path()).is_err() {
                continue;
            }
            let target_skill_dir = agent_dir.skills_dir.join(skill_name);
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
        skill_name: &str,
        source_directory_path: &str,
        source: SkillSource,
        workspace_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) {
        for agent_dir in self.configured_agent_dirs_for_scope(source, workspace_path, sync_targets)
        {
            let target_skill_dir = agent_dir.skills_dir.join(skill_name);
            symlink::remove_link_if_points_to(
                target_skill_dir.as_std_path(),
                Path::new(source_directory_path),
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
    let (frontmatter, body) = if raw.starts_with("---") {
        let rest = &raw[3..];
        if let Some(end) = rest.find("---") {
            let fm = rest[..end].trim().to_string();
            let body_start = end + 3;
            let body = rest[body_start..].trim_start().to_string();
            (fm, body)
        } else {
            (String::new(), raw.to_string())
        }
    } else {
        (String::new(), raw.to_string())
    };

    let mut parsed_name = default_name.to_string();
    let mut description = String::new();

    if !frontmatter.is_empty() {
        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(value) = line.strip_prefix("name:") {
                parsed_name = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("description:") {
                description = value.trim().to_string();
                if description.len() > MAX_SKILL_DESCRIPTION_LEN {
                    load_warnings.push(format!(
                        "description exceeds {MAX_SKILL_DESCRIPTION_LEN} bytes"
                    ));
                }
            }
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
        },
        body,
    ))
}

fn resolve_skill_dirs(
    agents: &BTreeMap<ManagedAgentType, ManagedAgentConfig>,
    source: SkillSource,
    workspace_path: Option<&str>,
    sync_targets: Option<&[String]>,
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
    for (agent_type, config) in agents {
        if sync_targets
            .map(|targets| !targets.iter().any(|target| target == agent_type.as_str()))
            .unwrap_or(false)
        {
            continue;
        }
        let dir_name = config.skills_dir_name(*agent_type).to_string();
        let agent_root = resolve_agent_root(&root, &dir_name);
        let skills_dir =
            Utf8PathBuf::from_path_buf(agent_root.join(SKILLS_DIR_NAME)).unwrap_or_default();
        if skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir() {
            dirs.push(AgentSkillDir {
                agent_type: *agent_type,
                dir_name,
                skills_dir,
            });
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

fn infer_agent_source(skill_dir: &Path) -> String {
    skill_dir
        .parent()
        .and_then(|parent| parent.parent())
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| ".agents".to_string())
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
    use crate::config::{AcpAdapterConfig, ManagedAgentConfig, ManagedAgentType};
    use std::fs;

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
        ManagedAgentConfig::new(AcpAdapterConfig::default())
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
    fn check_name_conflict_detects_existing_native_target() {
        let tmp = std::env::temp_dir().join(format!("gb-conflict-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let claude_skills = tmp.join(".claude").join("skills");
        tmp_skill_dir(&claude_skills, "my-skill");

        let mut agents = BTreeMap::new();
        let mut claude_config = claude_acp_config();
        claude_config.skills_dir_override = Some(tmp.join(".claude").to_string_lossy().to_string());
        agents.insert(ManagedAgentType::ClaudeAcp, claude_config);

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
        tmp_skill_dir(&tmp.join(".agents").join("skills"), "shared-skill");
        tmp_skill_dir(&tmp.join(".claude").join("skills"), "shared-skill");

        let mut agents = BTreeMap::new();
        agents.insert(ManagedAgentType::ClaudeAcp, claude_acp_config());
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
            .write(
                "duplicate-skill",
                SkillSource::Global,
                "---\nname: duplicate-skill\ndescription: test\n---\ncontent",
            )
            .unwrap();

        let error = manager
            .write(
                "duplicate-skill",
                SkillSource::Global,
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

        let source_dir = tmp.join(".agents").join("skills").join("my-skill");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: test\n---\ncontent",
        )
        .unwrap();

        let mut agents = BTreeMap::new();
        let mut claude_config = claude_acp_config();
        claude_config.skills_dir_override = Some(tmp.join(".claude").to_string_lossy().to_string());
        agents.insert(ManagedAgentType::ClaudeAcp, claude_config);
        let mut codex_config =
            ManagedAgentConfig::new(ManagedAgentType::CodexAcp.default_adapter_config());
        codex_config.skills_dir_override = Some(tmp.join(".codex").to_string_lossy().to_string());
        agents.insert(ManagedAgentType::CodexAcp, codex_config);

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
}
