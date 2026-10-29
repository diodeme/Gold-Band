//! Per-conversation composer drafts: prompt payloads the user saved for later.
//!
//! A draft carries no prompt identity and no dispatch state, so neither the
//! prompt queue nor any runtime path can send one. The only way out of the box
//! is an explicit user take (back into the composer) or delete. Drafts belong
//! to the conversation task rather than one attempt, so they survive new runs
//! and sessions of the same conversation.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{
    ConversationPromptInput, PromptWorkspaceFileRef, UserPromptQuote, UserPromptRole,
    conversation_prompt_has_payload,
};
use crate::storage::{read_json, with_file_lock, write_json};

pub const COMPOSER_DRAFTS_FILE_NAME: &str = "composer-drafts.json";
pub const MAX_COMPOSER_DRAFTS: usize = 20;
const COMPOSER_DRAFTS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerDraft {
    pub id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quotes: Vec<UserPromptQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<UserPromptRole>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_files: Vec<PromptWorkspaceFileRef>,
    pub created_at: String,
}

/// Drafts are kept newest first; `revision` advances on every mutation so the
/// UI can converge on the latest box with last-writer-wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerDraftBox {
    pub version: u32,
    pub revision: u64,
    #[serde(default)]
    pub items: Vec<ComposerDraft>,
}

impl Default for ComposerDraftBox {
    fn default() -> Self {
        Self {
            version: COMPOSER_DRAFTS_VERSION,
            revision: 0,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComposerDraftError {
    #[error("composer draft content is empty")]
    Empty,
    #[error("composer draft box is full")]
    Full,
    #[error("composer draft was not found")]
    NotFound,
    #[error("composer draft storage is unavailable")]
    Storage,
}

pub fn drafts_path(task_dir: &Utf8Path) -> Utf8PathBuf {
    task_dir.join(COMPOSER_DRAFTS_FILE_NAME)
}

/// Reads the box without creating storage: a conversation that never saved a
/// draft keeps no draft file.
pub fn load_composer_drafts(task_dir: &Utf8Path) -> Result<ComposerDraftBox, ComposerDraftError> {
    let path = drafts_path(task_dir);
    with_file_lock(&path, || load_unlocked(&path)).map_err(|_| ComposerDraftError::Storage)
}

pub fn save_composer_draft(
    task_dir: &Utf8Path,
    input: ConversationPromptInput,
    attachment_paths: Vec<String>,
) -> Result<(ComposerDraft, ComposerDraftBox), ComposerDraftError> {
    if !conversation_prompt_has_payload(
        &input.display_text,
        attachment_paths.len(),
        input.role.as_ref(),
        input.workspace_files.len(),
    ) {
        return Err(ComposerDraftError::Empty);
    }
    mutate(task_dir, |drafts| {
        if drafts.items.len() >= MAX_COMPOSER_DRAFTS {
            return Err(ComposerDraftError::Full);
        }
        let draft = ComposerDraft {
            id: format!("draft-{}", Uuid::new_v4().simple()),
            content: input.display_text,
            quotes: input.quotes,
            role: input.role,
            attachment_paths,
            workspace_files: input.workspace_files,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        drafts.items.insert(0, draft.clone());
        Ok(Some(draft))
    })
    .map(|(draft, drafts)| (draft.expect("saved draft"), drafts))
}

/// Removes one draft and hands its payload back to the composer.
pub fn take_composer_draft(
    task_dir: &Utf8Path,
    draft_id: &str,
) -> Result<(ComposerDraft, ComposerDraftBox), ComposerDraftError> {
    mutate(task_dir, |drafts| {
        let index = drafts
            .items
            .iter()
            .position(|draft| draft.id == draft_id)
            .ok_or(ComposerDraftError::NotFound)?;
        Ok(Some(drafts.items.remove(index)))
    })
    .map(|(draft, drafts)| (draft.expect("taken draft"), drafts))
}

/// Idempotent: deleting a draft that is already gone returns the current box.
pub fn delete_composer_draft(
    task_dir: &Utf8Path,
    draft_id: &str,
) -> Result<ComposerDraftBox, ComposerDraftError> {
    let path = drafts_path(task_dir);
    with_file_lock(&path, || {
        let mut drafts = load_unlocked(&path)?;
        let before = drafts.items.len();
        drafts.items.retain(|draft| draft.id != draft_id);
        if drafts.items.len() != before {
            persist_unlocked(&path, &mut drafts)?;
        }
        Ok(Ok(drafts))
    })
    .map_err(|_| ComposerDraftError::Storage)?
}

fn mutate<T>(
    task_dir: &Utf8Path,
    operation: impl FnOnce(&mut ComposerDraftBox) -> Result<Option<T>, ComposerDraftError>,
) -> Result<(Option<T>, ComposerDraftBox), ComposerDraftError> {
    let path = drafts_path(task_dir);
    with_file_lock(&path, || {
        let mut drafts = load_unlocked(&path)?;
        let value = match operation(&mut drafts) {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
        persist_unlocked(&path, &mut drafts)?;
        Ok(Ok((value, drafts)))
    })
    .map_err(|_| ComposerDraftError::Storage)?
}

fn load_unlocked(path: &Utf8Path) -> anyhow::Result<ComposerDraftBox> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(ComposerDraftBox::default())
    }
}

fn persist_unlocked(path: &Utf8Path, drafts: &mut ComposerDraftBox) -> anyhow::Result<()> {
    drafts.version = COMPOSER_DRAFTS_VERSION;
    drafts.revision = drafts.revision.saturating_add(1);
    write_json(path, drafts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_dir(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().join("task-001")).unwrap()
    }

    fn input(text: &str) -> ConversationPromptInput {
        ConversationPromptInput::from(text.to_string())
    }

    #[test]
    fn reading_an_unused_box_creates_no_storage() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);

        assert!(load_composer_drafts(&dir).unwrap().items.is_empty());
        assert!(
            delete_composer_draft(&dir, "draft-missing")
                .unwrap()
                .items
                .is_empty()
        );
        assert!(!drafts_path(&dir).exists());
    }

    #[test]
    fn saved_drafts_keep_the_full_payload_newest_first() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        save_composer_draft(&dir, input("first"), Vec::new()).unwrap();
        let (second, drafts) =
            save_composer_draft(&dir, input("second"), vec!["C:/notes.md".to_string()]).unwrap();

        assert_eq!(drafts.revision, 2);
        assert_eq!(drafts.items[0], second);
        assert_eq!(drafts.items[0].attachment_paths, vec!["C:/notes.md"]);
        assert_eq!(drafts.items[1].content, "first");
        assert_eq!(load_composer_drafts(&dir).unwrap(), drafts);
    }

