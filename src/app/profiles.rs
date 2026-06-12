use anyhow::{Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::DesktopLanguage;
use crate::prompts::{
    PROFILE_ACCEPT_EN, PROFILE_ACCEPT_ZH_CN, PROFILE_CLEAN_EN, PROFILE_CLEAN_ZH_CN, PROFILE_DEV_EN,
    PROFILE_DEV_ZH_CN, PROFILE_PLAN_EN, PROFILE_PLAN_ZH_CN, PROFILE_REVIEW_EN,
    PROFILE_REVIEW_ZH_CN, PROFILE_TEST_EN, PROFILE_TEST_ZH_CN, prompt_by_language,
};
use crate::storage::{GoldBandPaths, ensure_parent_dir};

static PROFILE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
const BUILT_IN_PROFILE_TIMESTAMP: &str = "2026-05-27 00:00:00";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileScope {
    BuiltIn,
    User,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub scope: ProfileScope,
    pub name: String,
    pub summary: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub content: String,
    pub scope: ProfileScope,
    pub is_built_in: bool,
    pub created_at: String,
    pub updated_at: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileList {
    pub profiles: Vec<ProfileEntry>,
}

struct ParsedProfile {
    id: String,
    name: String,
    summary: String,
    created_at: String,
    updated_at: String,
    content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultProfileIds {
    by_key: BTreeMap<String, String>,
}

impl DefaultProfileIds {
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.by_key.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy)]
struct DefaultProfileSeed {
    key: &'static str,
    id: &'static str,
    name: &'static str,
    summary: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileCommandError {
    #[error("profile.readonly-built-in")]
    ReadonlyBuiltIn,
    #[error("profile.built-in-scope-unsupported")]
    BuiltInScopeUnsupported,
    #[error("profile.delete-confirmation-required")]
    DeleteConfirmationRequired {
        template_count: usize,
        task_count: usize,
        run_count: usize,
    },
}

impl ProfileCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReadonlyBuiltIn => "profile.readonly-built-in",
            Self::BuiltInScopeUnsupported => "profile.built-in-scope-unsupported",
            Self::DeleteConfirmationRequired { .. } => "profile.delete-confirmation-required",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::ReadonlyBuiltIn | Self::BuiltInScopeUnsupported => json!({}),
            Self::DeleteConfirmationRequired {
                template_count,
                task_count,
                run_count,
            } => json!({
                "templateCount": template_count,
                "taskCount": task_count,
                "runCount": run_count,
            }),
        }
    }
}

const DEFAULT_PROFILE_SEEDS: &[DefaultProfileSeed] = &[
    DefaultProfileSeed {
        key: "plan",
        id: "pf-builtin-plan",
        name: "方案",
        summary: "方案角色，用于需求分析和实施方案设计。",
    },
    DefaultProfileSeed {
        key: "dev",
        id: "pf-builtin-dev",
        name: "开发",
        summary: "开发角色，用于实现需求并维护代码质量。",
    },
    DefaultProfileSeed {
        key: "review",
        id: "pf-builtin-review",
        name: "审查",
        summary: "审查角色，用于检查实现质量、风险和一致性。",
    },
    DefaultProfileSeed {
        key: "test",
        id: "pf-builtin-test",
        name: "测试",
        summary: "测试角色，用于执行验证并反馈质量结果。",
    },
    DefaultProfileSeed {
        key: "accept",
        id: "pf-builtin-accept",
        name: "验收",
        summary: "验收角色，用于对照需求判断交付是否满足目标。",
    },
    DefaultProfileSeed {
        key: "cleanup",
        id: "pf-builtin-cleanup",
        name: "清理",
        summary: "清理角色，用于验收成功后的资源释放、收尾和环境清理。",
    },
];

pub(crate) fn ensure_default_user_profiles(_paths: &GoldBandPaths) -> Result<DefaultProfileIds> {
    let by_key = DEFAULT_PROFILE_SEEDS
        .iter()
        .map(|seed| (seed.key.to_string(), seed.id.to_string()))
        .collect();
    Ok(DefaultProfileIds { by_key })
}

pub(crate) fn list_profiles(
    paths: &GoldBandPaths,
    language: DesktopLanguage,
) -> Result<ProfileList> {
    let mut profiles = Vec::new();
    profiles.extend(read_profile_dir(paths, ProfileScope::Project)?);
    profiles.extend(read_profile_dir(paths, ProfileScope::User)?);
    profiles.extend(built_in_profiles(language));
    profiles.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| scope_rank(left.scope).cmp(&scope_rank(right.scope)))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(ProfileList { profiles })
}

pub(crate) fn show_profile(
    paths: &GoldBandPaths,
    id: &str,
    language: DesktopLanguage,
) -> Result<ProfileEntry> {
    find_profile_by_id(paths, id, language)?.ok_or_else(|| anyhow!("profile `{id}` not found"))
}

