//! `AcpUiEvent` → multica 执行记录消息 的纯投影。
//!
//! 观测点在 `acp_live_update_emitter`（IPC 压缩前），因此这里拿得到完整 `raw`
//! （`rawInput` / `rawOutput` / agent transcript 元数据）。live 文本事件是 **全量累积快照**
//! （同一个 stable item id 反复携带全量 content），所以文本按「已见过的字符数」做差量投影，
//! 而不是把每次快照整段重发。
//!
//! 投影只产出「消息」，seq 由 reporter 取批时统一分配，保证 seq 顺序 = 事件顺序。

use std::collections::{HashMap, VecDeque};

use gold_band::acp::events::{
    AcpUiEvent, agent_transcript_tool_output, extract_agent_transcript_relation,
};
use serde_json::{Value, json};

use super::model::{
    MAX_BATCH_BYTES, MAX_BATCH_MESSAGES, MAX_OUTBOX_MESSAGES, MAX_TEXT_CHARS,
    MAX_TOOL_OUTPUT_CHARS, MAX_TRACKED_STREAMS, MAX_TRACKED_TOOLS, REPORT_THINKING, TYPE_TEXT,
    TYPE_THINKING, TranscriptMessage, sanitize_text, truncate_chars,
};
use super::tool_input::normalize_tool_input;

/// 工具行已到达终态状态集合（与 Gold Band 前端 `isTerminalToolStatus` 对齐）。
const TERMINAL_TOOL_STATUSES: [&str; 8] = [
    "completed",
    "success",
    "succeeded",
    "failed",
    "error",
    "cancelled",
    "canceled",
    "rejected",
];

struct Segment {
    kind: &'static str,
    item_id: String,
    content: String,
}

struct ToolState {
    name: String,
    use_sent: bool,
    result_sent: bool,
}

pub(crate) struct Projector {
    outbox: VecDeque<TranscriptMessage>,
    segment: Option<Segment>,
    /// stable item id → 已投影的字符数（live 文本是全量快照）。
    streams: HashMap<String, usize>,
    stream_order: VecDeque<String>,
    /// tool_call_id → 工具行状态（`use` 只能发一次，`result` 只能发一次）。
    tools: HashMap<String, ToolState>,
    tool_order: VecDeque<String>,
    dropped_messages: u64,
    /// 初始需求原话（已 sanitize）。仅用于回声去重：fresh 直发模式下首拍回声与它逐字
    /// 相等且先于任何 agent 输出，跳过以免与 seq=1 显式上报双报。
    initial_user_text: Option<String>,
    /// 是否已观测到 agent 产出（文本/工具）。回声去重只在该标志为假时生效。
    agent_activity_seen: bool,
}

impl Projector {
    /// `initial_user_text`：任务的初始需求原话（用户的话，非组装后 prompt）。构造时即推入
    /// outbox 最前部 → drain 分配 seq=1，保证「任务开始执行前，初始输入是第一条消息」。
    pub(crate) fn new(initial_user_text: Option<&str>) -> Self {
        let mut projector = Self {
            outbox: VecDeque::new(),
            segment: None,
            streams: HashMap::new(),
            stream_order: VecDeque::new(),
            tools: HashMap::new(),
            tool_order: VecDeque::new(),
            dropped_messages: 0,
            initial_user_text: None,
            agent_activity_seen: false,
        };
        if let Some(text) = initial_user_text {
            let text = sanitize_text(text);
            if !text.trim().is_empty() {
                projector.initial_user_text = Some(text.clone());
                projector.push_user(&text);
            }
        }
        projector
    }

    /// 观测一个 live 事件。非上报类型由调用方过滤，这里再兜一层。
    pub(crate) fn observe(&mut self, event: &AcpUiEvent) {
        match event.kind.as_str() {
            "textDelta" => self.observe_text(event, TYPE_TEXT),
            "thoughtDelta" if REPORT_THINKING => self.observe_text(event, TYPE_THINKING),
            "toolCall" | "toolCallUpdate" => self.observe_tool(event),
            "userTextDelta" => self.observe_user_echo(event),
            _ => {}
        }
    }

