//! Platform adapter for moving workspace entries to the system trash and
//! restoring them for undo.
//!
//! Restoring needs the item's location inside the trash. It is captured right
//! after the move so undo is a single rename instead of a scan of the whole
//! trash (a Windows Recycle Bin with thousands of items takes tens of seconds
//! to enumerate through the shell):
//! - macOS: `NSFileManager` returns the resulting trash URL directly.
//! - Windows: the `$I` metadata files written in the per-user
//!   `$Recycle.Bin` folder during this deletion are matched by original path.
//! - Other platforms (and a Windows location miss) fall back to the
//!   `trash::os_limited` listing at restore time.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub(crate) struct TrashedEntry {
    pub original_path: PathBuf,
    /// Seconds since the UNIX epoch captured right before the move, minus a
    /// second because trash metadata is recorded at second precision.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub deleted_after_unix: i64,
    /// Where the entry now lives inside the trash, when it could be located.
    pub trashed_path: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TrashError {
    /// The original location is occupied again.
    Collision,
    /// The item is no longer in the trash (emptied or restored elsewhere).
    Unavailable(String),
    Failed(String),
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn ensure_original_free(original: &Path) -> Result<(), TrashError> {
    match std::fs::symlink_metadata(original) {
        Ok(_) => Err(TrashError::Collision),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TrashError::Failed(error.to_string())),
    }
}

pub(crate) fn move_to_trash(path: &Path) -> Result<TrashedEntry, TrashError> {
    let deleted_after_unix = now_unix() - 1;
    let trashed_path = platform::move_to_trash(path, deleted_after_unix)?;
    Ok(TrashedEntry {
        original_path: path.to_path_buf(),
        deleted_after_unix,
        trashed_path,
    })
}

pub(crate) fn restore_from_trash(entry: &TrashedEntry) -> Result<(), TrashError> {
    ensure_original_free(&entry.original_path)?;
    match &entry.trashed_path {
        Some(trashed_path) => restore_by_rename(trashed_path, &entry.original_path)?,
        None => restore_from_listing(entry)?,
    }
    platform::after_restore(entry);
    Ok(())
}

fn restore_by_rename(trashed_path: &Path, original: &Path) -> Result<(), TrashError> {
    match std::fs::symlink_metadata(trashed_path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(TrashError::Unavailable("not-in-trash".to_string()));
        }
        Err(error) => return Err(TrashError::Unavailable(error.to_string())),
    }
    std::fs::rename(trashed_path, original).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            TrashError::Unavailable(error.to_string())
        } else {
            TrashError::Failed(error.to_string())
        }
    })
}

#[cfg(not(target_os = "macos"))]
fn restore_from_listing(entry: &TrashedEntry) -> Result<(), TrashError> {
    let items = trash::os_limited::list().map_err(|error| TrashError::Failed(error.to_string()))?;
    let item = select_trash_item(items, &entry.original_path, entry.deleted_after_unix)
        .ok_or_else(|| TrashError::Unavailable("not-in-trash".to_string()))?;
    trash::os_limited::restore_all([item]).map_err(|error| match error {
        trash::Error::RestoreCollision { .. } => TrashError::Collision,
        other => TrashError::Failed(other.to_string()),
    })
}

#[cfg(target_os = "macos")]
fn restore_from_listing(_entry: &TrashedEntry) -> Result<(), TrashError> {
    Err(TrashError::Unavailable(
        "missing-trash-location".to_string(),
    ))
}

/// Pick the newest trash item for `original` deleted inside the window. Older
/// deletions of the same path are never restored by mistake.
#[cfg(not(target_os = "macos"))]
fn select_trash_item(
    items: Vec<trash::TrashItem>,
    original: &Path,
    deleted_after_unix: i64,
) -> Option<trash::TrashItem> {
    items
        .into_iter()
        .filter(|item| {
            item.time_deleted >= deleted_after_unix
                && super::paths::same_path(&item.original_path(), original)
        })
        .max_by_key(|item| item.time_deleted)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{TrashError, TrashedEntry};
    use std::path::{Path, PathBuf};

    pub(super) fn move_to_trash(
        path: &Path,
        _deleted_after_unix: i64,
    ) -> Result<Option<PathBuf>, TrashError> {
        use objc2::rc::Retained;
        use objc2_foundation::{NSFileManager, NSString, NSURL};

        let path_text = path
            .to_str()
            .ok_or_else(|| TrashError::Failed("non-utf8-path".to_string()))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path_text));
        let mut resulting: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
            .map_err(|error| TrashError::Failed(error.to_string()))?;
        Ok(resulting
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string())))
    }

    pub(super) fn after_restore(_entry: &TrashedEntry) {}
}