    #[test]
    fn empty_payloads_and_a_full_box_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);

        assert_eq!(
            save_composer_draft(&dir, input("   "), Vec::new()),
            Err(ComposerDraftError::Empty)
        );
        for index in 0..MAX_COMPOSER_DRAFTS {
            save_composer_draft(&dir, input(&format!("draft {index}")), Vec::new()).unwrap();
        }
        assert_eq!(
            save_composer_draft(&dir, input("overflow"), Vec::new()),
            Err(ComposerDraftError::Full)
        );
        assert_eq!(
            load_composer_drafts(&dir).unwrap().items.len(),
            MAX_COMPOSER_DRAFTS
        );
    }

    #[test]
    fn taking_a_draft_returns_it_once() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let (draft, _) = save_composer_draft(&dir, input("take me"), Vec::new()).unwrap();

        let (taken, drafts) = take_composer_draft(&dir, &draft.id).unwrap();

        assert_eq!(taken, draft);
        assert!(drafts.items.is_empty());
        assert_eq!(
            take_composer_draft(&dir, &draft.id),
            Err(ComposerDraftError::NotFound)
        );
    }

    #[test]
    fn deleting_is_idempotent_and_only_advances_revision_on_change() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let (keep, _) = save_composer_draft(&dir, input("keep"), Vec::new()).unwrap();
        let (gone, _) = save_composer_draft(&dir, input("gone"), Vec::new()).unwrap();

        let drafts = delete_composer_draft(&dir, &gone.id).unwrap();
        let again = delete_composer_draft(&dir, &gone.id).unwrap();

        assert_eq!(drafts.items, vec![keep]);
        assert_eq!(again.revision, drafts.revision);
    }
}
