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
// multica 上游拉取决策点（M5-ch：首拉空值时交还控制权 / 任意路径收口），与 Rust 侧
// `src-tauri/src/multica/upstream.rs` 的事件常量对应。
export const MULTICA_UPSTREAM_PULL_PENDING_EVENT = 'gold-band://multica-upstream-pull-pending';
export const MULTICA_UPSTREAM_PULL_SETTLED_EVENT = 'gold-band://multica-upstream-pull-settled';
// multica 被动安装落地（M5-cr：runtime-assets push-install 成功落库后按 assetType 重拉
// 对应列表），与 Rust 侧 `src-tauri/src/multica/runtime_assets.rs` 的事件常量对应。
export const RUNTIME_ASSET_INSTALLED_EVENT = 'gold-band://runtime-asset-installed';
// multica 被动卸载落地（M5-cw：runtime-assets uninstall 成功移除后按 assetType 重拉对应
// 列表，与安装事件对称——否则本地运行中列表仍显示已删条目），Rust 侧同上对应。
export const RUNTIME_ASSET_UNINSTALLED_EVENT = 'gold-band://runtime-asset-uninstalled';

// ── 个人分析 / 排程 ──
export const PERSONAL_ANALYTICS_UPDATED_EVENT = 'gold-band://personal-analytics-updated';
export const SCHEDULED_NOTIFICATION_EVENT = 'gold-band://scheduled-notification';
export const SCHEDULED_TASK_UPDATED_EVENT = 'gold-band://scheduled-task-updated';
export const SCHEDULED_OCCURRENCE_UPDATED_EVENT = 'gold-band://scheduled-occurrence-updated';
