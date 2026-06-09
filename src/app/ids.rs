use anyhow::Result;
use camino::Utf8Path;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(crate) fn next_task_id(tasks_dir: &Utf8Path) -> Result<String> {
    let mut max_id = 0_u32;
    if tasks_dir.exists() {
        for entry in fs::read_dir(tasks_dir.as_std_path())? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && let Some(number) = name.strip_prefix("task-")
                && let Ok(parsed) = number.parse::<u32>()
            {
                max_id = max_id.max(parsed);
            }
        }
    }
    Ok(format!("task-{max:03}", max = max_id + 1))
}

pub(crate) fn next_run_id(runs_dir: &Utf8Path) -> Result<String> {
    let mut max_id = 0_u32;
    if runs_dir.exists() {
        for entry in fs::read_dir(runs_dir.as_std_path())? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && let Some(number) = name.strip_prefix("run-")
                && let Ok(parsed) = number.parse::<u32>()
            {
                max_id = max_id.max(parsed);
            }
        }
    }
    Ok(format!("run-{max:03}", max = max_id + 1))
}

pub(crate) fn next_round_id(rounds_dir: &Utf8Path) -> Result<String> {
    let mut max_id = 0_u32;
    if rounds_dir.exists() {
        for entry in fs::read_dir(rounds_dir.as_std_path())? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str()
                && let Some(number) = name.strip_prefix("round-")
                && let Ok(parsed) = number.parse::<u32>()
            {
                max_id = max_id.max(parsed);
            }
        }
    }
    Ok(format!("round-{max:03}", max = max_id + 1))
}

pub(crate) fn generate_uuid() -> String {
    Uuid::new_v4().simple().to_string()
}

pub(crate) fn next_attempt_id(node_dir: &Utf8Path) -> Result<String> {
    let next = latest_attempt_id(node_dir)?
        .and_then(|value| {
            value
                .strip_prefix("attempt-")
                .and_then(|v| v.parse::<u32>().ok())
        })
        .unwrap_or(0)
        + 1;
    Ok(format!("attempt-{next:03}"))
}

pub(crate) fn latest_attempt_id(node_dir: &Utf8Path) -> Result<Option<String>> {
    if !node_dir.exists() {
        return Ok(None);
    }
    let mut max_id = 0_u32;
    for entry in fs::read_dir(node_dir.as_std_path())? {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str()
            && let Some(number) = name.strip_prefix("attempt-")
            && let Ok(parsed) = number.parse::<u32>()
        {
            max_id = max_id.max(parsed);
        }
    }
    if max_id == 0 {
        Ok(None)
    } else {
        Ok(Some(format!("attempt-{max_id:03}")))
    }
}

pub(crate) fn next_workflow_id() -> String {
    format!("workflow-{}", Uuid::new_v4().simple())
}

pub(crate) fn now_rfc3339_like() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generate_uuid_length_32() {
        let uuid = generate_uuid();
        assert_eq!(uuid.len(), 32);
        assert!(uuid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_uuid_100_unique() {
        let mut ids: Vec<String> = (0..100).map(|_| generate_uuid()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn next_round_id_empty_dir() {
        let dir = TempDir::new().unwrap();
        let p = camino::Utf8Path::from_path(dir.path()).unwrap();
        assert_eq!(next_round_id(p).unwrap(), "round-001");
    }

    #[test]
    fn next_round_id_with_existing() {
        let dir = TempDir::new().unwrap();
        let p = camino::Utf8Path::from_path(dir.path()).unwrap();
        std::fs::create_dir_all(p.join("round-001")).unwrap();
        std::fs::create_dir_all(p.join("round-002")).unwrap();
        std::fs::create_dir_all(p.join("round-005")).unwrap();
        assert_eq!(next_round_id(p).unwrap(), "round-006");
    }
}
