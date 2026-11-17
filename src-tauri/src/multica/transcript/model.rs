//! multica 执行记录上报的线上 DTO 与硬上限。
//!
//! 契约（multica 服务端逐条核对结论）：
//! - 消息 `type` 服务端无白名单；码灵只用六种（multica 前端对 `user` 已规划专门渲染，
//!   其余未知 type 只做 generic 兜底）。
//! - `seq` 由码灵自增，作用域 = `task_id`，严格递增、允许空洞；表上无唯一约束，
//!   **重复 seq 会产生重复行**，绝不重发同一 seq（失败换新 seq）。
//! - 批次原子且失败不重试；服务端无条数/大小限制，故由本端自律（见下方上限）。
//! - 服务端只清洗 NUL 字节，且整批只要有一个清洗失败即整批 500 丢弃 → 本端先清洗。

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

pub(crate) const TYPE_TEXT: &str = "text";
pub(crate) const TYPE_THINKING: &str = "thinking";
pub(crate) const TYPE_TOOL_USE: &str = "tool_use";
pub(crate) const TYPE_TOOL_RESULT: &str = "tool_result";
pub(crate) const TYPE_ERROR: &str = "error";
/// 用户输入（multica 约定固定英文小写 `user`，前端按该字符串做专门渲染；只填 `content`）。
pub(crate) const TYPE_USER: &str = "user";

/// 是否上报模型思考过程。默认关闭：思考可能引用内部提示词/敏感推理，而 multica 的可见范围是
/// 该需求的工作区成员（比本地操作者广）。multica 前端对完全没有 thinking 行的任务渲染完全正常，
/// 且协议侧无强制要求。需要开启时改这里即可（projector 已实现完整映射与分段）。
pub(crate) const REPORT_THINKING: bool = false;

/// 批量 flush 间隔（multica 参考节奏 500ms；空闲批次跳过）。
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// 终态前等待最后一批落库的上限（multica daemon 的 waitForDrain 语义）。
pub(crate) const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// 单条 `tool_result.output` 上限（multica 建议值，与其自家 daemon 一致）。
pub(crate) const MAX_TOOL_OUTPUT_CHARS: usize = 8_192;
/// 单条 text/thinking 上限，超出拆成连续多条（seq 相邻，语义不丢）。
pub(crate) const MAX_TEXT_CHARS: usize = 32_768;
/// 工具输入单个字符串字段上限（content/new_string/old_string/diff/command 等）。
pub(crate) const MAX_INPUT_FIELD_CHARS: usize = 16_384;
/// 工具输入整体序列化上限（超出先二次收紧，再不行则上报 `{"truncated":true}`）。
pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;
/// 单批条数上限（服务端无限制，本端自律）。
pub(crate) const MAX_BATCH_MESSAGES: usize = 200;
/// 单批序列化字节上限（本端自律）。
pub(crate) const MAX_BATCH_BYTES: usize = 256 * 1024;
/// 未发送缓冲上限；溢出时丢最旧的文本行，工具行优先保留。
pub(crate) const MAX_OUTBOX_MESSAGES: usize = 512;
/// 工具调用/文本流跟踪表上限（防长跑内存无界；超限淘汰最旧）。
pub(crate) const MAX_TRACKED_TOOLS: usize = 2_048;
pub(crate) const MAX_TRACKED_STREAMS: usize = 64;

/// 一条待上报消息。`seq` 由 reporter 在取批时统一分配（构造期为 0）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TranscriptMessage {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl TranscriptMessage {
    pub(crate) fn body(message_type: &'static str, content: String) -> Self {
        Self {
            seq: 0,
            message_type,
            tool: None,
            content: Some(content),
            input: None,
            output: None,
        }
    }

    pub(crate) fn tool_use(tool: String, input: Value) -> Self {
        Self {
            seq: 0,
            message_type: TYPE_TOOL_USE,
            tool: Some(tool),
            content: None,
            input: Some(input),
            output: None,
        }
    }

    pub(crate) fn tool_result(tool: String, output: String) -> Self {
        Self {
            seq: 0,
            message_type: TYPE_TOOL_RESULT,
            tool: Some(tool),
            content: None,
            input: None,
            output: Some(output),
        }
    }

    pub(crate) fn error(content: String) -> Self {
        Self::body(TYPE_ERROR, content)
    }

    pub(crate) fn user(content: String) -> Self {
        Self::body(TYPE_USER, content)
    }
}

/// 一条 token 用量行（`POST /api/daemon/tasks/{id}/usage`）。
///
/// 服务端按 `(task_id, provider, model)` 三元组 **整体替换**（不是累加），因此每次上报都必须是
/// 该任务截至当前的累计总量；`cost_usd_ticks = 0` 表示「无权威费用，请服务端按费率估算」。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub(crate) struct UsageRow {
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd_ticks: u64,
}

/// 去掉服务端无法入库的 NUL 字节（整批有一个就整批丢弃）。
pub(crate) fn sanitize_text(value: &str) -> String {
    value.replace('\0', "")
}

/// 按字符（非字节）截断，避免切坏 UTF-8。
pub(crate) fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}