#[cfg(windows)]
mod platform {
    use super::{TrashError, TrashedEntry};
    use std::path::{Component, Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    const RECYCLE_BIN_DIRECTORY: &str = "$Recycle.Bin";
    const METADATA_PREFIX: &str = "$I";
    const CONTENT_PREFIX: &str = "$R";
    /// Seconds between 1601-01-01 (FILETIME epoch) and 1970-01-01.
    const FILETIME_UNIX_OFFSET_SECONDS: i64 = 11_644_473_600;
    const FILETIME_TICKS_PER_SECOND: i64 = 10_000_000;
    const LEGACY_PATH_CHARS: usize = 260;

    pub(super) fn move_to_trash(
        path: &Path,
        deleted_after_unix: i64,
    ) -> Result<Option<PathBuf>, TrashError> {
        trash::delete(path).map_err(|error| TrashError::Failed(error.to_string()))?;
        Ok(locate_recycled_item(path, deleted_after_unix))
    }

    /// The shell restore removes the `$I` record; a rename-based restore must
    /// do the same so the Recycle Bin view does not list a phantom entry.
    pub(super) fn after_restore(entry: &TrashedEntry) {
        if let Some(metadata) = entry.trashed_path.as_deref().and_then(metadata_sibling) {
            let _ = std::fs::remove_file(metadata);
        }
    }

    fn locate_recycled_item(original: &Path, deleted_after_unix: i64) -> Option<PathBuf> {
        let volume_root = volume_root(original)?;
        let recycle_bin = volume_root.join(RECYCLE_BIN_DIRECTORY);
        let not_before = UNIX_EPOCH + Duration::from_secs(deleted_after_unix.max(0) as u64);
        let mut best: Option<(i64, PathBuf)> = None;
        // Other users' SID folders are not readable and are skipped.
        for user_bin in std::fs::read_dir(recycle_bin).ok()?.flatten() {
            let Ok(entries) = std::fs::read_dir(user_bin.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(suffix) = name
                    .to_str()
                    .and_then(|name| name.strip_prefix(METADATA_PREFIX))
                else {
                    continue;
                };
                let recent = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|modified| modified >= not_before);
                if !recent {
                    continue;
                }
                let Some(record) = std::fs::read(entry.path())
                    .ok()
                    .and_then(|bytes| parse_metadata_record(&bytes))
                else {
                    continue;
                };
                if record.deleted_at_unix < deleted_after_unix
                    || !super::super::paths::same_path(&record.original_path, original)
                {
                    continue;
                }
                if best
                    .as_ref()
                    .is_none_or(|(deleted_at, _)| record.deleted_at_unix > *deleted_at)
                {
                    let content = user_bin.path().join(format!("{CONTENT_PREFIX}{suffix}"));
                    best = Some((record.deleted_at_unix, content));
                }
            }
        }
        best.map(|(_, content)| content)
    }

    fn volume_root(path: &Path) -> Option<PathBuf> {
        let mut components = path.components();
        match (components.next()?, components.next()?) {
            (Component::Prefix(prefix), Component::RootDir) => {
                let mut root = PathBuf::from(prefix.as_os_str());
                root.push(std::path::MAIN_SEPARATOR_STR);
                Some(root)
            }
            _ => None,
        }
    }

    fn metadata_sibling(content: &Path) -> Option<PathBuf> {
        let name = content.file_name()?.to_str()?;
        let suffix = name.strip_prefix(CONTENT_PREFIX)?;
        Some(content.with_file_name(format!("{METADATA_PREFIX}{suffix}")))
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct MetadataRecord {
        pub original_path: PathBuf,
        pub deleted_at_unix: i64,
    }

    /// Parse a Recycle Bin `$I` record: version (i64), size (u64), deletion
    /// FILETIME (u64), then the original path as UTF-16 — length-prefixed in
    /// version 2 (Windows 10+) or a fixed 260-character buffer in version 1.
    pub(super) fn parse_metadata_record(bytes: &[u8]) -> Option<MetadataRecord> {
        let read_u64 = |offset: usize| -> Option<u64> {
            Some(u64::from_le_bytes(
                bytes.get(offset..offset + 8)?.try_into().ok()?,
            ))
        };
        let version = read_u64(0)?;
        let filetime = read_u64(16)? as i64;
        let (path_offset, path_chars) = match version {
            1 => (24, LEGACY_PATH_CHARS),
            2 => {
                let chars = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?) as usize;
                (28, chars)
            }
            _ => return None,
        };
        let raw = bytes.get(path_offset..path_offset + path_chars * 2)?;
        let units = raw
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect::<Vec<_>>();
        Some(MetadataRecord {
            original_path: PathBuf::from(String::from_utf16(&units).ok()?),
            deleted_at_unix: filetime / FILETIME_TICKS_PER_SECOND - FILETIME_UNIX_OFFSET_SECONDS,
        })
    }

