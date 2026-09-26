//! 远程来源通用层词汇表（远程来源解耦设计 §2 / §4.2）。
//!
//! 本模块是「远程任务来源 / skill 来源」中立词汇的 Rust 侧单一事实源：
//! 跨边界契约（事件名）在此定义，multica 适配实现从这里引入并 emit——
//! 协议词汇（issue/claim/lease 等）不允许出现在本模块（不变量 1：方言不出模块）。
//!
//! 后续分期（P2 SkillSource / P3 TaskSource）的接口与能力名片也落在此模块；
//! P1a 先收拢事件常量（详细设计 §5.1）。

/// 远程任务列表变更事件。前端监听后 re-fetch `get_remote_tasks`。
pub const REMOTE_TASKS_UPDATED_EVENT: &str = "gold-band://remote-tasks-updated";

/// 远程来源连接/设置变更事件（连接、断开、地址覆盖、workspace 增删）。
/// 前端监听后 re-fetch 设置 VM 与任务列表。
pub const REMOTE_SOURCE_SETTINGS_UPDATED_EVENT: &str = "gold-band://remote-source-settings-updated";

/// 当前远程任务来源（`desktop_remote_task_source` 指针；RuntimeConfig 已把缺省归一为 "multica"）。
/// 通用层按此路由到对应 adapter（当前唯一 adapter = multica）。
pub fn remote_task_source(config: &gold_band::config::RuntimeConfig) -> String {
    config.desktop_remote_task_source.clone()
}
