use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{AgentSkillDirectoryPolicy, SKILL_FILE_NAME, SkillSource};
use crate::skill::{parse_skill_md_public, resolve_agent_skills_dir};
use crate::storage::GoldBandPaths;

pub const MAX_COMMANDS_PER_CATALOG: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpCommandItem {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpCommandCatalog {
    pub agent_type: String,
    pub project_id: String,
    /// 原始 ACP 命令。`None` 仅用于兼容尚未保存该字段的旧目录文件；
    /// `Some(Vec::new())` 表示 Agent 已明确返回空列表。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_commands: Option<Vec<AcpCommandItem>>,
    /// 查询时返回的瞬时 Skill 投影；持久目录不保存该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_commands: Option<Vec<AcpCommandItem>>,
    pub commands: Vec<AcpCommandItem>,
    pub updated_at: String,
}

#[derive(Debug)]
struct NativeSkillRoot {
    path: PathBuf,
    source: SkillSource,
    agent_source: String,
    directory_priority: usize,
    root_priority: usize,
}

#[derive(Debug)]
struct NativeSkillCommandCandidate {
    command: AcpCommandItem,
    directory_priority: usize,
    link_priority: usize,
    root_priority: usize,
    path_key: String,
}

pub fn project_id(workspace: &Utf8Path) -> String {
    GoldBandPaths::new(workspace.to_path_buf()).project_id
}

pub fn catalog_key(agent_type: &str, project_id: &str) -> String {
    format!("{}\n{}", agent_type.trim(), project_id.trim())
}

pub fn parse_available_commands(update: &Value) -> Option<Vec<AcpCommandItem>> {
    if update.get("sessionUpdate").and_then(Value::as_str) != Some("available_commands_update") {
        return None;
    }
    let commands = update.get("availableCommands")?.as_array()?;
    let mut names = BTreeSet::new();
    let parsed = commands
        .iter()
        .filter_map(parse_command)
        .filter(|command| names.insert(command.name.to_ascii_lowercase()))
        .take(MAX_COMMANDS_PER_CATALOG)
        .collect();
    Some(parsed)
}

pub fn merge_native_skill_commands(
    policy: &AgentSkillDirectoryPolicy,
    workspace: &Utf8Path,
    commands: Vec<AcpCommandItem>,
) -> Vec<AcpCommandItem> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    merge_native_skill_commands_at_home(policy, workspace, &home, commands)
}

pub fn scan_native_skill_commands(
    policy: &AgentSkillDirectoryPolicy,
    workspace: &Utf8Path,
) -> Vec<AcpCommandItem> {
    merge_native_skill_commands(policy, workspace, Vec::new())
}

pub fn merge_command_sources(
    preferred: Vec<AcpCommandItem>,
    fallback: Vec<AcpCommandItem>,
) -> Vec<AcpCommandItem> {
    let mut merged = Vec::new();
    let mut names = BTreeSet::new();
    for command in preferred.into_iter().chain(fallback) {
        if merged.len() >= MAX_COMMANDS_PER_CATALOG {
            break;
        }
        if names.insert(command.name.to_ascii_lowercase()) {
            merged.push(command);
        }
    }
    merged
}

fn merge_native_skill_commands_at_home(
    policy: &AgentSkillDirectoryPolicy,
    workspace: &Utf8Path,
    home: &Path,
    commands: Vec<AcpCommandItem>,
) -> Vec<AcpCommandItem> {
    let mut skill_commands = native_skill_roots(policy, workspace.as_std_path(), home)
        .into_iter()
        .flat_map(|root| {
            scan_native_skill_root(
                &root.path,
                root.source,
                &root.agent_source,
                root.directory_priority,
                root.root_priority,
                1,
            )
        })
        .collect::<Vec<_>>();
    skill_commands.sort_by(native_skill_candidate_order);
    merge_command_sources(
        commands,
        skill_commands
            .into_iter()
            .map(|candidate| candidate.command)
            .collect(),
    )
}

