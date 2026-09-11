use std::{fs::OpenOptions, io::Read};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::storage::{GoldBandPaths, write_json};

pub mod mcp;

pub const MAX_ENTRIES: usize = 100;
pub const MAX_KEY_CHARS: usize = 128;
pub const MAX_VALUE_CHARS: usize = 4_000;
pub const MAX_DESC_CHARS: usize = 500;
pub const MAX_EFFECTIVE_BYTES: usize = 32 * 1024;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const SCHEMA_VERSION: u32 = 1;
const WB_DEFAULT_KEY: &str = "subSysId1";
const WB_DEFAULT_DESC: &str = "xxxx子系统";

pub fn is_wb() -> bool {
    crate::storage::active_storage_path_config().app_key == "maling"
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub desc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    #[serde(flatten)]
    pub entry: Entry,
    pub revision: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Workspace,
    Task,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryFile {
    version: u32,
    entries: Vec<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteCommand {
    pub scope: Scope,
    pub key: String,
    pub expected_revision: Option<String>,
    pub entry: Option<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EffectiveEntry {
    #[serde(flatten)]
    pub entry: Entry,
    pub scope: Scope,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub project_id: String,
    pub task_id: Option<String>,
    pub workspace_path: Utf8PathBuf,
    pub task_path: Option<Utf8PathBuf>,
    pub workspace: Vec<Record>,
    pub task: Vec<Record>,
    pub effective: Vec<EffectiveEntry>,
    pub limits: Value,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}")]
pub struct MemoryError {
    pub code: &'static str,
    pub params: Value,
}
type Result<T> = std::result::Result<T, MemoryError>;
fn error(code: &'static str, params: Value) -> MemoryError {
    MemoryError { code, params }
}
fn io_error(path: &Utf8Path, cause: impl std::fmt::Display) -> MemoryError {
    error(
        "memory.io",
        json!({"path": path, "cause": cause.to_string()}),
    )
}

#[derive(Clone)]
pub struct MemoryService {
    paths: GoldBandPaths,
    task_id: Option<String>,
    wb: bool,
}

impl MemoryService {
    pub fn new(
        paths: GoldBandPaths,
        project_id: &str,
        task_id: Option<String>,
        wb: bool,
    ) -> Result<Self> {
        if paths.project_id != project_id
            || task_id.as_ref().is_some_and(|id| {
                id.is_empty()
                    || id == "."
                    || id == ".."
                    || !id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
        {
            return Err(error(
                "memory.locator",
                json!({"projectId": project_id, "taskId": task_id}),
            ));
        }
        paths
            .validate_project_manifest()
            .map_err(|e| io_error(&paths.project_manifest_file(), e))?;
        if let Some(id) = &task_id {
            if !paths.task_file(id).is_file() {
                return Err(error(
                    "memory.locator",
                    json!({"projectId": project_id, "taskId": id}),
                ));
            }
        }
        Ok(Self { paths, task_id, wb })
    }

    fn path(&self, scope: Scope) -> Result<Utf8PathBuf> {
        let path = match scope {
            Scope::Workspace => self.paths.runtime_root.join("memory.json"),
            Scope::Task => self
                .paths
                .task_dir(
                    self.task_id
                        .as_deref()
                        .ok_or_else(|| error("memory.locator", json!({})))?,
                )
                .join("memory.json"),
        };
        // Reject redirected files and directories before touching either data or locks.
        let root = dunce::canonicalize(&self.paths.runtime_root).map_err(|e| io_error(&path, e))?;
        for ancestor in path
            .ancestors()
            .take_while(|p| *p != self.paths.runtime_root)
        {
            if ancestor.exists() {
                let resolved = dunce::canonicalize(ancestor).map_err(|e| io_error(&path, e))?;
                if !resolved.starts_with(&root)
                    || std::fs::symlink_metadata(ancestor)
                        .map_err(|e| io_error(&path, e))?
                        .file_type()
                        .is_symlink()
                {
                    return Err(error("memory.locator", json!({"path": path})));
                }
            }
        }
        Ok(path)
    }

    fn locked<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        self.path(Scope::Workspace)?;
        let path = self.paths.runtime_root.join("memory.lock");
        if path.is_symlink() {
            return Err(error("memory.locator", json!({"path": path})));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        fs2::FileExt::lock_exclusive(&file).map_err(|e| io_error(&path, e))?;
        operation()
    }

    fn load(&self, scope: Scope) -> Result<MemoryFile> {
        let path = self.path(scope)?;
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let entries = if scope == Scope::Workspace && self.wb {
                    vec![Record {
                        entry: Entry {
                            key: WB_DEFAULT_KEY.into(),
                            value: String::new(),
                            desc: WB_DEFAULT_DESC.into(),
                        },
                        revision: uuid::Uuid::new_v4().to_string(),
                    }]
                } else {
                    Vec::new()
                };
                let data = MemoryFile {
                    version: SCHEMA_VERSION,
                    entries,
                };
                write_json(&path, &data).map_err(|e| io_error(&path, e))?;
                return Ok(data);
            }
            Err(e) => return Err(io_error(&path, e)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_error(&path, e))?;
        let corrupt = || error("memory.corrupt", json!({"path": path}));
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(corrupt());
        }
        let data: MemoryFile = serde_json::from_slice(&bytes).map_err(|_| corrupt())?;
        if data.version != SCHEMA_VERSION || data.entries.len() > MAX_ENTRIES {
            return Err(corrupt());
        }
        let mut keys = std::collections::HashSet::new();
        for record in &data.entries {
            if record.revision.is_empty()
                || !keys.insert(&record.entry.key)
                || validate_entry(&record.entry).is_err()
            {
                return Err(corrupt());
            }
        }
        Ok(data)
    }

    fn snapshot(&self, workspace: MemoryFile, task: MemoryFile) -> Result<Snapshot> {
        let mut effective =
            indexmap::IndexMap::with_capacity(workspace.entries.len() + task.entries.len());
        for (scope, file) in [(Scope::Workspace, &workspace), (Scope::Task, &task)] {
            for record in &file.entries {
                effective.insert(
                    record.entry.key.clone(),
                    EffectiveEntry {
                        entry: record.entry.clone(),
                        scope,
                    },
                );
            }
        }
        let effective: Vec<_> = effective.into_values().collect();
        let size = serde_json::to_vec(&effective)
            .expect("serializable entries")
            .len();
        if size > MAX_EFFECTIVE_BYTES {
            return Err(error(
                "memory.capacity",
                json!({"bytes": size, "maxBytes": MAX_EFFECTIVE_BYTES}),
            ));
        }
        Ok(Snapshot {
            project_id: self.paths.project_id.clone(),
            task_id: self.task_id.clone(),
            workspace_path: self.path(Scope::Workspace)?,
            task_path: self
                .task_id
                .as_ref()
                .map(|_| self.path(Scope::Task))
                .transpose()?,
            workspace: workspace.entries,
            task: task.entries,
            effective,
            limits: json!({"entries": MAX_ENTRIES, "key": MAX_KEY_CHARS, "value": MAX_VALUE_CHARS, "desc": MAX_DESC_CHARS, "effectiveBytes": MAX_EFFECTIVE_BYTES}),
        })
    }

    fn files(&self) -> Result<(MemoryFile, MemoryFile)> {
        let workspace = self.load(Scope::Workspace)?;
        let task = if self.task_id.is_some() {
            self.load(Scope::Task)?
        } else {
            MemoryFile {
                version: SCHEMA_VERSION,
                entries: Vec::new(),
            }
        };
        Ok((workspace, task))
    }

    pub fn read(&self) -> Result<Snapshot> {
        self.locked(|| {
            let (workspace, task) = self.files()?;
            self.snapshot(workspace, task)
        })
    }

    pub fn write(&self, command: WriteCommand) -> Result<Snapshot> {
        self.write_with(command, |path, data| {
            write_json(path, data).map_err(|e| io_error(path, e))
        })
    }

    fn write_with(
        &self,
        command: WriteCommand,
        persist: impl FnOnce(&Utf8Path, &Value) -> Result<()>,
    ) -> Result<Snapshot> {
        self.locked(|| {
            let path = self.path(command.scope)?;
            let (mut workspace, mut task) = self.files()?;
            let target = match command.scope {
                Scope::Workspace => &mut workspace,
                Scope::Task => &mut task,
            };
            let current = target.entries.iter().find(|r| r.entry.key == command.key);
            if current.map(|r| r.revision.as_str()) != command.expected_revision.as_deref() {
                return Err(error(
                    "memory.conflict",
                    json!({"key": command.key, "scope": command.scope, "latest": current}),
                ));
            }
            if let Some(entry) = &command.entry {
                validate_entry(entry)?;
                if entry.key != command.key
                    && let Some(latest) = target.entries.iter().find(|r| r.entry.key == entry.key)
                {
                    return Err(error(
                        "memory.conflict",
                        json!({"key": entry.key, "scope": command.scope, "latest": latest}),
                    ));
                }
            }
            target.entries.retain(|r| r.entry.key != command.key);
            if let Some(entry) = command.entry {
                target.entries.push(Record {
                    entry,
                    revision: uuid::Uuid::new_v4().to_string(),
                });
            }
            if target.entries.len() > MAX_ENTRIES {
                return Err(error("memory.capacity", json!({"maxEntries": MAX_ENTRIES})));
            }
            let serialized = serde_json::to_value(&*target).expect("serializable memory file");
            if command.scope == Scope::Workspace {
                let standalone: Vec<_> = workspace
                    .entries
                    .iter()
                    .map(|record| EffectiveEntry {
                        entry: record.entry.clone(),
                        scope: Scope::Workspace,
                    })
                    .collect();
                if serde_json::to_vec(&standalone).unwrap().len() > MAX_EFFECTIVE_BYTES {
                    return Err(error(
                        "memory.capacity",
                        json!({"maxBytes": MAX_EFFECTIVE_BYTES}),
                    ));
                }
            }
            let snapshot = self.snapshot(workspace, task)?;
            persist(&path, &serialized)?;
            Ok(snapshot)
        })
    }
}

fn validate_entry(entry: &Entry) -> Result<()> {
    for (field, value, max) in [
        ("key", &entry.key, MAX_KEY_CHARS),
        ("value", &entry.value, MAX_VALUE_CHARS),
        ("desc", &entry.desc, MAX_DESC_CHARS),
    ] {
        if value.chars().count() > max || (field == "key" && value.trim().is_empty()) {
            return Err(error("memory.field", json!({"field": field, "max": max})));
        }
    }
    Ok(())
}

pub fn system_rules(language: crate::config::DesktopLanguage) -> &'static str {
    match language {
        crate::config::DesktopLanguage::ZhCn => {
            include_str!("../prompts/zh-CN/runtime/memory-rules.md")
        }
        crate::config::DesktopLanguage::En => include_str!("../prompts/en/runtime/memory-rules.md"),
    }
}

pub fn prepare_invocation(req: &mut crate::provider::WorkerInvocation) -> anyhow::Result<String> {
    let paths = GoldBandPaths::new(req.adapter_workspace_dir.clone());
    let service = MemoryService::new(
        paths.clone(),
        &req.runtime_context.project_id,
        Some(req.runtime_context.task_id.clone()),
        is_wb(),
    )?;
    let rendered = service.render_context(req.runtime_context.language)?;
    req.mcp_servers
        .retain(|server| server.get("name").and_then(Value::as_str) != Some(mcp::SERVER_NAME));
    req.mcp_servers.push(mcp::server_config(
        &paths,
        &req.runtime_context.task_id,
        req.runtime_context.language,
    )?);
    Ok(rendered)
}

impl MemoryService {
    pub fn render_context(
        &self,
        language: crate::config::DesktopLanguage,
    ) -> anyhow::Result<String> {
        let snapshot = self.read()?;
        let data = json!({"workspacePath": snapshot.workspace_path, "taskPath": snapshot.task_path, "effective": snapshot.effective});
        // Escape markup delimiters so values cannot close the surrounding data block.
        let data = serde_json::to_string(&data)?
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026");
        let template = match language {
            crate::config::DesktopLanguage::ZhCn => {
                include_str!("../prompts/zh-CN/runtime/memory.md")
            }
            crate::config::DesktopLanguage::En => include_str!("../prompts/en/runtime/memory.md"),
        };
        let rendered =
            minijinja::Environment::new().render_str(template, minijinja::context! { data })?;
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests;
