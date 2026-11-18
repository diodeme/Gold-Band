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
use crate::provider::{
    MAX_USER_PROMPT_QUOTES, MAX_USER_PROMPT_ROLE_CONTENT_CHARS, MAX_USER_PROMPT_TEXT_CHARS,
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
    #[error("composer draft box is invalid")]
    Invalid,
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
    with_composer_draft_lock(&path, || load_unlocked(&path))
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

/// Reads a payload without consuming it. The caller deletes it only after the
/// composer accepts the payload, so a failed handoff leaves the saved copy intact.
pub fn get_composer_draft(
    task_dir: &Utf8Path,
    draft_id: &str,
) -> Result<(ComposerDraft, ComposerDraftBox), ComposerDraftError> {
    let drafts = load_composer_drafts(task_dir)?;
    let draft = drafts
        .items
        .iter()
        .find(|draft| draft.id == draft_id)
        .cloned()
        .ok_or(ComposerDraftError::NotFound)?;
    Ok((draft, drafts))
}

/// Idempotent: deleting a draft that is already gone returns the current box.
pub fn delete_composer_draft(
    task_dir: &Utf8Path,
    draft_id: &str,
) -> Result<ComposerDraftBox, ComposerDraftError> {
    let path = drafts_path(task_dir);
    with_composer_draft_lock(&path, || {
        let mut drafts = load_unlocked(&path)?;
        let before = drafts.items.len();
        drafts.items.retain(|draft| draft.id != draft_id);
        if drafts.items.len() != before {
            persist_unlocked(&path, &mut drafts)?;
        }
        Ok(drafts)
    })
}

fn with_composer_draft_lock<T>(
    path: &Utf8Path,
    operation: impl FnOnce() -> Result<T, ComposerDraftError>,
) -> Result<T, ComposerDraftError> {
    with_file_lock(path, || match operation() {
        Ok(value) => Ok(value),
        Err(error) => Err(anyhow::Error::new(error)),
    })
    .map_err(|error| {
        error
            .downcast::<ComposerDraftError>()
            .unwrap_or(ComposerDraftError::Storage)
    })
}

fn mutate<T>(
    task_dir: &Utf8Path,
    operation: impl FnOnce(&mut ComposerDraftBox) -> Result<Option<T>, ComposerDraftError>,
) -> Result<(Option<T>, ComposerDraftBox), ComposerDraftError> {
    let path = drafts_path(task_dir);
    with_composer_draft_lock(&path, || {
        let mut drafts = load_unlocked(&path)?;
        let value = match operation(&mut drafts) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        persist_unlocked(&path, &mut drafts)?;
        Ok((value, drafts))
    })
}

fn load_unlocked(path: &Utf8Path) -> Result<ComposerDraftBox, ComposerDraftError> {
    if path.exists() {
        let drafts: ComposerDraftBox = read_json(path).map_err(|_| ComposerDraftError::Storage)?;
        validate_invariants(&drafts)?;
        Ok(drafts)
    } else {
        Ok(ComposerDraftBox::default())
    }
}

fn validate_invariants(drafts: &ComposerDraftBox) -> Result<(), ComposerDraftError> {
    if drafts.version != COMPOSER_DRAFTS_VERSION
        || drafts.items.len() > MAX_COMPOSER_DRAFTS
        || drafts
            .items
            .iter()
            .map(|draft| draft.id.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
            != drafts.items.len()
    {
        return Err(ComposerDraftError::Invalid);
    }
    // Files written before the shared prompt bounds existed (or edited by
    // hand) must not turn the box into an unbounded JSON payload; the same
    // limits the save path enforces also hold on load.
    for draft in &drafts.items {
        if draft.content.chars().count() > MAX_USER_PROMPT_TEXT_CHARS
            || draft.quotes.len() > MAX_USER_PROMPT_QUOTES
            || draft.role.as_ref().is_some_and(|role| {
                role.content.chars().count() > MAX_USER_PROMPT_ROLE_CONTENT_CHARS
            })
        {
            return Err(ComposerDraftError::Invalid);
        }
    }
    Ok(())
}

fn persist_unlocked(
    path: &Utf8Path,
    drafts: &mut ComposerDraftBox,
) -> Result<(), ComposerDraftError> {
    drafts.version = COMPOSER_DRAFTS_VERSION;
    drafts.revision = drafts.revision.saturating_add(1);
    write_json(path, drafts).map_err(|_| ComposerDraftError::Storage)
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
    fn reading_a_draft_does_not_consume_it() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let (draft, _) = save_composer_draft(&dir, input("take me"), Vec::new()).unwrap();

        let (taken, drafts) = get_composer_draft(&dir, &draft.id).unwrap();

        assert_eq!(taken, draft);
        assert_eq!(drafts.items, vec![draft.clone()]);
        assert_eq!(get_composer_draft(&dir, &draft.id).unwrap().0, draft);
        delete_composer_draft(&dir, &draft.id).unwrap();
        assert_eq!(
            get_composer_draft(&dir, &draft.id),
            Err(ComposerDraftError::NotFound)
        );
    }

    #[test]
    fn reading_a_draft_keeps_the_saved_payload_until_explicit_delete() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let (draft, saved) =
            save_composer_draft(&dir, input("retain until received"), Vec::new()).unwrap();
        let (read, _) = get_composer_draft(&dir, &draft.id).unwrap();
        assert_eq!(read, draft);
        assert_eq!(load_composer_drafts(&dir).unwrap(), saved);
    }

    #[test]
    fn saved_payload_keeps_quotes_role_and_fixed_file_sources() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let sources = serde_json::from_value(serde_json::json!([
            {"kind":"git-file", "projectId":"p", "workspacePath":"D:/saved-worktree", "relativePath":"src/a.ts"},
            {"kind":"artifact", "locator":{"projectId":"p", "taskId":"t", "taskUuid":"uuid", "runId":"r", "roundId":"round", "nodeId":"n", "attemptId":"a", "branchId":"root"}, "name":"review.json"}
        ])).unwrap();
        let payload = ConversationPromptInput {
            display_text: "saved context".to_string(),
            quotes: vec![UserPromptQuote {
                id: "quote-1".to_string(),
                text: "original message".to_string(),
                source: crate::provider::UserPromptQuoteSource::AgentMessage {
                    message_key: "message-1".to_string(),
                },
            }],
            role: Some(UserPromptRole {
                profile_id: "dev".to_string(),
                name: "Developer".to_string(),
                content: "role snapshot".to_string(),
            }),
            workspace_files: sources,
        };
        let (draft, _) =
            save_composer_draft(&dir, payload.clone(), vec!["C:/image.png".to_string()]).unwrap();
        let (read, _) = get_composer_draft(&dir, &draft.id).unwrap();
        assert_eq!(read.quotes, payload.quotes);
        assert_eq!(read.role, payload.role);
        assert_eq!(read.workspace_files, payload.workspace_files);
        assert_eq!(read.attachment_paths, vec!["C:/image.png"]);
    }

    #[test]
    fn unreadable_storage_reports_an_error_and_is_not_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(drafts_path(&dir), b"{broken").unwrap();
        assert_eq!(load_composer_drafts(&dir), Err(ComposerDraftError::Storage));
        assert_eq!(
            save_composer_draft(&dir, input("new"), Vec::new()),
            Err(ComposerDraftError::Storage)
        );
        assert_eq!(std::fs::read(drafts_path(&dir)).unwrap(), b"{broken");
    }

    #[test]
    fn invalid_storage_invariants_are_rejected_without_rewrite() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            drafts_path(&dir),
            serde_json::to_string(&ComposerDraftBox {
                version: COMPOSER_DRAFTS_VERSION + 1,
                revision: 1,
                items: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();

        assert_eq!(load_composer_drafts(&dir), Err(ComposerDraftError::Invalid));
        assert_eq!(
            save_composer_draft(&dir, input("new"), Vec::new()),
            Err(ComposerDraftError::Invalid)
        );
        assert!(
            serde_json::from_str::<ComposerDraftBox>(
                &std::fs::read_to_string(drafts_path(&dir)).unwrap()
            )
            .unwrap()
            .version
                == COMPOSER_DRAFTS_VERSION + 1
        );
    }

    #[test]
    fn oversized_legacy_payloads_are_rejected_at_load_time() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        std::fs::create_dir_all(&dir).unwrap();
        let oversized = ComposerDraftBox {
            version: COMPOSER_DRAFTS_VERSION,
            revision: 1,
            items: vec![ComposerDraft {
                id: "draft-oversized".to_string(),
                content: "x".repeat(MAX_USER_PROMPT_TEXT_CHARS + 1),
                quotes: Vec::new(),
                role: None,
                attachment_paths: Vec::new(),
                workspace_files: Vec::new(),
                created_at: "2026-11-03T00:00:00Z".to_string(),
            }],
        };
        std::fs::write(
            drafts_path(&dir),
            serde_json::to_string(&oversized).unwrap(),
        )
        .unwrap();

        assert_eq!(load_composer_drafts(&dir), Err(ComposerDraftError::Invalid));
        // A file written before the bounds existed is never rewritten by a read.
        assert!(
            std::fs::read_to_string(drafts_path(&dir))
                .unwrap()
                .contains("draft-oversized")
        );
    }

    #[test]
    fn over_quoted_and_over_sized_role_drafts_are_rejected_at_load_time() {
        let temp = tempfile::tempdir().unwrap();
        let dir = task_dir(&temp);
        let mk = |quotes: Vec<UserPromptQuote>, role: Option<UserPromptRole>| ComposerDraftBox {
            version: COMPOSER_DRAFTS_VERSION,
            revision: 1,
            items: vec![ComposerDraft {
                id: "draft-boundary".to_string(),
                content: "text".to_string(),
                quotes,
                role,
                attachment_paths: Vec::new(),
                workspace_files: Vec::new(),
                created_at: "2026-11-03T00:00:00Z".to_string(),
            }],
        };
        let quote = UserPromptQuote {
            id: "quote-1".to_string(),
            text: "text".to_string(),
            source: crate::provider::UserPromptQuoteSource::AgentMessage {
                message_key: "message-1".to_string(),
            },
        };

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            drafts_path(&dir),
            serde_json::to_string(&mk(
                (0..=MAX_USER_PROMPT_QUOTES)
                    .map(|index| UserPromptQuote {
                        id: format!("quote-{index}"),
                        ..quote.clone()
                    })
                    .collect(),
                None,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(load_composer_drafts(&dir), Err(ComposerDraftError::Invalid));

        std::fs::write(
            drafts_path(&dir),
            serde_json::to_string(&mk(
                Vec::new(),
                Some(UserPromptRole {
                    profile_id: "dev".to_string(),
                    name: "Developer".to_string(),
                    content: "x".repeat(MAX_USER_PROMPT_ROLE_CONTENT_CHARS + 1),
                }),
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(load_composer_drafts(&dir), Err(ComposerDraftError::Invalid));
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
