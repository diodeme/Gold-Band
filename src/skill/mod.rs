pub mod symlink;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
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
    configured_agent_skill_read_dirs_at_root(&home, agents, SkillSource::Global)
}

fn configured_agent_skill_read_dirs_at_root(
    root: &Path,
    agents: &BTreeMap<ManagedAgentId, ManagedAgentConfig>,
    source: SkillSource,
) -> Vec<AgentSkillReadDir> {
    let mut dirs = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for config in agents.values() {
        let policy = config.skill_directory_policy();
        let Some(scope_policy) = policy.for_source(source) else {
            continue;
        };
        for dir_name in &scope_policy.read_dir_names {
            let skills_dir = resolve_agent_skills_dir(root, &dir_name);
            if skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir() {
                let canonical_path = canonicalize_lossy(skills_dir.as_std_path());
                if seen_paths.insert(canonical_path) {
                    dirs.push(AgentSkillReadDir {
                        dir_name: dir_name.clone(),
                        skills_dir,
                    });
                }
            }
        }
    }
    dirs
}

/// SKILL bundle 尺寸契约（与 multica 服务端约束对齐；推送/拉取双侧共用同一事实源）：
/// 单文件 ≤1MiB、整包（SKILL.md + 支撑文件）≤8MiB、文件总数 ≤256、路径深度（含文件名段）≤4。
pub const SKILL_BUNDLE_MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const SKILL_BUNDLE_MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
pub const SKILL_BUNDLE_MAX_FILES: usize = 256;
pub const SKILL_BUNDLE_MAX_DEPTH: usize = 4;

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
            SkillSource::Project,
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
            SkillSource::Project,
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

    /// 多文件 skill 写入（远程 skill 来源拉取镜像落库）：SKILL.md 原文 + 支撑文件整目录写入。
    ///
    /// 与 [`Self::write_instance`] 共享目标解析 / 同步冲突 / 链接调和契约，差异仅在文件集：
    /// - `content` 为**完整** SKILL.md（调用方组装完成，不走编辑器 merge）；
    /// - `support_files` 为 `(相对路径, 文本内容)` 支撑文件，落盘前由
    ///   [`validate_skill_bundle_entries`] 复核（尺寸/数量/深度/路径安全——与推送侧同一契约）。
    ///
    /// 覆盖（`current_directory_path` 指向既有目录）为**镜像语义**：拉取源是权威事实源，
    /// 目录整体替换（staged 目录 → rename swap），远端已删的本地支撑文件随之消失；远端删、
    /// 本地新增的文件也一并清除。本签名无 `old_name`（拉取不重命名），覆盖时目录身份不变，
    /// path 级 symlink 同步身份跨 swap 天然保持有效。
    ///
    /// 原子性：staged 目录与目标目录同父（同卷 rename），先写支撑文件、**最后写 SKILL.md**
    /// ——SKILL.md 写入前崩溃的残留对扫描器不可见；swap 任一步失败回滚到写前状态。
    /// 崩溃兜底：写入开始时按前缀清扫本 skill 的 staging/backup 残留（任意 pid，见
    /// [`remove_stale_bundle_staging`]）；提交后的 backup 清理失败时先删其 SKILL.md
    /// 使残留对扫描器不可见。
    pub fn write_bundle_instance(
        &self,
        name: &str,
        source: SkillSource,
        content: &str,
        support_files: &[(String, String)],
        workspace_path: Option<&str>,
        current_directory_path: Option<&str>,
        sync_targets: Option<&[String]>,
    ) -> Result<SkillWriteResult> {
        validate_skill_bundle_entries(content, support_files)?;

        let target_dir =
            self.save_target_dir(name, source, workspace_path, None, current_directory_path)?;
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
        if let Some(ref current_dir) = current_dir {
            if !current_dir.exists() {
                bail!("SKILL dir not found: {:?}", current_dir);
            }
        }
        let previous_sync_targets = current_dir
            .as_ref()
            .map(|dir| self.synced_agent_types_for_directory(dir.as_str(), source, workspace_path));

        // staged / backup 固定名含 pid；写入前按前缀清扫本 skill 的崩溃残留（任意 pid）。
        let parent = target_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid skill directory: {}", target_dir.as_str()))?;
        remove_stale_bundle_staging(parent, skill_dir_name);
        let staging = parent.join(format!(".mstaging-{skill_dir_name}-{}", std::process::id()));
        let backup = parent.join(format!(".mbackup-{skill_dir_name}-{}", std::process::id()));

        if let Err(error) = write_staged_bundle(&staging, content, support_files) {
            let _ = fs::remove_dir_all(staging.as_std_path());
            return Err(error);
        }

        let is_overwrite = current_dir.is_some();
        if is_overwrite {
            if let Err(error) = fs::rename(target_dir.as_std_path(), backup.as_std_path()) {
                let _ = fs::remove_dir_all(staging.as_std_path());
                return Err(error.into());
            }
        }
        if let Err(error) = fs::rename(staging.as_std_path(), target_dir.as_std_path()) {
            let _ = fs::remove_dir_all(staging.as_std_path());
            if is_overwrite {
                let _ = fs::rename(backup.as_std_path(), target_dir.as_std_path());
            }
            return Err(error.into());
        }

        if let Err(error) = self.reconcile_skill_instance_links(
            name,
            target_dir.as_str(),
            source,
            workspace_path,
            sync_targets,
        ) {
            // swap 回滚：内容未提交成功，恢复旧目录并还原链接（与 write_instance 同语义）。
            let _ = fs::rename(target_dir.as_std_path(), staging.as_std_path());
            if is_overwrite {
                let _ = fs::rename(backup.as_std_path(), target_dir.as_std_path());
            }
            let _ = fs::remove_dir_all(staging.as_std_path());
            self.restore_skill_links(
                target_dir.as_str(),
                source,
                workspace_path,
                previous_sync_targets.as_deref(),
            );
            return Err(error);
        }

        // 提交点已过：先删 backup 内 SKILL.md（残留对扫描器不可见），再尽力整体清除。
        if is_overwrite {
            let _ = fs::remove_file(backup.join(SKILL_FILE_NAME).as_std_path());
            if let Err(error) = fs::remove_dir_all(backup.as_std_path()) {
                debug!("skill bundle backup cleanup failed: {error}");
            }
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

/// 多文件 skill 落库前的入口校验（与推送侧 `multica/local_skills.rs` 同一契约、同一组常量）：
/// 尺寸（单文件 / 整包）、文件总数（SKILL.md + 支撑文件）、路径安全（穿越 / 绝对路径 /
/// Windows 非法字符 / 深度）、根 `SKILL.md` 不得混入支撑文件（由 `content` 承载）、路径去重。
/// 超限即整体失败，不部分写入。
pub fn validate_skill_bundle_entries(
    content: &str,
    support_files: &[(String, String)],
) -> Result<()> {
    if content.len() as u64 > SKILL_BUNDLE_MAX_FILE_BYTES {
        bail!("SKILL.md exceeds 1MiB limit");
    }
    if 1 + support_files.len() > SKILL_BUNDLE_MAX_FILES {
        bail!(
            "skill bundle exceeds {SKILL_BUNDLE_MAX_FILES} file limit ({} files)",
            1 + support_files.len()
        );
    }
    let mut total = content.len() as u64;
    let mut seen = BTreeSet::new();
    for (rel_path, file_content) in support_files {
        validate_support_file_path(rel_path)?;
        if !seen.insert(rel_path.as_str()) {
            bail!("duplicate skill file path: {rel_path}");
        }
        if file_content.len() as u64 > SKILL_BUNDLE_MAX_FILE_BYTES {
            bail!("file exceeds 1MiB limit: {rel_path}");
        }
        total += file_content.len() as u64;
        if total > SKILL_BUNDLE_MAX_BUNDLE_BYTES {
            bail!("skill bundle exceeds 8MiB limit");
        }
    }
    Ok(())
}

/// 支撑文件相对路径校验：`/` 分隔、相对路径、无穿越段、无 Windows 非法字符，
/// 深度（含文件名段）≤ [`SKILL_BUNDLE_MAX_DEPTH`]（与推送侧 `collect_files_recursive` 同口径）。
fn validate_support_file_path(rel_path: &str) -> Result<()> {
    if rel_path == SKILL_FILE_NAME {
        bail!("root SKILL.md is carried by content, not a support file");
    }
    if rel_path.is_empty() {
        bail!("empty skill file path");
    }
    if rel_path.contains('\\') {
        bail!("skill file path must use '/' separators: {rel_path}");
    }
    if rel_path.starts_with('/') {
        bail!("skill file path must be relative: {rel_path}");
    }
    let segments: Vec<&str> = rel_path.split('/').collect();
    if segments.len() > SKILL_BUNDLE_MAX_DEPTH {
        bail!("skill file depth exceeds {SKILL_BUNDLE_MAX_DEPTH}: {rel_path}");
    }
    for segment in segments {
        if segment.is_empty() || segment == "." || segment == ".." {
            bail!("invalid skill file path segment in: {rel_path}");
        }
        if segment
            .chars()
            .any(|c| c.is_control() || "<>:\"|?*".contains(c))
        {
            bail!("illegal character in skill file path: {rel_path}");
        }
        if segment.ends_with('.') || segment.ends_with(' ') {
            bail!("illegal trailing dot or space in skill file path: {rel_path}");
        }
    }
    Ok(())
}

/// 本 skill 的 staging/backup 崩溃残留清扫（**按前缀、不限 pid**）：进程在 swap 完成前崩溃后
/// 重启，pid 已变，pid 隔离的清扫永远够不到残留；其中含 SKILL.md 的残留（staging 已写完
/// SKILL.md、backup 的 SKILL.md 删除失败）对 [`scan_skills_dir`] 可见。单实例 app + 拉取
/// 弹窗 UI 串行化保证不存在并发写同一 skill 的在飞 staging 被误删。
fn remove_stale_bundle_staging(parent: &Utf8Path, skill_dir_name: &str) {
    let staging_prefix = format!(".mstaging-{skill_dir_name}-");
    let backup_prefix = format!(".mbackup-{skill_dir_name}-");
    let Ok(entries) = fs::read_dir(parent.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(&staging_prefix) || name.starts_with(&backup_prefix) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// staged 目录写入：先写支撑文件（按需建父目录），**最后写 SKILL.md**——SKILL.md 写入前
/// 崩溃的残留对 [`scan_skills_dir`] 不可见；写入后、swap 完成前的崩溃残留（毫秒级窗口）
/// 由下次写入的前缀清扫清除（[`remove_stale_bundle_staging`]）。
fn write_staged_bundle(
    staging: &Utf8PathBuf,
    content: &str,
    support_files: &[(String, String)],
) -> Result<()> {
    // staging 目录自身必须先建：支撑文件为空（单文件 skill）时无任何 create_dir_all 触发点。
    fs::create_dir_all(staging.as_std_path())?;
    for (rel_path, file_content) in support_files {
        let dest = staging.join(rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }
        fs::write(dest.as_std_path(), file_content)?;
    }
    fs::write(staging.join(SKILL_FILE_NAME).as_std_path(), content)?;
    Ok(())
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
        let policy = config.skill_directory_policy();
        let Some(scope_policy) = policy.for_source(source) else {
            continue;
        };
        for dir_name in &scope_policy.write_dir_names {
            let skills_dir = resolve_agent_skills_dir(&root, &dir_name);
            if include_missing
                || (skills_dir.as_std_path().exists() && skills_dir.as_std_path().is_dir())
            {
                dirs.push(AgentSkillDir {
                    agent_id: agent_id.clone(),
                    dir_name: dir_name.clone(),
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

        let dirs =
            configured_agent_skill_read_dirs_at_root(temp.path(), &agents, SkillSource::Global);

        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0].dir_name, ".codex");
        assert_eq!(dirs[1].dir_name, ".agents");
    }

    #[test]
    fn configured_agent_skill_read_dirs_respect_split_scope_roots() {
        let temp = tempfile::tempdir().unwrap();
        for directory in [".pi/agent/skills", ".pi/skills", ".agents/skills"] {
            fs::create_dir_all(temp.path().join(directory)).unwrap();
        }
        let mut agents = BTreeMap::new();
        agents.insert(
            agent_id("pi-acp"),
            catalog_agent_default_config("pi-acp").unwrap(),
        );

        let global =
            configured_agent_skill_read_dirs_at_root(temp.path(), &agents, SkillSource::Global);
        let project =
            configured_agent_skill_read_dirs_at_root(temp.path(), &agents, SkillSource::Project);

        assert_eq!(
            global
                .iter()
                .map(|directory| directory.dir_name.as_str())
                .collect::<Vec<_>>(),
            vec![".pi/agent", ".agents"]
        );
        assert_eq!(
            project
                .iter()
                .map(|directory| directory.dir_name.as_str())
                .collect::<Vec<_>>(),
            vec![".pi", ".agents"]
        );
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

    /// staging / backup 无残留断言：按**目录条目**检查（`Utf8PathBuf::iter` 迭代的是路径
    /// 组件而非目录内容，不能用于本断言）。
    fn assert_no_staging_leftovers(skills_root: &Utf8PathBuf) {
        let leftovers: Vec<String> = fs::read_dir(skills_root.as_std_path())
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with(".mstaging-") || name.starts_with(".mbackup-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging/backup leftovers: {leftovers:?}"
        );
    }

    /// bundle 写入测试夹具：临时 repo + 仅 claude-acp 的 manager，Project 作用域
    /// （与既有 write 测试同款——Global 落真实 home 不可测；写入链路对两种 source 一致）。
    fn bundle_test_manager(tag: &str) -> (PathBuf, SkillManager, Utf8PathBuf) {
        let tmp = std::env::temp_dir().join(format!("gb-bundle-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let repo_root = Utf8PathBuf::from_path_buf(tmp.join("repo")).unwrap();
        fs::create_dir_all(repo_root.as_std_path()).unwrap();
        let mut agents = BTreeMap::new();
        agents.insert(agent_id("claude-acp"), claude_acp_config());
        let manager = SkillManager::new(GoldBandPaths::new(repo_root.clone()), agents);
        (tmp, manager, repo_root)
    }

    #[test]
    fn bundle_create_writes_skill_md_and_support_files() {
        // 新建：SKILL.md 原文落盘 + 嵌套支撑文件建父目录；无 staging 残留；扫描器可见。
        let (tmp, manager, repo_root) = bundle_test_manager("create");
        let support = vec![
            ("assets/template.md".to_string(), "# tpl".to_string()),
            ("nested/deep/ref.md".to_string(), "ref".to_string()),
        ];

        let result = manager
            .write_bundle_instance(
                "demo-skill",
                SkillSource::Project,
                "---\nname: demo-skill\ndescription: d\n---\n\nBody",
                &support,
                Some(repo_root.as_str()),
                None,
                None,
            )
            .unwrap();

        let skills_root = repo_root.join(".gold-band").join("skills");
        let skill_dir = skills_root.join("demo-skill");
        assert_eq!(result.directory_path, skill_dir);
        assert_eq!(
            fs::read_to_string(skill_dir.join("SKILL.md").as_std_path()).unwrap(),
            "---\nname: demo-skill\ndescription: d\n---\n\nBody"
        );
        assert_eq!(
            fs::read_to_string(skill_dir.join("assets").join("template.md").as_std_path()).unwrap(),
            "# tpl"
        );
        assert_eq!(
            fs::read_to_string(
                skill_dir
                    .join("nested")
                    .join("deep")
                    .join("ref.md")
                    .as_std_path()
            )
            .unwrap(),
            "ref"
        );
        // staging / backup 无残留。
        assert_no_staging_leftovers(&skills_root);
        // 落盘后对扫描器可见（staging 不可见语义的反向验收）。
        let scanned = scan_skills_dir(&skills_root, SkillSource::Project, ".gold-band");
        assert_eq!(
            scanned
                .iter()
                .map(|meta| meta.name.clone())
                .collect::<Vec<_>>(),
            ["demo-skill"]
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundle_overwrite_is_mirror_and_removes_stale_local_files() {
        // 覆盖 = 远端镜像：远端已删的本地支撑文件（stale/old.md、assets/keep.md）随之清除，
        // 新支撑文件写入、SKILL.md 替换；agent symlink 指向目录路径，跨 swap 身份保持。
        let (tmp, manager, repo_root) = bundle_test_manager("overwrite");
        let skills_root = repo_root.join(".gold-band").join("skills");
        let skill_dir = skills_root.join("demo-skill");
        fs::create_dir_all(skill_dir.join("stale").as_std_path()).unwrap();
        fs::create_dir_all(skill_dir.join("assets").as_std_path()).unwrap();
        fs::write(
            skill_dir.join("SKILL.md").as_std_path(),
            "---\nname: demo-skill\ndescription: old\n---\nold body",
        )
        .unwrap();
        fs::write(
            skill_dir.join("stale").join("old.md").as_std_path(),
            "stale",
        )
        .unwrap();
        fs::write(
            skill_dir.join("assets").join("keep.md").as_std_path(),
            "keep",
        )
        .unwrap();

        // 覆盖前建立 agent 同步链接。
        manager
            .sync_skill_instance(
                "demo-skill",
                skill_dir.as_str(),
                SkillSource::Project,
                Some(repo_root.as_str()),
                Some(&["claude-acp".to_string()]),
            )
            .unwrap();
        let link = repo_root.join(".claude").join("skills").join("demo-skill");
        assert!(is_link_pointing_to(
            link.as_std_path(),
            &canonicalize_lossy(skill_dir.as_std_path())
        ));

        let support = vec![("assets/new.md".to_string(), "new".to_string())];
        manager
            .write_bundle_instance(
                "demo-skill",
                SkillSource::Project,
                "---\nname: demo-skill\ndescription: new\n---\n\nNew body",
                &support,
                Some(repo_root.as_str()),
                Some(skill_dir.as_str()),
                None,
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(skill_dir.join("SKILL.md").as_std_path()).unwrap(),
            "---\nname: demo-skill\ndescription: new\n---\n\nNew body"
        );
        assert!(skill_dir.join("assets").join("new.md").is_file());
        assert!(!skill_dir.join("stale").join("old.md").exists());
        assert!(!skill_dir.join("stale").exists());
        assert!(!skill_dir.join("assets").join("keep.md").exists());
        // 链接仍指向同一目录路径（swap 对 path 级链接透明）。
        assert!(is_link_pointing_to(
            link.as_std_path(),
            &canonicalize_lossy(skill_dir.as_std_path())
        ));
        assert_no_staging_leftovers(&skills_root);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundle_write_sweeps_crash_leftovers_across_pids() {
        // 崩溃残留兜底：其他 pid 的 staging/backup 残留（含 SKILL.md 的形态对扫描器可见）
        // 由同 skill 下次写入的前缀清扫清除——pid 隔离的清扫在重启后永远够不到；
        // 前缀按 skill 目录名隔离，其他 skill 的残留不受影响。
        let (tmp, manager, repo_root) = bundle_test_manager("sweep");
        let skills_root = repo_root.join(".gold-band").join("skills");
        let foreign_staging = skills_root.join(".mstaging-demo-skill-4242");
        let foreign_backup = skills_root.join(".mbackup-demo-skill-777");
        let other_skill_staging = skills_root.join(".mstaging-other-skill-4242");
        for dir in [&foreign_staging, &foreign_backup, &other_skill_staging] {
            fs::create_dir_all(dir.as_std_path()).unwrap();
            // 模拟「SKILL.md 已写完、swap 未完成」的崩溃形态。
            fs::write(
                dir.join("SKILL.md").as_std_path(),
                "---\nname: stale\n---\nbody",
            )
            .unwrap();
        }

        manager
            .write_bundle_instance(
                "demo-skill",
                SkillSource::Project,
                "---\nname: demo-skill\ndescription: d\n---\n\nBody",
                &[],
                Some(repo_root.as_str()),
                None,
                None,
            )
            .unwrap();

        assert!(!foreign_staging.exists());
        assert!(!foreign_backup.exists());
        assert!(other_skill_staging.exists());
        // 清扫发生在写入前，正常落盘不受残留影响。
        assert!(skills_root.join("demo-skill").join("SKILL.md").is_file());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundle_write_fails_when_current_dir_missing() {
        // current_directory_path 指向不存在目录 → 明确错误（与 write_instance 同语义）。
        let (tmp, manager, repo_root) = bundle_test_manager("missing");
        let error = manager
            .write_bundle_instance(
                "demo-skill",
                SkillSource::Project,
                "---\nname: demo-skill\n---\nbody",
                &[],
                Some(repo_root.as_str()),
                Some(repo_root.join(".gold-band/skills/absent").as_str()),
                None,
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("SKILL dir not found"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bundle_validation_rejects_unsafe_and_oversized_entries() {
        // 路径安全：穿越段、绝对路径、反斜杠、根 SKILL.md 混入、重复路径、非法字符。
        for bad_path in [
            "../escape.md",
            "a/../b.md",
            "/abs.md",
            "a\\b.md",
            "SKILL.md",
            "a<b.md",
            "a/b/c/d/e.md",
        ] {
            let error = validate_skill_bundle_entries(
                "---\nname: x\n---\nb",
                &[(bad_path.to_string(), "c".to_string())],
            )
            .unwrap_err();
            assert!(
                !format!("{error:#}").is_empty(),
                "expected rejection for path: {bad_path}"
            );
        }
        // 重复路径。
        assert!(
            validate_skill_bundle_entries(
                "---\nname: x\n---\nb",
                &[
                    ("a.md".to_string(), "c".to_string()),
                    ("a.md".to_string(), "d".to_string())
                ]
            )
            .is_err()
        );
        // 文件总数：SKILL.md + 256 支撑文件 = 257 超限。
        let many: Vec<(String, String)> = (0..SKILL_BUNDLE_MAX_FILES)
            .map(|i| (format!("f{i}.md"), "c".to_string()))
            .collect();
        let error = validate_skill_bundle_entries("---\nname: x\n---\nb", &many).unwrap_err();
        assert!(format!("{error:#}").contains("256 file limit"));
        // 边界恰好到顶（255 支撑 + SKILL.md = 256）必须通过。
        let edge = &many[..SKILL_BUNDLE_MAX_FILES - 1];
        assert!(validate_skill_bundle_entries("---\nname: x\n---\nb", edge).is_ok());
        // 单文件超限：SKILL.md 与支撑文件各 1MiB+1。
        let big = "x".repeat(SKILL_BUNDLE_MAX_FILE_BYTES as usize + 1);
        assert!(validate_skill_bundle_entries(&big, &[]).is_err());
        assert!(
            validate_skill_bundle_entries("---\nname: x\n---\nb", &[("big.md".to_string(), big)])
                .is_err()
        );
        // 整包超限：9 个恰好 1MiB 的支撑文件（单文件均不超限）合计 > 8MiB。
        let one_mib = "x".repeat(SKILL_BUNDLE_MAX_FILE_BYTES as usize);
        let nine: Vec<(String, String)> = (0..9)
            .map(|i| (format!("f{i}.md"), one_mib.clone()))
            .collect();
        let error = validate_skill_bundle_entries("---\nname: x\n---\nb", &nine).unwrap_err();
        assert!(format!("{error:#}").contains("8MiB limit"));
    }
}
