use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::DesktopLanguage;
use crate::frontmatter::{
    FrontmatterUpdate, parse_frontmatter_document, render_frontmatter_document,
    update_frontmatter_document,
};
use crate::prompts::{
    PROFILE_ACCEPT_EN, PROFILE_ACCEPT_ZH_CN, PROFILE_CLEAN_EN, PROFILE_CLEAN_ZH_CN, PROFILE_INTERVIEW_EN, PROFILE_INTERVIEW_ZH_CN, PROFILE_DEV_EN,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
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
    pub summary_source: String,
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
    summary_source: String,
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
    DefaultProfileSeed {
        key: "interview",
        id: "pf-builtin-interview",
        name: "访谈",
        summary: "访谈角色，用于需求澄清，通过深度访谈把模糊需求转化为清晰规格。",
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
        summary_source: input.summary.trim().to_string(),
        content: input.content,
        scope: ProfileScope::User,
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
        summary_source: input.summary.trim().to_string(),
        content: input.content,
        scope: ProfileScope::User,
        is_built_in: false,
        created_at: existing.created_at,
        updated_at: local_timestamp(),
        path: String::new(),
    };
    entry.path = profile_path(paths, entry.scope, &entry.name, &entry.id)?.to_string();
    let old_profile_path = existing.path.clone();
    let old_path = Utf8PathBuf::from(old_profile_path.as_str());
    let old_content = fs::read_to_string(old_path.as_std_path()).ok();
    if old_profile_path != entry.path {
        if old_path.exists() {
            fs::remove_file(old_path.as_std_path())?;
        }
    }
    write_profile_preserving_frontmatter(paths, &entry, old_content.as_deref())?;
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
            summary_source: seed.summary.to_string(),
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
            summary_source: seed.summary.to_string(),
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
        "interview" => prompt_by_language(language, PROFILE_INTERVIEW_ZH_CN, PROFILE_INTERVIEW_EN),
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
            summary_source: parsed.summary_source,
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
    let document =
        parse_frontmatter_document(&content).with_context(|| format!("profile `{path}`"))?;
    let fields = document.fields;
    let id = fields
        .get("id")
        .map(|value| value.trim().to_string())
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
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| "未命名角色".to_string()),
        summary: fields
            .get("summary")
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        summary_source: document
            .field_sources
            .get("summary")
            .cloned()
            .or_else(|| fields.get("summary").cloned())
            .unwrap_or_default(),
        created_at: fields
            .get("createdAt")
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| now.clone()),
        updated_at: fields
            .get("updatedAt")
            .map(|value| value.trim().to_string())
            .unwrap_or(now),
        content: document.body,
    })
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

fn write_profile_preserving_frontmatter(
    paths: &GoldBandPaths,
    profile: &ProfileEntry,
    old_content: Option<&str>,
) -> Result<()> {
    if profile.is_built_in || profile.scope == ProfileScope::BuiltIn {
        return Err(ProfileCommandError::ReadonlyBuiltIn.into());
    }
    let path = profile_path(paths, profile.scope, &profile.name, &profile.id)?;
    ensure_parent_dir(&path)?;
    let markdown = if let Some(old_content) = old_content {
        update_frontmatter_document(
            old_content,
            &profile_frontmatter_updates(profile),
            &profile.content,
        )?
    } else {
        profile_markdown(profile)
    };
    fs::write(path.as_std_path(), markdown)?;
    Ok(())
}

fn profile_markdown(profile: &ProfileEntry) -> String {
    render_frontmatter_document(&profile_frontmatter_updates(profile), &profile.content)
}

fn profile_frontmatter_updates(profile: &ProfileEntry) -> [FrontmatterUpdate<'_>; 5] {
    [
        FrontmatterUpdate {
            key: "id",
            value: &profile.id,
            source: None,
        },
        FrontmatterUpdate {
            key: "name",
            value: &profile.name,
            source: None,
        },
        FrontmatterUpdate {
            key: "summary",
            value: &profile.summary,
            source: Some(&profile.summary_source),
        },
        FrontmatterUpdate {
            key: "createdAt",
            value: &profile.created_at,
            source: None,
        },
        FrontmatterUpdate {
            key: "updatedAt",
            value: &profile.updated_at,
            source: None,
        },
    ]
}

fn ensure_profile_input(input: &ProfileInput) -> Result<()> {
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
        ProfileScope::User => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_profile_file_supports_folded_summary_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(tmp.path().join("review-pf-test.md")).unwrap();
        fs::write(
            path.as_std_path(),
            r#"---
id: pf-test
name: review
summary: >
  审查角色，
  用于检查实现质量。
createdAt: 2026-07-09 10:00:00
updatedAt: 2026-07-09 10:00:00
---
profile body
"#,
        )
        .unwrap();

        let profile = parse_profile_file(&path).unwrap();

        assert_eq!(profile.id, "pf-test");
        assert_eq!(profile.name, "review");
        assert_eq!(profile.summary, "审查角色， 用于检查实现质量。");
        assert_eq!(profile.summary_source, "审查角色，\n用于检查实现质量。");
        assert_eq!(profile.content, "profile body\n");
    }

    #[test]
    fn update_profile_preserves_unknown_frontmatter_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        fs::create_dir_all(paths.user_context_profiles_dir().as_std_path()).unwrap();
        let path = paths.user_context_profiles_dir().join("review-pf-test.md");
        fs::write(
            path.as_std_path(),
            "---\nid: pf-test\nname: review\nsummary: >\n  审查角色，\n  用于检查实现质量。\nextra: keep-me\ncreatedAt: 2026-07-09 10:00:00\nupdatedAt: 2026-07-09 10:00:00\n---\nold body\n",
        )
        .unwrap();

        update_profile(
            &paths,
            "pf-test",
            ProfileInput {
                name: "review".to_string(),
                summary: "审查角色，\n用于检查输出质量。".to_string(),
                content: "new body\n".to_string(),
            },
        )
        .unwrap();

        let saved = fs::read_to_string(path.as_std_path()).unwrap();
        assert!(saved.contains("extra: keep-me"));
        assert!(saved.contains("summary: >\n  审查角色，\n  用于检查输出质量。\n"));
        assert!(saved.ends_with("---\nnew body\n"));
    }

    #[test]
    fn profile_input_rejects_legacy_scope_field() {
        let err = serde_json::from_str::<ProfileInput>(
            r#"{"scope":"project","name":"role","summary":"summary","content":"body"}"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unknown field `scope`"));
    }
}