    /// 运行期错误（失败/取消）作为一条 `error` 行进入消息流。
    pub(crate) fn push_error(&mut self, message: &str) {
        let content = truncate_chars(&sanitize_text(message), MAX_TEXT_CHARS);
        if content.trim().is_empty() {
            return;
        }
        self.close_segment();
        self.push(TranscriptMessage::error(content));
    }

    /// 用户中途插话：观测 `userTextDelta` 回声事件（`record_user_prompt_event` 合成的
    /// goldBandPrompt 事件），原样上报用户原话——与 agent 事件混入同一批队列，收到即投影，
    /// seq 分配器照常给号，「用户说了什么 → agent 如何响应」的因果顺序自动成立。
    ///
    /// 过滤（防把引擎内部 prompt 报成用户的话）：
    /// - `raw.source == "goldBandPrompt"`：只认码灵合成的用户 prompt 事件（agent 回显的
    ///   session/update 原生事件在 client 侧已抑制，这里双保险）；
    /// - `hiddenFromChat != true`：隐藏注入不上报；
    /// - `raw.originalUserText == true`：`prompt.display_text` 存在 = composer 里用户亲手
    ///   输入的文本；workflow/auto 节点驱动的 turn（display_text 缺失 → 组装 prompt）不报。
    fn observe_user_echo(&mut self, event: &AcpUiEvent) {
        let Some(raw) = event.raw.as_ref() else {
            return;
        };
        if raw.get("source").and_then(Value::as_str) != Some("goldBandPrompt") {
            return;
        }
        if raw.get("hiddenFromChat").and_then(Value::as_bool) == Some(true) {
            return;
        }
        if raw.get("originalUserText").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let Some(content) = event.content.as_deref() else {
            return;
        };
        let content = sanitize_text(content);
        if content.trim().is_empty() {
            return;
        }
        // 初始输入的回声去重：fresh 直发场景 seq=1 已显式上报初始需求，而首拍回声与它逐字
        // 相等且必然先于任何 agent 输出（回声在 prompt 发送给 agent 之前合成）——跳过以免
        // 双报。agent 一旦有产出，之后所有 originalUserText 回声都是真实中途插话，照常上报。
        if !self.agent_activity_seen && self.initial_user_text.as_deref() == Some(content.as_str())
        {
            return;
        }
        // 先收起进行中的 agent 文本段，用户消息排在其后，保持时间线因果顺序。
        self.close_segment();
        self.push_user(&content);
    }

    /// 用户原话入队：不改写、不摘要、不拼接；超 `MAX_TEXT_CHARS` 拆连续多条（seq 相邻）。
    fn push_user(&mut self, content: &str) {
        let mut rest = content;
        while !rest.is_empty() {
            let chunk = truncate_chars(rest, MAX_TEXT_CHARS);
            self.push(TranscriptMessage::user(chunk.clone()));
            rest = &rest[chunk.len()..];
        }
    }

    /// 收起当前文本分段。每次 flush 前调用，让文本按 flush 节奏成为独立消息，
    /// 而不是整个回合攒成一条巨块。
    pub(crate) fn close_segment(&mut self) {
        let Some(segment) = self.segment.take() else {
            return;
        };
        if segment.content.trim().is_empty() {
            return;
        }
        self.push(TranscriptMessage::body(segment.kind, segment.content));
    }

    /// 取走一批待上报消息并分配 seq（顺序 = 事件顺序）。
    ///
    /// 按条数与序列化字节数双上限切批：服务端没有限制，但单请求体过大会放大失败代价
    /// （批次原子，一失败整批丢）；剩下的留在缓冲里由下一拍继续发。
    pub(crate) fn drain(&mut self, next_seq: &mut u64) -> Vec<TranscriptMessage> {
        let mut batch: Vec<TranscriptMessage> = Vec::new();
        let mut bytes = 0usize;
        while batch.len() < MAX_BATCH_MESSAGES {
            let Some(front) = self.outbox.front() else {
                break;
            };
            let size = serde_json::to_vec(front)
                .map(|body| body.len())
                .unwrap_or(0);
            if !batch.is_empty() && bytes.saturating_add(size) > MAX_BATCH_BYTES {
                break;
            }
            let Some(mut message) = self.outbox.pop_front() else {
                break;
            };
            message.seq = *next_seq;
            *next_seq = next_seq.saturating_add(1);
            bytes = bytes.saturating_add(size);
            batch.push(message);
        }
        batch
    }

