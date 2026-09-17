/**
 * 应用事件名单一事实源（远程来源解耦设计 §4.2 / 回归清单 E2）。
 *
 * 所有 `gold-band://` 事件名常量集中于此，前端任何 `listen` 不允许散写字面量——
 * 事件名是跨端契约（后端 emit 名 / 前端订阅名），收拢后改名只需改一处。
 * remote 两个事件常量与 Rust 侧 `src-tauri/src/remote/mod.rs` 对应。
 */

// ── Agent / 命令目录 ──
export const AGENT_REGISTRY_UPDATED_EVENT = 'gold-band://agent-registry-updated';
export const AGENT_COMMANDS_UPDATED_EVENT = 'gold-band://agent-commands-updated';

// ── 更新器 ──
export const UPDATE_STATUS_EVENT = 'gold-band://update-status';
export const UPDATE_DOWNLOAD_PROGRESS_EVENT = 'gold-band://update-download-progress';

// ── Git ──
export const GIT_OPERATION_UPDATED_EVENT = 'gold-band://git-operation-updated';
export const GIT_STATE_CHANGED_EVENT = 'gold-band://git-state-changed';
export const GITHUB_OPERATION_UPDATED_EVENT = 'gold-band://github-operation-updated';

// ── 会话 / ACP ──
export const ACP_SESSION_UPDATED_EVENT = 'gold-band://acp-session-updated';
export const CONVERSATION_RUN_STATE_UPDATED_EVENT = 'gold-band://conversation-run-state-updated';
export const CONVERSATION_TERMINAL_RESULT_UPDATED_EVENT = 'gold-band://conversation-terminal-result-updated';

// ── 干预 / 退出 / 工作区文件 ──
export const INTERVENTION_NAVIGATE_EVENT = 'gold-band://intervention-navigate';
export const APP_EXIT_REQUESTED_EVENT = 'gold-band://app-exit-requested';
export const WORKSPACE_FILE_CHANGED_EVENT = 'gold-band://workspace-file-changed';

// ── 远程来源（任务生命周期 / 来源连接配置变更）──
export const REMOTE_TASKS_UPDATED_EVENT = 'gold-band://remote-tasks-updated';
export const REMOTE_SOURCE_SETTINGS_UPDATED_EVENT = 'gold-band://remote-source-settings-updated';

// ── 个人分析 / 排程 ──
export const PERSONAL_ANALYTICS_UPDATED_EVENT = 'gold-band://personal-analytics-updated';
export const SCHEDULED_NOTIFICATION_EVENT = 'gold-band://scheduled-notification';
export const SCHEDULED_TASK_UPDATED_EVENT = 'gold-band://scheduled-task-updated';
export const SCHEDULED_OCCURRENCE_UPDATED_EVENT = 'gold-band://scheduled-occurrence-updated';
