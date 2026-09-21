//! multica issue 完成输出（completion_output）提取（issue 完成输出传递特性，码灵侧设计方案 §5）。
//!
//! 写路径协议：agent 最终回复末尾附加 info 为 `completion-output` 的代码围栏块；run 成功收尾时
//! [`extract_completion_output`] 从投影出的最终 assistant 回复中取**最后一个**匹配块，随
//! `update_issue_status(done)` 一次 PUT 原子上送（status + completion_output 单请求，与 multica
//! 服务端单 PATCH 设计对齐）。提取失败一律 fail-open（None）——issue 照常 done，写作可选、不门控。

use gold_band::acp::events::{
    is_semantically_empty_agent_content, load_timeline_items, AcpAttemptPaths, AcpUiEvent,
};

/// 服务端同口径上限（multica 按 rune 计数，超长 400 会让整个 PUT 失败、issue 卡非 done）。
/// 客户端按 Unicode 标量截断，保证状态流转优先于内容完整。
/// 2026-09-21 服务端从 64_000 收紧为 16_000（改动清单 §1.2）。
pub(crate) const MAX_COMPLETION_OUTPUT_CHARS: usize = 16_000;

/// 从最终 assistant 回复提取交付说明块（设计方案 §5.1 规则，纯函数）。
///
/// - 找 info 串**恰为** `completion-output`（区分大小写）的代码围栏块；围栏未闭合时按 Markdown
///   语义视作延伸到文本末尾（对 agent 漏写闭合围栏鲁棒）；
/// - 任意围栏（含其他 info 串）一旦开栏即吞掉其内容——嵌在其他围栏内的 `completion-output`
///   行是围栏内容、不当成开栏（Markdown 嵌套语义）；
/// - 多个匹配块取最后一个（协议要求放末尾，取最后与指令一致且对「复述协议再产出真块」鲁棒）；
/// - 返回块内容（未 trim）；无匹配 → None。
pub(crate) fn extract_completion_output(reply: &str) -> Option<&str> {
    let mut capture: Option<(usize, usize)> = None; // (内容起始字节, 内容结束字节)
    let mut open: Option<OpenFence> = None;
    let mut line_start = 0usize;
    for line in reply.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        match &mut open {
            None => {
                if let Some((fence_len, info)) = opening_fence(bare) {
                    open = Some(OpenFence {
                        fence_len,
                        content_start: line_start + line.len(),
                        is_target: info == "completion-output",
                    });
                }
            }
            Some(fence) => {
                if is_closing_fence(bare, fence.fence_len) {
                    if fence.is_target {
                        capture = Some((fence.content_start, line_start));
                    }
                    open = None;
                }
            }
        }
        line_start += line.len();
    }
    // 围栏未闭合 → 按 Markdown 语义延伸到文本末尾（在最后的匹配块上优先）。
    if let Some(fence) = open.filter(|fence| fence.is_target) {
        capture = Some((fence.content_start, reply.len()));
    }
    capture.map(|(start, end)| &reply[start..end])
}

/// 开栏状态（任意 info 串都跟踪，防其他围栏内容里的 `completion-output` 行误判为开栏）。
struct OpenFence {
    fence_len: usize,
    content_start: usize,
    is_target: bool,
}

/// 行首 ``` 围栏（≥3 个反引号，允许 ≤3 空格缩进）。返回 (围栏长度, info 串)。
fn opening_fence(line: &str) -> Option<(usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let fence_len = rest.chars().take_while(|&c| c == '`').count();
    if fence_len < 3 {
        return None;
    }
    Some((fence_len, rest[fence_len..].trim()))
}

/// 闭合围栏：仅反引号（可带尾随空白），长度 ≥ 开栏长度。
fn is_closing_fence(line: &str, fence_len: usize) -> bool {
    let trimmed = line.trim();
    let len = trimmed.chars().filter(|&c| c == '`').count();
    len >= fence_len && len == trimmed.chars().count()
}

/// 提取 + trim + 16k 截断（设计方案 §5.1 组合入口，bridge Success 分支调用）。
///
/// trim 后为空 → None（空块不发送，服务端保持/清空由键缺席语义决定）。
pub(crate) fn completion_output_from_reply(reply: &str) -> Option<String> {
    let extracted = extract_completion_output(reply)?.trim();
    if extracted.is_empty() {
        return None;
    }
    Some(extracted.chars().take(MAX_COMPLETION_OUTPUT_CHARS).collect())
}