    pub(crate) fn dropped_messages(&self) -> u64 {
        self.dropped_messages
    }

    fn observe_text(&mut self, event: &AcpUiEvent, kind: &'static str) {
        let Some(content) = event.content.as_deref() else {
            return;
        };
        if content.is_empty() {
            return;
        }
        // 非空文本 = agent 已有可见产出（回声去重从此失效，用户消息一律上报）。
        self.agent_activity_seen = true;
        let content = sanitize_text(content);
        let total = content.chars().count();
        let seen = self.streams.get(&event.id).copied().unwrap_or(0);
        // 常规情形 live 文本是全量快照 → 只取新增部分；若快照变短（压缩/重建），
        // 说明旧快照已失效，按新分段整段重投影，避免静默丢内容。
        let delta = if total >= seen {
            content.chars().skip(seen).collect::<String>()
        } else {
            content.clone()
        };
        self.remember_stream(&event.id, total);
        if delta.is_empty() {
            return;
        }

        let mut pending = delta;
        loop {
            let need_new_segment = self.segment.as_ref().is_none_or(|segment| {
                segment.kind != kind
                    || segment.item_id != event.id
                    || segment.content.chars().count() >= MAX_TEXT_CHARS
            });
            if need_new_segment {
                self.close_segment();
                self.segment = Some(Segment {
                    kind,
                    item_id: event.id.clone(),
                    content: String::new(),
                });
            }
            let Some(segment) = self.segment.as_mut() else {
                return;
            };
            let used = segment.content.chars().count();
            let take = (MAX_TEXT_CHARS - used).min(pending.chars().count());
            let split_at = pending
                .char_indices()
                .nth(take)
                .map(|(index, _)| index)
                .unwrap_or(pending.len());
            segment.content.push_str(&pending[..split_at]);
            if split_at >= pending.len() {
                return;
            }
            pending = pending[split_at..].to_string();
        }
    }

    fn observe_tool(&mut self, event: &AcpUiEvent) {
        // 工具事件本身就是 agent 活动（无论是否产出 use/result 行）。
        self.agent_activity_seen = true;
        let key = event
            .tool_call_id
            .clone()
            .unwrap_or_else(|| event.id.clone());
        let name = match self.tools.get(&key) {
            Some(state) => state.name.clone(),
            None => {
                let name = agent_tool_name(event);
                self.remember_tool(&key, name.clone());
                name
            }
        };
        let terminal = event.status.as_deref().is_some_and(is_terminal_tool_status);

        let use_sent = self.tools.get(&key).is_some_and(|state| state.use_sent);
        if !use_sent {
            if let Some(input) = event_tool_input(event) {
                self.mark_tool(&key, |state| state.use_sent = true);
                self.push(TranscriptMessage::tool_use(
                    name.clone(),
                    normalize_tool_input(input),
                ));
            } else if terminal {
                self.mark_tool(&key, |state| state.use_sent = true);
                self.push(TranscriptMessage::tool_use(name.clone(), json!({})));
            }
        }

        let result_sent = self.tools.get(&key).is_some_and(|state| state.result_sent);
        if terminal && !result_sent {
            // 结果必须排在 use 之后：没有输入也要补一条 use，保证前端能配对。
            if !self.tools.get(&key).is_some_and(|state| state.use_sent) {
                self.mark_tool(&key, |state| state.use_sent = true);
                self.push(TranscriptMessage::tool_use(name.clone(), json!({})));
            }
            self.mark_tool(&key, |state| state.result_sent = true);
            let output = event_tool_output(event)
                .map(|output| truncate_chars(&sanitize_text(&output), MAX_TOOL_OUTPUT_CHARS))
                .unwrap_or_default();
            self.push(TranscriptMessage::tool_result(name, output));
        }
    }

    fn mark_tool(&mut self, key: &str, update: impl FnOnce(&mut ToolState)) {
        if let Some(state) = self.tools.get_mut(key) {
            update(state);
        }
    }

