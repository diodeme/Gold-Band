use anyhow::{Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::{GoldBandPaths, ensure_parent_dir};

static PROFILE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileScope {
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
    name: &'static str,
    summary: &'static str,
}

const DEFAULT_PROFILE_SEEDS: &[DefaultProfileSeed] = &[
    DefaultProfileSeed {
        key: "plan",
        name: "方案",
        summary: "方案角色，用于需求分析和实施方案设计。",
    },
    DefaultProfileSeed {
        key: "dev",
        name: "开发",
        summary: "开发角色，用于实现需求并维护代码质量。",
    },
    DefaultProfileSeed {
        key: "review",
        name: "审查",
        summary: "审查角色，用于检查实现质量、风险和一致性。",
    },
    DefaultProfileSeed {
        key: "test",
        name: "测试",
        summary: "测试角色，用于执行验证并反馈质量结果。",
    },
    DefaultProfileSeed {
        key: "accept",
        name: "验收",
        summary: "验收角色，用于对照需求判断交付是否满足目标。",
    },
    DefaultProfileSeed {
        key: "cleanup",
        name: "清理",
        summary: "清理角色，用于验收成功后的资源释放、收尾和环境清理。",
    },
];

pub(crate) fn ensure_default_user_profiles(paths: &GoldBandPaths) -> Result<DefaultProfileIds> {
    fs::create_dir_all(paths.user_context_profiles_dir().as_std_path())?;
    let mut user_profiles = read_profile_dir(paths, ProfileScope::User)?;
    let project_profiles = read_profile_dir(paths, ProfileScope::Project)?;
    let mut by_key = BTreeMap::new();
    for seed in DEFAULT_PROFILE_SEEDS {
        if let Some(project_profile) = project_profiles
            .iter()
            .find(|profile| profile.name == seed.name)
        {
            remove_seeded_user_defaults_with_name(&user_profiles, seed.name, seed.summary)?;
            by_key.insert(seed.key.to_string(), project_profile.id.clone());
            continue;
        }
        if let Some(existing) = user_profiles
            .iter()
            .find(|profile| profile.name == seed.name)
        {
            if legacy_default_profile_id(&existing.id) {
                let path = Utf8PathBuf::from(&existing.path);
                if path.exists() {
                    fs::remove_file(path.as_std_path())?;
                }
            } else {
                by_key.insert(seed.key.to_string(), existing.id.clone());
                continue;
            }
        }
        let now = local_timestamp();
        let entry = ProfileEntry {
            id: next_profile_id(paths)?,
            name: seed.name.to_string(),
            summary: seed.summary.to_string(),
            content: String::new(),
            scope: ProfileScope::User,
            created_at: now.clone(),
            updated_at: now,
            path: String::new(),
        };
        write_profile(paths, &entry)?;
        by_key.insert(seed.key.to_string(), entry.id.clone());
        user_profiles.push(ProfileEntry {
            path: profile_path(paths, entry.scope, &entry.name, &entry.id).to_string(),
            ..entry
        });
    }
    Ok(DefaultProfileIds { by_key })
}

pub(crate) fn list_profiles(paths: &GoldBandPaths) -> Result<ProfileList> {
    ensure_default_user_profiles(paths)?;
    let mut profiles = Vec::new();
    profiles.extend(read_profile_dir(paths, ProfileScope::Project)?);
    profiles.extend(read_profile_dir(paths, ProfileScope::User)?);
    profiles.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| scope_rank(left.scope).cmp(&scope_rank(right.scope)))
    });
    Ok(ProfileList { profiles })
}

pub(crate) fn show_profile(paths: &GoldBandPaths, id: &str) -> Result<ProfileEntry> {
    ensure_default_user_profiles(paths)?;
    find_profile_by_id(paths, id)?.ok_or_else(|| anyhow!("profile `{id}` not found"))
}

pub(crate) fn create_profile(paths: &GoldBandPaths, input: ProfileInput) -> Result<ProfileEntry> {
    ensure_profile_input(&input)?;
    ensure_default_user_profiles(paths)?;
    let now = local_timestamp();
    let mut entry = ProfileEntry {
        id: next_profile_id(paths)?,
        name: input.name.trim().to_string(),
        summary: input.summary.trim().to_string(),
        content: input.content,
        scope: input.scope,
        created_at: now.clone(),
        updated_at: now,
        path: String::new(),
    };
    entry.path = profile_path(paths, entry.scope, &entry.name, &entry.id).to_string();
    write_profile(paths, &entry)?;
    show_profile(paths, &entry.id)
}

pub(crate) fn update_profile(
    paths: &GoldBandPaths,
    id: &str,
    input: ProfileInput,
) -> Result<ProfileEntry> {
    ensure_profile_input(&input)?;
    let existing = show_profile(paths, id)?;
    let mut entry = ProfileEntry {
        id: existing.id.clone(),
        name: input.name.trim().to_string(),
        summary: input.summary.trim().to_string(),
        content: input.content,
        scope: input.scope,
        created_at: existing.created_at,
        updated_at: local_timestamp(),
        path: String::new(),
    };
    entry.path = profile_path(paths, entry.scope, &entry.name, &entry.id).to_string();
    if existing.path != entry.path {
        let old_path = Utf8PathBuf::from(existing.path);
        if old_path.exists() {
            fs::remove_file(old_path.as_std_path())?;
        }
    }
    write_profile(paths, &entry)?;
    show_profile(paths, &entry.id)
}

pub(crate) fn find_profile_by_id(paths: &GoldBandPaths, id: &str) -> Result<Option<ProfileEntry>> {
    if id.trim().is_empty() {
        return Ok(None);
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

fn read_profile_dir(paths: &GoldBandPaths, scope: ProfileScope) -> Result<Vec<ProfileEntry>> {
    let dir = profile_dir(paths, scope);
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
    let path = profile_path(paths, profile.scope, &profile.name, &profile.id);
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
    if input.name.trim().is_empty() {
        bail!("profile name cannot be empty");
    }
    if input.summary.trim().is_empty() {
        bail!("profile summary cannot be empty");
    }
    Ok(())
}

fn profile_dir(paths: &GoldBandPaths, scope: ProfileScope) -> Utf8PathBuf {
    match scope {
        ProfileScope::User => paths.user_context_profiles_dir(),
        ProfileScope::Project => paths.project_context_profiles_dir(),
    }
}

fn profile_path(paths: &GoldBandPaths, scope: ProfileScope, name: &str, id: &str) -> Utf8PathBuf {
    profile_dir(paths, scope).join(format!("{}-{id}.md", sanitize_profile_name(name)))
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
        if find_profile_by_id(paths, &id)?.is_none() {
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

fn legacy_default_profile_id(id: &str) -> bool {
    matches!(
        id,
        "profile-plan" | "profile-dev" | "profile-review" | "profile-test" | "profile-accept"
    )
}

fn remove_seeded_user_defaults_with_name(
    profiles: &[ProfileEntry],
    name: &str,
    summary: &str,
) -> Result<()> {
    for profile in profiles.iter().filter(|profile| {
        profile.name == name
            && (legacy_default_profile_id(&profile.id)
                || (profile.summary == summary && profile.content.trim().is_empty()))
    }) {
        let path = Utf8PathBuf::from(&profile.path);
        if path.exists() {
            fs::remove_file(path.as_std_path())?;
        }
    }
    Ok(())
}

fn local_timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn scope_rank(scope: ProfileScope) -> u8 {
    match scope {
        ProfileScope::Project => 0,
        ProfileScope::User => 1,
    }
}