pub(crate) fn create_profile(paths: &GoldBandPaths, input: ProfileInput) -> Result<ProfileEntry> {
    ensure_profile_input(&input)?;
    let now = local_timestamp();
    let mut entry = ProfileEntry {
        id: next_profile_id(paths)?,
        name: input.name.trim().to_string(),
        summary: input.summary.trim().to_string(),
        content: input.content,
        scope: input.scope,
        is_built_in: false,
        created_at: now.clone(),
        updated_at: now,
        path: String::new(),
    };
    entry.path = profile_path(paths, entry.scope, &entry.name, &entry.id)?.to_string();
    write_profile(paths, &entry)?;
    show_profile(paths, &entry.id, DesktopLanguage::ZhCn)
}

pub(crate) fn update_profile(
    paths: &GoldBandPaths,
    id: &str,
    input: ProfileInput,
) -> Result<ProfileEntry> {
    ensure_profile_input(&input)?;
    let existing = show_profile(paths, id, DesktopLanguage::ZhCn)?;
    if existing.is_built_in {
        return Err(ProfileCommandError::ReadonlyBuiltIn.into());
    }
    let mut entry = ProfileEntry {
        id: existing.id.clone(),
        name: input.name.trim().to_string(),
        summary: input.summary.trim().to_string(),
        content: input.content,
        scope: input.scope,
        is_built_in: false,
        created_at: existing.created_at,
        updated_at: local_timestamp(),
        path: String::new(),
    };
    entry.path = profile_path(paths, entry.scope, &entry.name, &entry.id)?.to_string();
    if existing.path != entry.path {
        let old_path = Utf8PathBuf::from(existing.path);
        if old_path.exists() {
            fs::remove_file(old_path.as_std_path())?;
        }
    }
    write_profile(paths, &entry)?;
    show_profile(paths, &entry.id, DesktopLanguage::ZhCn)
}

pub(crate) fn delete_profile(paths: &GoldBandPaths, id: &str) -> Result<()> {
    let existing = show_profile(paths, id, DesktopLanguage::ZhCn)?;
    if existing.is_built_in {
        return Err(ProfileCommandError::ReadonlyBuiltIn.into());
    }
    let path = Utf8PathBuf::from(existing.path);
    if path.exists() {
        fs::remove_file(path.as_std_path())?;
    }
    Ok(())
}

pub(crate) fn find_profile_by_id(
    paths: &GoldBandPaths,
    id: &str,
    language: DesktopLanguage,
) -> Result<Option<ProfileEntry>> {
    if id.trim().is_empty() {
        return Ok(None);
    }
    if let Some(profile) = built_in_profile_by_id(id, language) {
        return Ok(Some(profile));
    }
    if let Some(profile) = read_profile_dir(paths, ProfileScope::Project)?
        .into_iter()
        .find(|profile| profile.id == id)
    {
        return Ok(Some(profile));
    }
    Ok(read_profile_dir(paths, ProfileScope::User)?
        .into_iter()
        .find(|profile| profile.id == id))
}

fn built_in_profiles(language: DesktopLanguage) -> Vec<ProfileEntry> {
    DEFAULT_PROFILE_SEEDS
        .iter()
        .map(|seed| ProfileEntry {
            id: seed.id.to_string(),
            name: seed.name.to_string(),
            summary: seed.summary.to_string(),
            content: built_in_profile_content(seed.key, language).to_string(),
            scope: ProfileScope::BuiltIn,
            is_built_in: true,
            created_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            updated_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            path: format!("builtin://profiles/{}", seed.key),
        })
        .collect()
}

fn built_in_profile_by_id(id: &str, language: DesktopLanguage) -> Option<ProfileEntry> {
    DEFAULT_PROFILE_SEEDS
        .iter()
        .find(|seed| seed.id == id)
        .map(|seed| ProfileEntry {
            id: seed.id.to_string(),
            name: seed.name.to_string(),
            summary: seed.summary.to_string(),
            content: built_in_profile_content(seed.key, language).to_string(),
            scope: ProfileScope::BuiltIn,
            is_built_in: true,
            created_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            updated_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            path: format!("builtin://profiles/{}", seed.key),
        })
}

fn built_in_profile_content(key: &str, language: DesktopLanguage) -> &'static str {
    match key {
        "plan" => prompt_by_language(language, PROFILE_PLAN_ZH_CN, PROFILE_PLAN_EN),
        "dev" => prompt_by_language(language, PROFILE_DEV_ZH_CN, PROFILE_DEV_EN),
        "review" => prompt_by_language(language, PROFILE_REVIEW_ZH_CN, PROFILE_REVIEW_EN),
        "test" => prompt_by_language(language, PROFILE_TEST_ZH_CN, PROFILE_TEST_EN),
        "accept" => prompt_by_language(language, PROFILE_ACCEPT_ZH_CN, PROFILE_ACCEPT_EN),
        "cleanup" => prompt_by_language(language, PROFILE_CLEAN_ZH_CN, PROFILE_CLEAN_EN),
        _ => "",
    }
}

