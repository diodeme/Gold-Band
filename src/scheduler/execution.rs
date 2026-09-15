use super::ScheduledTaskContentSnapshot;
use chrono::{DateTime, Utc};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

pub const SCHEDULED_INSTRUCTION_SUMMARY_MAX_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledAutomaticTriggerContext {
    pub scheduled_at: DateTime<Utc>,
    pub schedule_summary: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledExecutionSnapshot {
    pub accepted_at: DateTime<Utc>,
    pub definition_revision: i64,
    pub content_fingerprint: String,
    pub content: ScheduledTaskContentSnapshot,
    pub instruction_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automatic: Option<ScheduledAutomaticTriggerContext>,
}

pub fn instruction_summary(markdown: &str, max_chars: usize) -> String {
    let mut text = String::new();
    let mut in_block = false;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item | Tag::CodeBlock(_))
                if !in_block =>
            {
                in_block = true;
            }
            Event::Text(value) | Event::Code(value) if in_block => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak if in_block => text.push(' '),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) if in_block && !text.trim().is_empty() => break,
            _ => {}
        }
    }

    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ScheduledExecutionSnapshot, instruction_summary};
    use crate::scheduler::ScheduledTaskContentSnapshot;
    use chrono::{TimeZone, Utc};

    #[test]
    fn instruction_summary_uses_first_non_empty_markdown_block() {
        assert_eq!(
            instruction_summary("\n# 每日代码检查\n\n检查主分支测试。", 120),
            "每日代码检查"
        );
    }

    #[test]
    fn instruction_summary_collapses_whitespace_and_has_a_stable_limit() {
        assert_eq!(instruction_summary("- alpha   beta", 8), "alpha be");
    }

    #[test]
    fn instruction_summary_keeps_the_complete_first_markdown_block() {
        assert_eq!(
            instruction_summary("first line\nsecond line\n\nignored block", 120),
            "first line second line"
        );
    }

    #[test]
    fn accepted_snapshot_keeps_full_content_and_presentation_summary_separate() {
        let instruction = "# 每日代码检查\n\n检查主分支测试。";
        let snapshot = ScheduledExecutionSnapshot {
            accepted_at: Utc.with_ymd_and_hms(2026, 8, 25, 8, 0, 0).unwrap(),
            definition_revision: 7,
            content_fingerprint: "fingerprint-1".to_string(),
            content: ScheduledTaskContentSnapshot::direct(
                instruction,
                Vec::<String>::new(),
                "workspace-1",
                "agent-1",
            ),
            instruction_summary: instruction_summary(instruction, 120),
            automatic: None,
        };

        assert_eq!(snapshot.instruction_summary, "每日代码检查");
        assert_eq!(snapshot.content.instruction, instruction);
    }
}
