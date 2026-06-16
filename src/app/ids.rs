use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;
use std::io;
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

pub(crate) fn reserve_next_run_dir(runs_dir: &Utf8Path) -> Result<(String, Utf8PathBuf)> {
    fs::create_dir_all(runs_dir.as_std_path())?;
    loop {
        let run_id = next_run_id(runs_dir)?;
        let run_dir = runs_dir.join(&run_id);
        match fs::create_dir(run_dir.as_std_path()) {
            Ok(()) => return Ok((run_id, run_dir)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
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

    #[test]
    fn next_run_id_uses_max_existing_run_number() {
        let dir = TempDir::new().unwrap();
        let p = camino::Utf8Path::from_path(dir.path()).unwrap();
        std::fs::create_dir_all(p.join("run-001")).unwrap();
        std::fs::create_dir_all(p.join("run-002")).unwrap();
        std::fs::create_dir_all(p.join("run-005")).unwrap();

        assert_eq!(next_run_id(p).unwrap(), "run-006");
    }

    #[test]
    fn reserve_next_run_dir_skips_existing_highest_run() {
        let dir = TempDir::new().unwrap();
        let p = camino::Utf8Path::from_path(dir.path()).unwrap();
        std::fs::create_dir_all(p.join("run-001")).unwrap();
        std::fs::create_dir_all(p.join("run-002")).unwrap();
        std::fs::create_dir_all(p.join("run-005")).unwrap();

        let (run_id, run_dir) = reserve_next_run_dir(p).unwrap();

        assert_eq!(run_id, "run-006");
        assert!(run_dir.exists());
    }

    #[test]
    fn reserve_next_run_dir_allocates_unique_dirs_concurrently() {
        let dir = TempDir::new().unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(p.join("run-001")).unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let runs_dir = p.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    reserve_next_run_dir(&runs_dir).unwrap().0
                })
            })
            .collect::<Vec<_>>();
        let mut run_ids = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        run_ids.sort();

        assert_eq!(
            run_ids,
            vec![
                "run-002".to_string(),
                "run-003".to_string(),
                "run-004".to_string(),
                "run-005".to_string(),
                "run-006".to_string(),
                "run-007".to_string(),
                "run-008".to_string(),
                "run-009".to_string(),
            ]
        );
    }
}
