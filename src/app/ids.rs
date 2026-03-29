use anyhow::Result;
use camino::Utf8Path;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

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

pub(crate) fn next_attempt_id(node_dir: &Utf8Path) -> Result<String> {
    let next = latest_attempt_id(node_dir)?
        .and_then(|value| value.strip_prefix("attempt-").and_then(|v| v.parse::<u32>().ok()))
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

pub(crate) fn now_rfc3339_like() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}Z")
}