    fn remember_stream(&mut self, item_id: &str, total: usize) {
        if self.streams.contains_key(item_id) {
            self.streams.insert(item_id.to_string(), total);
            return;
        }
        if self.streams.len() >= MAX_TRACKED_STREAMS
            && let Some(evicted) = self.stream_order.pop_front()
        {
            self.streams.remove(&evicted);
        }
        self.streams.insert(item_id.to_string(), total);
        self.stream_order.push_back(item_id.to_string());
    }

    fn remember_tool(&mut self, key: &str, name: String) {
        if self.tools.contains_key(key) {
            return;
        }
        if self.tools.len() >= MAX_TRACKED_TOOLS
            && let Some(evicted) = self.tool_order.pop_front()
        {
            self.tools.remove(&evicted);
        }
        self.tools.insert(
            key.to_string(),
            ToolState {
                name,
                use_sent: false,
                result_sent: false,
            },
        );
        self.tool_order.push_back(key.to_string());
    }

    fn push(&mut self, message: TranscriptMessage) {
        if self.outbox.len() >= MAX_OUTBOX_MESSAGES {
            // 溢出时优先丢最旧的文本行，工具行（含 use/result 配对）尽量保留。
            let evict = self
                .outbox
                .iter()
                .position(|item| matches!(item.message_type, TYPE_TEXT | TYPE_THINKING))
                .unwrap_or(0);
            self.outbox.remove(evict);
            self.dropped_messages = self.dropped_messages.saturating_add(1);
        }
        self.outbox.push_back(message);
    }
}

fn is_terminal_tool_status(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    TERMINAL_TOOL_STATUSES.contains(&status.as_str())
}

