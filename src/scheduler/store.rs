use crate::storage::{GoldBandPaths, read_json, write_json};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ScheduledTaskStore {
    paths: GoldBandPaths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTriggerRecord {
    pub version: String,
    pub id: String,
    pub scheduled_task_id: String,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub attempts: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ScheduledTriggerRecord {
    pub fn new(
        scheduled_task_id: impl Into<String>,
        scheduled_at: DateTime<Utc>,
        status: impl Into<String>,
        task_id: Option<String>,
        run_id: Option<String>,
        attempts: u32,
    ) -> Self {
        let now = Utc::now();
        Self {
            version: "0.1".to_string(),
            id: String::new(),
            scheduled_task_id: scheduled_task_id.into(),
            scheduled_at,
            status: status.into(),
            task_id,
            run_id,
            attempts,
            created_at: now,
            updated_at: now,
        }
    }
}

impl ScheduledTaskStore {
    pub fn new(paths: GoldBandPaths) -> Self {
        Self { paths }
    }

    pub fn save(&self, definition: &super::ScheduledTaskDefinition) -> Result<()> {
        validate_component(definition.id())?;
        write_json(&self.paths.scheduled_task_file(definition.id()), definition)
    }

    pub fn load(&self, id: &str) -> Result<super::ScheduledTaskDefinition> {
        validate_component(id)?;
        read_json(&self.paths.scheduled_task_file(id))
    }

    pub fn update(&self, definition: &super::ScheduledTaskDefinition) -> Result<()> {
        validate_component(definition.id())?;
        let current = self.load(definition.id())?;
        if current.id != definition.id {
            bail!("scheduled task definition id does not match its storage path");
        }
        write_json(&self.paths.scheduled_task_file(definition.id()), definition)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        validate_component(id)?;
        let definition = self.load(id)?;
        if definition.id != id {
            bail!("scheduled task definition id does not match its storage path");
        }
        let directory = self.paths.scheduled_task_dir(id);
        if !directory.exists() {
            bail!("scheduled task definition directory does not exist");
        }
        std::fs::remove_dir_all(directory.as_std_path())?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<super::ScheduledTaskDefinition>> {
        let dir = self.paths.scheduled_tasks_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut result: Vec<super::ScheduledTaskDefinition> = Vec::new();
        for entry in std::fs::read_dir(dir.as_std_path())? {
            let path = entry?.path().join("scheduled-task.json");
            if path.is_file() {
                let path = camino::Utf8PathBuf::from_path_buf(path)
                    .map_err(|_| anyhow::anyhow!("scheduled task path is not UTF-8"))?;
                result.push(read_json(&path)?);
            }
        }
        result.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(result)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<super::ScheduledTaskDefinition> {
        let mut definition = self.load(id)?;
        let was_enabled = definition.enabled;
        definition.enabled = enabled;
        if enabled && !was_enabled {
            if let super::ScheduleKind::Every { anchor_at, .. } = &mut definition.schedule.kind {
                *anchor_at = chrono::Utc::now();
            }
        }
        definition.updated_at = chrono::Utc::now();
        self.update(&definition)?;
        Ok(definition)
    }

    pub fn append_trigger(
        &self,
        mut record: ScheduledTriggerRecord,
    ) -> Result<ScheduledTriggerRecord> {
        validate_component(&record.scheduled_task_id)?;
        let definition = self.load(&record.scheduled_task_id)?;
        if definition.id != record.scheduled_task_id {
            bail!("scheduled trigger task id does not match its definition");
        }

        let directory = self.paths.scheduled_triggers_dir(&record.scheduled_task_id);
        std::fs::create_dir_all(directory.as_std_path())?;
        let next = next_trigger_number(&directory)?;
        record.id = format!("trigger-{next:03}");
        record.created_at = Utc::now();
        record.updated_at = record.created_at;
        let path = self
            .paths
            .scheduled_trigger_file(&record.scheduled_task_id, &record.id);
        if path.exists() {
            bail!("scheduled trigger record already exists: {}", record.id);
        }
        write_json(&path, &record)?;
        Ok(record)
    }

    pub fn list_triggers(&self, scheduled_task_id: &str) -> Result<Vec<ScheduledTriggerRecord>> {
        validate_component(scheduled_task_id)?;
        let directory = self.paths.scheduled_triggers_dir(scheduled_task_id);
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut records: Vec<ScheduledTriggerRecord> = Vec::new();
        for entry in std::fs::read_dir(directory.as_std_path())? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let path = camino::Utf8PathBuf::from_path_buf(path)
                .map_err(|_| anyhow::anyhow!("scheduled trigger path is not UTF-8"))?;
            records.push(read_json(&path)?);
        }
        records.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }
}

fn validate_component(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
        || Path::new(value).file_name().and_then(|name| name.to_str()) != Some(value)
    {
        bail!("scheduled task identifier must be a single path component");
    }
    Ok(())
}

fn next_trigger_number(directory: &camino::Utf8Path) -> Result<u64> {
    let mut next = 1;
    for entry in std::fs::read_dir(directory.as_std_path())? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(number) = name
            .strip_prefix("trigger-")
            .and_then(|value| value.strip_suffix(".json"))
            .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        next = next.max(number.saturating_add(1));
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::super::{OverlapPolicy, ScheduleSpec, ScheduledTaskDefinition};
    use crate::storage::GoldBandPaths;
    use camino::Utf8PathBuf;
    use chrono::{TimeZone, Utc};
    use tempfile::tempdir;

    #[test]
    fn scheduled_task_store_round_trips_definition_in_project_runtime() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let definition = ScheduledTaskDefinition::new(
            &paths.project_id,
            "scheduled-task-001",
            "direct",
            ScheduleSpec::every(
                30,
                "minutes",
                Utc.with_ymd_and_hms(2026, 7, 30, 10, 10, 0).unwrap(),
            )
            .unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let store = super::ScheduledTaskStore::new(paths.clone());

        store.save(&definition).unwrap();

        assert!(paths.scheduled_task_file(definition.id()).exists());
        assert_eq!(store.load(definition.id()).unwrap(), definition);
    }

    #[test]
    fn scheduled_task_store_keeps_multiple_definitions() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let store = super::ScheduledTaskStore::new(paths);
        let first = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-001",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 7, 31, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        let second = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-002",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();

        store.save(&first).unwrap();
        store.save(&second).unwrap();

        let ids = store
            .list()
            .unwrap()
            .into_iter()
            .map(|definition| definition.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["scheduled-task-001", "scheduled-task-002"]);
    }

    #[test]
    fn update_replaces_one_definition_and_delete_keeps_task_history() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let store = super::ScheduledTaskStore::new(paths.clone());
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-001",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        store.save(&definition).unwrap();
        std::fs::create_dir_all(paths.task_dir("task-001").as_std_path()).unwrap();

        definition.instruction = "updated".to_string();
        store.update(&definition).unwrap();

        assert_eq!(store.load(definition.id()).unwrap().instruction, "updated");
        store.delete(definition.id()).unwrap();
        assert!(!paths.scheduled_task_dir(definition.id()).exists());
        assert!(paths.task_dir("task-001").exists());
    }

    #[test]
    fn reenable_resets_every_anchor_only_when_transitioning_from_disabled() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let store = super::ScheduledTaskStore::new(paths);
        let original = Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap();
        let mut definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-001",
            "direct",
            ScheduleSpec::every(1, "hours", original).unwrap(),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        store.save(&definition).unwrap();

        let still_enabled = store.set_enabled(definition.id(), true).unwrap();
        assert_eq!(still_enabled.schedule.anchor_at(), original);

        definition = store.set_enabled(definition.id(), false).unwrap();
        let reenabled = store.set_enabled(definition.id(), true).unwrap();
        assert_ne!(reenabled.schedule.anchor_at(), original);
    }

    #[test]
    fn trigger_records_are_immutable_and_monotonically_numbered() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let store = super::ScheduledTaskStore::new(paths);
        let definition = ScheduledTaskDefinition::new(
            "project-a",
            "scheduled-task-001",
            "direct",
            ScheduleSpec::at(Utc.with_ymd_and_hms(2026, 8, 1, 1, 0, 0).unwrap()),
            OverlapPolicy::SkipWhenRunning,
        )
        .unwrap();
        store.save(&definition).unwrap();

        let first = store
            .append_trigger(super::ScheduledTriggerRecord::new(
                definition.id(),
                definition.created_at,
                "completed",
                Some("task-001".to_string()),
                Some("run-001".to_string()),
                1,
            ))
            .unwrap();
        let second = store
            .append_trigger(super::ScheduledTriggerRecord::new(
                definition.id(),
                definition.created_at,
                "skipped",
                None,
                None,
                2,
            ))
            .unwrap();

        assert_eq!(first.id, "trigger-001");
        assert_eq!(second.id, "trigger-002");
        assert_eq!(store.list_triggers(definition.id()).unwrap().len(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(
                    store
                        .paths
                        .scheduled_trigger_file(definition.id(), "trigger-001")
                        .as_std_path(),
                )
                .unwrap(),
            )
            .unwrap()["status"],
            "completed"
        );
    }

    #[test]
    fn definition_identifiers_cannot_escape_the_scheduled_task_directory() {
        let temp = tempdir().unwrap();
        let repo_root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = GoldBandPaths::new(repo_root);
        let store = super::ScheduledTaskStore::new(paths);

        assert!(store.load("..").is_err());
        assert!(store.load("nested/task").is_err());
    }
}