fn read_profile_dir(paths: &GoldBandPaths, scope: ProfileScope) -> Result<Vec<ProfileEntry>> {
    let dir = profile_dir(paths, scope)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    let mut entries = fs::read_dir(dir.as_std_path())?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        let Some(path) = Utf8PathBuf::from_path_buf(path).ok() else {
            continue;
        };
        if path.extension() != Some("md") {
            continue;
        }
        let parsed = parse_profile_file(&path)?;
        profiles.push(ProfileEntry {
            id: parsed.id,
            name: parsed.name,
            summary: parsed.summary,
            content: parsed.content,
            scope,
            is_built_in: false,
            created_at: parsed.created_at,
            updated_at: parsed.updated_at,
            path: path.to_string(),
        });
    }
    Ok(profiles)
}

fn parse_profile_file(path: &Utf8Path) -> Result<ParsedProfile> {
    let content = fs::read_to_string(path.as_std_path())?;
    let Some(rest) = content.strip_prefix("---\n") else {
        bail!("profile `{path}` is missing front matter");
    };
    let Some((front_matter, body)) = rest.split_once("\n---") else {
        bail!("profile `{path}` has invalid front matter");
    };
    let body = body.strip_prefix('\n').unwrap_or(body).to_string();
    let fields = parse_front_matter(front_matter);
    let id = fields
        .get("id")
        .cloned()
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.rsplit_once('-').map(|(_, id)| id.to_string()))
        })
        .ok_or_else(|| anyhow!("profile `{path}` is missing id"))?;
    let now = local_timestamp();
    Ok(ParsedProfile {
        id,
        name: fields
            .get("name")
            .cloned()
            .unwrap_or_else(|| "未命名角色".to_string()),
        summary: fields.get("summary").cloned().unwrap_or_default(),
        created_at: fields
            .get("createdAt")
            .cloned()
            .unwrap_or_else(|| now.clone()),
        updated_at: fields.get("updatedAt").cloned().unwrap_or(now),
        content: body,
    })
}

fn parse_front_matter(front_matter: &str) -> BTreeMap<String, String> {
    front_matter
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), unquote(value.trim()).to_string()))
        })
        .collect()
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn write_profile(paths: &GoldBandPaths, profile: &ProfileEntry) -> Result<()> {
    if profile.is_built_in || profile.scope == ProfileScope::BuiltIn {
        return Err(ProfileCommandError::ReadonlyBuiltIn.into());
    }
    let path = profile_path(paths, profile.scope, &profile.name, &profile.id)?;
    ensure_parent_dir(&path)?;
    fs::write(path.as_std_path(), profile_markdown(profile))?;
    Ok(())
}

fn profile_markdown(profile: &ProfileEntry) -> String {
    format!(
        "---\nid: {}\nname: {}\nsummary: {}\ncreatedAt: {}\nupdatedAt: {}\n---\n{}",
        yaml_scalar(&profile.id),
        yaml_scalar(&profile.name),
        yaml_scalar(&profile.summary),
        yaml_scalar(&profile.created_at),
        yaml_scalar(&profile.updated_at),
        profile.content
    )
}

fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn ensure_profile_input(input: &ProfileInput) -> Result<()> {
    if input.scope == ProfileScope::BuiltIn {
        return Err(ProfileCommandError::BuiltInScopeUnsupported.into());
    }
    if input.name.trim().is_empty() {
        bail!("profile name cannot be empty");
    }
    if input.summary.trim().is_empty() {
        bail!("profile summary cannot be empty");
    }
    Ok(())
}

fn profile_dir(paths: &GoldBandPaths, scope: ProfileScope) -> Result<Utf8PathBuf> {
    match scope {
        ProfileScope::User => Ok(paths.user_context_profiles_dir()),
        ProfileScope::Project => Ok(paths.project_context_profiles_dir()),
        ProfileScope::BuiltIn => Err(ProfileCommandError::BuiltInScopeUnsupported.into()),
    }
}

fn profile_path(
    paths: &GoldBandPaths,
    scope: ProfileScope,
    name: &str,
    id: &str,
) -> Result<Utf8PathBuf> {
    Ok(profile_dir(paths, scope)?.join(format!("{}-{id}.md", sanitize_profile_name(name))))
}

fn sanitize_profile_name(name: &str) -> String {
    let mut sanitized = String::new();
    for character in name.trim().chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
            sanitized.push(character);
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }
    let sanitized = sanitized.trim_matches('-').to_string();
    if sanitized.is_empty() {
        "profile".to_string()
    } else {
        sanitized
    }
}

fn next_profile_id(paths: &GoldBandPaths) -> Result<String> {
    loop {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let counter = PROFILE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!(
            "pf-{}-{}-{}",
            base36(timestamp),
            base36(u128::from(std::process::id())),
            base36(u128::from(counter))
        );
        if find_profile_by_id(paths, &id, DesktopLanguage::ZhCn)?.is_none() {
            return Ok(id);
        }
    }
}

fn base36(mut value: u128) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut output = Vec::new();
    while value > 0 {
        output.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    output.reverse();
    String::from_utf8(output).expect("base36 uses ascii digits")
}

fn local_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn scope_rank(scope: ProfileScope) -> u8 {
    match scope {
        ProfileScope::BuiltIn => 0,
        ProfileScope::Project => 1,
        ProfileScope::User => 2,
    }
}