fn native_skill_roots(
    policy: &AgentSkillDirectoryPolicy,
    workspace: &Path,
    home: &Path,
) -> Vec<NativeSkillRoot> {
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let max_directory_count = policy
        .global
        .read_dir_names
        .len()
        .max(policy.project.read_dir_names.len());
    for directory_priority in 0..max_directory_count {
        for (scope_priority, (scope, base, source)) in [
            (&policy.global, home, SkillSource::Global),
            (&policy.project, workspace, SkillSource::Project),
        ]
        .into_iter()
        .enumerate()
        {
            let Some(dir_name) = scope.read_dir_names.get(directory_priority) else {
                continue;
            };
            let root = resolve_agent_skills_dir(base, dir_name).into_std_path_buf();
            if seen.insert(root.clone()) {
                roots.push(NativeSkillRoot {
                    path: root,
                    source,
                    agent_source: dir_name.clone(),
                    directory_priority,
                    root_priority: directory_priority * 2 + scope_priority,
                });
            }
        }
    }
    roots
}

fn scan_native_skill_root(
    root: &Path,
    source: SkillSource,
    agent_source: &str,
    directory_priority: usize,
    root_priority: usize,
    nested_depth: usize,
) -> Vec<NativeSkillCommandCandidate> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join(SKILL_FILE_NAME);
        if skill_file.is_file() {
            let Ok(raw) = fs::read_to_string(&skill_file) else {
                continue;
            };
            let default_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("unknown");
            let dir_path = path.to_string_lossy();
            let (meta, _) =
                parse_skill_md_public(&raw, default_name, source, &dir_path, agent_source);
            let name = meta.name.trim().trim_start_matches('/').trim();
            if !name.is_empty() && name.chars().all(is_command_char) {
                commands.push(NativeSkillCommandCandidate {
                    command: AcpCommandItem {
                        name: name.to_string(),
                        description: meta.description,
                        input_hint: None,
                    },
                    directory_priority,
                    link_priority: usize::from(is_link_path(&path)),
                    root_priority,
                    path_key: path.to_string_lossy().into_owned(),
                });
            }
        } else if nested_depth > 0 {
            commands.extend(scan_native_skill_root(
                &path,
                source,
                agent_source,
                directory_priority,
                root_priority,
                nested_depth - 1,
            ));
        }
    }
    commands
}

fn is_link_path(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
        || path.read_link().is_ok()
}

fn native_skill_candidate_order(
    left: &NativeSkillCommandCandidate,
    right: &NativeSkillCommandCandidate,
) -> std::cmp::Ordering {
    left.command
        .name
        .to_ascii_lowercase()
        .cmp(&right.command.name.to_ascii_lowercase())
        .then_with(|| left.directory_priority.cmp(&right.directory_priority))
        .then_with(|| left.link_priority.cmp(&right.link_priority))
        .then_with(|| left.root_priority.cmp(&right.root_priority))
        .then_with(|| left.path_key.cmp(&right.path_key))
}