    #[cfg(test)]
    pub(super) fn encode_v2_record(path: &str, deleted_at_unix: i64) -> Vec<u8> {
        let units = path.encode_utf16().chain([0]).collect::<Vec<_>>();
        let filetime = (deleted_at_unix + FILETIME_UNIX_OFFSET_SECONDS) * FILETIME_TICKS_PER_SECOND;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        bytes.extend_from_slice(&42_u64.to_le_bytes());
        bytes.extend_from_slice(&(filetime as u64).to_le_bytes());
        bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[cfg(test)]
    pub(super) fn metadata_sibling_for_test(content: &Path) -> Option<PathBuf> {
        metadata_sibling(content)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::{TrashError, TrashedEntry};
    use std::path::{Path, PathBuf};

    pub(super) fn move_to_trash(
        path: &Path,
        _deleted_after_unix: i64,
    ) -> Result<Option<PathBuf>, TrashError> {
        trash::delete(path).map_err(|error| TrashError::Failed(error.to_string()))?;
        Ok(None)
    }

    pub(super) fn after_restore(_entry: &TrashedEntry) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    fn item(parent: &Path, name: &str, time_deleted: i64) -> trash::TrashItem {
        trash::TrashItem {
            id: format!("{}-{time_deleted}", name).into(),
            name: name.into(),
            original_parent: parent.to_path_buf(),
            time_deleted,
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn selects_the_newest_matching_item_inside_the_deletion_window() {
        let parent = std::env::temp_dir().join("gold-band-trash-select");
        let original = parent.join("notes.md");
        let selected = select_trash_item(
            vec![
                item(&parent, "notes.md", 90),
                item(&parent, "notes.md", 120),
                item(&parent, "notes.md", 110),
                item(&parent, "other.md", 130),
            ],
            &original,
            100,
        )
        .unwrap();
        assert_eq!(selected.time_deleted, 120);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ignores_older_deletions_of_the_same_path() {
        let parent = std::env::temp_dir().join("gold-band-trash-select");
        let original = parent.join("notes.md");
        assert!(select_trash_item(vec![item(&parent, "notes.md", 90)], &original, 100).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn parses_windows_recycle_bin_metadata_records() {
        let bytes = platform::encode_v2_record(r"D:\repo\notes.md", 1_700_000_000);
        let record = platform::parse_metadata_record(&bytes).unwrap();
        assert_eq!(record.original_path, PathBuf::from(r"D:\repo\notes.md"));
        assert_eq!(record.deleted_at_unix, 1_700_000_000);
        assert!(platform::parse_metadata_record(&bytes[..20]).is_none());
        assert_eq!(
            platform::metadata_sibling_for_test(Path::new(r"D:\$Recycle.Bin\S-1\$RAB12.md")),
            Some(PathBuf::from(r"D:\$Recycle.Bin\S-1\$IAB12.md"))
        );
    }

    #[test]
    fn restore_refuses_to_overwrite_an_occupied_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("occupied.txt");
        std::fs::write(&original, "new").unwrap();
        let entry = TrashedEntry {
            original_path: original,
            deleted_after_unix: now_unix(),
            trashed_path: Some(dir.path().join("missing")),
        };
        assert_eq!(restore_from_trash(&entry), Err(TrashError::Collision));
    }

    #[test]
    fn restore_reports_an_item_that_left_the_trash_as_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let entry = TrashedEntry {
            original_path: dir.path().join("restored.txt"),
            deleted_after_unix: now_unix(),
            trashed_path: Some(dir.path().join("emptied")),
        };
        assert!(matches!(
            restore_from_trash(&entry),
            Err(TrashError::Unavailable(_))
        ));
    }

    /// Exercises the real system trash. Runs on CI desktop runners (including
    /// macOS, where it is the only automated evidence for the NSFileManager
    /// path); ignored by default so local runs never touch the user's trash.
    #[test]
    #[ignore = "touches the system trash; run with --ignored on CI"]
    fn moves_an_entry_to_the_system_trash_and_restores_it() {
        let dir = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let folder = root.join(format!("gold-band-trash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("inner.txt"), "inner").unwrap();

        let entry = move_to_trash(&folder).unwrap();
        assert!(!folder.exists());
        if cfg!(any(windows, target_os = "macos")) {
            assert!(
                entry.trashed_path.is_some(),
                "trash location must be captured"
            );
        }
        restore_from_trash(&entry).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("inner.txt")).unwrap(),
            "inner"
        );
    }
}
