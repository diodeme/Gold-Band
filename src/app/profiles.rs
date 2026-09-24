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

use crate::channel::{
    ProfileChannelCapability, RELEASE_CHANNEL, profile_channel_capability_enabled,
};
use crate::config::DesktopLanguage;
use crate::frontmatter::{
    FrontmatterUpdate, parse_frontmatter_document, parse_optional_frontmatter_document,
    render_frontmatter_document, update_frontmatter_document,
};
use crate::prompts::{
    LocalizedText, PROFILE_ACCEPT, PROFILE_CICD, PROFILE_CLEAN, PROFILE_DEV, PROFILE_DEV_TEST,
    PROFILE_GRILLME, PROFILE_INTERVIEW, PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT,
    PROFILE_OVERLAY_REQUIREMENT_IDENTITY, PROFILE_PLAN, PROFILE_REVIEW, PROFILE_TEST,
    profile_template_validation_contexts, prompt_by_language, render,
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
struct LocalizedProfileText(LocalizedText);

impl LocalizedProfileText {
    const fn all(
        zh_cn: &'static str,
        zh_tw: &'static str,
        en: &'static str,
        ja_jp: &'static str,
        ko_kr: &'static str,
        pt_br: &'static str,
        es: &'static str,
    ) -> Self {
        Self(LocalizedText::all(
            zh_cn, zh_tw, en, ja_jp, ko_kr, pt_br, es,
        ))
    }

    const fn zh_en(zh_cn: &'static str, en: &'static str) -> Self {
        Self(LocalizedText::zh_en(zh_cn, en))
    }

    const fn from_text(text: LocalizedText) -> Self {
        Self(text)
    }

    fn value(self, language: DesktopLanguage) -> &'static str {
        self.0.resolve(language)
    }
}

#[derive(Debug, Clone, Copy)]
struct DefaultProfileSeed {
    key: &'static str,
    id: &'static str,
    required_capability: Option<ProfileChannelCapability>,
    name: LocalizedProfileText,
    summary: LocalizedProfileText,
    dynamic_template: bool,
}

