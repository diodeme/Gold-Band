//! User prompt quotes: text excerpts frozen when the user quotes them, sent to
//! the Agent ahead of the user's own input together with where they came from.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::DesktopLanguage;
use crate::prompts::{RUNTIME_USER_QUOTE, prompt_by_language, render as render_template};

/// Shared character budget for selection quotes (Agent messages, files, diff fragments).
pub const MAX_USER_PROMPT_QUOTE_CHARS: usize = 12_000;
/// Shared UTF-8 byte budget for whole-file diff quotes in one prompt.
pub const MAX_USER_PROMPT_DIFF_QUOTE_BYTES: usize = 64_000;
pub const MAX_USER_PROMPT_QUOTES: usize = 64;
pub const MAX_USER_PROMPT_QUOTE_ID_BYTES: usize = 128;
pub const MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES: usize = 512;
pub const MAX_USER_PROMPT_QUOTE_SOURCE_PATH_BYTES: usize = 1_024;
pub const MAX_USER_PROMPT_QUOTE_REVISION_BYTES: usize = 128;

const MIN_CODE_FENCE_LEN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptQuote {
    pub id: String,
    pub text: String,
    pub source: UserPromptQuoteSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UserPromptQuoteSource {
    AgentMessage {
        message_key: String,
    },
    File {
        /// Workspace-relative path, run-directory path, or attachment name.
        label: String,
        start_line: u32,
        end_line: u32,
    },
    Diff {
        path: String,
        origin: DiffQuoteOrigin,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
        scope: DiffQuoteScope,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffQuoteOrigin {
    WorkingTreeStaged,
    WorkingTreeUnstaged,
    Commit,
    PullRequest,
    AgentTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffQuoteScope {
    Selection,
    File,
}

impl UserPromptQuote {
    /// Whole-file diffs use the byte budget; every other quote uses the character budget.
    pub fn is_whole_file_diff(&self) -> bool {
        matches!(
            self.source,
            UserPromptQuoteSource::Diff {
                scope: DiffQuoteScope::File,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserPromptQuoteError {
    CountExceeded,
    Invalid,
    MetadataTooLong,
    LimitExceeded,
    DiffLimitExceeded,
}

impl UserPromptQuoteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CountExceeded => "conversation.prompt-quote-count-exceeded",
            Self::Invalid => "conversation.prompt-quote-invalid",
            Self::MetadataTooLong => "conversation.prompt-quote-metadata-too-long",
            Self::LimitExceeded => "conversation.prompt-quote-limit-exceeded",
            Self::DiffLimitExceeded => "conversation.prompt-quote-diff-limit-exceeded",
        }
    }

    pub fn params(&self) -> Value {
        match self {
            Self::CountExceeded => json!({ "maxQuotes": MAX_USER_PROMPT_QUOTES }),
            Self::Invalid => json!({}),
            Self::MetadataTooLong => json!({
                "maxIdBytes": MAX_USER_PROMPT_QUOTE_ID_BYTES,
                "maxSourceKeyBytes": MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES,
                "maxPathBytes": MAX_USER_PROMPT_QUOTE_SOURCE_PATH_BYTES,
            }),
            Self::LimitExceeded => json!({ "maxChars": MAX_USER_PROMPT_QUOTE_CHARS }),
            Self::DiffLimitExceeded => json!({ "maxBytes": MAX_USER_PROMPT_DIFF_QUOTE_BYTES }),
        }
    }
}

pub fn validate_user_prompt_quotes(quotes: &[UserPromptQuote]) -> Result<(), UserPromptQuoteError> {
    if quotes.len() > MAX_USER_PROMPT_QUOTES {
        return Err(UserPromptQuoteError::CountExceeded);
    }
    let mut ids = std::collections::HashSet::with_capacity(quotes.len());
    let mut chars = 0usize;
    let mut diff_bytes = 0usize;
    for quote in quotes {
        if quote.id.trim().is_empty() || quote.text.trim().is_empty() || !ids.insert(&quote.id) {
            return Err(UserPromptQuoteError::Invalid);
        }
        validate_quote_source(&quote.source)?;
        if quote.id.len() > MAX_USER_PROMPT_QUOTE_ID_BYTES {
            return Err(UserPromptQuoteError::MetadataTooLong);
        }
        if quote.is_whole_file_diff() {
            diff_bytes += quote.text.len();
            if diff_bytes > MAX_USER_PROMPT_DIFF_QUOTE_BYTES {
                return Err(UserPromptQuoteError::DiffLimitExceeded);
            }
        } else {
            let remaining = MAX_USER_PROMPT_QUOTE_CHARS.saturating_sub(chars);
            chars += quote.text.chars().take(remaining + 1).count();
            if chars > MAX_USER_PROMPT_QUOTE_CHARS {
                return Err(UserPromptQuoteError::LimitExceeded);
            }
        }
    }
    Ok(())
}

fn validate_quote_source(source: &UserPromptQuoteSource) -> Result<(), UserPromptQuoteError> {
    match source {
        UserPromptQuoteSource::AgentMessage { message_key } => {
            require_text(message_key, MAX_USER_PROMPT_QUOTE_SOURCE_KEY_BYTES)
        }
        UserPromptQuoteSource::File {
            label,
            start_line,
            end_line,
        } => {
            if *start_line == 0 || end_line < start_line {
                return Err(UserPromptQuoteError::Invalid);
            }
            require_text(label, MAX_USER_PROMPT_QUOTE_SOURCE_PATH_BYTES)
        }
        UserPromptQuoteSource::Diff { path, revision, .. } => {
            require_text(path, MAX_USER_PROMPT_QUOTE_SOURCE_PATH_BYTES)?;
            match revision {
                Some(revision) => require_text(revision, MAX_USER_PROMPT_QUOTE_REVISION_BYTES),
                None => Ok(()),
            }
        }
    }
}

fn require_text(value: &str, max_bytes: usize) -> Result<(), UserPromptQuoteError> {
    if value.trim().is_empty() {
        return Err(UserPromptQuoteError::Invalid);
    }
    if value.len() > max_bytes {
        return Err(UserPromptQuoteError::MetadataTooLong);
    }
    Ok(())
}

/// The user turn as the Agent sees it: rendered quotes first, then the user's input.
pub fn conversation_prompt_text(
    display_text: &str,
    quotes: &[UserPromptQuote],
    language: DesktopLanguage,
) -> String {
    let display_text = display_text.trim();
    if quotes.is_empty() {
        return display_text.to_string();
    }
    let template = prompt_by_language(language, RUNTIME_USER_QUOTE);
    let mut blocks = quotes
        .iter()
        .map(|quote| {
            render_template(template, json!({ "quote": quote_template_context(quote) }))
                .expect("bundled user quote prompt renders")
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>();
    if !display_text.is_empty() {
        blocks.push(display_text.to_string());
    }
    blocks.join("\n\n")
}

fn quote_template_context(quote: &UserPromptQuote) -> Value {
    let fence = code_fence(&quote.text);
    match &quote.source {
        UserPromptQuoteSource::AgentMessage { .. } => json!({
            "kind": "agentMessage",
            "lines": quote.text.lines().collect::<Vec<_>>(),
        }),
        UserPromptQuoteSource::File {
            label,
            start_line,
            end_line,
        } => json!({
            "kind": "file",
            "label": label,
            "start_line": start_line,
            "end_line": end_line,
            "fence": fence,
            "info": Path::new(label)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default(),
            "text": quote.text,
        }),
        UserPromptQuoteSource::Diff {
            path,
            origin,
            revision,
            scope,
        } => json!({
            "kind": "diff",
            "path": path,
            "origin": origin,
            "revision": revision,
            "scope": scope,
            "fence": fence,
            "text": quote.text,
        }),
    }
}

/// A backtick fence longer than any backtick run inside the quoted text.
fn code_fence(text: &str) -> String {
    let longest_run = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    "`".repeat((longest_run + 1).max(MIN_CODE_FENCE_LEN))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(text: &str, source: UserPromptQuoteSource) -> UserPromptQuote {
        UserPromptQuote {
            id: format!("quote-{}", text.len()),
            text: text.to_string(),
            source,
        }
    }

    fn agent(text: &str) -> UserPromptQuote {
        quote(
            text,
            UserPromptQuoteSource::AgentMessage {
                message_key: "message-1".to_string(),
            },
        )
    }

    fn whole_diff(id: &str, text: String) -> UserPromptQuote {
        UserPromptQuote {
            id: id.to_string(),
            text,
            source: UserPromptQuoteSource::Diff {
                path: "src/lib.rs".to_string(),
                origin: DiffQuoteOrigin::WorkingTreeUnstaged,
                revision: None,
                scope: DiffQuoteScope::File,
            },
        }
    }

    #[test]
    fn agent_message_quotes_render_as_blockquotes_before_the_input() {
        let text = conversation_prompt_text(
            "解释一下",
            &[agent("第一行\n第二行")],
            DesktopLanguage::ZhCn,
        );
        assert_eq!(text, "> 第一行\n> 第二行\n\n解释一下");
    }

    #[test]
    fn file_quotes_carry_the_path_line_range_and_a_fence_longer_than_the_content() {
        let text = conversation_prompt_text(
            "",
            &[quote(
                "let s = \"```\";",
                UserPromptQuoteSource::File {
                    label: "src/main.rs".to_string(),
                    start_line: 10,
                    end_line: 12,
                },
            )],
            DesktopLanguage::En,
        );
        assert!(text.contains("`src/main.rs`"), "{text}");
        assert!(text.contains("10-12"), "{text}");
        assert!(text.contains("\n````rs\nlet s = \"```\";\n````"), "{text}");
    }

    #[test]
    fn diff_quotes_name_their_origin_and_use_a_diff_fence() {
        let text = conversation_prompt_text(
            "review",
            &[quote(
                "@@ -1 +1 @@\n-a\n+b",
                UserPromptQuoteSource::Diff {
                    path: "src/lib.rs".to_string(),
                    origin: DiffQuoteOrigin::Commit,
                    revision: Some("abc1234".to_string()),
                    scope: DiffQuoteScope::Selection,
                },
            )],
            DesktopLanguage::En,
        );
        assert!(text.contains("`src/lib.rs`"), "{text}");
        assert!(text.contains("abc1234"), "{text}");
        assert!(text.contains("```diff\n@@ -1 +1 @@\n-a\n+b\n```"), "{text}");
        assert!(text.ends_with("\n\nreview"), "{text}");
    }

    #[test]
    fn every_language_renders_every_quote_kind() {
        let quotes = [
            agent("a"),
            quote(
                "b",
                UserPromptQuoteSource::File {
                    label: "x".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            ),
            whole_diff("d", "+c".to_string()),
        ];
        for language in [
            DesktopLanguage::ZhCn,
            DesktopLanguage::ZhTw,
            DesktopLanguage::En,
            DesktopLanguage::JaJp,
            DesktopLanguage::KoKr,
            DesktopLanguage::PtBr,
            DesktopLanguage::Es,
        ] {
            let text = conversation_prompt_text("go", &quotes, language);
            assert!(text.starts_with("> a"), "{language:?}: {text}");
            assert!(text.contains("`src/lib.rs`"), "{language:?}: {text}");
        }
    }

    #[test]
    fn selection_quotes_and_whole_file_diffs_have_separate_budgets() {
        let near_char_limit = agent(&"字".repeat(MAX_USER_PROMPT_QUOTE_CHARS));
        let big_diff = whole_diff("diff", "+".repeat(MAX_USER_PROMPT_DIFF_QUOTE_BYTES));
        assert_eq!(
            validate_user_prompt_quotes(&[near_char_limit.clone(), big_diff.clone()]),
            Ok(())
        );

        let second_diff = whole_diff("diff-2", "+".to_string());
        assert_eq!(
            validate_user_prompt_quotes(&[big_diff, second_diff]),
            Err(UserPromptQuoteError::DiffLimitExceeded)
        );
        assert_eq!(
            validate_user_prompt_quotes(&[near_char_limit, agent("x")]),
            Err(UserPromptQuoteError::LimitExceeded)
        );
    }

    #[test]
    fn invalid_sources_and_oversized_metadata_are_rejected() {
        let bad_range = quote(
            "a",
            UserPromptQuoteSource::File {
                label: "a.rs".to_string(),
                start_line: 5,
                end_line: 4,
            },
        );
        assert_eq!(
            validate_user_prompt_quotes(&[bad_range]),
            Err(UserPromptQuoteError::Invalid)
        );
        let long_path = quote(
            "a",
            UserPromptQuoteSource::File {
                label: "a".repeat(MAX_USER_PROMPT_QUOTE_SOURCE_PATH_BYTES + 1),
                start_line: 1,
                end_line: 1,
            },
        );
        assert_eq!(
            validate_user_prompt_quotes(&[long_path]),
            Err(UserPromptQuoteError::MetadataTooLong)
        );
        let duplicate = agent("a");
        assert_eq!(
            validate_user_prompt_quotes(&[duplicate.clone(), duplicate]),
            Err(UserPromptQuoteError::Invalid)
        );
    }
}