/// 工具名优先级与 Gold Band 前端一致，另加 agent transcript 元数据兜底：
/// `_meta.goldBandConversation.toolName` → agent transcript 关系 → 标题首词 → `tool`。
fn agent_tool_name(event: &AcpUiEvent) -> String {
    if let Some(name) = event
        .raw
        .as_ref()
        .and_then(|raw| raw.pointer("/_meta/goldBandConversation/toolName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }
    if let Some(name) = event
        .raw
        .as_ref()
        .and_then(extract_agent_transcript_relation)
        .and_then(|relation| relation.tool_name)
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
    {
        return name;
    }
    if event_tool_input(event)
        .and_then(|input| input.get("command"))
        .and_then(Value::as_str)
        .is_some()
    {
        return "Bash".to_string();
    }
    if let Some(token) = event
        .title
        .as_deref()
        .and_then(|title| title.split_whitespace().next())
        .filter(|token| !token.is_empty())
    {
        return token.to_string();
    }
    "tool".to_string()
}

fn event_tool_input(event: &AcpUiEvent) -> Option<&Value> {
    let raw = event.raw.as_ref()?;
    raw.pointer("/toolCall/rawInput")
        .or_else(|| raw.pointer("/toolCall/input"))
        .or_else(|| raw.get("rawInput"))
        .or_else(|| raw.get("input"))
        .filter(|value| value.is_object())
}

fn event_tool_output(event: &AcpUiEvent) -> Option<String> {
    let raw = event.raw.as_ref()?;
    if let Some(output) = agent_transcript_tool_output(raw).and_then(value_to_text) {
        return Some(output);
    }
    for pointer in [
        "/toolCall/rawOutput/formatted_output",
        "/toolCall/output",
        "/rawOutput/formatted_output",
        "/output",
    ] {
        if let Some(output) = raw.pointer(pointer).and_then(value_to_text) {
            return Some(output);
        }
    }
    if let Some(content) = raw.pointer("/toolCall/content").and_then(value_to_text) {
        return Some(content);
    }
    None
}

/// `rawOutput` 在 ACP 里可能是字符串、`{text}`、`[{type:"text",text}]` 或
/// `{content:[{type:"text",text}]}`，统一取纯文本。
fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(value_to_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return (!text.is_empty()).then(|| text.to_string());
            }
            if let Some(content) = object.get("content").and_then(value_to_text) {
                return Some(content);
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::TYPE_USER;
    use super::*;

    fn event(id: &str, kind: &str) -> AcpUiEvent {
        AcpUiEvent {
            id: id.to_string(),
            seq: 1,
            timestamp: "2026-11-12T00:00:00Z".to_string(),
            kind: kind.to_string(),
            session_id: None,
            content: None,
            title: None,
            tool_call_id: None,
            status: None,
            started_seq: None,
            ended_seq: None,
            started_at: None,
            ended_at: None,
            timing: None,
            raw: None,
        }
    }

    fn text_chunk(id: &str, content: &str) -> AcpUiEvent {
        let mut event = event(id, "textDelta");
        event.content = Some(content.to_string());
        event
    }

    fn tool_event(id: &str, tool_call_id: &str, status: &str, input: Value) -> AcpUiEvent {
        let mut event = event(id, "toolCallUpdate");
        event.tool_call_id = Some(tool_call_id.to_string());
        event.status = Some(status.to_string());
        event.raw = Some(json!({"toolCall": {"rawInput": input}}));
        event
    }

    fn drain(projector: &mut Projector) -> Vec<TranscriptMessage> {
        let mut next_seq = 1;
        projector.drain(&mut next_seq)
    }

    #[test]
    fn live_text_snapshots_are_projected_as_increments() {
        let mut projector = Projector::new(None);
        projector.observe(&text_chunk("item-1", "你"));
        projector.observe(&text_chunk("item-1", "你好"));
        projector.observe(&text_chunk("item-1", "你好，世界"));
        projector.close_segment();
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].message_type, TYPE_TEXT);
        assert_eq!(batch[0].content.as_deref(), Some("你好，世界"));
        assert_eq!(batch[0].seq, 1);
    }

    #[test]
    fn text_segments_split_on_flush_and_item_change() {
        let mut projector = Projector::new(None);
        projector.observe(&text_chunk("item-1", "第一段"));
        projector.close_segment();
        projector.observe(&text_chunk("item-1", "第一段第二段"));
        projector.observe(&text_chunk("item-2", "另一个流"));
        projector.close_segment();
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].content.as_deref(), Some("第一段"));
        assert_eq!(batch[1].content.as_deref(), Some("第二段"));
        assert_eq!(batch[2].content.as_deref(), Some("另一个流"));
        assert_eq!(
            batch.iter().map(|m| m.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn shrinking_snapshot_reprojects_full_content() {
        let mut projector = Projector::new(None);
        projector.observe(&text_chunk("item-1", "abcdef"));
        projector.close_segment();
        projector.observe(&text_chunk("item-1", "abc"));
        projector.close_segment();
        let batch = drain(&mut projector);
        assert_eq!(batch[0].content.as_deref(), Some("abcdef"));
        assert_eq!(batch[1].content.as_deref(), Some("abc"));
    }

    #[test]
    fn thinking_is_skipped_when_disabled() {
        let mut projector = Projector::new(None);
        let mut event = event("thought-1", "thoughtDelta");
        event.content = Some("内部推理".to_string());
        projector.observe(&event);
        projector.close_segment();
        assert!(drain(&mut projector).is_empty());
    }

    #[test]
    fn tool_use_is_emitted_once_then_result_pairs_by_tool_call_id() {
        let mut projector = Projector::new(None);
        projector.observe(&tool_event(
            "call-1",
            "call-1",
            "in_progress",
            json!({"file_path": "a.rs", "content": "hello"}),
        ));
        projector.observe(&tool_event(
            "update-2",
            "call-1",
            "in_progress",
            json!({"file_path": "a.rs", "content": "hello"}),
        ));
        let mut done = tool_event("update-3", "call-1", "completed", json!({}));
        done.raw = Some(json!({"toolCall": {"content": [{"type": "text", "text": "ok"}]}}));
        projector.observe(&done);
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message_type, super::super::model::TYPE_TOOL_USE);
        assert_eq!(batch[0].tool.as_deref(), Some("tool"));
        assert_eq!(
            batch[0].input,
            Some(json!({"file_path": "a.rs", "content": "hello"}))
        );
        assert_eq!(batch[1].message_type, super::super::model::TYPE_TOOL_RESULT);
        assert_eq!(batch[1].output.as_deref(), Some("ok"));
    }

    #[test]
    fn terminal_tool_without_input_still_emits_use_then_result() {
        let mut projector = Projector::new(None);
        let mut done = event("call-9", "toolCallUpdate");
        done.tool_call_id = Some("call-9".to_string());
        done.status = Some("failed".to_string());
        done.raw = Some(json!({"output": "boom"}));
        projector.observe(&done);
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message_type, super::super::model::TYPE_TOOL_USE);
        assert_eq!(batch[0].input, Some(json!({})));
        assert_eq!(batch[1].output.as_deref(), Some("boom"));
    }

    #[test]
    fn tool_output_is_capped() {
        let mut projector = Projector::new(None);
        let mut done = tool_event("call-1", "call-1", "completed", json!({"command": "x"}));
        done.raw = Some(json!({"output": "y".repeat(MAX_TOOL_OUTPUT_CHARS * 2)}));
        projector.observe(&done);
        let batch = drain(&mut projector);
        assert_eq!(
            batch[1].output.as_deref().map(str::len),
            Some(MAX_TOOL_OUTPUT_CHARS)
        );
    }

    #[test]
    fn outbox_overflow_drops_oldest_text_first() {
        let mut projector = Projector::new(None);
        for index in 0..(MAX_OUTBOX_MESSAGES + 10) {
            projector.push(TranscriptMessage::body(TYPE_TEXT, format!("t{index}")));
        }
        assert_eq!(projector.dropped_messages(), 10);
        // 溢出丢最旧文本后仍有 MAX_OUTBOX_MESSAGES 条；单拍按上限切批，剩余下一拍补发。
        let mut next_seq = 1;
        let mut total = 0usize;
        let mut first = true;
        loop {
            let batch = projector.drain(&mut next_seq);
            if batch.is_empty() {
                break;
            }
            if first {
                // 首拍恰好卡在条数上限，且第一条已是被丢剩下的 t10。
                assert_eq!(batch.len(), MAX_BATCH_MESSAGES);
                assert_eq!(batch[0].content.as_deref(), Some("t10"));
                first = false;
            }
            // seq 跨批连续，不重号。
            assert_eq!(batch[0].seq, total as u64 + 1);
            total += batch.len();
        }
        assert_eq!(total, MAX_OUTBOX_MESSAGES);
    }

    #[test]
    fn error_line_is_sanitized_and_capped() {
        let mut projector = Projector::new(None);
        projector.push_error("boom\0");
        let batch = drain(&mut projector);
        assert_eq!(batch[0].message_type, super::super::model::TYPE_ERROR);
        assert_eq!(batch[0].content.as_deref(), Some("boom"));
    }

    /// 用户回声事件（`record_user_prompt_event` 合成的 goldBandPrompt 形状）。
    fn user_echo(content: &str) -> AcpUiEvent {
        let mut event = event("gold-band-user-prompt-1", "userTextDelta");
        event.content = Some(content.to_string());
        event.raw = Some(json!({
            "source": "goldBandPrompt",
            "synthetic": true,
            "originalUserText": true,
        }));
        event
    }

    #[test]
    fn initial_user_text_is_seq_one_and_wire_shape_is_type_content_only() {
        let mut projector = Projector::new(Some("帮我修这个 bug\0"));
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].seq, 1);
        assert_eq!(batch[0].message_type, TYPE_USER);
        assert_eq!(batch[0].content.as_deref(), Some("帮我修这个 bug"));
        // 线上形状：只有 seq/type/content（multica 约定 user 行不填 tool/input/output）。
        let wire = serde_json::to_value(&batch[0]).expect("serialize");
        assert_eq!(
            wire,
            json!({"seq": 1, "type": "user", "content": "帮我修这个 bug"})
        );
    }

    #[test]
    fn blank_initial_user_text_is_skipped() {
        let mut projector = Projector::new(Some("  \n\0"));
        assert!(drain(&mut projector).is_empty());
    }

    #[test]
    fn mid_conversation_user_echo_interleaves_with_agent_text() {
        let mut projector = Projector::new(Some("初始需求"));
        projector.observe(&text_chunk("item-1", "第一轮回答"));
        projector.close_segment();
        projector.observe(&user_echo("中途插话：换个思路"));
        projector.observe(&text_chunk("item-2", "好的，按新思路"));
        projector.close_segment();
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 4);
        let kinds: Vec<(&str, &str)> = batch
            .iter()
            .map(|m| (m.message_type, m.content.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (TYPE_USER, "初始需求"),
                (TYPE_TEXT, "第一轮回答"),
                (TYPE_USER, "中途插话：换个思路"),
                (TYPE_TEXT, "好的，按新思路"),
            ]
        );
        // seq 顺序 = 事件顺序，用户消息天然插在流中间。
        assert_eq!(
            batch.iter().map(|m| m.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn initial_echo_before_agent_activity_is_deduplicated() {
        let mut projector = Projector::new(Some("初始需求"));
        // fresh 直发场景：首拍回声与初始需求逐字相等，且先于任何 agent 输出。
        projector.observe(&user_echo("初始需求"));
        projector.observe(&text_chunk("item-1", "开始处理"));
        projector.close_segment();
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message_type, TYPE_USER);
        assert_eq!(batch[1].message_type, TYPE_TEXT);
    }

    #[test]
    fn identical_user_message_after_agent_activity_is_reported() {
        let mut projector = Projector::new(Some("同一段话"));
        projector.observe(&text_chunk("item-1", "第一轮回答"));
        projector.close_segment();
        // agent 已产出 → 内容相同也是真实中途插话，照常上报。
        projector.observe(&user_echo("同一段话"));
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[2].message_type, TYPE_USER);
        assert_eq!(batch[2].content.as_deref(), Some("同一段话"));
    }

    #[test]
    fn user_echo_rejects_non_user_prompt_events() {
        let cases: Vec<Option<Value>> = vec![
            // 非 goldBandPrompt 合成源（agent 回显的原生 user_message_chunk）。
            Some(json!({"originalUserText": true})),
            // 隐藏注入。
            Some(json!({
                "source": "goldBandPrompt",
                "hiddenFromChat": true,
                "originalUserText": true,
            })),
            // 无 display_text（引擎组装的 prompt，如 workflow 节点 turn / 隐藏 section 注入）。
            Some(json!({"source": "goldBandPrompt", "originalUserText": false})),
            // 有源但 originalUserText 缺失（保守跳过）。
            Some(json!({"source": "goldBandPrompt"})),
            // 无 raw。
            None,
        ];
        for raw in cases {
            let mut projector = Projector::new(None);
            let mut echo = user_echo("内容");
            echo.raw = raw;
            projector.observe(&echo);
            assert!(drain(&mut projector).is_empty(), "must skip: {echo:?}");
        }
        // 空白内容也不上报。
        let mut projector = Projector::new(None);
        projector.observe(&user_echo("  \n"));
        assert!(drain(&mut projector).is_empty());
    }

    #[test]
    fn user_message_closes_open_text_segment_preserving_order() {
        let mut projector = Projector::new(None);
        // agent 文本段未收起时用户插话：先落已积累文本，用户行排其后。
        projector.observe(&text_chunk("item-1", "说到一半的内容"));
        projector.observe(&user_echo("先停一下"));
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message_type, TYPE_TEXT);
        assert_eq!(batch[0].content.as_deref(), Some("说到一半的内容"));
        assert_eq!(batch[1].message_type, TYPE_USER);
        assert_eq!(batch[1].content.as_deref(), Some("先停一下"));
    }

    #[test]
    fn oversized_user_text_splits_into_consecutive_rows() {
        let long = "长".repeat(MAX_TEXT_CHARS + 10);
        let mut projector = Projector::new(Some(&long));
        let batch = drain(&mut projector);
        assert_eq!(batch.len(), 2);
        assert!(batch.iter().all(|m| m.message_type == TYPE_USER));
        assert_eq!(batch.iter().map(|m| m.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            batch[0].content.as_deref().map(|text| text.chars().count()),
            Some(MAX_TEXT_CHARS)
        );
        assert_eq!(batch[1].content.as_deref(), Some("长".repeat(10).as_str()));
    }
}