fn parse_command(value: &Value) -> Option<AcpCommandItem> {
    let name = value
        .get("name")
        .or_else(|| value.get("command"))
        .and_then(Value::as_str)?
        .trim()
        .trim_start_matches('/')
        .trim();
    if name.is_empty() || !name.chars().all(is_command_char) {
        return None;
    }
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let input_hint = value
        .get("inputHint")
        .or_else(|| value.get("hint"))
        .or_else(|| value.pointer("/input/hint"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|hint| !hint.is_empty())
        .map(str::to_string);
    Some(AcpCommandItem {
        name: name.to_string(),
        description,
        input_hint,
    })
}

fn is_command_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs};

    use camino::Utf8Path;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        AcpCommandCatalog, AcpCommandItem, NativeSkillCommandCandidate, catalog_key,
        merge_native_skill_commands_at_home, native_skill_candidate_order,
        parse_available_commands, project_id,
    };
    use crate::config::{AgentSkillDirectoryPolicy, AgentSkillDirectoryScopePolicy};

    fn shared_skill_policy(primary: &str, compatible: &[&str]) -> AgentSkillDirectoryPolicy {
        let scope = AgentSkillDirectoryScopePolicy {
            write_dir_names: vec![primary.to_string()],
            read_dir_names: std::iter::once(primary.to_string())
                .chain(compatible.iter().map(|directory| (*directory).to_string()))
                .collect(),
        };
        AgentSkillDirectoryPolicy {
            global: scope.clone(),
            project: scope,
        }
    }

    #[test]
    fn parses_and_deduplicates_acp_commands() {
        let commands = parse_available_commands(&json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [
                {
                    "name": "ckm:design",
                    "description": "Design assets",
                    "input": { "hint": "topic" }
                },
                {
                    "name": "/CKM:DESIGN",
                    "description": "duplicate"
                },
                {
                    "command": "review.fix-v2",
                    "hint": "path"
                },
                {
                    "name": "测试",
                    "description": "Unicode skill"
                },
                {
                    "name": "invalid command"
                }
            ]
        }))
        .unwrap();

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name, "ckm:design");
        assert_eq!(commands[0].input_hint.as_deref(), Some("topic"));
        assert_eq!(commands[1].name, "review.fix-v2");
        assert_eq!(commands[1].input_hint.as_deref(), Some("path"));
        assert_eq!(commands[2].name, "测试");
    }

    #[test]
    fn ignores_non_command_updates() {
        assert!(
            parse_available_commands(&json!({
                "sessionUpdate": "usage_update",
                "availableCommands": []
            }))
            .is_none()
        );
    }

    #[test]
    fn catalog_identity_is_agent_and_project_id() {
        let workspace = project_id(Utf8Path::new("D:/Projects/Example/../Example"));
        assert_eq!(
            catalog_key("codex-acp", &workspace),
            format!("codex-acp\n{workspace}")
        );
    }

    #[test]
    fn codex_catalog_merges_codex_and_agents_skills_after_acp_commands() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        for (root, name, description) in [
            (
                home.path().join(".agents/skills"),
                "agent-browser",
                "Browse",
            ),
            (home.path().join(".codex/skills"), "openai-docs", "Docs"),
            (
                workspace.path().join(".codex/skills"),
                "review",
                "Skill duplicate",
            ),
        ] {
            let skill_dir = root.join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\n"),
            )
            .unwrap();
        }
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = shared_skill_policy(".codex", &[".agents"]);
        let commands = merge_native_skill_commands_at_home(
            &policy,
            &workspace,
            home.path(),
            vec![AcpCommandItem {
                name: "review".to_string(),
                description: "ACP review".to_string(),
                input_hint: Some("instructions".to_string()),
            }],
        );

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name, "review");
        assert_eq!(commands[0].description, "ACP review");
        assert!(
            commands
                .iter()
                .any(|command| command.name == "agent-browser")
        );
        assert!(commands.iter().any(|command| command.name == "openai-docs"));
    }

    #[test]
    fn split_directory_policy_reads_the_correct_global_and_project_roots() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        for (root, name) in [
            (home.path().join(".pi/agent/skills"), "global-primary"),
            (workspace.path().join(".pi/skills"), "project-primary"),
            (home.path().join(".agents/skills"), "global-compatible"),
            (
                workspace.path().join(".agents/skills"),
                "project-compatible",
            ),
            (home.path().join(".pi/skills"), "wrong-global-root"),
            (
                workspace.path().join(".pi/agent/skills"),
                "wrong-project-root",
            ),
        ] {
            let skill_dir = root.join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test\n---\n"),
            )
            .unwrap();
        }
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = AgentSkillDirectoryPolicy {
            global: AgentSkillDirectoryScopePolicy {
                write_dir_names: vec![".pi/agent".to_string()],
                read_dir_names: vec![".pi/agent".to_string(), ".agents".to_string()],
            },
            project: AgentSkillDirectoryScopePolicy {
                write_dir_names: vec![".pi".to_string()],
                read_dir_names: vec![".pi".to_string(), ".agents".to_string()],
            },
        };

        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());
        let names = commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            names,
            BTreeSet::from([
                "global-primary",
                "project-primary",
                "global-compatible",
                "project-compatible",
            ])
        );
    }

    #[test]
    fn duplicate_skill_menu_entries_prefer_primary_directory_metadata() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        for (root, folder, description) in [
            (
                home.path().join(".codex/skills"),
                "primary-version",
                "Primary directory",
            ),
            (
                home.path().join(".agents/skills"),
                "compatible-version",
                "Compatible directory",
            ),
        ] {
            let skill_dir = root.join(folder);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: duplicate-skill\ndescription: {description}\n---\n"),
            )
            .unwrap();
        }
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = shared_skill_policy(".codex", &[".agents"]);

        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "duplicate-skill");
        assert_eq!(commands[0].description, "Primary directory");
    }

    #[test]
    fn duplicate_skill_menu_priority_is_directory_then_entity() {
        let candidate =
            |directory_priority, link_priority, path: &str| NativeSkillCommandCandidate {
                command: AcpCommandItem {
                    name: "duplicate-skill".to_string(),
                    description: path.to_string(),
                    input_hint: None,
                },
                directory_priority,
                link_priority,
                root_priority: directory_priority * 2,
                path_key: path.to_string(),
            };
        let primary_entity = candidate(0, 0, "primary-entity");
        let primary_link = candidate(0, 1, "primary-link");
        let compatible_entity = candidate(1, 0, "compatible-entity");
        let compatible_link = candidate(1, 1, "compatible-link");

        assert!(native_skill_candidate_order(&primary_entity, &primary_link).is_lt());
        assert!(native_skill_candidate_order(&primary_link, &compatible_entity).is_lt());
        assert!(native_skill_candidate_order(&compatible_entity, &compatible_link).is_lt());
    }

    #[test]
    fn claude_catalog_does_not_read_agents_compatibility_directory() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        for (root, name) in [
            (workspace.path().join(".claude/skills"), "claude-skill"),
            (workspace.path().join(".agents/skills"), "shared-skill"),
        ] {
            let skill_dir = root.join(name);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test\n---\n"),
            )
            .unwrap();
        }
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = shared_skill_policy(".claude", &[]);

        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "claude-skill");
    }

    #[test]
    fn native_skill_scan_follows_managed_skill_links() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let source = workspace.path().join(".gold-band/skills/linked-skill");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: linked-skill\ndescription: Linked\n---\n",
        )
        .unwrap();
        let target = workspace.path().join(".codex/skills/linked-skill");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        crate::skill::symlink::create_link(&source, &target);
        if !target.exists() {
            return;
        }
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = shared_skill_policy(".codex", &[".agents"]);

        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "linked-skill");
    }

    #[test]
    fn persisted_catalog_without_project_id_is_discarded() {
        let error = serde_json::from_value::<AcpCommandCatalog>(json!({
            "agentType": "codex-acp",
            "workspaceKey": "D:/workspace",
            "commands": [{ "name": "review", "description": "Review" }],
            "updatedAt": "2026-07-24 12:00:00"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("projectId"));
    }

    #[test]
    fn rescanning_from_raw_acp_commands_removes_deleted_skills() {
        let home = tempdir().unwrap();
        let workspace = tempdir().unwrap();
        let skill_dir = workspace.path().join(".agents/skills/temporary-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: temporary-skill\ndescription: Temporary\n---\n",
        )
        .unwrap();
        let workspace = camino::Utf8PathBuf::from_path_buf(workspace.path().to_path_buf()).unwrap();
        let policy = shared_skill_policy(".codex", &[".agents"]);

        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());
        assert_eq!(commands.len(), 1);

        fs::remove_dir_all(skill_dir).unwrap();
        let commands =
            merge_native_skill_commands_at_home(&policy, &workspace, home.path(), Vec::new());
        assert!(commands.is_empty());
    }
}
