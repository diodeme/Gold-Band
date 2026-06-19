// ── SKILL Manager ──
// 对标 Zed crates/agent_skills/src/agent_skills.rs

pub mod symlink;
//
// 职责：
//   1. SKILL 文件系统管理（.agents/skills/ 全局 + 项目级）
//   2. SKILL.md 解析（frontmatter + body）
//   3. SKILL CRUD（创建、读取、删除）
//   4. System prompt SKILL catalog 渲染

use std::fs;

use anyhow::{Result, bail};
use camino::Utf8PathBuf;

use crate::config::{
    SkillMeta, SkillSource,
    AGENTS_DIR_NAME, SKILLS_DIR_NAME, SKILL_FILE_NAME,
    MAX_SKILL_DESCRIPTION_LEN,
};
use crate::storage::GoldBandPaths;

/// 对标 Zed select_catalog_skills: 50KB catalog budget
const MAX_CATALOG_BYTES: usize = 50 * 1024;

/// 对标 Zed SkillSource::precedence: Project(2) > Global(1) > BuiltIn(0)
fn precedence(source: SkillSource) -> u8 {
    match source {
        SkillSource::BuiltIn => 0,
        SkillSource::Global => 1,
        SkillSource::Project => 2,
    }
}

/// 对标 Zed apply_skill_overrides: 按优先级去重，高优先级遮蔽低优先级
fn apply_skill_overrides(skills: &[SkillMeta]) -> Vec<SkillMeta> {
    let mut overrides: std::collections::BTreeMap<&str, &SkillMeta> = std::collections::BTreeMap::new();
    for s in skills {
        let entry = overrides.entry(&s.name).or_insert(s);
        if precedence(s.source) > precedence(entry.source) {
            *entry = s;
        }
    }
    let mut result: Vec<_> = overrides.into_values().cloned().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// 对标 Zed select_catalog_skills: 按字母序截断，不超过 MAX_CATALOG_BYTES
fn select_catalog_skills(skills: &[SkillMeta]) -> Vec<SkillMeta> {
    let mut selected = Vec::new();
    let mut total: usize = 0;
    for s in skills {
        let entry_bytes = s.name.len() + s.description.len();
        if total + entry_bytes > MAX_CATALOG_BYTES && !selected.is_empty() {
            break;
        }
        selected.push(s.clone());
        total += entry_bytes;
    }
    selected
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

pub struct SkillManager {
    paths: GoldBandPaths,
}

impl SkillManager {
    pub fn new(paths: GoldBandPaths) -> Self {
        Self { paths }
    }

    /// 从 workspace path 构建项目 SKILL 目录
    pub fn workspace_skills_dir(workspace_path: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(workspace_path)
            .join(AGENTS_DIR_NAME)
            .join(SKILLS_DIR_NAME)
    }

    // ── CRUD ──

    pub fn list(&self) -> Result<SkillListResult> {
        let global = scan_skills_dir(&GoldBandPaths::global_skills_dir(), SkillSource::Global);
        // 项目级 SKILL 扫描所有已知 workspace + 默认项目目录
        let mut project = scan_skills_dir(&self.paths.project_skills_dir(), SkillSource::Project);
        project.sort_by(|a, b| a.name.cmp(&b.name));
        project.dedup_by(|a, b| a.name == b.name);
        Ok(SkillListResult { global, project })
    }

    pub fn list_by_workspace(&self, workspace_path: &str) -> Result<Vec<SkillMeta>> {
        let dir = Self::workspace_skills_dir(workspace_path);
        Ok(scan_skills_dir(&dir, SkillSource::Project))
    }

    pub fn read(&self, name: &str, source: SkillSource) -> Result<SkillContent> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_path = dir.join(name).join(SKILL_FILE_NAME);
        if !skill_path.exists() {
            bail!("SKILL `{name}` not found");
        }
        let raw = fs::read_to_string(&skill_path)?;
        let (meta, body) = parse_skill_md(&raw, name, source, skill_path.as_str())?;
        Ok(SkillContent { meta, body })
    }

    pub fn write(&self, name: &str, source: SkillSource, content: &str) -> Result<SkillMeta> {
        let dir = skills_dir_for_source(source, &self.paths)?;
        let skill_dir = dir.join(name);
        fs::create_dir_all(skill_dir.as_std_path())?;
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let (meta, _) = parse_skill_md(content, name, source, skill_path.as_str())?;
        Ok(meta)
    }

    /// 写入到指定 workspace 的项目 SKILL 目录
    pub fn write_to_workspace(&self, name: &str, workspace_path: &str, content: &str) -> Result<SkillMeta> {
        let dir = Self::workspace_skills_dir(workspace_path);
        let skill_dir = dir.join(name);
        fs::create_dir_all(skill_dir.as_std_path())?;
        let skill_path = skill_dir.join(SKILL_FILE_NAME);
        fs::write(skill_path.as_std_path(), content)?;
        let (meta, _) = parse_skill_md(content, name, SkillSource::Project, skill_path.as_str())?;
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

    // ── System Prompt Catalog ──

    /// 用于渲染 {{SKILL_CATALOG}} 占位符的 SKILL 列表（对标 Zed system_prompt.hbs）
    /// 不含 workspace 过滤 — 返回全局 + 默认项目目录的全部 SKILL
    pub fn catalog_skills(&self) -> Result<Vec<SkillMeta>> {
        let list = self.list()?;
        let mut all = list.global;
        all.extend(list.project);
        all.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(all)
    }

    /// 对标 Zed ProjectState.skills: 仅返回当前 workspace 的项目 SKILL + 全局 SKILL
    pub fn catalog_skills_for_workspace(&self, workspace_path: &str) -> Result<Vec<SkillMeta>> {
        let list = self.list()?;
        let project = self.list_by_workspace(workspace_path)?;
        let mut all = list.global;
        all.extend(project);
        all.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(all)
    }

    /// 对标 Zed: 排除 disable_model_invocation + 优先级去重 + 预算截断
    pub fn catalog_skills_for_agent(&self) -> Result<Vec<SkillMeta>> {
        let skills: Vec<SkillMeta> = self
            .catalog_skills()?
            .into_iter()
            .filter(|s| !s.disable_model_invocation)
            .collect();
        let overridden = apply_skill_overrides(&skills);
        Ok(select_catalog_skills(&overridden))
    }

    /// 对标 Zed ProjectState: 仅当前 workspace 的项目 SKILL + 全局 SKILL
    pub fn catalog_skills_for_agent_workspace(&self, workspace_path: &str) -> Result<Vec<SkillMeta>> {
        let skills: Vec<SkillMeta> = self
            .catalog_skills_for_workspace(workspace_path)?
            .into_iter()
            .filter(|s| !s.disable_model_invocation)
            .collect();
        let overridden = apply_skill_overrides(&skills);
        Ok(select_catalog_skills(&overridden))
    }

    /// 渲染 SKILL 目录为 system prompt 片段（仅目录，不含 body — 对标 Zed）
    pub fn render_skill_catalog(&self, language: crate::config::DesktopLanguage) -> Result<String> {
        self.render_skill_catalog_for_workspace(language, None)
    }

    /// 按 workspace 隔离：仅加载当前 workspace 的项目 SKILL + 全局 SKILL
    pub fn render_skill_catalog_for_workspace(
        &self,
        language: crate::config::DesktopLanguage,
        workspace_path: Option<&str>,
    ) -> Result<String> {
        let skills = match workspace_path {
            Some(path) => self.catalog_skills_for_agent_workspace(path)?,
            None => self.catalog_skills_for_agent()?,
        };
        if skills.is_empty() {
            return Ok(String::new());
        }
        let template = crate::prompts::prompt_by_language(
            language,
            crate::prompts::SKILL_CATALOG_BLOCK_ZH_CN,
            crate::prompts::SKILL_CATALOG_BLOCK_EN,
        );
        let has_skills = true;
        let context = serde_json::json!({
            "has_skills": has_skills,
            "skills": skills,
        });
        crate::prompts::render(template, context)
    }
}

// ── Helpers ──

fn skills_dir_for_source(
    source: SkillSource,
    paths: &GoldBandPaths,
) -> Result<Utf8PathBuf> {
    match source {
        SkillSource::Global => Ok(GoldBandPaths::global_skills_dir()),
        SkillSource::Project => Ok(paths.project_skills_dir()),
        SkillSource::BuiltIn => bail!("built-in skills are not supported yet"),
    }
}

pub(crate) fn scan_skills_dir(dir: &Utf8PathBuf, source: SkillSource) -> Vec<SkillMeta> {
    let mut skills = Vec::new();
    let Ok(entries) = fs::read_dir(dir.as_std_path()) else {
        return skills;
    };
    for entry in entries.flatten() {
        let path = entry.path();
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
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let dir_path = Utf8PathBuf::from_path_buf(path.clone()).unwrap_or_default();
        match parse_skill_md(&raw, name, source, dir_path.as_str()) {
            Ok((meta, _)) => skills.push(meta),
            Err(_) => continue,
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

pub fn parse_skill_md_public(
    raw: &str,
    default_name: &str,
    source: SkillSource,
    dir_path: &str,
) -> (SkillMeta, String) {
    parse_skill_md(raw, default_name, source, dir_path).unwrap_or_else(|_| {
        (SkillMeta {
            name: default_name.to_string(),
            description: String::new(),
            source,
            directory_path: dir_path.to_string(),
            disable_model_invocation: false,
            load_warnings: vec![],
        }, raw.to_string())
    })
}

fn parse_skill_md(
    raw: &str,
    default_name: &str,
    source: SkillSource,
    dir_path: &str,
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
    let mut disable_model_invocation = false;

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
            } else if line == "disable-model-invocation: true" {
                disable_model_invocation = true;
            }
        }
    }

    Ok((
        SkillMeta {
            name: parsed_name,
            description,
            source,
            directory_path: dir_path.to_string(),
            disable_model_invocation,
            load_warnings,
        },
        body,
    ))
}
