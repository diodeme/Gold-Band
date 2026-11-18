//! Short, process-wide and cross-process boundary for settings and workflow bindings.
//! Recovery only completes known before/after states; external edits are never overwritten.
use super::{atomic_write_file, ensure_parent_dir, with_file_lock};
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    fs::{self, OpenOptions},
    io::Write,
};

const LOCK_NAME: &str = ".configuration.lock";
const PENDING_NAME: &str = ".onboarding-commit.json";
thread_local! { static HELD_ROOTS: RefCell<Vec<Utf8PathBuf>> = const { RefCell::new(Vec::new()) }; }

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FileRevision {
    #[default]
    Untracked,
    Missing,
    Present(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "params")]
pub enum ConfigurationError {
    #[error("setup.configuration-conflict")]
    #[serde(rename = "setup.configuration-conflict")]
    Conflict { file: String },
    #[error("setup.recovery-required")]
    #[serde(rename = "setup.recovery-required")]
    RecoveryRequired { phase: String },
}

fn bytes(path: &Utf8Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn revision(path: &Utf8Path) -> Result<FileRevision> {
    Ok(match bytes(path)? {
        None => FileRevision::Missing,
        Some(bytes) => FileRevision::Present(blake3::hash(&bytes).to_hex().to_string()),
    })
}

pub fn verify_revision(path: &Utf8Path, expected: &FileRevision) -> Result<()> {
    let current = revision(path)?;
    if &current != expected
        && !matches!(
            (expected, current),
            (FileRevision::Untracked, FileRevision::Missing)
        )
    {
        return Err(ConfigurationError::Conflict {
            file: path.file_name().unwrap_or_default().into(),
        }
        .into());
    }
    Ok(())
}

/// Only the two canonical configuration targets participate in this boundary.
pub(super) fn root_for_target(path: &Utf8Path) -> Option<Utf8PathBuf> {
    match path.file_name()? {
        "settings.json" => Some(path.parent()?.to_path_buf()),
        "workflows.json" if path.parent()?.file_name() == Some("context") => {
            Some(path.parent()?.parent()?.to_path_buf())
        }
        _ => None,
    }
}

struct HeldRoot;
impl Drop for HeldRoot {
    fn drop(&mut self) {
        HELD_ROOTS.with(|roots| {
            roots.borrow_mut().pop();
        });
    }
}

pub fn with_configuration<T>(root: &Utf8Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    fs::create_dir_all(root)?;
    let root = Utf8PathBuf::from_path_buf(dunce::canonicalize(root)?)
        .map_err(|_| anyhow::anyhow!("configuration root must be UTF-8"))?;
    if HELD_ROOTS.with(|roots| roots.borrow().contains(&root)) {
        return operation();
    }
    let lock_path = root.join(LOCK_NAME);
    with_file_lock(&lock_path, || {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        file.lock_exclusive()?;
        HELD_ROOTS.with(|roots| roots.borrow_mut().push(root.clone()));
        let _held = HeldRoot;
        recover_unlocked(&root)?;
        let result = operation();
        let _ = FileExt::unlock(&file);
        result
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingCommit {
    operation_id: String,
    settings: Change,
    workflows: Change,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Change {
    before: Option<String>,
    after: String,
}

fn current_text(path: &Utf8Path) -> Result<Option<String>> {
    bytes(path)?
        .map(String::from_utf8)
        .transpose()
        .map_err(Into::into)
}

fn write_text(path: &Utf8Path, text: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    atomic_write_file(path.as_std_path(), |file| -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(text.as_bytes())?;
        Ok(())
    })
}

fn state_matches(path: &Utf8Path, change: &Change) -> Result<bool> {
    let current = current_text(path)?;
    if current.as_ref() == Some(&change.after) {
        return Ok(true);
    }
    if current == change.before {
        return Ok(false);
    }
    Err(ConfigurationError::Conflict {
        file: path.file_name().unwrap_or_default().into(),
    }
    .into())
}

fn recover_unlocked(root: &Utf8Path) -> Result<()> {
    let pending_path = root.join(PENDING_NAME);
    let Some(content) = bytes(&pending_path)? else {
        return Ok(());
    };
    let pending: PendingCommit =
        serde_json::from_slice(&content).map_err(|_| ConfigurationError::RecoveryRequired {
            phase: "read-journal".into(),
        })?;
    let settings = root.join("settings.json");
    let workflows = root.join("context/workflows.json");
    // Validate both before writing either: a conflict in settings must not rewrite workflows.
    let settings_done = state_matches(&settings, &pending.settings)?;
    let workflows_done = state_matches(&workflows, &pending.workflows)?;
    if !workflows_done {
        write_text(&workflows, &pending.workflows.after)?;
    }
    if !settings_done {
        write_text(&settings, &pending.settings.after)?;
    }
    fs::remove_file(pending_path)?;
    Ok(())
}

pub fn commit_pair(
    root: &Utf8Path,
    operation_id: &str,
    settings: &impl Serialize,
    workflows: &impl Serialize,
) -> Result<()> {
    commit_pair_with_hook(root, operation_id, settings, workflows, |_| Ok(()))
}

fn commit_pair_with_hook(
    root: &Utf8Path,
    operation_id: &str,
    settings: &impl Serialize,
    workflows: &impl Serialize,
    mut after_write: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    with_configuration(root, || {
        let settings_path = root.join("settings.json");
        let workflows_path = root.join("context/workflows.json");
        let pending_path = root.join(PENDING_NAME);
        let pending = PendingCommit {
            operation_id: operation_id.into(),
            settings: Change {
                before: current_text(&settings_path)?,
                after: serde_json::to_string_pretty(settings)?,
            },
            workflows: Change {
                before: current_text(&workflows_path)?,
                after: serde_json::to_string_pretty(workflows)?,
            },
        };
        write_text(&pending_path, &serde_json::to_string(&pending)?)?;
        after_write(0)?;
        write_text(&workflows_path, &pending.workflows.after)?;
        after_write(1)?;
        write_text(&settings_path, &pending.settings.after)?;
        after_write(2)?;
        if !state_matches(&settings_path, &pending.settings)?
            || !state_matches(&workflows_path, &pending.workflows)?
        {
            return Err(ConfigurationError::RecoveryRequired {
                phase: "verify".into(),
            }
            .into());
        }
        // Both files are committed. Leaving a journal after a cleanup failure is safe to retry.
        let _ = fs::remove_file(pending_path);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn root(temp: &tempfile::TempDir) -> &Utf8Path {
        Utf8Path::from_path(temp.path()).unwrap()
    }

    #[test]
    fn every_interrupted_commit_stage_recovers_without_reexecuting_the_operation() {
        for stage in 0..3 {
            let temp = tempfile::tempdir().unwrap();
            let root = root(&temp);
            write_text(&root.join("settings.json"), "{\"old\":true}").unwrap();
            write_text(&root.join("context/workflows.json"), "{\"old\":true}").unwrap();
            assert!(
                commit_pair_with_hook(
                    root,
                    "apply",
                    &json!({"completed":true}),
                    &json!({"revision":1}),
                    |at| {
                        if at == stage {
                            anyhow::bail!("injected write failure");
                        }
                        Ok(())
                    }
                )
                .is_err()
            );
            with_configuration(root, || Ok(())).unwrap();
            assert_eq!(
                super::super::read_json::<serde_json::Value>(&root.join("settings.json")).unwrap(),
                json!({"completed":true})
            );
            assert_eq!(
                super::super::read_json::<serde_json::Value>(&root.join("context/workflows.json"))
                    .unwrap(),
                json!({"revision":1})
            );
            assert!(!root.join(PENDING_NAME).exists());
            with_configuration(root, || Ok(())).unwrap();
        }
    }

    #[test]
    fn recovery_checks_both_files_before_overwriting_anything() {
        let temp = tempfile::tempdir().unwrap();
        let root = root(&temp);
        assert!(
            commit_pair_with_hook(
                root,
                "apply",
                &json!({"new":true}),
                &json!({"new":true}),
                |_| anyhow::bail!("stop")
            )
            .is_err()
        );
        write_text(&root.join("settings.json"), "{\"external\":true}").unwrap();
        assert!(with_configuration(root, || Ok(())).is_err());
        assert!(!root.join("context/workflows.json").exists());
        assert_eq!(
            current_text(&root.join("settings.json")).unwrap().unwrap(),
            "{\"external\":true}"
        );
        assert!(root.join(PENDING_NAME).exists());
    }

    #[test]
    fn stale_edit_conflicts_and_nested_boundary_does_not_deadlock() {
        let temp = tempfile::tempdir().unwrap();
        let root = root(&temp);
        let path = root.join("settings.json");
        with_configuration(root, || {
            let original = revision(&path)?;
            with_configuration(root, || write_text(&path, "{}"))?;
            assert!(verify_revision(&path, &original).is_err());
            Ok(())
        })
        .unwrap();
    }
}