#[derive(Debug, Clone, Copy)]
struct ProfileOverlay {
    target_profile_keys: &'static [&'static str],
    required_capability: ProfileChannelCapability,
    content: LocalizedProfileText,
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
        required_capability: None,
        id: "pf-builtin-plan",
        name: LocalizedProfileText::all("方案", "方案", "Plan", "計画", "계획", "Plano", "Plan"),
        summary: LocalizedProfileText::all(
            "方案角色，用于需求分析和实施方案设计。",
            "方案角色，用於需求分析和實施方案設計。",
            "Planning role for analyzing requirements and designing implementation plans.",
            "要件を分析し、実装計画を設計する Plan ロールです。",
            "요구사항을 분석하고 구현 계획을 설계하는 Plan 역할입니다.",
            "Papel de Plan para analisar requisitos e desenhar planos de implementação.",
            "Rol de Plan para analizar requisitos y diseñar planes de implementación.",
        ),
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "dev",
        required_capability: None,
        id: "pf-builtin-dev",
        name: LocalizedProfileText::all(
            "开发",
            "開發",
            "Development",
            "開発",
            "개발",
            "Desenvolvimento",
            "Desarrollo",
        ),
        summary: LocalizedProfileText::all(
            "开发角色，用于实现需求并维护代码质量。",
            "開發角色，用於實現需求並維護程式品質。",
            "Development role for implementing requirements and maintaining code quality.",
            "要件を実装し、コード品質を維持する Development ロールです。",
            "요구사항을 구현하고 코드 품질을 유지하는 Development 역할입니다.",
            "Papel de Development para implementar requisitos e manter a qualidade do código.",
            "Rol de Development para implementar requisitos y mantener la calidad del código.",
        ),
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "dev-test",
        required_capability: None,
        id: "pf-builtin-dev-test",
        name: LocalizedProfileText::all(
            "开发测试",
            "開發測試",
            "Development and Testing",
            "開発とテスト",
            "개발 및 테스트",
            "Desenvolvimento e testes",
            "Desarrollo y pruebas",
        ),
        summary: LocalizedProfileText::all(
            "开发测试角色，用于在同一节点完成需求实现、自动化测试与必要回归。",
            "開發測試角色，用於在同一節點完成需求實現、自動化測試與必要回歸。",
            "Development and testing role for implementing requirements and running automated verification in one node.",
            "同一ノードで要件の実装、自動テスト、必要な回帰を行う Development and Testing ロールです。",
            "같은 노드에서 요구사항 구현, 자동화 테스트, 필요한 회귀를 수행하는 Development and Testing 역할입니다.",
            "Papel de Development and Testing para implementar requisitos e executar verificação automatizada no mesmo nó.",
            "Rol de Development and Testing para implementar requisitos y ejecutar la verificación automatizada en el mismo nodo.",
        ),
        dynamic_template: true,
    },
    DefaultProfileSeed {
        key: "review",
        required_capability: None,
        id: "pf-builtin-review",
        name: LocalizedProfileText::all(
            "审查",
            "審查",
            "Review",
            "レビュー",
            "검토",
            "Revisão",
            "Revisión",
        ),
        summary: LocalizedProfileText::all(
            "审查角色，用于检查实现质量、风险和一致性。",
            "審查角色，用於檢查實作品質、風險和一致性。",
            "Review role for checking implementation quality, risks, and consistency.",
            "実装品質、リスク、一貫性を確認する Review ロールです。",
            "구현 품질, 위험, 일관성을 확인하는 Review 역할입니다.",
            "Papel de Review para verificar a qualidade, os riscos e a consistência da implementação.",
            "Rol de Review para comprobar la calidad, los riesgos y la consistencia de la implementación.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "test",
        required_capability: None,
        id: "pf-builtin-test",
        name: LocalizedProfileText::all(
            "测试",
            "測試",
            "Testing",
            "テスト",
            "테스트",
            "Testes",
            "Pruebas",
        ),
        summary: LocalizedProfileText::all(
            "测试角色，用于执行验证并反馈质量结果。",
            "測試角色，用於執行驗證並回報品質結果。",
            "Testing role for running verification and reporting quality results.",
            "検証を実行し、品質結果を返す Testing ロールです。",
            "검증을 실행하고 품질 결과를 보고하는 Testing 역할입니다.",
            "Papel de Testing para executar a verificação e reportar os resultados de qualidade.",
            "Rol de Testing para ejecutar la verificación e informar los resultados de calidad.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "accept",
        required_capability: None,
        id: "pf-builtin-accept",
        name: LocalizedProfileText::all(
            "验收",
            "驗收",
            "Acceptance",
            "受け入れ",
            "인수",
            "Aceite",
            "Aceptación",
        ),
        summary: LocalizedProfileText::all(
            "验收角色，用于对照需求判断交付是否满足目标。",
            "驗收角色，用於對照需求判斷交付是否滿足目標。",
            "Acceptance role for determining whether the delivery meets the requirements.",
            "要件と照合して、成果物が目標を満たすか判断する Acceptance ロールです。",
            "요구사항과 대조해 전달물이 목표를 충족하는지 판단하는 Acceptance 역할입니다.",
            "Papel de Acceptance para decidir se a entrega atende aos requisitos.",
            "Rol de Acceptance para decidir si la entrega cumple los requisitos.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "cicd",
        required_capability: Some(ProfileChannelCapability::Cicd),
        id: "pf-builtin-cicd",
        name: LocalizedProfileText::zh_en("CI/CD", "CI/CD"),
        summary: LocalizedProfileText::zh_en(
            "CI/CD 角色，使用 WeTest 完成构建，并与用户交互确认按构建或包名部署。",
            "CI/CD role for WeTest builds and interactive deployment from a build or by package name.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "cleanup",
        required_capability: None,
        id: "pf-builtin-cleanup",
        name: LocalizedProfileText::all(
            "清理",
            "清理",
            "Cleanup",
            "クリーンアップ",
            "정리",
            "Limpeza",
            "Limpieza",
        ),
        summary: LocalizedProfileText::all(
            "清理角色，用于验收成功后的资源释放、收尾和环境清理。",
            "清理角色，用於驗收成功後的資源釋放、收尾和環境清理。",
            "Cleanup role for releasing resources, finalizing handoff notes, and cleaning up the environment after acceptance.",
            "Acceptance 成功後のリソース解放、引き継ぎ、環境のクリーンアップを行う Cleanup ロールです。",
            "Acceptance 성공 후 리소스 해제, 인수인계, 환경 정리를 수행하는 Cleanup 역할입니다.",
            "Papel de Cleanup para liberar recursos, finalizar as notas de handoff e limpar o ambiente após o Acceptance.",
            "Rol de Cleanup para liberar recursos, cerrar las notas de handoff y limpiar el entorno tras el Acceptance.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "interview",
        required_capability: None,
        id: "pf-builtin-interview",
        name: LocalizedProfileText::all(
            "访谈",
            "訪談",
            "Interview",
            "インタビュー",
            "인터뷰",
            "Entrevista",
            "Entrevista",
        ),
        summary: LocalizedProfileText::all(
            "访谈角色，用于需求澄清，通过深度访谈把模糊需求转化为清晰规格。",
            "訪談角色，用於需求釐清，透過深度訪談把模糊需求轉化為清晰規格。",
            "Interview role for clarifying requirements and turning ambiguity into clear specifications through deep interviews.",
            "要件を明確にし、深いインタビューで曖昧な要件を明確な仕様へ変える Interview ロールです。",
            "요구사항을 명확히 하고, 심층 인터뷰로 모호한 요구를 명확한 스펙으로 바꾸는 Interview 역할입니다.",
            "Papel de Interview para esclarecer requisitos e transformar ambiguidades em especificações claras.",
            "Rol de Interview para aclarar requisitos y convertir la ambigüedad en especificaciones claras.",
        ),
        dynamic_template: false,
    },
    DefaultProfileSeed {
        key: "grill",
        required_capability: None,
        id: "pf-builtin-grill",
        name: LocalizedProfileText::all(
            "拷问",
            "詰問",
            "Grill",
            "追及",
            "심층 질의",
            "Questionamento",
            "Interrogatorio",
        ),
        summary: LocalizedProfileText::all(
            "拷问角色，围绕计划或决策进行毫不留情的深度访谈，直到达成共同理解。",
            "詰問角色，圍繞計畫或決策進行毫不留情的深度訪談，直到達成共同理解。",
            "Grill role for rigorously challenging plans or decisions through deep interviews until shared understanding is reached.",
            "計画や意思決定を深いインタビューで厳しく問い、共通理解に至るまで詰める Grill ロールです。",
            "계획이나 의사결정을 심층 인터뷰로 엄격히 따져 공통 이해에 도달하는 Grill 역할입니다.",
            "Papel de Grill para questionar planos ou decisões em entrevistas profundas até haver entendimento comum.",
            "Rol de Grill para cuestionar planes o decisiones en entrevistas profundas hasta alcanzar un entendimiento común.",
        ),
        dynamic_template: false,
    },
];

const PROFILE_OVERLAYS: &[ProfileOverlay] = &[
    ProfileOverlay {
        target_profile_keys: &["interview", "grill"],
        required_capability: ProfileChannelCapability::RequirementIdentity,
        content: LocalizedProfileText::from_text(PROFILE_OVERLAY_REQUIREMENT_IDENTITY),
    },
    ProfileOverlay {
        target_profile_keys: &["dev-test"],
        required_capability: ProfileChannelCapability::DevTestAutoCommit,
        content: LocalizedProfileText::from_text(PROFILE_OVERLAY_DEV_TEST_AUTO_COMMIT),
    },
];

fn default_profile_seeds_for_channel(
    channel: &str,
) -> impl Iterator<Item = &'static DefaultProfileSeed> + '_ {
    DEFAULT_PROFILE_SEEDS.iter().filter(move |seed| {
        seed.required_capability
            .is_none_or(|capability| profile_channel_capability_enabled(channel, capability))
    })
}

fn available_default_profile_seeds() -> impl Iterator<Item = &'static DefaultProfileSeed> {
    default_profile_seeds_for_channel(RELEASE_CHANNEL)
}

pub(crate) fn ensure_default_user_profiles(_paths: &GoldBandPaths) -> Result<DefaultProfileIds> {
    let by_key = available_default_profile_seeds()
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
    available_default_profile_seeds()
        .map(|seed| ProfileEntry {
            id: seed.id.to_string(),
            name: seed.name.value(language).to_string(),
            summary: seed.summary.value(language).to_string(),
            summary_source: seed.summary.value(language).to_string(),
            content: built_in_profile_content(seed.key, language),
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
    available_default_profile_seeds()
        .find(|seed| seed.id == id)
        .map(|seed| ProfileEntry {
            id: seed.id.to_string(),
            name: seed.name.value(language).to_string(),
            summary: seed.summary.value(language).to_string(),
            summary_source: seed.summary.value(language).to_string(),
            content: built_in_profile_content(seed.key, language),
            dynamic_template: seed.dynamic_template,
            scope: ProfileScope::BuiltIn,
            is_built_in: true,
            created_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            updated_at: BUILT_IN_PROFILE_TIMESTAMP.to_string(),
            path: format!("builtin://profiles/{}", seed.key),
        })
}

fn built_in_profile_content(key: &str, language: DesktopLanguage) -> String {
    let content = match key {
        "plan" => prompt_by_language(language, PROFILE_PLAN),
        "dev" => prompt_by_language(language, PROFILE_DEV),
        "dev-test" => prompt_by_language(language, PROFILE_DEV_TEST),
        "review" => prompt_by_language(language, PROFILE_REVIEW),
        "test" => prompt_by_language(language, PROFILE_TEST),
        "cicd" => prompt_by_language(language, PROFILE_CICD),
        "accept" => prompt_by_language(language, PROFILE_ACCEPT),
        "cleanup" => prompt_by_language(language, PROFILE_CLEAN),
        "interview" => prompt_by_language(language, PROFILE_INTERVIEW),
        "grill" => prompt_by_language(language, PROFILE_GRILLME),
        _ => "",
    };
    let mut composed = content.to_string();
    for overlay in PROFILE_OVERLAYS.iter().filter(|overlay| {
        overlay.target_profile_keys.contains(&key)
            && profile_channel_capability_enabled(RELEASE_CHANNEL, overlay.required_capability)
    }) {
        composed.push_str("\n\n");
        composed.push_str(overlay.content.value(language).trim());
    }
    composed
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
    use crate::prompts::{PROFILE_CICD_EN, PROFILE_CICD_ZH_CN, PROFILE_PLAN_EN};
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
        let cicd_available =
            profile_channel_capability_enabled(RELEASE_CHANNEL, ProfileChannelCapability::Cicd);
        assert_eq!(
            by_id.get("pf-builtin-cicd"),
            cicd_available.then_some(&false)
        );
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

        let ja_plan = built_in_profiles(DesktopLanguage::JaJp)
            .into_iter()
            .find(|profile| profile.id == "pf-builtin-plan")
            .expect("Japanese plan role");
        assert_eq!(ja_plan.id, "pf-builtin-plan");
        assert_eq!(ja_plan.name, "計画");
        assert_ne!(ja_plan.content, PROFILE_PLAN_EN);
    }

    #[test]
    fn built_in_profiles_reject_update_and_delete_in_every_channel() {
        // 渠道目录只决定哪些内置角色可见；在当前渠道可见的内置角色都必须只读。
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap());
        let built_ins = built_in_profiles(DesktopLanguage::ZhCn);
        assert!(!built_ins.is_empty());
        for profile in built_ins {
            let input = ProfileInput {
                name: "Edited role".to_string(),
                summary: "Edited role".to_string(),
                content: "Edited content".to_string(),
                dynamic_template: false,
            };
            for error in [
                update_profile(&paths, &profile.id, input).unwrap_err(),
                delete_profile(&paths, &profile.id).unwrap_err(),
            ] {
                assert!(
                    matches!(
                        error.downcast_ref::<ProfileCommandError>(),
                        Some(ProfileCommandError::ReadonlyBuiltIn)
                    ),
                    "built-in profile `{}` must reject writes",
                    profile.id
                );
            }
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

    #[test]
    fn channel_profile_catalog_preserves_shared_roles_and_restricts_cicd() {
        for channel in ["default", "wb", "enterprise", ""] {
            let profiles = default_profile_seeds_for_channel(channel).collect::<Vec<_>>();
            assert_eq!(profiles.len(), if channel == "wb" { 10 } else { 9 });
            assert_eq!(
                profiles.iter().any(|seed| seed.id == "pf-builtin-cicd"),
                channel == "wb"
            );
            for seed in DEFAULT_PROFILE_SEEDS
                .iter()
                .filter(|seed| seed.required_capability.is_none())
            {
                assert!(profiles.iter().any(|available| available.id == seed.id));
            }
        }
    }

    #[test]
    fn cicd_profile_availability_matches_build_channel() {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap());
        let available =
            profile_channel_capability_enabled(RELEASE_CHANNEL, ProfileChannelCapability::Cicd);
        for language in [DesktopLanguage::ZhCn, DesktopLanguage::En] {
            let list = list_profiles(&paths, language).unwrap();
            assert_eq!(
                list.profiles
                    .iter()
                    .any(|profile| profile.id == "pf-builtin-cicd"),
                available,
                "CI/CD visibility must match the profile channel capability"
            );
            assert_eq!(
                find_profile_by_id(&paths, "pf-builtin-cicd", language)
                    .unwrap()
                    .is_some(),
                available
            );
            assert_eq!(
                show_profile(&paths, "pf-builtin-cicd", language).is_ok(),
                available
            );
            assert!(show_profile(&paths, "pf-builtin-dev", language).is_ok());
            let resolved = crate::app::profile_resolver::resolve_profile(
                &paths,
                "cicd-node",
                "pf-builtin-cicd",
                language,
            );
            assert_eq!(resolved.is_ok(), available);
        }
        let ids = ensure_default_user_profiles(&paths).unwrap();
        assert_eq!(ids.get("cicd"), available.then_some("pf-builtin-cicd"));
        assert_eq!(ids.get("dev"), Some("pf-builtin-dev"));
    }

    #[test]
    fn cicd_profile_interfaces_return_localized_builtin_content_without_enabling_default_execution()
    {
        let tmp = tempfile::tempdir().unwrap();
        let paths =
            GoldBandPaths::new(Utf8PathBuf::from_path_buf(tmp.path().join("repo")).unwrap());
        let id = "pf-builtin-cicd";
        if !profile_channel_capability_enabled(RELEASE_CHANNEL, ProfileChannelCapability::Cicd) {
            assert!(show_profile(&paths, id, DesktopLanguage::ZhCn).is_err());
            assert!(show_profile(&paths, id, DesktopLanguage::En).is_err());
            return;
        }
        for (language, content) in [
            (DesktopLanguage::ZhCn, PROFILE_CICD_ZH_CN),
            (DesktopLanguage::En, PROFILE_CICD_EN),
        ] {
            let list = list_profiles(&paths, language).unwrap();
            let matches = list
                .profiles
                .iter()
                .filter(|profile| profile.id == id)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1);
            let shown = show_profile(&paths, id, language).unwrap();
            assert_eq!(shown.content, content);
            assert_eq!(matches[0].content, shown.content);
            assert_eq!(shown.name, "CI/CD");
            assert_eq!(shown.scope, ProfileScope::BuiltIn);
            assert!(shown.is_built_in);
            assert!(!shown.dynamic_template);
            assert_eq!(shown.path, "builtin://profiles/cicd");
        }
        assert_ne!(
            show_profile(&paths, id, DesktopLanguage::ZhCn)
                .unwrap()
                .summary,
            show_profile(&paths, id, DesktopLanguage::En)
                .unwrap()
                .summary
        );

        let input = ProfileInput {
            name: "CI/CD".to_string(),
            summary: "Edited role".to_string(),
            content: "Edited content".to_string(),
            dynamic_template: false,
        };
        for error in [
            update_profile(&paths, id, input).unwrap_err(),
            delete_profile(&paths, id).unwrap_err(),
        ] {
            assert!(matches!(
                error.downcast_ref::<ProfileCommandError>(),
                Some(ProfileCommandError::ReadonlyBuiltIn)
            ));
        }
        let ids = ensure_default_user_profiles(&paths).unwrap();
        assert_eq!(ids.get("cicd"), Some(id));
        let workflow = crate::app::default_workflow_dsl("claude-acp", &ids, DesktopLanguage::ZhCn);
        assert!(!serde_json::to_string(&workflow).unwrap().contains(id));
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

    #[test]
    fn cicd_profile_requires_interactive_build_and_deploy_without_automatic_extras() {
        for (content, clauses) in [
            (
                PROFILE_CICD_ZH_CN,
                [
                    "默认任务是构建 + 部署",
                    "默认推荐按构建部署",
                    "两种部署方式都必须与用户交互确认",
                    "每次 run 都必须重新确认构建和部署参数",
                    "不能复用上一次 run 的确认",
                    "未选择的附加操作不执行，也不影响构建部署任务完成",
                    "工作空间与任务记忆统一使用字符串 `key/value/desc` 条目",
                    "`wetest <cmd> --help` 动态发现",
                    "查询自由、触发类确认",
                    "不通过试运行触发类命令来探测参数",
                ],
            ),
            (
                PROFILE_CICD_EN,
                [
                    "The default task is build + deploy",
                    "Recommend deployment from a build by default",
                    "Both deployment modes require interactive confirmation with the user",
                    "Every run must freshly confirm its build and deployment parameters",
                    "A confirmation from a previous run cannot be reused",
                    "Unselected optional operations are not executed and do not block completion of build and deployment",
                    "Workspace and task memory share string `key/value/desc` entries",
                    "dynamically discover it with `wetest <cmd> --help`",
                    "queries are free; triggers require confirmation",
                    "never probe parameters by trial-running a trigger command",
                ],
            ),
        ] {
            for clause in clauses {
                assert!(content.contains(clause), "missing CI/CD contract: {clause}");
            }
        }
    }

    #[test]
    fn cicd_profiles_share_task_build_and_subsystem_deployment_keys() {
        let keys = |content: &str| {
            content
                .lines()
                .filter(|line| line.starts_with("| `cicd."))
                .map(|line| line.split('|').nth(1).unwrap().trim().to_owned())
                .collect::<Vec<_>>()
        };
        let zh = keys(PROFILE_CICD_ZH_CN);
        assert_eq!(zh.len(), 14);
        assert_eq!(zh, keys(PROFILE_CICD_EN));
        assert_eq!(
            zh,
            [
                "`cicd.build.jobId`",
                "`cicd.build.branch`",
                "`cicd.build.appList`",
                "`cicd.build.appCoverage`",
                "`cicd.deploy.<S>.selected`",
                "`cicd.deploy.<S>.mode`",
                "`cicd.deploy.<S>.templateId`",
                "`cicd.deploy.<S>.templateName`",
                "`cicd.deploy.<S>.deployType`",
                "`cicd.deploy.<S>.env`",
                "`cicd.deploy.<S>.ips`",
                "`cicd.deploy.<S>.containers`",
                "`cicd.deploy.<S>.pkgNames`",
                "`cicd.deploy.<S>.inputParams`",
            ]
        );
        for content in [PROFILE_CICD_ZH_CN, PROFILE_CICD_EN] {
            assert!(!content.contains("Current-task `memory.json`"));
            assert!(!content.contains("当前 task 的 `memory.json`"));
            assert!(!content.contains("\"targets\""));
        }
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
