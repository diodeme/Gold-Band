use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;
use walkdir::WalkDir;

use crate::config::DesktopLanguage;
use crate::frontmatter::{
    FrontmatterUpdate, parse_frontmatter_document, parse_optional_frontmatter_document,
    render_frontmatter_document, update_frontmatter_document,
};
use crate::prompts::{
    PROFILE_ACCEPT_EN, PROFILE_ACCEPT_ZH_CN, PROFILE_CLEAN_EN, PROFILE_CLEAN_ZH_CN, PROFILE_DEV_EN,
    PROFILE_DEV_TEST_EN, PROFILE_DEV_TEST_ZH_CN, PROFILE_DEV_ZH_CN, PROFILE_GRILLME_EN,
    PROFILE_GRILLME_ZH_CN, PROFILE_INTERVIEW_EN, PROFILE_INTERVIEW_ZH_CN, PROFILE_PLAN_EN,
    PROFILE_PLAN_ZH_CN, PROFILE_REVIEW_EN, PROFILE_REVIEW_ZH_CN, PROFILE_TEST_EN,
    PROFILE_TEST_ZH_CN, profile_template_validation_contexts, prompt_by_language, render,
};
use crate::storage::{GoldBandPaths, ensure_parent_dir};

static PROFILE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
const BUILT_IN_PROFILE_TIMESTAMP: &str = "2026-05-27 00:00:00";
const IMPORT_PROFILE_FILE_CAP: usize = 5000;

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
    #[serde(default)]
    pub dynamic_template: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEntry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub summary_source: String,
    pub content: String,
    pub dynamic_template: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProfilesInput {
    pub folder_path: String,
    #[serde(default)]
    pub dynamic_template: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportRecordStatus {
    Imported,
    ImportedWithFallbacks,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileFieldFallback {
    Name,
    Summary,
    FrontmatterMissing,
    DynamicTemplateDowngraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportProfileErrorCode {
    ReadFailed,
    InvalidFrontmatter,
    EmptyFile,
    MissingName,
    CreateFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProfileError {
    pub code: ImportProfileErrorCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedProfileRecord {
    pub source_path: String,
    pub status: ImportRecordStatus,
    pub name: String,
    pub fallbacks: Vec<ProfileFieldFallback>,
    pub imported_id: Option<String>,
    pub error: Option<ImportProfileError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProfilesResult {
    pub total_scanned: usize,
    pub imported: Vec<ImportedProfileRecord>,
    pub failed: Vec<ImportedProfileRecord>,
    pub truncated: bool,
}

struct ParsedProfile {
    id: String,
    name: String,
    summary: String,
    summary_source: String,
    created_at: String,
    updated_at: String,
    content: String,
    dynamic_template: bool,
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
struct LocalizedProfileText {
    zh_cn: &'static str,
    en: &'static str,
}

impl LocalizedProfileText {
    fn value(self, language: DesktopLanguage) -> &'static str {
        prompt_by_language(language, self.zh_cn, self.en)
    }
}

#[derive(Debug, Clone, Copy)]
struct DefaultProfileSeed {
    key: &'static str,
    id: &'static str,
    name: LocalizedProfileText,
    summary: LocalizedProfileText,
    dynamic_template: bool,
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
    #[error("profile.dynamic-template-invalid")]
    InvalidDynamicTemplate { reason: String },
    #[error("profile.import.folder-not-found")]
    ImportFolderNotFound,
    #[error("profile.import.folder-not-directory")]
    ImportFolderNotDirectory,
    #[error("profile.import.folder-read-failed")]
    ImportFolderReadFailed,
    #[error("profile.import.no-markdown-files")]
    ImportNoMarkdownFiles,
}

impl ProfileCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReadonlyBuiltIn => "profile.readonly-built-in",
            Self::BuiltInScopeUnsupported => "profile.built-in-scope-unsupported",
            Self::DeleteConfirmationRequired { .. } => "profile.delete-confirmation-required",
            Self::InvalidDynamicTemplate { .. } => "profile.dynamic-template-invalid",
            Self::ImportFolderNotFound => "profile.import.folder-not-found",
            Self::ImportFolderNotDirectory => "profile.import.folder-not-directory",
            Self::ImportFolderReadFailed => "profile.import.folder-read-failed",
            Self::ImportNoMarkdownFiles => "profile.import.no-markdown-files",
        }
    }

    pub fn params(&self) -> serde_json::Value {
        match self {
            Self::ReadonlyBuiltIn
            | Self::BuiltInScopeUnsupported
            | Self::ImportFolderNotFound
            | Self::ImportFolderNotDirectory
            | Self::ImportFolderReadFailed
            | Self::ImportNoMarkdownFiles => json!({}),
            Self::DeleteConfirmationRequired {
                template_count,
                task_count,
                run_count,
            } => json!({
                "templateCount": template_count,
                "taskCount": task_count,
                "runCount": run_count,
            }),
            Self::InvalidDynamicTemplate { reason } => json!({ "reason": reason }),
        }
    }
}

const DEFAULT_PROFILE_SEEDS: &[DefaultProfileSeed] = &[
    DefaultProfileSeed {
        key: "plan",
        id: "pf-builtin-plan",
        name: LocalizedProfileText {
            zh_cn: "方案",
            en: "Plan",
        },
        summary: LocalizedProfileText {
            zh_cn: "方案角色，用于需求分析和实施方案设计。",
            en: "Planning role for analyzing requirements and designing implementation plans.",
        },
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "dev",
        id: "pf-builtin-dev",
        name: LocalizedProfileText {
            zh_cn: "开发",
            en: "Development",
        },
        summary: LocalizedProfileText {
            zh_cn: "开发角色，用于实现需求并维护代码质量。",
            en: "Development role for implementing requirements and maintaining code quality.",
        },
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "dev-test",
        id: "pf-builtin-dev-test",
        name: LocalizedProfileText {
            zh_cn: "开发测试",
            en: "Development and Testing",
        },
        summary: LocalizedProfileText {
            zh_cn: "开发测试角色，用于在同一节点完成需求实现、自动化测试与必要回归。",
            en: "Development and testing role for implementing requirements and running automated verification in one node.",
        },
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "review",
        id: "pf-builtin-review",
        name: LocalizedProfileText {
            zh_cn: "审查",
            en: "Review",
        },
        summary: LocalizedProfileText {
            zh_cn: "审查角色，用于检查实现质量、风险和一致性。",
            en: "Review role for checking implementation quality, risks, and consistency.",
        },
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "test",
        id: "pf-builtin-test",
        name: LocalizedProfileText {
            zh_cn: "测试",
            en: "Testing",
        },
        summary: LocalizedProfileText {
            zh_cn: "测试角色，用于执行验证并反馈质量结果。",
            en: "Testing role for running verification and reporting quality results.",
        },
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "accept",
        id: "pf-builtin-accept",
        name: LocalizedProfileText {
            zh_cn: "验收",
            en: "Acceptance",
        },
        summary: LocalizedProfileText {
            zh_cn: "验收角色，用于对照需求判断交付是否满足目标。",
            en: "Acceptance role for determining whether the delivery meets the requirements.",
        },
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "cleanup",
        id: "pf-builtin-cleanup",
        name: LocalizedProfileText {
            zh_cn: "清理",
            en: "Cleanup",
        },
        summary: LocalizedProfileText {
            zh_cn: "清理角色，用于验收成功后的资源释放、收尾和环境清理。",
            en: "Cleanup role for releasing resources, finalizing handoff notes, and cleaning up the environment after acceptance.",
        },
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "interview",
        id: "pf-builtin-interview",
        name: LocalizedProfileText {
            zh_cn: "访谈",
            en: "Interview",
        },
        summary: LocalizedProfileText {
            zh_cn: "访谈角色，用于需求澄清，通过深度访谈把模糊需求转化为清晰规格。",
            en: "Interview role for clarifying requirements and turning ambiguity into clear specifications through deep interviews.",
        },
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "grill",
        id: "pf-builtin-grill",
        name: LocalizedProfileText {
            zh_cn: "拷问",
            en: "Grill",
        },
        summary: LocalizedProfileText {
            zh_cn: "拷问角色，围绕计划或决策进行毫不留情的深度访谈，直到达成共同理解。",
            en: "Grill role for rigorously challenging plans or decisions through deep interviews until shared understanding is reached.",
        },
        dynamic_template: false,
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
        dynamic_template: input.dynamic_template,
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

pub(crate) fn import_profiles_from_folder(
    paths: &GoldBandPaths,
    input: ImportProfilesInput,
) -> Result<ImportProfilesResult> {
    let folder = Utf8PathBuf::from(input.folder_path.as_str());
    if !folder.exists() {
        return Err(ProfileCommandError::ImportFolderNotFound.into());
    }
    if !folder.is_dir() {
        return Err(ProfileCommandError::ImportFolderNotDirectory.into());
    }

    let (files, truncated) = collect_md_files(&folder, IMPORT_PROFILE_FILE_CAP)?;
    if files.is_empty() {
        return Err(ProfileCommandError::ImportNoMarkdownFiles.into());
    }

    let existing = list_profiles(paths, DesktopLanguage::ZhCn)?;
    let mut used_names = existing
        .profiles
        .iter()
        .map(|profile| profile.name.clone())
        .collect::<BTreeSet<String>>();

    let mut imported = Vec::new();
    let mut failed = Vec::new();
    for path in &files {
        match import_one_profile(paths, path, input.dynamic_template, &mut used_names) {
            Ok(record) => imported.push(record),
            Err(record) => failed.push(record),
        }
    }

    Ok(ImportProfilesResult {
        total_scanned: files.len(),
        imported,
        failed,
        truncated,
    })
}

fn import_one_profile(
    paths: &GoldBandPaths,
    path: &Utf8Path,
    dynamic_template: bool,
    used_names: &mut BTreeSet<String>,
) -> Result<ImportedProfileRecord, ImportedProfileRecord> {
    let source = path.to_string();
    let file_stem = path
        .file_stem()
        .map(|stem| stem.to_string())
        .unwrap_or_default();

    let content = match fs::read_to_string(path.as_std_path()) {
        Ok(content) => content,
        Err(_) => {
            return Err(failed_record(
                &source,
                &file_stem,
                ImportProfileErrorCode::ReadFailed,
            ));
        }
    };
    if content.trim().is_empty() {
        return Err(failed_record(
            &source,
            &file_stem,
            ImportProfileErrorCode::EmptyFile,
        ));
    }

    let document = match parse_optional_frontmatter_document(&content) {
        Ok(document) => document,
        Err(_) => {
            return Err(failed_record(
                &source,
                &file_stem,
                ImportProfileErrorCode::InvalidFrontmatter,
            ));
        }
    };

    let mut fallbacks = Vec::new();
    if !content_has_frontmatter(&content) {
        fallbacks.push(ProfileFieldFallback::FrontmatterMissing);
    }

    let name = match first_non_empty(&document.fields, &["name", "title"]) {
        Some(name) => name,
        None => {
            fallbacks.push(ProfileFieldFallback::Name);
            if file_stem.trim().is_empty() {
                return Err(failed_record(
                    &source,
                    &file_stem,
                    ImportProfileErrorCode::MissingName,
                ));
            }
            file_stem.clone()
        }
    };

    let summary = match first_non_empty(&document.fields, &["summary", "description"]) {
        Some(summary) => summary,
        None => {
            fallbacks.push(ProfileFieldFallback::Summary);
            let from_body = summary_from_body(&document.body);
            if from_body.trim().is_empty() {
                name.clone()
            } else {
                from_body
            }
        }
    };

    let body = document.body;
    let mut final_dynamic = dynamic_template;
    if dynamic_template {
        let mut renders_ok = true;
        for context in profile_template_validation_contexts() {
            if render(&body, context).is_err() {
                renders_ok = false;
                break;
            }
        }
        if !renders_ok {
            fallbacks.push(ProfileFieldFallback::DynamicTemplateDowngraded);
            final_dynamic = false;
        }
    }

    let final_name = resolve_unique_name(&name, used_names);

    let entry = match create_profile(
        paths,
        ProfileInput {
            name: final_name.clone(),
            summary: summary.clone(),
            content: body.clone(),
            dynamic_template: final_dynamic,
        },
    ) {
        Ok(entry) => entry,
        Err(_) => {
            return Err(failed_record(
                &source,
                &file_stem,
                ImportProfileErrorCode::CreateFailed,
            ));
        }
    };
    used_names.insert(final_name.clone());

    let status = if fallbacks.is_empty() {
        ImportRecordStatus::Imported
    } else {
        ImportRecordStatus::ImportedWithFallbacks
    };
    Ok(ImportedProfileRecord {
        source_path: source,
        status,
        name: final_name,
        fallbacks,
        imported_id: Some(entry.id),
        error: None,
    })
}

fn collect_md_files(dir: &Utf8Path, cap: usize) -> Result<(Vec<Utf8PathBuf>, bool)> {
    let mut files = Vec::new();
    let entries = WalkDir::new(dir)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter();
    for entry in entries {
        let entry = entry.map_err(|_| ProfileCommandError::ImportFolderReadFailed)?;
        if !entry.file_type().is_file()
            || !entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        if files.len() == cap {
            return Ok((files, true));
        }
        let path = Utf8PathBuf::from_path_buf(entry.into_path())
            .map_err(|_| ProfileCommandError::ImportFolderReadFailed)?;
        files.push(path);
    }
    Ok((files, false))
}

fn content_has_frontmatter(content: &str) -> bool {
    let stripped = content.strip_prefix('\u{FEFF}').unwrap_or(content);
    stripped.starts_with("---\n") || stripped.starts_with("---\r\n")
}

fn resolve_unique_name(desired: &str, used: &BTreeSet<String>) -> String {
    let base = desired.trim().to_string();
    if !used.contains(&base) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}-{index}");
        if !used.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn first_non_empty(fields: &BTreeMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = fields.get(*key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn summary_from_body(body: &str) -> String {
    for block in body.split("\n\n") {
        let cleaned = clean_summary_block(block);
        if !cleaned.is_empty() {
            return truncate_text(&cleaned, 80);
        }
    }
    for line in body.lines() {
        let cleaned = clean_summary_line(line);
        if !cleaned.is_empty() {
            return truncate_text(&cleaned, 80);
        }
    }
    String::new()
}

fn clean_summary_block(block: &str) -> String {
    block
        .lines()
        .map(clean_summary_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn clean_summary_line(line: &str) -> String {
    line.trim_start_matches('#')
        .trim_start_matches('>')
        .trim()
        .to_string()
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let truncated: String = value.chars().take(limit).collect();
    format!("{truncated}…")
}

fn failed_record(source: &str, name: &str, code: ImportProfileErrorCode) -> ImportedProfileRecord {
    ImportedProfileRecord {
        source_path: source.to_string(),
        status: ImportRecordStatus::Failed,
        name: name.to_string(),
        fallbacks: Vec::new(),
        imported_id: None,
        error: Some(ImportProfileError { code }),
    }
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
        dynamic_template: input.dynamic_template,
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
            name: seed.name.value(language).to_string(),
            summary: seed.summary.value(language).to_string(),
            summary_source: seed.summary.value(language).to_string(),
            content: built_in_profile_content(seed.key, language).to_string(),
            dynamic_template: seed.dynamic_template,
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
            name: seed.name.value(language).to_string(),
            summary: seed.summary.value(language).to_string(),
            summary_source: seed.summary.value(language).to_string(),
            content: built_in_profile_content(seed.key, language).to_string(),
            dynamic_template: seed.dynamic_template,
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
        "dev-test" => prompt_by_language(language, PROFILE_DEV_TEST_ZH_CN, PROFILE_DEV_TEST_EN),
        "review" => prompt_by_language(language, PROFILE_REVIEW_ZH_CN, PROFILE_REVIEW_EN),
        "test" => prompt_by_language(language, PROFILE_TEST_ZH_CN, PROFILE_TEST_EN),
        "accept" => prompt_by_language(language, PROFILE_ACCEPT_ZH_CN, PROFILE_ACCEPT_EN),
        "cleanup" => prompt_by_language(language, PROFILE_CLEAN_ZH_CN, PROFILE_CLEAN_EN),
        "interview" => prompt_by_language(language, PROFILE_INTERVIEW_ZH_CN, PROFILE_INTERVIEW_EN),
        "grill" => prompt_by_language(language, PROFILE_GRILLME_ZH_CN, PROFILE_GRILLME_EN),
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
        let parsed = match parse_profile_file(&path) {
            Ok(parsed) => parsed,
            Err(error) => {
                warn!("skipping unreadable profile `{path}`: {:#}", error);
                continue;
            }
        };
        profiles.push(ProfileEntry {
            id: parsed.id,
            name: parsed.name,
            summary: parsed.summary,
            summary_source: parsed.summary_source,
            content: parsed.content,
            dynamic_template: parsed.dynamic_template,
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
        dynamic_template: fields
            .get("dynamicTemplate")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true")),
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

fn profile_frontmatter_updates(profile: &ProfileEntry) -> [FrontmatterUpdate<'_>; 6] {
    let dynamic_template = if profile.dynamic_template {
        "true"
    } else {
        "false"
    };
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
        FrontmatterUpdate {
            key: "dynamicTemplate",
            value: dynamic_template,
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
    if input.dynamic_template {
        for context in profile_template_validation_contexts() {
            render(&input.content, context).map_err(|error| {
                ProfileCommandError::InvalidDynamicTemplate {
                    reason: error.to_string(),
                }
            })?;
        }
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
        assert!(!profile.dynamic_template);
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
                dynamic_template: false,
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

    #[test]
    fn read_profile_dir_skips_corrupt_profile_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        fs::create_dir_all(paths.user_context_profiles_dir().as_std_path()).unwrap();

        let good = paths.user_context_profiles_dir().join("role-pf-good.md");
        fs::write(
            good.as_std_path(),
            "---\nid: pf-good\nname: role\nsummary: ok\ncreatedAt: 2026-07-09 10:00:00\nupdatedAt: 2026-07-09 10:00:00\n---\nbody\n",
        )
        .unwrap();

        let corrupt = paths.user_context_profiles_dir().join("broken-pf-bad.md");
        fs::write(corrupt.as_std_path(), "no front matter here\n").unwrap();

        let profiles = read_profile_dir(&paths, ProfileScope::User).unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "pf-good");
    }

    #[test]
    fn read_profile_dir_reads_bom_prefixed_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        fs::create_dir_all(paths.user_context_profiles_dir().as_std_path()).unwrap();

        let path = paths.user_context_profiles_dir().join("role-pf-bom.md");
        fs::write(
            path.as_std_path(),
            "\u{FEFF}---\nid: pf-bom\nname: role\nsummary: bom\ncreatedAt: 2026-07-09 10:00:00\nupdatedAt: 2026-07-09 10:00:00\n---\nbody\n",
        )
        .unwrap();

        let profiles = read_profile_dir(&paths, ProfileScope::User).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "pf-bom");
    }

    #[test]
    fn profile_dynamic_template_round_trips_through_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());

        let created = create_profile(
            &paths,
            ProfileInput {
                name: "dynamic role".to_string(),
                summary: "renders execution context".to_string(),
                content: "{% if execution.can_route_next %}route{% else %}wait{% endif %}"
                    .to_string(),
                dynamic_template: true,
            },
        )
        .unwrap();

        assert!(created.dynamic_template);
        let saved = fs::read_to_string(created.path).unwrap();
        assert!(saved.contains("dynamicTemplate: true"));
        let loaded = show_profile(&paths, &created.id, DesktopLanguage::ZhCn).unwrap();
        assert!(loaded.dynamic_template);
    }

    #[test]
    fn profile_dynamic_template_rejects_unknown_variables_when_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());

        let error = create_profile(
            &paths,
            ProfileInput {
                name: "broken role".to_string(),
                summary: "invalid template".to_string(),
                content: "{{ execution.unknown }}".to_string(),
                dynamic_template: true,
            },
        )
        .unwrap_err();

        let command_error = error.downcast_ref::<ProfileCommandError>().unwrap();
        assert_eq!(command_error.code(), "profile.dynamic-template-invalid");
        assert!(
            !command_error.params()["reason"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn disabled_profile_allows_literal_template_syntax() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());

        let created = create_profile(
            &paths,
            ProfileInput {
                name: "literal role".to_string(),
                summary: "keeps template text".to_string(),
                content: "{{ execution.unknown }}".to_string(),
                dynamic_template: false,
            },
        )
        .unwrap();

        assert_eq!(created.content, "{{ execution.unknown }}");
    }

    #[test]
    fn built_in_profiles_enable_dynamic_templates_only_when_needed() {
        let profiles = built_in_profiles(DesktopLanguage::ZhCn);
        let by_id = profiles
            .iter()
            .map(|profile| (profile.id.as_str(), profile.dynamic_template))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(by_id["pf-builtin-plan"], true);
        assert_eq!(by_id["pf-builtin-dev"], true);
        assert_eq!(by_id["pf-builtin-dev-test"], true);
        assert_eq!(by_id["pf-builtin-review"], false);
        assert_eq!(by_id["pf-builtin-test"], false);
        assert_eq!(by_id["pf-builtin-accept"], false);
        assert_eq!(by_id["pf-builtin-cleanup"], false);
        assert_eq!(by_id["pf-builtin-interview"], false);
    }

    #[test]
    fn built_in_profile_metadata_localizes_without_changing_profile_ids() {
        let zh_profiles = built_in_profiles(DesktopLanguage::ZhCn);
        let en_profiles = built_in_profiles(DesktopLanguage::En);
        let expected = [
            ("pf-builtin-plan", "方案", "Plan"),
            ("pf-builtin-dev", "开发", "Development"),
            ("pf-builtin-dev-test", "开发测试", "Development and Testing"),
            ("pf-builtin-review", "审查", "Review"),
            ("pf-builtin-test", "测试", "Testing"),
            ("pf-builtin-accept", "验收", "Acceptance"),
            ("pf-builtin-cleanup", "清理", "Cleanup"),
            ("pf-builtin-interview", "访谈", "Interview"),
            ("pf-builtin-grill", "拷问", "Grill"),
        ];

        for (id, zh_name, en_name) in expected {
            let zh = zh_profiles
                .iter()
                .find(|profile| profile.id == id)
                .expect("Chinese built-in profile should exist");
            let en = en_profiles
                .iter()
                .find(|profile| profile.id == id)
                .expect("English built-in profile should exist");

            assert_eq!(zh.id, en.id);
            assert_eq!(zh.name, zh_name);
            assert_eq!(en.name, en_name);
            assert_ne!(zh.summary, en.summary);
        }
    }

    #[test]
    fn enabled_built_in_profiles_render_in_all_supported_contexts() {
        for profile in built_in_profiles(DesktopLanguage::ZhCn)
            .into_iter()
            .chain(built_in_profiles(DesktopLanguage::En))
            .filter(|profile| profile.dynamic_template)
        {
            for context in profile_template_validation_contexts() {
                render(&profile.content, context).unwrap_or_else(|error| {
                    panic!("built-in profile {} failed to render: {error}", profile.id)
                });
            }
        }
    }

    fn setup_import_dir() -> (tempfile::TempDir, GoldBandPaths, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        fs::create_dir_all(paths.user_context_profiles_dir().as_std_path()).unwrap();
        let import_dir = tmp.path().join("import");
        fs::create_dir_all(&import_dir).unwrap();
        (tmp, paths, import_dir)
    }

    fn run_import(
        paths: &GoldBandPaths,
        import_dir: &std::path::Path,
        dynamic: bool,
    ) -> ImportProfilesResult {
        import_profiles_from_folder(
            paths,
            ImportProfilesInput {
                folder_path: import_dir.to_string_lossy().to_string(),
                dynamic_template: dynamic,
            },
        )
        .unwrap()
    }

    #[test]
    fn import_profile_complete_format() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("role-a.md"),
            "---\nname: 完整角色\nsummary: 完整摘要\n---\n正文内容\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.imported.len(), 1);
        assert!(result.failed.is_empty());
        let record = &result.imported[0];
        assert_eq!(record.name, "完整角色");
        assert_eq!(record.status, ImportRecordStatus::Imported);
        assert!(record.fallbacks.is_empty());
        assert!(record.imported_id.is_some());
    }

    #[test]
    fn import_profile_missing_frontmatter_falls_back() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("plain-role.md"),
            "# 普通角色\n\n这是正文第一段，用于兜底。\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let record = &result.imported[0];
        assert_eq!(record.status, ImportRecordStatus::ImportedWithFallbacks);
        assert!(
            record
                .fallbacks
                .contains(&ProfileFieldFallback::FrontmatterMissing)
        );
        assert!(record.fallbacks.contains(&ProfileFieldFallback::Name));
        assert!(record.fallbacks.contains(&ProfileFieldFallback::Summary));
        assert_eq!(record.name, "plain-role");
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert_eq!(entry.summary, "普通角色");
    }

    #[test]
    fn import_profile_missing_name_uses_filename() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("no-name.md"),
            "---\nsummary: 有摘要但无名字\n---\n正文\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let record = &result.imported[0];
        assert_eq!(record.name, "no-name");
        assert!(record.fallbacks.contains(&ProfileFieldFallback::Name));
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert_eq!(entry.summary, "有摘要但无名字");
    }

    #[test]
    fn import_profile_missing_summary_uses_body() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("no-summary.md"),
            "---\nname: 有名字\n---\n正文首段内容\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let record = &result.imported[0];
        assert!(record.fallbacks.contains(&ProfileFieldFallback::Summary));
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert_eq!(entry.summary, "正文首段内容");
    }

    #[test]
    fn import_profile_compatible_field_names() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("ext-role.md"),
            "---\ntitle: 外部角色\ndescription: 外部描述\n---\n正文\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let record = &result.imported[0];
        assert_eq!(record.name, "外部角色");
        assert_eq!(record.status, ImportRecordStatus::Imported);
        assert!(record.fallbacks.is_empty());
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert_eq!(entry.summary, "外部描述");
    }

    #[test]
    fn import_profile_renames_on_conflict() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        create_profile(
            &paths,
            ProfileInput {
                name: "方案".to_string(),
                summary: "预置".to_string(),
                content: "x".to_string(),
                dynamic_template: false,
            },
        )
        .unwrap();
        fs::write(
            import_dir.join("a.md"),
            "---\nname: 方案\nsummary: 导入1\n---\n正文1\n",
        )
        .unwrap();
        fs::write(
            import_dir.join("b.md"),
            "---\nname: 方案\nsummary: 导入2\n---\n正文2\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let names: Vec<&str> = result.imported.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"方案-2"));
        assert!(names.contains(&"方案-3"));
        assert!(!names.contains(&"方案"));
    }

    #[test]
    fn import_profile_empty_file_fails() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(import_dir.join("empty.md"), "   \n  ").unwrap();
        let result = run_import(&paths, &import_dir, false);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(
            result.failed[0].error.map(|error| error.code),
            Some(ImportProfileErrorCode::EmptyFile)
        );
    }

    #[test]
    fn import_profile_invalid_frontmatter_returns_typed_error() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("invalid.md"),
            "---\nname: [unterminated\n---\n正文\n",
        )
        .unwrap();

        let result = run_import(&paths, &import_dir, false);

        assert_eq!(result.failed.len(), 1);
        assert_eq!(
            result.failed[0].error.map(|error| error.code),
            Some(ImportProfileErrorCode::InvalidFrontmatter)
        );
    }

    #[test]
    fn collect_md_files_is_deterministic_and_honors_the_cap() {
        let (_tmp, _paths, import_dir) = setup_import_dir();
        fs::write(import_dir.join("c.md"), "c").unwrap();
        fs::write(import_dir.join("a.MD"), "a").unwrap();
        fs::write(import_dir.join("b.md"), "b").unwrap();
        fs::write(import_dir.join("ignored.txt"), "ignored").unwrap();
        let import_dir = Utf8PathBuf::from_path_buf(import_dir).unwrap();

        let (files, truncated) = collect_md_files(&import_dir, 2).unwrap();
        let names = files
            .iter()
            .filter_map(|path| path.file_name())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["a.MD", "b.md"]);
        assert!(truncated);
    }

    #[cfg(unix)]
    #[test]
    fn collect_md_files_does_not_follow_directory_links() {
        use std::os::unix::fs::symlink;

        let (tmp, _paths, import_dir) = setup_import_dir();
        let linked_dir = tmp.path().join("linked");
        fs::create_dir_all(&linked_dir).unwrap();
        fs::write(linked_dir.join("linked.md"), "linked").unwrap();
        fs::write(import_dir.join("local.md"), "local").unwrap();
        symlink(&linked_dir, import_dir.join("linked-dir")).unwrap();
        let import_dir = Utf8PathBuf::from_path_buf(import_dir).unwrap();

        let (files, truncated) = collect_md_files(&import_dir, IMPORT_PROFILE_FILE_CAP).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name(), Some("local.md"));
        assert!(!truncated);
    }

    #[test]
    fn import_profile_dynamic_off_ignores_template_syntax() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("tpl.md"),
            "---\nname: 模板角色\nsummary: 摘要\n---\n{% if execution.unknown %}A{% endif %}\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, false);
        let record = &result.imported[0];
        assert_eq!(record.status, ImportRecordStatus::Imported);
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert!(!entry.dynamic_template);
    }

    #[test]
    fn import_profile_dynamic_on_valid_template() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("tpl.md"),
            "---\nname: 模板角色\nsummary: 摘要\n---\n{% if execution.can_route_next %}A{% else %}B{% endif %}\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, true);
        let record = &result.imported[0];
        assert_eq!(record.status, ImportRecordStatus::Imported);
        assert!(record.fallbacks.is_empty());
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert!(entry.dynamic_template);
    }

    #[test]
    fn import_profile_dynamic_on_invalid_downgrades() {
        let (_tmp, paths, import_dir) = setup_import_dir();
        fs::write(
            import_dir.join("tpl.md"),
            "---\nname: 模板角色\nsummary: 摘要\n---\n{% if execution.unknown %}A{% endif %}\n",
        )
        .unwrap();
        let result = run_import(&paths, &import_dir, true);
        let record = &result.imported[0];
        assert!(
            record
                .fallbacks
                .contains(&ProfileFieldFallback::DynamicTemplateDowngraded)
        );
        let entry = show_profile(
            &paths,
            record.imported_id.as_ref().unwrap(),
            DesktopLanguage::ZhCn,
        )
        .unwrap();
        assert!(!entry.dynamic_template);
    }

    #[test]
    fn import_profile_folder_not_found_maps_to_error_code() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        let error = import_profiles_from_folder(
            &paths,
            ImportProfilesInput {
                folder_path: "/no/such/gold-band-dir".to_string(),
                dynamic_template: false,
            },
        )
        .unwrap_err();
        let command_error = error.downcast_ref::<ProfileCommandError>().unwrap();
        assert_eq!(command_error.code(), "profile.import.folder-not-found");
    }

    #[test]
    fn grill_profile_is_built_in_but_not_in_default_workflow() {
        // The grill profile must appear in the built-in profile list.
        let built_in = built_in_profiles(DesktopLanguage::ZhCn);
        let grill = built_in.iter().find(|p| p.id == "pf-builtin-grill");
        assert!(grill.is_some(), "grill profile should be built-in");
        let grill = grill.unwrap();
        assert!(grill.is_built_in);
        assert_eq!(grill.scope, ProfileScope::BuiltIn);
        assert!(
            !grill.content.is_empty(),
            "grill profile content must not be empty"
        );

        // The grill profile id must NOT be resolvable via the default workflow
        // profile-id map. The default workflow only references: interview, plan,
        // dev, review, test, accept, cleanup.
        let ids = ensure_default_user_profiles(&GoldBandPaths::new(
            Utf8PathBuf::from_path_buf(std::env::temp_dir().join("gb-grill-test")).unwrap(),
        ))
        .unwrap();
        // The map still contains the key (it lists all built-in ids), but the
        // default workflow never references "grill".
        assert_eq!(ids.get("grill"), Some("pf-builtin-grill"));
        // Verify the default workflow does not embed the grill profile id.
        let dsl = crate::app::default_workflow_dsl("claude-acp", &ids, DesktopLanguage::ZhCn);
        let serialized = serde_json::to_string(&dsl).unwrap();
        assert!(
            !serialized.contains("pf-builtin-grill"),
            "default workflow must not embed the grill profile"
        );

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("gb-grill-test"));
    }
}
