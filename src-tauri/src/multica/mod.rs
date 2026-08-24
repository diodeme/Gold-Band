//! multica 远程任务接入（码灵作为 multica daemon 角色）。
//!
//! 模块组织见《Multica开发设计》2.1：
//! - [`client`]: reqwest HTTP client + 指数退避重试 + 全部 multica API 调用
//! - [`config`]: 配置 VM + pat/daemon_id getter + `multica_settings` 聚合 + base_url 容错
//! - [`error`]:  `MulticaError` → `CommandErrorVm` 映射（code 前缀 `multica.*`）
//!
//! 后续里程碑追加：
//! - `loop_`（M2）：启动全量 register / 常驻 15s 心跳（连接后即持续）/ recover-orphans / 取消检测
//! - `state`（M2+）：运行期内存状态（runtime_id 映射、在飞任务映射）
//! - `vm`（M3）：远程任务展示 VM（RemoteTaskVm / RemoteConversationSidebarVm）
//! - `commands`（M3）：远程任务命令（get_multica_tasks / get_multica_task_requirement /
//!   start_multica_conversation_run）
//! - `bridge`（M4）：lifecycle 事件转译 multica 终态（NodeCompleted 采 session pin /
//!   RunCompleted 4 分支 complete/fail；订阅 `RuntimeLifecycleBus`）

pub mod bridge;
pub mod client;
pub mod commands;
pub mod config;
pub mod error;
pub mod loop_;
pub mod state;
pub mod vm;

pub use client::MulticaClient;
pub use config::{
    MulticaSettingsVm, clear_multica_session, clear_multica_state_indices,
    clear_multica_workspace_bindings, ensure_daemon_id, multica_account_changed, multica_app_url,
    multica_base_url, multica_settings,
};
pub use error::MulticaError;
pub use loop_::start_multica_loop;
pub use state::shared_state;