/// 投影 run 最终 attempt 的最终 assistant 回复文本（设计方案 §5.2 支撑改动 2）。
///
/// `acp.timeline.jsonl` 中同流 textDelta 已由 runtime 合并为单条目（`apply_streaming_delta`，
/// 内容累积），`load_timeline_items` 物化后按 seq 排序——取**最后一条**非占位 textDelta 的 content
/// 即最终 assistant 消息。读失败/无文本 → None（fail-open，调用方仍照常流转 issue）。
pub(crate) fn final_assistant_reply(attempt_dir: &str) -> Option<String> {
    let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.into());
    let items = match load_timeline_items(&paths.timeline) {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(
                timeline = %paths.timeline,
                %error,
                "multica completion-output: timeline read failed (fail-open)"
            );
            return None;
        }
    };
    items
        .iter()
        .filter(|item| is_assistant_text(item))
        .next_back()
        .and_then(|item| item.content.clone())
        .filter(|content| !content.trim().is_empty())
}

/// 最终 assistant 文本判据：textDelta 且非占位（复用 runtime 的语义空判据，保持与 UI 投影同源）。
fn is_assistant_text(item: &AcpUiEvent) -> bool {
    item.kind == "textDelta" && !is_semantically_empty_agent_content(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn extract_hits_completion_output_block() {
        let reply = "工作完成。\n\n```completion-output\n部署地址: http://x\n自测结论: 通过\n```\n";
        assert_eq!(
            extract_completion_output(reply),
            Some("部署地址: http://x\n自测结论: 通过\n")
        );
    }

    #[test]
    fn extract_ignores_other_info_strings_and_plain_fences() {
        let reply = "```json\n{\"a\":1}\n```\n```Completion-Output\n大小写敏感\n```\n普通说明";
        assert_eq!(extract_completion_output(reply), None);
    }

    #[test]
    fn extract_takes_last_block_when_agent_restates_protocol() {
        let reply = "协议要求：\n```completion-output\n<示例内容>\n```\n实际交付：\n```completion-output\n真实交付说明\n```\n";
        assert_eq!(extract_completion_output(reply), Some("真实交付说明\n"));
    }

    #[test]
    fn extract_accepts_unclosed_fence_at_eof() {
        let reply = "结论如下：\n```completion-output\n部署地址: http://y";
        assert_eq!(extract_completion_output(reply), Some("部署地址: http://y"));
    }

    #[test]
    fn extract_skips_completion_output_inside_unrelated_fence() {
        // 非 completion-output 围栏内的文本是围栏内容，不当成开栏
        let reply = "```text\n```completion-output\n```\n结尾";
        assert_eq!(extract_completion_output(reply), None);
    }

    #[test]
    fn completion_output_from_reply_trims_and_blank_is_none() {
        assert_eq!(
            completion_output_from_reply("```completion-output\n  \n```\n"),
            None
        );
        assert_eq!(
            completion_output_from_reply("前文\n```completion-output\n\n  说明  \n```\n"),
            Some("说明".to_string())
        );
    }

    #[test]
    fn completion_output_caps_at_16k_chars_by_unicode_scalar() {
        // 16_001 个 CJK 字符：按 rune 截断到 16_000（服务端同口径收紧上限，非 UTF-16/字节）
        let body = "门".repeat(16_001);
        let reply = format!("```completion-output\n{body}\n```\n");
        let output = completion_output_from_reply(&reply).expect("hit");
        assert_eq!(output.chars().count(), MAX_COMPLETION_OUTPUT_CHARS);
        assert!(output.chars().all(|c| c == '门'));
    }

    #[test]
    fn final_assistant_reply_reads_last_non_placeholder_text_delta() {
        let temp = tempfile::tempdir().expect("tempdir");
        let attempt_dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf())
            .expect("utf8 temp path");
        let paths = AcpAttemptPaths::from_attempt_dir(attempt_dir.clone());
        gold_band::acp::events::write_timeline_items(
            &paths.timeline,
            &[
                text_delta("user-echo", 1, "用户输入"),
                text_delta("assistant-1", 2, "中间说明"),
                text_delta("assistant-final", 3, "最终回复\n```completion-output\n交付\n```\n"),
            ],
        )
        .expect("write timeline");

        assert_eq!(
            final_assistant_reply(attempt_dir.as_str()),
            Some("最终回复\n```completion-output\n交付\n```\n".to_string())
        );
        assert_eq!(
            completion_output_from_reply(
                final_assistant_reply(attempt_dir.as_str()).as_deref().unwrap_or_default()
            ),
            Some("交付".to_string())
        );
    }

    #[test]
    fn final_assistant_reply_missing_dir_is_none() {
        let missing = Utf8PathBuf::from_path_buf(std::env::temp_dir().join("gold-band-handoff-missing"))
            .expect("utf8 temp path");
        assert_eq!(final_assistant_reply(missing.as_str()), None);
    }

    fn text_delta(id: &str, seq: u64, content: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq,
            timestamp: format!("{seq}Z"),
            kind: "textDelta".to_string(),
            session_id: None,
            content: Some(content.to_string()),
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: Some(seq),
            ended_seq: Some(seq),
            started_at: Some(format!("{seq}Z")),
            ended_at: Some(format!("{seq}Z")),
            timing: None,
            raw: None,
        }
    }
}
