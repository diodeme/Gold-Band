//! multica HTTP client（码灵作为 daemon 角色）。
//!
//! Wire 契约照搬 multica-main 参考：
//! - `browser_login`: `cmd_auth.go:240-358`（IPv4 本地 listener + cli_callback + state CSRF + JWT→PAT→verify）
//! - `create_token`: `POST /api/tokens`（Bearer JWT，body `{name, expires_in_days}`，resp `{token}`）
//! - `verify_pat`:   `GET /api/me`（Bearer PAT，resp `{name, email}`）
//! - `list_workspaces`: `GET /api/workspaces`（Bearer PAT，resp `[{id,name}]` 或 `{workspaces:[...]}`）
//! - 终态上报 complete/fail 指数退避：`TERMINAL_RETRY_SCHEDULE_SECS = [4,8,16,32,64]`（multica `postJSONWithRetry`）
//!
//! 后端只返回 `MulticaError`（→ `CommandErrorVm { code, params }`），不含任何对客文案。

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::multica::error::MulticaError;
use crate::multica::markdown_source::markdown_source_to_plain_text;
use crate::multica::transcript::{TranscriptMessage, UsageRow};

/// PAT 创建请求（`POST /api/tokens` body）。serde 默认序列化为 snake_case 键。
#[derive(Debug, Serialize)]
struct CreateTokenRequest {
    name: String,
    expires_in_days: u32,
}

/// PAT 创建响应（`POST /api/tokens` resp）。
#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
}

/// `/api/me` 响应（verify_pat）。字段均可缺失（旧 server）。
#[derive(Debug, Default, Deserialize)]
pub struct UserInfo {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// multica workspace 成员（id+name，对齐 server 侧 `WorkspaceInfo`）。
///
/// `Serialize` 供 `list_server_multica_workspaces` 命令直接回传前端（下拉单选用，开发设计 M3.5）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
}

/// list_workspaces 响应容错：包装 `{workspaces:[...]}` 或裸数组 `[...]`（不同 server 版本）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WorkspacesResponse {
    Wrapped { workspaces: Vec<WorkspaceInfo> },
    Bare(Vec<WorkspaceInfo>),
}

/// 终态上报（complete/fail）指数退避重试间隔（秒），照搬 multica `postJSONWithRetry`。
const TERMINAL_RETRY_SCHEDULE_SECS: &[u64] = &[4, 8, 16, 32, 64];

/// 一般请求（register/list/claim）网络错误重试次数（开发设计 2.3 / 2.6）。
///
/// 4xx 不重试（经 `map_status` 直接映射确定错误码），仅对 reqwest 传输/超时错误退避重试。
const NETWORK_RETRY_ATTEMPTS: u32 = 3;
/// 网络重试退避基数（秒）：第 n 次重试 sleep `NETWORK_RETRY_BASE_SECS * 2^n`。
const NETWORK_RETRY_BASE_SECS: u64 = 1;

/// 单次 liveness/轻量请求超时（秒），覆盖 client 级 30s 默认值。
///
/// 心跳 tick 内的高频调用（heartbeat / 取消检测 / 自愈 register）正常 <1s；server 慢响应或网络抖动时，
/// 若沿用 30s 全局超时，单 tick 串行阻塞会累积，威胁 runtime 在线维持（离线超 150s 被 sweeper 回收）
/// 与取消检测的及时性。给这些调用更短的 per-request 上界，使退化网络下单 tick 也能快速失败、
/// 下一 tick（3s）重试——而非一个 tick 阻塞数分钟。
const LIVENESS_TIMEOUT_SECS: u64 = 10;

/// 执行记录（messages/usage）单次上报超时（秒）。
///
/// 上报由独立的 500ms flush worker 驱动、失败即丢，不需要与心跳共享短超时语义；但也不能用
/// client 级 30s 把一批消息吊太久（worker 串行、会推迟后续批次与终态排空）。10s 足以覆盖一次
/// 正常批量写入。
const TRANSCRIPT_REQUEST_TIMEOUT_SECS: u64 = 10;

/// 单个 issue 附件下载超时（秒），覆盖 client 级 30s 默认值（M5-bu）。
///
/// 附件经 server 从 DPMS 实时代理流转发，体积可达数十 MB，30s 对大文件偏紧；120s 在「大文件正常
/// 下载」与「故障时不长时间阻塞发送」之间取衡。超时按逐文件软失败处理（局部失败隔离，不阻断其他
/// 附件与任务启动）。
const ATTACHMENT_DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// issue 完成态（接入方案 D2：码灵完成远程任务后用 PAT 把关联 issue 流转到 done）。
pub const MULTICA_ISSUE_DONE_STATUS: &str = "done";

/// issue 进行中态（改动五：start_task 成功后用 PAT 把关联 issue 流转到 in_progress，与 done 对称）。
///
/// server 设计把 issue 状态交给 agent 管（start 只 flip task→running + 广播 EventTaskRunning，
/// 看板列由 issue.status 派生故卡片不会自动移到「进行中」列）；码灵作为中介补齐这条流转。
pub const MULTICA_ISSUE_IN_PROGRESS_STATUS: &str = "in_progress";

// ===== M2 daemon 注册/心跳 wire 类型（开发设计 2.2.7）=====
// 字段权威源：multica `server/internal/daemon/types.go` 的 json tag。

/// daemon 注册请求（`POST /api/daemon/register` body）。
///
/// 幂等：同一 (workspace_id, daemon_id) 永远稳定取回 runtime_id（已绑 agent 直接复用）。
#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub workspace_id: String,
    pub daemon_id: String,
    pub device_name: String,
    /// 码灵 cli 版本（`env!("CARGO_PKG_VERSION")`）。
    pub cli_version: String,
    /// 本 daemon 暴露的执行 runtime 列表（码灵每 workspace 绑一个 provider → 一个 runtime）。
    pub runtimes: Vec<RuntimeSpec>,
}

/// 单个执行 runtime。`type` = provider（如 `claude-acp`），固定；其余为展示/版本字段。
#[derive(Debug, Serialize)]
pub struct RuntimeSpec {
    pub name: String,
    /// provider 固定值（claude-acp / codex-acp），序列化为 `"type"` 键。
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub version: String,
    /// runtime 就绪状态（`"ready"`）。
    pub status: String,
}

/// 注册响应：返回本 daemon 的 runtime 列表（含 server 分配的 runtime_id）。
#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub runtimes: Vec<RuntimeRow>,
}

/// 注册返回的单个 runtime 行。`id` = server 分配的 runtime_id（心跳/领取/终态上报均用它）。
#[derive(Debug, Deserialize)]
pub struct RuntimeRow {
    pub id: String,
}

// ===== M3 任务列表/领取/启动 wire 类型（开发设计 2.2.7 / 接入方案 B/C）=====
// 字段权威源：multica `server/internal/daemon/types.go` 的 json tag；
// 除 `id` 外均 `#[serde(default)]`，缺字段不阻断解析（不同 server 版本字段集可能不同）。

/// multica 远程任务（pending 列表 / claim 响应通用）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RemoteTask {
    pub id: String,
    #[serde(default)]
    pub issue_id: Option<String>,
    #[serde(default)]
    pub status: String,
    /// server 在 claim 时签发的 task-scoped 短期凭证（mat_，webank `GenerateAgentTaskToken`）。
    ///
    /// **码灵 Option B 中介设计下的唯一消费点：issue 附件下载通道**（M5-bu）——ACP agent 从不直接调
    /// multica API（开发设计 选项 B：码灵作为中介，而非 agent 直调），claim 响应携带的 mat_ 令牌由
    /// 码灵持有并用于 `GET /api/attachments/{id}/download`（server 侧任务作用域附件既定通道，DPMS 附件
    /// 实时流转发；mat_ 为通用 actor 凭证，user-scoped 下载路由天然接受）；其余 multica 调用仍由码灵
    /// 用自身 PAT 完成。令牌不落盘、不进 prompt。
    #[serde(default)]
    pub auth_token: Option<String>,
    /// 续跑指针（**只输出**）：server claim 响应回填的父任务 ACP session_id。
    ///
    /// 角色为「从响应消费」——server claim 处理器**不读请求体**，续跑指针全由响应回填
    /// （webank `daemon.go:2025-2054` 经 `GetLastTaskSession` 解析父任务 session）。客户端用它做
    /// 续跑兜底/校验；主路径是 [`Self::parent_task_id`] 反查本地索引（更稳，不依赖 server session 解析）。
    #[serde(default)]
    #[allow(dead_code)]
    pub prior_session_id: Option<String>,
    /// 续跑血缘：server claim 响应/任务列表携带的父任务 id（auto-retry 子任务 T' 指向父任务 T）。
    ///
    /// webank `AgentTaskResponse.ParentTaskID`（JSON `parent_task_id`，`agent.go:296,635`）。客户端
    /// 续跑判定按它反查父任务的本地索引（断点续跑方案 §3.3），使「崩溃/关闭重启后领取重试子任务」
    /// 能续上父任务被中断的本地 run + ACP session。
    #[serde(default)]
    pub parent_task_id: Option<String>,
    /// 任务标题（pending 列表展示；缺失前端用 id 兜底）。
    ///
    /// wire 字段权威源：webank `AgentTaskResponse.ThreadName`（JSON `thread_name`），
    /// claim 响应与 pending 列表均带此键。Rust 字段名沿用语义化的 `title`，
    /// 仅 serde key 对齐 server（下游 `task.title` 消费点 / VM 输出 camelCase 不受影响）。
    #[serde(default, rename = "thread_name")]
    pub title: Option<String>,
    /// 首轮需求文本——来源字段（镜像 server `AgentTaskResponse` 的来源互斥分支，仅 claim 响应携带）。
    ///
    /// pending 列表只有 `thread_name`，无这些字段。供 [`RemoteTask::requirement_text`] 取「最佳可用」
    /// 预填文本：quick-create/chat/comment/autopilot/handoff 任一非空取之；皆空即无正文（title 不进
    /// 正文槽，M5-ca 起由 vm 任务名称块承载）。
    #[serde(default)]
    pub quick_create_prompt: Option<String>,
    #[serde(default)]
    pub chat_message: Option<String>,
    #[serde(default)]
    pub trigger_comment_content: Option<String>,
    #[serde(default)]
    pub autopilot_description: Option<String>,
    #[serde(default)]
    pub handoff_note: Option<String>,
    /// issue 正文（webank `AgentTaskResponse.IssueDescription`，仅 issue 任务 + 仅 claim 响应携带）。
    ///
    /// issue 任务无 quick_create/chat/comment/autopilot/handoff 来源时，正本是 issue 自身的 body；
    /// `requirement_text()` 把它排在 handoff_note 之后（末位正文来源）——比「具体指令」次之。
    #[serde(default)]
    pub issue_description: Option<String>,
    #[serde(default)]
    pub last_activity_at: Option<String>,
    /// issue 类型（`dev`|`test`|`bug`|`general`，story dev/test 拆分改造）。
    ///
    /// wire 键名 `issue_kind`（**不复用 `kind`**——该键是任务来源判别符
    /// `comment|autopilot|chat|quick_create|direct`，与 issue 类型是两个概念）。旧 server 不发 → None，
    /// 看板不渲染类型徽标、无门控——行为与拆分前一致（版本解耦：码灵可先于服务端发版）。
    #[serde(default)]
    pub issue_kind: Option<String>,
    /// test 任务就绪标记（服务端派生：父 dev issue `Effective(status)==done` 才 true）。
    ///
    /// 仅作看板提醒展示（未就绪徽标 + 原因 Tooltip），**不做执行门控**——经实测评审，
    /// 未就绪 test 任务提醒但放行（§12.42）。旧 server 不发 → None，不渲染提醒。
    #[serde(default)]
    pub is_ready: Option<bool>,
    /// DPMS 发布计划 ID（issue 行镜像字段，2026-09-17 daemon 任务接口新增）。
    ///
    /// 仅 issue 来源任务、且 DPMS 列非 NULL 时 server 才发（`omitempty`：缺 key 而非发 0/null）。
    /// 纯展示/溯源语义，**无门控语义**（门控仍只看 `is_ready` + `issue_kind`）。旧 server /
    /// 非 issue 任务（chat / autopilot / quick-create）→ None。
    #[serde(default)]
    pub release_plan_id: Option<i64>,
    /// DPMS 开发负责人（逗号分隔账号串，如 `"alice,bob"`，码灵侧原样使用不 split）。
    /// 可缺省语义同 [`Self::release_plan_id`]。
    #[serde(default)]
    pub dev_user: Option<String>,
    /// DPMS 测试负责人（逗号分隔账号串）。可缺省语义同 [`Self::release_plan_id`]。
    #[serde(default)]
    pub test_user: Option<String>,
    /// DPMS 业务需求 ID。可缺省语义同 [`Self::release_plan_id`]。
    #[serde(default)]
    pub business_story_id: Option<i64>,
    /// DPMS 需求链接（故事页 URL）。可缺省语义同 [`Self::release_plan_id`]。
    #[serde(default)]
    pub origin_url: Option<String>,
    /// DPMS 创建分支门（M5-cl，multica 2026-10 回传对接新增）：`true` = 本任务需要创建 DPMS 系统
    /// 需求并在终态回写。服务端非 omitempty bool；**缺键 = false 是双方认可的灰度契约**——旧 server
    /// / 非 issue 任务 → None，码灵全程跳过 DPMS 语义（不注入创建门块、终态不回写）。
    ///
    /// 消费点：hidden section 的 DPMS 创建门块（vm.rs）与终态回写触发条件（bridge.rs，经
    /// [`crate::multica::state::ActiveRemoteRun`] 的 claim 快照）。
    #[serde(default)]
    pub dpms_required: Option<bool>,
    /// claim 时已链接的 DPMS story id（M5-cl，`omitempty *int64`）：非空 = 该 issue 已有 DPMS 需求
    /// （rerun 场景）。码灵消费点：claim 后种进任务记忆 `storyId`（核实复用，不重复创建）+ 终态
    /// 回写条件排除（已链接不再回写）。缺键 → None。
    #[serde(default)]
    pub system_story_id: Option<i64>,
    /// 溯源 story 行的产品 ID（M5-cl，`omitempty *int64`）：DPMS 创建「产品」必答字段的任务指定
    /// 推荐值（非 story_sync 转换的 issue 不带）。渲染进预填溯源块，只预填不分支。缺键 → None。
    #[serde(default)]
    pub dpms_product_id: Option<i64>,
    /// 直接父 issue 的完成输出（multica `AgentTaskResponse.ParentOutput`，JSON `parent_output`，
    /// issue 完成输出传递特性）。
    ///
    /// 服务端 claim 时派生读取父行、不物化到子任务（父 done 后补写/改写，子任务下次 claim 拿新值）。
    /// 仅 issue 来源任务、且直接父 issue 有 `completion_output` 时 server 才发（`omitempty`：缺 key
    /// 而非发 null）。码灵消费点：`detail_prefill` 渲染为「上游交付说明」块、随 DPMS 溯源与需求正文
    /// 一起预填 composer（用户可见可编辑，三次调整）。
    /// 纯上下文语义，**无门控语义**（门控仍只看 `is_ready` + `issue_kind`；父曾 done 又 reopen 时
    /// 本字段仍可能存在，任务是否执行继续按 is_ready 判定）。旧 server / 非 issue 任务 → None。
    #[serde(default)]
    pub parent_output: Option<String>,
    /// issue 直属附件元数据（multica dev `1be391b6`，M5-bu）：pending 列表 / 任务详情 / claim 响应
    /// 三条路径的 task 对象内均携带。
    ///
    /// 语义：该任务所属 issue 的直属附件（**不含评论附件**；DPMS 需求同步转成的需求文件代理卡在其中）。
    /// 元素结构与 chat_message_attachments 同构（id / filename / content_type，content_type 可空）。
    /// 兼容性契约：server 侧 `omitempty`——issue 无附件、非 issue 任务、旧版本服务端都不带这个键；
    /// 解析端容忍字段缺失，缺省视为空数组（`#[serde(default)]`）。码灵消费点：发送时（claim-at-send）
    /// 用 claim 响应的 mat_ 令牌逐个中介下载（`MulticaClient::download_issue_attachment`），成功文件
    /// 并入 `attachment_paths` 走任务输入附件既有管线；pending / 详情路径仅解析不消费。
    #[serde(default)]
    pub issue_attachments: Vec<IssueAttachment>,
    /// 任务归因（rerun 血缘，multica 任务重试改造）：`attribution.rerun_of_task_id` 指回被
    /// 重新执行的源任务。仅 rerun 产生的新任务携带（`POST /api/daemon/tasks/{tid}/rerun` 的
    /// 202 响应与该新任务进 pending 列表后的列表项）；普通任务 / 旧 server 无此键 → None。
    /// 码灵消费点：看板 queued 卡片的「重新执行」徽标（multica「归组到源任务行」语义的看板投影）。
    #[serde(default)]
    pub attribution: Option<TaskAttribution>,
}

/// 任务归因对象（wire 镜像 multica `AgentTaskResponse.Attribution`）。
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct TaskAttribution {
    #[serde(default)]
    pub rerun_of_task_id: Option<String>,
}

/// issue 直属附件元数据（wire 镜像 multica daemon `types.go` `IssueAttachments` 元素）。
///
/// `id` = server 附件 id（下载通道 `GET /api/attachments/{id}/download` 的路径参数）；`filename` 为上传名
/// （**不可信输入**，落盘前须清洗，见 commands.rs `sanitize_attachment_filename`）；`content_type`
/// 可为空/缺失，码灵侧不消费（mime 由落盘文件扩展名经 provider `file_mime_type` 判定，与本地附件
/// 同一事实源）。
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct IssueAttachment {
    pub id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub content_type: Option<String>,
}

impl IssueAttachment {
    /// 展示名：server 文件名优先，空/纯空白回退附件 id（下载软失败清单与输入框元数据 chips 共用）。
    pub fn display_name(&self) -> String {
        let trimmed = self.filename.trim();
        if trimmed.is_empty() {
            self.id.clone()
        } else {
            trimmed.to_string()
        }
    }
}

impl RemoteTask {
    /// 预填用「最佳可用需求文本」（镜像 server `computeTaskKind` 来源互斥优先级）。
    ///
    /// quick-create → chat → comment → autopilot → handoff → issue_description 任一非空取之；
    /// 皆空即无正文（M5-ca 起不回退 title——任务名称由 vm 预填的专属名称块承载，回退只会让
    /// 无正文任务把标题重复一遍）。issue 正文排在 handoff 之后——比「具体指令/交接」次之。
    /// 空白值视为缺失跳过——按来源**逐个**过滤后再短路，保证纯空白的上游来源不会吞掉下游
    /// （如空白 quick_create 仍会落到 chat）。
    ///
    /// 返回值是**纯文本**：全部来源统一过 [`markdown_source_to_plain_text`]——multica web
    /// 富文本编辑器保存的是 markdown 源码（`\_` 等转义由 tiptap 序列化产生），码灵把需求
    /// 正文当纯文本消费（composer 预填 + agent 首轮 prompt），在此边界解掉转义层（M5-bx）。
    pub fn requirement_text(&self) -> Option<String> {
        [
            self.quick_create_prompt.as_deref(),
            self.chat_message.as_deref(),
            self.trigger_comment_content.as_deref(),
            self.autopilot_description.as_deref(),
            self.handoff_note.as_deref(),
            self.issue_description.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .map(markdown_source_to_plain_text)
    }
}

/// selective claim 请求（接入方案 B2：body 恒 `{}`）。
///
/// 服务端 claim 处理器**不解码请求体**（webank `daemon.go:2508,2671`）——续跑指针
/// （`prior_session_id` / `parent_task_id`）全由**响应**回填，非请求传入。故 body 恒为空对象。
#[derive(Debug, Serialize)]
pub struct ClaimRequest {}

/// claim 响应：`{ "task": <RemoteTask> }`（接入方案 B2 / line 578）。
#[derive(Debug, Deserialize)]
pub struct ClaimResponse {
    pub task: RemoteTask,
}

/// pending 列表响应容错：包装 `{tasks:[...]}` 或裸数组 `[...]`。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TasksListResponse {
    Wrapped { tasks: Vec<RemoteTask> },
    Bare(Vec<RemoteTask>),
}

/// start 请求（接入方案 C2：body `{force_fresh_session}`，rerun 整任务重跑时 true）。
#[derive(Debug, Serialize)]
pub struct StartRequest {
    pub force_fresh_session: bool,
}

/// `GET /tasks/{tid}/status` 响应（接入方案 C5：`{ "status": "running" }`）。
#[derive(Debug, Deserialize)]
pub struct TaskStatusResponse {
    pub status: String,
}

// ===== 终态对账 + 任务重新执行（multica 任务重试改造）=====

/// batch-status 请求 body（`POST /api/daemon/tasks/batch-status`）。
///
/// 上限 500 id/批（超出 server 400，调用方按批分片）；重复 id server 自动去重——调用方无需预处理。
#[derive(Debug, Serialize)]
pub struct BatchTaskStatusRequest {
    pub task_ids: Vec<String>,
}

/// batch-status 响应行（响应为数组，可为空——全部 id 都不认识时）。
///
/// 只含 token 工作空间持有的任务；缺失 id = 不存在/非本工作空间/非 issue 任务（三者不可区分，
/// 调用方按「无对账数据」处理，不据此清除本地值）。`issue_status_category`
/// （`todo`|`backlog`|`in_progress`|`done`|`canceled`）是客户端展示/动作判据；
/// `issue_status_name` 仅自定义状态时携带。字段全 `#[serde(default)]`：缺字段不阻断解析。
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct TaskIssueStatus {
    #[serde(default)]
    pub task_id: String,
    /// server 侧任务状态（参考值；本地终态行的 status 是本地执行事实，对账不覆写它）。
    #[serde(default)]
    pub task_status: String,
    #[serde(default)]
    pub issue_id: Option<String>,
    /// server 原始 issue 状态值（码灵不消费——category 才是判据，透传保真供诊断）。
    #[serde(default)]
    pub issue_status: Option<String>,
    #[serde(default)]
    pub issue_status_category: Option<String>,
    #[serde(default)]
    pub issue_status_name: Option<String>,
}

// ===== M4 终态上报/会话固定/重跑 wire 类型（开发设计 2.2.7 / 接入方案 C6-C8/D1）=====

/// complete 请求（接入方案 C6 + multica C6 改造：body output + session_id? + work_dir? + completion_output?）。
///
/// `output` = 会话产物摘要；`session_id`/`work_dir` 来自 worker_ref 采集（可能缺失）。
/// `completion_output` = issue 交付说明（multica 改造清单 §1：随 complete 同事务写入
/// `issue.completion_output`，后写胜；≤16_000 rune，超长 400 且任务不置 completed）：None 时键
/// 整体缺席（不动 issue 现值）；仅 issue-bound 任务生效（其余 server 静默忽略）；旧 server
/// 未知键忽略——新客户端对旧 server 安全灰度。
#[derive(Debug, Serialize)]
pub struct CompleteRequest {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_output: Option<String>,
}

/// fail 请求（接入方案 C7：body error + failure_reason）。
///
/// `failure_reason` 如实传值（runtime_offline/agent_error/runtime_recovery），供 server 决定 auto-retry。
#[derive(Debug, Serialize)]
pub struct FailRequest {
    pub error: String,
    pub failure_reason: String,
}

/// PinTaskSession 请求（接入方案 C8：写 task 行 session_id/work_dir，断点续跑依据）。
#[derive(Debug, Serialize)]
pub struct PinTaskSessionRequest {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
}

/// issue 状态更新请求（接入方案 D2：`PUT /api/issues/{id}` body `{status}`）。
///
/// 码灵完成远程任务后用自身 PAT 把关联 issue 流转到 [`MULTICA_ISSUE_DONE_STATUS`]（码灵作为中介，
/// 非 agent 直调 multica API）。issue 维度接口，path 不含 workspace，靠 `X-Workspace-ID` 头路由。
///
/// multica C6 改造（2026-10）后纯状态流转：`completion_output` 改随 [`complete_task`] 携带
/// （单一写入来源），本 PUT 不再携带输出键；迟到补写走 [`UpdateIssueCompletionOutputRequest`]
/// 的单键形态。
#[derive(Debug, Serialize)]
struct UpdateIssueStatusRequest {
    status: String,
}

/// issue 详情的最小消费投影（`GET /api/issues/{id}`，M5-cf 拉取链路）。
///
/// 只解码码灵实际消费的字段：拉取侧解析自身 issue 的 `parent_issue_id`（父寻址）与轮询父
/// issue 的 `completion_output`（阶段性快照判定）。`completion_output` 为内网 C6+ 服务端
/// detail 响应才有的键（`*string` + `omitempty`——NULL 时键整个不下发）；旧服务端缺失 →
/// None（版本解耦，与 `parent_output` 的 serde-optional 惯例一致）。
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct IssueDetail {
    pub status: String,
    #[serde(default)]
    pub parent_issue_id: Option<String>,
    #[serde(default)]
    pub completion_output: Option<String>,
}

/// issue 迟到输出补发请求（issue 完成输出传递特性，多 run 场景）。
///
/// 与 [`UpdateIssueStatusRequest`] 同一 `PUT /api/issues/{id}` 端点，但**只写
/// `completion_output`、不带 `status` 键**——服务端字段级 merge「未触摸字段保留现值」，
/// 用于任务终态后的追问 run 补交交付说明的场景：issue 状态（可能已被服务端 reopen 或
/// 人工流转）不被码灵的迟到写入强行拉回 done。结构体上不存在 status 字段即契约本身。
#[derive(Debug, Serialize)]
struct UpdateIssueCompletionOutputRequest {
    completion_output: String,
}

/// DPMS 需求回写请求（M5-cl 建立；M5-cm 一度扩展为 7 字段载荷，M5-cn 收缩回单字段；
/// M5-cu 补回产品 ID 作为 join key）。
///
/// 码灵作为中介持 PAT 回写（角色只写任务记忆，不发 HTTP）。契约：body 带 `dpms_story_id`
/// （本任务创建出的 story 是哪一个）与可选的 `dpms_product_id`（在哪个产品下创建）——
/// DPMS 查询按产品隔离，二者共同构成 multica 从 DPMS 拉取字段所需的 join key；其余字段
/// 镜像（链接/负责人/发布计划等）仍由 multica 从 DPMS 拉取，码灵不搬运值（搬运时刻 ≠
/// 查看时刻，值在 DPMS 后续会变，推送注定陈旧）。`dpms_product_id` 缺省时 body 与 M5-cn
/// 单字段契约逐字节等价（旧角色未写记忆键时的灰度兼容）。幂等：同 id 重放 200；
/// 异 id 409（issue 已链接不同 story，不落任何字段）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportDpmsStoryRequest {
    /// 必填：创建的 DPMS 系统需求 id（任务记忆 `storyId`）。
    pub dpms_story_id: i64,
    /// 选填：本次创建所用产品的 id（任务记忆 `dpms.productId`）。DPMS 侧按产品隔离查询，
    /// 服务端需要它把 story id 解析到具体产品；缺失（旧角色/记忆未写）时只回传 story id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpms_product_id: Option<i64>,
}

// ===== skill 同步 wire 类型（设计 `.claude/design/multica_skill/` §3-§5）=====
//
// 心跳 ack（C-link 推送侧）与发现/导入上报 body（此文件定义），拉取 REST 响应（同上）。
// 上报结构 `status` 用 `&'static str`：值域固定 completed/failed，编译期约束 + 零 String 分配；
// Option 字段**不加** skip_serializing_if——multica 端按显式 null/缺失区分上报形态（测试锁定该契约）。

/// 心跳 ack（`POST /api/daemon/heartbeat` 响应，multica `pkg/protocol/messages.go:315-329`）。
///
/// server 只回非空 pending 字段（缺失 = 无待办）；`pending_update`/`pending_model_list` 为 v1 占位，
/// 仅反序列化不消费（模型/更新推送后续版本接入）。单数旧字段 `pending_local_skill_import` 不在结构体上，
/// serde 未知字段容忍即忽略——码灵已声明 `supports_batch_import: true`，服务端按批量形式下发。
#[derive(Debug, Deserialize)]
pub struct HeartbeatAck {
    pub status: String,
    pub pending_local_skills: Option<PendingLocalSkillsRef>,
    pub pending_local_skill_imports: Option<Vec<PendingLocalSkillImportRef>>,
    /// v1 占位不消费。
    pub pending_update: Option<serde_json::Value>,
    /// v1 占位不消费。
    pub pending_model_list: Option<serde_json::Value>,
    /// runtime-assets（角色共享）工单引用（通道 B；与 skill 待办同模型：server 只回非空 pending
    /// 字段，缺失 = 无待办）。每次心跳最多一个工单，多个排队靠后续心跳逐个消化（server 侧限速）。
    /// 处理 spawn 独立任务（`runtime_assets::handle_runtime_assets_request`），不阻塞心跳 tick。
    pub pending_runtime_assets: Option<PendingRuntimeAssetsRef>,
    /// 就绪变化 diff（story dev/test 拆分，R8 方案 1；wire 最终形态由 multica 定稿，
    /// 码灵按此提案实现，不一致时仅调整本结构反序列化）。
    ///
    /// 码灵**只把它当刷新信号**：非空 → 发 `remote-tasks-updated` 事件 → 页面重取 pending
    /// （pending 列表是唯一事实源，不做本地 is_ready 增量 patch）。旧 server 不发 → None。
    #[serde(default)]
    pub pending_readiness_changes: Option<Vec<ReadinessChange>>,
}

/// 心跳 ack：本地 skill 发现待办引用（仅 requestId，详情由发现处理自行扫描）。
#[derive(Debug, Deserialize)]
pub struct PendingLocalSkillsRef {
    pub id: String,
}

/// 心跳 ack：就绪变化 diff 的单条记录（`{task_id, is_ready}`）。
///
/// 与 ack 现有 `pending_local_skill_imports` 等复数 pending 字段同构。仅作刷新信号消费，
/// `is_ready` 的权威事实源是 pending/claim/detail 载荷（见 [`HeartbeatAck`] 注释）。
///
/// 两字段带 `#[serde(default)]`：wire 形态尚未定稿（本字段是码灵提案），字段命名/取值一旦与提案
/// 不一致，解析必须退化为「有变化但内容不可用」而不是 `Err`——ack 是**整条**反序列化的，失败会连带
/// 跳过同一 tick 的 skill 待办派发，并只留一句通用心跳失败日志（故障不可归因）。
/// 码灵只消费「diff 是否非空」，条目内容不参与任何判定，故缺省值不影响语义。
#[derive(Debug, Default, Deserialize)]
pub struct ReadinessChange {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub is_ready: bool,
}

/// 心跳 ack：本地 skill 导入待办引用（requestId + 目标 skill_key）。
#[derive(Debug, Deserialize)]
pub struct PendingLocalSkillImportRef {
    pub id: String,
    pub skill_key: String,
}

/// 心跳 ack：runtime-assets（角色共享）工单引用（requestId + 建议 action）。
///
/// action 只是建议性预分类，**权威 action 以工单详情为准**（详情 GET 是认领后的必经步骤），
/// 分发器不依赖本字段判定工单类型。
#[derive(Debug, Deserialize)]
pub struct PendingRuntimeAssetsRef {
    pub id: String,
    #[serde(default)]
    pub action: String,
}

/// 发现上报 body（`POST .../local-skills/{requestId}/result`，multica `runtime_local_skills.go`）。
#[derive(Debug, Serialize)]
pub struct LocalSkillListReport {
    pub status: &'static str,
    pub skills: Vec<LocalSkillSummary>,
    pub supported: bool,
    /// 码灵不支持 MCP skill 上报，恒 false（如实上报能力，server 据此不下发 MCP 待办）。
    pub mcp_supported: bool,
    pub error: Option<String>,
}

/// 发现上报的单个 skill summary。`key` = 本地目录名（canonical 身份，导入待办回传它定位目录）；
/// `root` 恒 `"provider"`（v1 只上报码灵自有全局库一层）。
#[derive(Debug, Serialize)]
pub struct LocalSkillSummary {
    pub key: String,
    pub name: String,
    pub description: String,
    pub source_path: String,
    pub provider: String,
    pub root: &'static str,
    pub file_count: i64,
}

/// 导入上报 body（`POST .../local-skills/import/{requestId}/result`）。
#[derive(Debug, Serialize)]
pub struct LocalSkillImportReport {
    pub status: &'static str,
    pub skill: Option<LocalSkillBundle>,
    pub error: Option<String>,
}

/// 导入上报的 skill 打包。`content` = SKILL.md **原文**（含 frontmatter 与附加字段，Q5 修正：原样透传）。
#[derive(Debug, Serialize)]
pub struct LocalSkillBundle {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source_path: String,
    pub provider: String,
    pub files: Vec<LocalSkillFile>,
}

/// 导入上报的支撑文件（`path` 为相对 skill 目录的 `/` 分隔路径）。
#[derive(Debug, Serialize)]
pub struct LocalSkillFile {
    pub path: String,
    pub content: String,
}

/// 拉取：`GET /api/skills` 列表项（summary 不含 content——列表性能刻意省略，正文按需 GET 详情）。
#[derive(Debug, Deserialize)]
pub struct RemoteSkillSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub updated_at: Option<String>,
}

/// 拉取：`GET /api/skills/{id}` 详情（content 可能带 frontmatter——multica DB 不强制 body-only，
/// 组装端判别剥离，见 `local_skills::assemble_pulled_skill_md`）。
#[derive(Debug, Deserialize)]
pub struct RemoteSkillDetail {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub content: String,
}

/// 拉取：`GET /api/skills/{id}/files` 的支撑文件项（端点默认 `includeContent=true` 返回正文）。
///
/// files 列表**不含根 SKILL.md**——它单独由 detail 的 `content` 承载（multica 契约硬约定，
/// 每层显式排除：`builtin_skills.go:85-93` WalkDir 跳过、daemon `local_skills.go:363` 发现排除、
/// `execenv/context.go:976-994` 落盘兜底去重）。落库侧仍校验防御（拒绝根 `SKILL.md` 混入）。
#[derive(Debug, Deserialize)]
pub struct RemoteSkillFile {
    pub path: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub content: String,
}

/// 拉取：files 响应容错包装（`{files:[...]}` 或裸数组，照搬 [`WorkspacesResponse`] untagged 惯例）。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SkillFilesResponse {
    Wrapped { files: Vec<RemoteSkillFile> },
    Bare(Vec<RemoteSkillFile>),
}

// ===== runtime-assets（角色/工作流共享通道 B）wire 类型 =====
//
// 设计：角色 `.claude/design/multica-role-management/` §4；工作流
// `.claude/design/multica-workflow-management/码灵对接说明(1).md` §4。两类资产共用同一组
// 端点与工单形态，由详情 `asset_type`（role / workflow）区分；三种工单由 action/item_id 区分：
// - list（盘点）：`GET .../assets/request/{id}` → 枚举本地资产，**只报摘要不报正文**；
// - export（提取）：action=install 且 item_id 空、payload={asset_key} → 按 key 读完整资产回传；
// - push-install（推送安装）：action=install 且 item_id 非空、payload 即资产正文 → 落库 + 回报名字。

/// `GET /api/daemon/runtimes/{rid}/assets/request/{requestId}` —— 工单详情（权威 action 来源）。
///
/// 只建模消费字段（id/slug/version 等信息性字段不承载；serde 未知字段容忍即忽略）。
/// `payload` 形态随工单变化（export 是 `{asset_key}`、push-install 是资产正文），故承载为
/// `Value` 由处理侧按工单解析（`runtime_assets::classify_runtime_assets_request`）。
#[derive(Debug, Deserialize)]
pub struct RuntimeAssetsRequest {
    /// `list` | `install` | `uninstall`（详情权威值；ack 里的 action 只是建议）。wire 上
    /// export 仍是 `install`（multica 兼容已部署客户端的别名：item_id 空 + payload.asset_key
    /// 即 export）。
    pub action: String,
    /// `role` | `workflow`（list 工单也以详情该字段为权威——ack 对 list 不带 asset_type，
    /// 必须先 GET 再回报；空 = 尚未分化的旧 server，按 role 容忍）。
    #[serde(default)]
    pub asset_type: String,
    /// 空 = export（本机提取），非空（market item uuid）= push-install（推送安装）。
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// export 工单详情 payload（`{"asset_key":"pf-my-role"}`；两类资产同形，key 空间各归各）。
#[derive(Debug, Deserialize)]
pub struct RuntimeAssetsExportPayload {
    pub asset_key: String,
}

/// list 工单上报 body（`POST .../assets/{requestId}/result`）。
///
/// Option 字段不加 skip_serializing_if（与 skill 上报同约定：server 按显式 null/缺失区分上报形态）。
#[derive(Debug, Serialize)]
pub struct RuntimeAssetsListReport {
    pub status: &'static str,
    pub supported: bool,
    pub assets: Vec<RuntimeAssetSummary>,
    pub error: Option<String>,
}

/// list 上报的单个资产摘要（无正文；`key` = 本地资产 id，export 工单按它回取；
/// `type` = `role` | `workflow`，web 弹窗按传入类型渲染，混类会显示错乱）。
#[derive(Debug, Serialize)]
pub struct RuntimeAssetSummary {
    pub key: String,
    pub r#type: &'static str,
    pub name: String,
    pub description: String,
    pub is_builtin: bool,
    pub updated_at: String,
}

/// export 工单回传的交付物 payload：角色交换文档或工作流模板 envelope（两类资产同端点，
/// 形态由工单 asset_type 决定；untagged 序列化即内层文档本身，不带类型标签）。
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RuntimeAssetPayload {
    Role(gold_band::app::MulticaRoleExchange),
    Workflow(gold_band::app::MulticaWorkflowExchange),
}

/// export / push-install / uninstall 工单上报 body（`POST .../assets/install/{requestId}/result`
/// 三动作共用——路径段 install 是历史命名，multica 侧约定不新建 /uninstall/.../result）。
///
/// export completed → `payload`（资产正文）；push-install completed → `result`（实际落库 key
/// + 可读展示名）；uninstall completed → `result`（`removed_key`）；failed → `error`。
#[derive(Debug, Serialize)]
pub struct RuntimeAssetsInstallReport {
    pub status: &'static str,
    pub payload: Option<RuntimeAssetPayload>,
    pub result: Option<RuntimeAssetsReportResult>,
    pub error: Option<String>,
}

/// completed 上报的 `result` 载荷（push-install 与 uninstall 双形态）。
///
/// multica 对 result 仅透传给 UI 轮询、不解析具体字段（自由 map）；untagged 序列化即内层
/// 对象本身，与该契约兼容且保留码灵侧类型安全。
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RuntimeAssetsReportResult {
    Install(RuntimeAssetsInstallResult),
    Uninstall(RuntimeAssetsUninstallResult),
}

/// push-install completed 的结果。
///
/// `installed_key` = 实际落库资产 key（role：新本地 profile.id；workflow：落库模板 id——
/// 均与 list/export 同一 key 空间，网页端可据此回查；M5-cz 字段名对齐 2026-11-13 协议文档
/// §3.3，旧字段名 `installed_name`）；`installed_display_name` = 实际落库展示名（multica 反馈
/// P3——id 形如 `pf-mcx1a2` 用户不可读；改名拷贝场景下它是**实际**名「xx-2」而非市场原名，
/// multica 侧市场名兜底无法覆盖；builtin 改名副本时必传）；`outcome` = 落库形态（#6，
/// `created` / `renamed-copy` / `reused-existing`）——服务端不再从 id/名称兜底猜测「装了副本」
/// 还是「全新安装/已装过」。
#[derive(Debug, Serialize)]
pub struct RuntimeAssetsInstallResult {
    pub installed_key: String,
    pub installed_display_name: String,
    pub outcome: &'static str,
}

/// uninstall completed 的结果：带回被卸载的 asset key 便于 multica UI / 排查对账
/// （2026-11-13 协议文档 §3.4 示例字段 `removed_key`，M5-cz 对齐；旧字段名 `uninstalled_key`）。
#[derive(Debug, Serialize)]
pub struct RuntimeAssetsUninstallResult {
    pub removed_key: String,
}

/// 进程级共享的 `reqwest::Client`（连接池/TLS 上下文复用）。
///
/// `reqwest::Client` 内部为 `Arc`：构造昂贵（建连接池 + TLS 上下文）、clone 廉价。官方明确「应创建一次并复用」，
/// 否则每次 `new` → 新连接池 → 旧 client 析构关闭连接 → 下次请求重做 TCP+TLS 握手（弱网下放大失败率）。
/// 故全进程共享一个实例，[`MulticaClient::new`] 取其廉价 clone。用 `OnceLock`（本项目 metrics.rs /
/// view_models.rs 已用同一惯用），零新增依赖。client 级 30s 超时在此设定（liveness 调用再 per-request 缩短）。
///
/// `Client::builder().build()` 在默认设置下实质不会失败（缺失 TLS provider 才会，那是进程级故障，每次 `new`
/// 都会同样失败）——故用 `expect` 在首次初始化时快速失败，而非把不可恢复的构造错误一路包成 `NetworkFailed`。
///
/// [`MulticaClient::new`]: MulticaClient::new
fn shared_http() -> &'static Client {
    static HTTP: OnceLock<Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client build with default settings must not fail")
    })
}

/// multica HTTP client。`token` 为 PAT（已登录）或登录期的临时 JWT（None=未认证）。
///
/// `Clone` 廉价（`http` 是 `reqwest::Client` 的 Arc clone；`base_url`/`token` 仅 String）——供并发扇出
/// （把 client clone 进 spawn 任务）。复用 [`shared_http`] 的连接池，杜绝每调用重建 client。
#[derive(Clone)]
pub struct MulticaClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

impl MulticaClient {
    /// 构造 client（`base_url` 应已 normalize）。`token=None` 时请求不带 Authorization。
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Result<Self, MulticaError> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err(MulticaError::NotConfigured);
        }
        Ok(Self {
            http: shared_http().clone(),
            base_url,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    /// 发送已认证请求并按状态码映射错误（map_status）。
    ///
    /// `per_request_timeout` 覆盖 client 级 30s 默认——liveness/轻量调用（取消检测）传短超时，
    /// 使单次调用在 server 慢响应时快速失败，避免拖垮整个心跳 tick（3s 周期，单 tick 阻塞会延误
    /// runtime 在线维持与取消检测）。`workspace_id` 注入 `X-Workspace-ID` 头（skill 拉取 REST 是
    /// workspace 维度接口，与 [`json_send`] 对称）。
    async fn send(
        &self,
        method: Method,
        path: &str,
        workspace_id: Option<&str>,
        per_request_timeout: Option<Duration>,
    ) -> Result<Response, MulticaError> {
        let mut req = self.http.request(method.clone(), self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        if let Some(ws) = workspace_id {
            req = req.header("X-Workspace-ID", ws);
        }
        if let Some(d) = per_request_timeout {
            req = req.timeout(d);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("{method} {path} failed: {e}")))?;
        map_status(path, resp.status())?;
        Ok(resp)
    }

    /// 发送带 JSON body 的已认证请求（统一 auth + 可选 `X-Workspace-ID` 头 + status 映射）。
    ///
    /// `post_json` / issue PUT 的共用底座——杜绝重复请求构造
    /// （否则 issue PUT 会带来另一份几乎相同的 auth+send+map_status 模板）。用 `self.token`（PAT）做
    /// Bearer；create_token（登录期用临时 JWT）token 来源不同，仍走自有请求。返回 `Response` 供调用方
    /// 按需解码（complete/fail/issue PUT 丢弃 body，故不解码）。
    async fn json_send<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        workspace_id: Option<&str>,
        body: &T,
        per_request_timeout: Option<Duration>,
    ) -> Result<Response, MulticaError> {
        let mut req = self.http.request(method.clone(), self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        if let Some(ws) = workspace_id {
            req = req.header("X-Workspace-ID", ws);
        }
        if let Some(d) = per_request_timeout {
            req = req.timeout(d);
        }
        let resp = req
            .json(body)
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("{method} {path} failed: {e}")))?;
        map_status(path, resp.status())?;
        Ok(resp)
    }

    /// 发送带 body 的已认证 POST 并按状态码映射错误，返回反序列化结果。
    ///
    /// 用 `self.token`（PAT）做 Bearer；create_token（登录期用临时 JWT）因 token 来源不同，仍走自有请求。
    async fn post_json<T, R>(&self, path: &str, body: &T) -> Result<R, MulticaError>
    where
        T: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let resp = self.json_send(Method::POST, path, None, body, None).await?;
        resp.json::<R>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
    }

    /// 一般请求网络错误重试（开发设计 2.3：网络错误重试 3 次，4xx 不重试直接映射错误码）。
    ///
    /// 仅对 `NetworkFailed`（reqwest 传输/超时/解码错误）退避重试；AuthFailed/ClaimConflict/
    /// TaskNotFound 等确定错误码立即返回，不浪费重试次数。
    async fn with_network_retry<T, F, Fut>(
        &self,
        operation: &str,
        mut attempt: F,
    ) -> Result<T, MulticaError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, MulticaError>>,
    {
        let mut last: Option<MulticaError> = None;
        for n in 0..NETWORK_RETRY_ATTEMPTS {
            match attempt().await {
                Ok(value) => return Ok(value),
                Err(err @ MulticaError::NetworkFailed(_)) => {
                    last = Some(err);
                    if n + 1 < NETWORK_RETRY_ATTEMPTS {
                        let backoff = NETWORK_RETRY_BASE_SECS.saturating_mul(1u64 << n.min(5));
                        tokio::time::sleep(Duration::from_secs(backoff)).await;
                    }
                }
                Err(other) => return Err(other),
            }
        }
        Err(last.unwrap_or_else(|| {
            MulticaError::NetworkFailed(format!("{operation} failed after retries"))
        }))
    }

    /// 终态上报（complete/fail）严格重试（开发设计第 5 章：4/8/16/32/64s 共 6 次，服务端幂等）。
    ///
    /// 与 `with_network_retry` 区别：退避更长更密（确保终态送达，服务端 complete/fail 幂等），
    /// 共 6 次（初始 + `TERMINAL_RETRY_SCHEDULE_SECS.len()` 次退避重试）。仍仅对 `NetworkFailed`
    /// （传输/5xx/解码）重试；AuthFailed/TaskNotFound 等确定错误码立即返回（重试无意义）。
    async fn with_terminal_retry<T, F, Fut>(
        &self,
        operation: &str,
        mut attempt: F,
    ) -> Result<T, MulticaError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, MulticaError>>,
    {
        let mut last: Option<MulticaError> = None;
        // 初始尝试 + 每个退避间隔一次重试 = schedule.len()+1 次（开发设计：6 次）。
        let total = TERMINAL_RETRY_SCHEDULE_SECS.len() + 1;
        for n in 0..total {
            match attempt().await {
                Ok(value) => return Ok(value),
                Err(err @ MulticaError::NetworkFailed(_)) => {
                    last = Some(err);
                    if n < TERMINAL_RETRY_SCHEDULE_SECS.len() {
                        tokio::time::sleep(Duration::from_secs(TERMINAL_RETRY_SCHEDULE_SECS[n]))
                            .await;
                    }
                }
                Err(other) => return Err(other),
            }
        }
        Err(last.unwrap_or_else(|| {
            MulticaError::NetworkFailed(format!("{operation} failed after terminal retries"))
        }))
    }

    // ===== M1 登录相关 =====

    /// `GET /api/me` —— 验证 PAT 有效，返回用户信息。
    pub async fn verify_pat(&self) -> Result<UserInfo, MulticaError> {
        let resp = self.send(Method::GET, "/api/me", None, None).await?;
        resp.json::<UserInfo>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode /api/me failed: {e}")))
    }

    /// `POST /api/tokens` —— 用 JWT 换 PAT（browser_login 第二步）。
    /// `jwt` 为回调收到的临时 JWT；返回长期 PAT（`mul_...`）。
    pub async fn create_token(
        &self,
        jwt: &str,
        name: &str,
        expires_in_days: u32,
    ) -> Result<String, MulticaError> {
        let body = CreateTokenRequest {
            name: name.to_string(),
            expires_in_days,
        };
        let resp = self
            .http
            .request(Method::POST, self.url("/api/tokens"))
            .bearer_auth(jwt)
            .json(&body)
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("POST /api/tokens failed: {e}")))?;
        map_status("/api/tokens", resp.status())?;
        let token_resp = resp
            .json::<TokenResponse>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode /api/tokens failed: {e}")))?;
        if token_resp.token.trim().is_empty() {
            return Err(MulticaError::AuthFailed(
                "empty PAT in /api/tokens response".into(),
            ));
        }
        Ok(token_resp.token)
    }

    /// `GET /api/workspaces` —— workspace 成员列表（容错包装/裸数组）。
    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>, MulticaError> {
        let resp = self
            .send(Method::GET, "/api/workspaces", None, None)
            .await?;
        let parsed = resp.json::<WorkspacesResponse>().await.map_err(|e| {
            MulticaError::NetworkFailed(format!("decode /api/workspaces failed: {e}"))
        })?;
        Ok(match parsed {
            WorkspacesResponse::Wrapped { workspaces } => workspaces,
            WorkspacesResponse::Bare(v) => v,
        })
    }

    // ===== M2 daemon 注册/心跳/恢复（开发设计 2.6 / 4.2）=====

    /// `POST /api/daemon/register` —— 全量注册（幂等），取回 server 分配的 runtime_id 列表。
    ///
    /// 一般请求：网络错误重试 3 次，4xx 不重试（直接映射错误码）。用于**一次性**调用（启动 / connect /
    /// 绑定即时）——这些路径无上层循环兜底，故在 client 内退避重试。
    pub async fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse, MulticaError> {
        self.with_network_retry("register", || async {
            self.post_json("/api/daemon/register", req).await
        })
        .await
    }

    /// `POST /api/daemon/register` —— 单次注册（无 client 内重试 + liveness 短超时），供常驻心跳 tick 自愈用。
    ///
    /// 与 [`register`](MulticaClient::register) 的区别：后者带 `with_network_retry`（3 次，单次最长 30s），
    /// 适合一次性调用。而自愈注册由 3s 心跳 tick 驱动——**循环即重试**；若再嵌套 client 内 3×30s 退避，
    /// 弱网下单 tick 可超 90s，阻塞后续心跳/取消检测（runtime 离线超 150s 会被 sweeper 回收）。
    /// 故自愈路径用单次 register + per-request 短超时：失败下 tick 自然重试，单 tick 耗时有界。
    pub async fn register_once(
        &self,
        req: &RegisterRequest,
    ) -> Result<RegisterResponse, MulticaError> {
        let resp = self
            .json_send(
                Method::POST,
                "/api/daemon/register",
                None,
                req,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        resp.json::<RegisterResponse>().await.map_err(|e| {
            MulticaError::NetworkFailed(format!("decode /api/daemon/register failed: {e}"))
        })
    }

    /// `POST /api/daemon/heartbeat` —— 维持 runtime 在线（常驻 3s）并取回待办 ack。
    ///
    /// body `{runtime_id, supports_batch_import: true}`。响应携带 skill 发现/导入待办
    /// （[`HeartbeatAck`]，仅非空时下发）——调用方（心跳 tick）据 ack 分发处理，**绝不**在本方法内
    /// 处理待办（保持 liveness 单次请求短超时有界，待办处理 spawn 独立任务）。失败仅记日志
    /// （下一 tick 自然重试），不在 client 内重试（循环即重试）。走单次请求 + **liveness 短超时**
    /// （覆盖 client 级 30s），防 server 慢响应拖垮整个心跳 tick。
    pub async fn heartbeat(&self, runtime_id: &str) -> Result<HeartbeatAck, MulticaError> {
        let body = serde_json::json!({
            "runtime_id": runtime_id,
            "supports_batch_import": true,
        });
        let resp = self
            .json_send(
                Method::POST,
                "/api/daemon/heartbeat",
                None,
                &body,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        resp.json::<HeartbeatAck>().await.map_err(|e| {
            MulticaError::NetworkFailed(format!("decode /api/daemon/heartbeat ack failed: {e}"))
        })
    }

    /// `POST /api/daemon/runtimes/{rid}/recover-orphans` —— 启动时清理残留的在飞任务（无条件置失败态）。
    ///
    /// 失败不阻断启动（仅记日志）；server 对 `runtime_recovery` 等 retryable reason 自动重试（本期接受）。
    pub async fn recover_orphans(&self, runtime_id: &str) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/recover-orphans");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    // ===== skill 同步：发现/导入上报 + 拉取 REST（设计 §3-§5）=====

    /// `POST /api/daemon/runtimes/{rid}/local-skills/{requestId}/result` —— 上报发现结果。
    ///
    /// 一般请求（`with_network_retry` 3 次）：上报由 spawn 出的独立任务驱动，无循环兜底，重试在 client 内
    /// 完成；3 次仍失败由调用方记日志放弃，服务端 60s running 超时终态化兜底（multica 对迟到/重复上报幂等）。
    pub async fn report_local_skill_list_result(
        &self,
        runtime_id: &str,
        request_id: &str,
        report: &LocalSkillListReport,
    ) -> Result<(), MulticaError> {
        self.with_network_retry("report_local_skill_list", || async {
            let path =
                format!("/api/daemon/runtimes/{runtime_id}/local-skills/{request_id}/result");
            self.json_send(Method::POST, &path, None, report, None)
                .await?;
            Ok(())
        })
        .await
    }

    /// `POST /api/daemon/runtimes/{rid}/local-skills/import/{requestId}/result` —— 上报导入结果。
    pub async fn report_local_skill_import_result(
        &self,
        runtime_id: &str,
        request_id: &str,
        report: &LocalSkillImportReport,
    ) -> Result<(), MulticaError> {
        self.with_network_retry("report_local_skill_import", || async {
            let path = format!(
                "/api/daemon/runtimes/{runtime_id}/local-skills/import/{request_id}/result"
            );
            self.json_send(Method::POST, &path, None, report, None)
                .await?;
            Ok(())
        })
        .await
    }

    // ===== runtime-assets（角色共享通道 B）：详情 + list/install 上报 =====

    /// `GET /api/daemon/runtimes/{rid}/assets/request/{requestId}` —— 工单详情（权威 action）。
    ///
    /// 由 spawn 出的处理任务调用（非心跳 tick 内）；404（server 内存态请求已消失/重启）按确定
    /// 错误立即返回，处理侧记日志放弃该工单（不重试——server 侧请求已不存在，重试无意义）。
    pub async fn get_runtime_assets_request(
        &self,
        runtime_id: &str,
        request_id: &str,
    ) -> Result<RuntimeAssetsRequest, MulticaError> {
        self.with_network_retry("get_runtime_assets_request", || async {
            let path = format!("/api/daemon/runtimes/{runtime_id}/assets/request/{request_id}");
            let resp = self.send(Method::GET, &path, None, None).await?;
            resp.json::<RuntimeAssetsRequest>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
        })
        .await
    }

    /// `POST /api/daemon/runtimes/{rid}/assets/{requestId}/result` —— list（盘点）工单上报。
    pub async fn report_runtime_assets_list_result(
        &self,
        runtime_id: &str,
        request_id: &str,
        report: &RuntimeAssetsListReport,
    ) -> Result<(), MulticaError> {
        self.with_network_retry("report_runtime_assets_list", || async {
            let path = format!("/api/daemon/runtimes/{runtime_id}/assets/{request_id}/result");
            self.json_send(Method::POST, &path, None, report, None)
                .await?;
            Ok(())
        })
        .await
    }

    /// `POST /api/daemon/runtimes/{rid}/assets/install/{requestId}/result` —— export/push-install/uninstall
    /// 上报（三动作共用端点，install 路径段为 multica 历史命名）。
    pub async fn report_runtime_assets_install_result(
        &self,
        runtime_id: &str,
        request_id: &str,
        report: &RuntimeAssetsInstallReport,
    ) -> Result<(), MulticaError> {
        self.with_network_retry("report_runtime_assets_install", || async {
            let path =
                format!("/api/daemon/runtimes/{runtime_id}/assets/install/{request_id}/result");
            self.json_send(Method::POST, &path, None, report, None)
                .await?;
            Ok(())
        })
        .await
    }

    /// `GET /api/skills` —— workspace 维度远端 skill 列表（`X-Workspace-ID` 头路由，PAT 认证）。
    ///
    /// 列表只含 summary（无 content），正文/文件在拉取落库时按需 GET 详情，避免列表 N+1 预取。
    pub async fn list_remote_skills(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<RemoteSkillSummary>, MulticaError> {
        self.with_network_retry("list_remote_skills", || async {
            let resp = self
                .send(Method::GET, "/api/skills", Some(workspace_id), None)
                .await?;
            resp.json::<Vec<RemoteSkillSummary>>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode /api/skills failed: {e}")))
        })
        .await
    }

    /// `GET /api/skills/{id}` —— 单个远端 skill 详情（含 content）。
    pub async fn get_remote_skill(
        &self,
        workspace_id: &str,
        skill_id: &str,
    ) -> Result<RemoteSkillDetail, MulticaError> {
        self.with_network_retry("get_remote_skill", || async {
            let path = format!("/api/skills/{skill_id}");
            let resp = self
                .send(Method::GET, &path, Some(workspace_id), None)
                .await?;
            resp.json::<RemoteSkillDetail>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
        })
        .await
    }

    /// `GET /api/skills/{id}/files` —— 远端 skill 支撑文件（含正文；多文件拉取消费）。
    ///
    /// 端点默认 `includeContent=true`（multica 答复调整 2，`skill.go:2430` + `resolveSkillInclude`），
    /// 返回全部支撑文件正文；根 SKILL.md 不在列表中（见 [`RemoteSkillFile`] 注释）。落库前由
    /// `gold_band::skill::validate_skill_bundle_entries` 复核尺寸/路径契约，超限整项 failed。
    pub async fn get_remote_skill_files(
        &self,
        workspace_id: &str,
        skill_id: &str,
    ) -> Result<Vec<RemoteSkillFile>, MulticaError> {
        self.with_network_retry("get_remote_skill_files", || async {
            let path = format!("/api/skills/{skill_id}/files");
            let resp = self
                .send(Method::GET, &path, Some(workspace_id), None)
                .await?;
            let parsed = resp
                .json::<SkillFilesResponse>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))?;
            Ok(match parsed {
                SkillFilesResponse::Wrapped { files } => files,
                SkillFilesResponse::Bare(v) => v,
            })
        })
        .await
    }

    // ===== M3 任务列表/领取/启动/状态（开发设计 2.3 / 接入方案 B/C）=====

    /// `GET /api/daemon/runtimes/{rid}/tasks/pending` —— 只读 queued/dispatched 列表（接入方案 B1）。
    ///
    /// 调用方按 `status == "queued"` 过滤真正可领取的（dispatched 已被自己领过）。一般请求：网络错误重试 3 次。
    pub async fn list_pending_tasks(
        &self,
        runtime_id: &str,
    ) -> Result<Vec<RemoteTask>, MulticaError> {
        self.with_network_retry("list_pending", || async {
            let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/pending");
            let resp = self.send(Method::GET, &path, None, None).await?;
            let parsed = resp
                .json::<TasksListResponse>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))?;
            Ok(match parsed {
                TasksListResponse::Wrapped { tasks } => tasks,
                TasksListResponse::Bare(v) => v,
            })
        })
        .await
    }

    /// `GET /api/daemon/runtimes/{rid}/tasks/{tid}` —— 只读取单个任务详情（接入方案 B3，claim-at-send）。
    ///
    /// claim-at-send：点击「认领执行」只读拉需求正文 + 续跑血缘预填 composer、绑定 chip，**不**改 server
    /// 任务状态（任务仍 queued）；真正 claim（pending→dispatched）推迟到用户点「发送」时由
    /// [`claim_specific_task`] 执行。响应即裸 `AgentTaskResponse`（与 pending 列表项同构 +
    /// `resolveTaskRequirementBody` 回填的需求来源字段），Rust 直接反序列化成 [`RemoteTask`]。一般请求：
    /// 网络错误重试 3 次，404 直接映射 [`TaskNotFound`](MulticaError::TaskNotFound)。
    ///
    /// 注意：该端点**不**回填 claim-only 富字段（skills / mcp overlay / `prior_session_id` /
    /// connected_apps / repos / 用户上下文等）——这些在发送时由 [`claim_specific_task`] 取回。
    pub async fn get_task_requirement(
        &self,
        runtime_id: &str,
        task_id: &str,
    ) -> Result<RemoteTask, MulticaError> {
        self.with_network_retry("get_task_requirement", || async {
            let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}");
            let resp = self.send(Method::GET, &path, None, None).await?;
            resp.json::<RemoteTask>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
        })
        .await
    }

    /// `POST /api/daemon/runtimes/{rid}/tasks/{tid}/claim` —— selective claim（点哪领哪，接入方案 B2）。
    ///
    /// body 恒 `{}`（服务端不读请求体）；续跑指针（`parent_task_id` / `prior_session_id`）由**响应**回填，
    /// 调用方从返回的 `RemoteTask` 消费。返回值含 `auth_token`（执行凭证）+ 续跑血缘。
    /// 一般请求：网络错误重试 3 次，404/409 直接映射 TaskNotFound/ClaimConflict。
    pub async fn claim_specific_task(
        &self,
        runtime_id: &str,
        task_id: &str,
    ) -> Result<RemoteTask, MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}/claim");
        let body = ClaimRequest {};
        self.with_network_retry("claim", || async {
            let resp: ClaimResponse = self.post_json(&path, &body).await?;
            Ok(resp.task)
        })
        .await
    }

    /// `GET /api/attachments/{id}/download` —— issue 附件下载（M5-bu，任务作用域通道；文件本体路由，
    /// 基路径 `/api/attachments/{id}` 是元数据 JSON 路由，§12.63）。
    ///
    /// 鉴权用 claim 响应的 mat_ 任务令牌（[`RemoteTask::auth_token`]），不用 PAT——server 侧任务
    /// 附件是任务作用域资源（multica 既定设计：mat_ 按需下载）。DPMS 附件由服务端代理实时流转发，
    /// 响应体即文件流。chunk 循环流式写入 `destination`（tokio 异步文件写，不在内存聚合整个文件）；
    /// 调用方负责 destination 的目录创建与文件名清洗。
    ///
    /// **单次尝试、不重试**：失败由调用方按逐文件软失败处理（局部失败隔离）。状态码不走 `map_status`
    /// （其 404→TaskNotFound 语义对附件不成立），统一按附件维度映射 `NetworkFailed`。
    pub async fn download_issue_attachment(
        &self,
        attachment_id: &str,
        task_token: &str,
        destination: &std::path::Path,
    ) -> Result<(), MulticaError> {
        // 路由语义（webank file.go）：`/api/attachments/{id}` 是元数据 JSON 路由（GetAttachmentByID），
        // 文件本体在 `/download` 子路径（DownloadAttachment；CloudFront 302 / presign 302 / proxy 直出
        // 三模式——共享 client 默认跟随重定向，跨主机自动剥 Authorization，对签名 URL 正确）。
        let path = format!("/api/attachments/{attachment_id}/download");
        let mut resp = self
            .http
            .request(Method::GET, self.url(&path))
            .bearer_auth(task_token)
            .timeout(Duration::from_secs(ATTACHMENT_DOWNLOAD_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("GET {path} failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(MulticaError::NetworkFailed(format!(
                "GET {path}: HTTP {}",
                resp.status()
            )));
        }
        let mut file = tokio::fs::File::create(destination).await.map_err(|e| {
            MulticaError::NetworkFailed(format!("create attachment file failed: {e}"))
        })?;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("GET {path} body failed: {e}")))?
        {
            file.write_all(&chunk).await.map_err(|e| {
                MulticaError::NetworkFailed(format!("write attachment file failed: {e}"))
            })?;
        }
        Ok(())
    }

    /// `POST /api/daemon/runtimes/{rid}/tasks/{tid}/release` —— claim 回滚（接入方案 B4，claim-at-send 失败恢复）。
    ///
    /// claim-at-send 下「发送」先 claim（pending→dispatched）再起本地 run；若 claim 成功但本地起 run 失败
    /// （workspace 校验 / ACP run-start 失败），任务会卡在 dispatched（无本地 run、无心跳）。此方法暴露
    /// server 的 `RequeueTaskAfterClaimFailure`（CAS `dispatched→queued`）做事务性回滚，把任务还回可领取态。
    ///
    /// 主动 release 是 claim-at-send 失败的**首选恢复**（立即回可领取态供重试）；server 仍保留 prepare-lease
    /// （claim 时写 `prepare_lease_expires_at = now+45s`）与 dispatched 兜底 sweeper（`FailStaleTasks`：
    /// `dispatched_at + 300s` 未 start 即 fail）作为**被动兜底**。码灵不在 45s 内续 lease，故 lease 兜底实际
    /// 不可靠——显式 release 才是正确契约，仅在 release 不可达时由 5min dispatched sweeper 兜底。
    ///
    /// **best-effort、不重试**：仅在失败路径调用，失败只记日志。server 侧该端点对「已非 dispatched」幂等
    /// 返回 200（no-op），故即便任务已被 sweeper 回收也不会报错；真正的 404（任务不存在）映射
    /// [`TaskNotFound`](MulticaError::TaskNotFound)，调用方忽略即可。body 丢弃——只关心 HTTP 状态。
    pub async fn release_task(&self, runtime_id: &str, task_id: &str) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}/release");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    /// `POST /api/daemon/tasks/{tid}/start` —— dispatched→running（接入方案 C2）。
    ///
    /// `force_fresh_session=true` 仅 rerun 整任务重跑时（开发设计 4.4）；正常执行 false。
    /// claim 后 5 分钟内不 start 会被 server 超时 fail。
    pub async fn start_task(
        &self,
        task_id: &str,
        force_fresh_session: bool,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/start");
        let body = StartRequest {
            force_fresh_session,
        };
        let _: serde_json::Value = self.post_json(&path, &body).await?;
        Ok(())
    }

    /// `GET /api/daemon/tasks/{tid}/status` —— 取消检测（接入方案 C5）。
    ///
    /// 执行期周期查询，`cancelled`/`failed`/404 → 调用方中断本地 run。读请求不重试（下一 tick 自然重试）。
    pub async fn get_task_status(&self, task_id: &str) -> Result<String, MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/status");
        // liveness：取消检测读请求，per-request 短超时（覆盖 client 级 30s），防拖垮心跳 tick。
        let resp = self
            .send(
                Method::GET,
                &path,
                None,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        let parsed = resp
            .json::<TaskStatusResponse>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))?;
        Ok(parsed.status)
    }

    /// `POST /api/daemon/tasks/batch-status` —— 终态任务 issue 状态对账（multica 任务重试改造）。
    ///
    /// 空输入直接返回空（不发请求——server 对空批 400，那是调用方应避免的形态而非错误信号）。
    /// **单次请求、liveness 短超时、client 内不重试**：对账由页面 30s 周期驱动，循环即重试
    /// （与 heartbeat 同策略），防慢响应在对账 tick 内累积。旧 server 无此端点 → 404 →
    /// `map_status` 映射 [`TaskNotFound`](MulticaError::TaskNotFound)，调用方静默降级
    /// （无对账数据，前端不渲染 issue 状态与「重新执行」入口）——无版本协商的自然前向兼容。
    pub async fn batch_task_statuses(
        &self,
        task_ids: &[String],
    ) -> Result<Vec<TaskIssueStatus>, MulticaError> {
        if task_ids.is_empty() {
            return Ok(Vec::new());
        }
        const PATH: &str = "/api/daemon/tasks/batch-status";
        let body = BatchTaskStatusRequest {
            task_ids: task_ids.to_vec(),
        };
        let resp = self
            .json_send(
                Method::POST,
                PATH,
                None,
                &body,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        resp.json::<Vec<TaskIssueStatus>>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {PATH} failed: {e}")))
    }

    /// `POST /api/daemon/tasks/{tid}/rerun` —— 重新执行终态任务（multica 任务重试改造）。
    ///
    /// 无请求体；202 回**新任务**全量对象（与 pending/claim 的 AgentTaskResponse 同构）：新 id、
    /// `status=queued`、同 issue_id、`attribution.rerun_of_task_id` 指回源任务。源任务与 issue
    /// 状态不被修改；新任务自然进 server pending 池（调用方 re-fetch 列表即见，按 id 天然去重，
    /// 服务端按 (issue, agent) pending 槽唯一保证幂等——重复点击至多复用同一新任务）。
    ///
    /// 错误体透传：400（任务未绑 issue 等业务错误）的 body 携带 server message，经宽容提取
    /// （JSON `error`/`message` 字段，否则原文）入 `NetworkFailed(msg)` → 前端经错误码 params
    /// `{{message}}` 原样展示。故**不走** `json_send`/`map_status`（其吞 body，裸 `HTTP 400`
    /// 对用户无信息量；参照 `download_issue_attachment` 的自建请求先例）；404（任务不存在/
    /// 非本工作空间）仍映射 [`TaskNotFound`](MulticaError::TaskNotFound)。用户主动触发的一次性
    /// 动作，走一般网络重试（3 次）。
    pub async fn rerun_task(&self, task_id: &str) -> Result<RemoteTask, MulticaError> {
        self.with_network_retry("rerun", || async {
            let path = format!("/api/daemon/tasks/{task_id}/rerun");
            let mut req = self.http.request(Method::POST, self.url(&path));
            if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
                req = req.bearer_auth(t);
            }
            let resp = req
                .timeout(Duration::from_secs(LIVENESS_TIMEOUT_SECS))
                .send()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("POST {path} failed: {e}")))?;
            let status = resp.status();
            if status == StatusCode::NOT_FOUND {
                return Err(MulticaError::TaskNotFound);
            }
            if !status.is_success() {
                let message = extract_error_message(resp).await;
                return Err(MulticaError::NetworkFailed(format!(
                    "{path}: HTTP {status}: {message}"
                )));
            }
            resp.json::<RemoteTask>()
                .await
                .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
        })
        .await
    }

    // ===== M4 终态上报/会话固定/重跑（开发设计 2.5 / 接入方案 C6-C8/D1）=====

    /// `POST /api/daemon/tasks/{tid}/complete` —— 成功终态（接入方案 C6 + multica C6 改造）。
    ///
    /// 终态回调严格重试（4/8/16/32/64s 共 6 次，服务端幂等）。`output` = 会话产物摘要，
    /// `session_id`/`work_dir` 来自 worker_ref 采集（可能缺失 → 不序列化该键）。
    ///
    /// `completion_output`（multica 改造清单 §1/§2.2）：issue 交付说明随 complete 同事务写入。
    /// 携带该键收到 400（超长等业务拒绝）时**立即**降级——去字段重发一次（无退避，先于终态
    /// 重试：`map_status` 把 400 并入 `NetworkFailed`，若走 `post_json` 会对业务拒绝盲退避
    /// 6 次、任务反而送不出去），保证任务能完成；降级后仍 400 才按可重试错误走终态重试上抛。
    pub async fn complete_task(
        &self,
        task_id: &str,
        output: &str,
        session_id: Option<&str>,
        work_dir: Option<&str>,
        completion_output: Option<&str>,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/complete");
        // 降级态：携带 completion_output 的尝试收到 400 后置 false，后续重试（含降级重发后的
        // 网络重试）均不带该键——完成优先于交付说明送达（multica 改造清单 §2.2）。闭包内可变
        // 状态需跨 `FnMut` 逃逸（async future 不能借用闭包捕获），用共享原子标志承载。
        let carrying = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            completion_output.is_some(),
        ));
        let degraded_flag = std::sync::Arc::clone(&carrying);
        self.with_terminal_retry("complete", move || {
            let degraded_flag = std::sync::Arc::clone(&degraded_flag);
            let path = path.clone();
            async move {
                let carrying = degraded_flag.load(std::sync::atomic::Ordering::Relaxed);
                let body = CompleteRequest {
                    output: output.to_string(),
                    session_id: session_id.map(String::from),
                    work_dir: work_dir.map(String::from),
                    completion_output: if carrying {
                        completion_output.map(String::from)
                    } else {
                        None
                    },
                };
                let outcome = self.complete_post_once(&path, &body).await;
                match outcome {
                    CompletePostOutcome::Delivered => Ok(()),
                    CompletePostOutcome::Rejected(reason) if carrying => {
                        degraded_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                        tracing::warn!(
                            task = task_id,
                            %reason,
                            "multica complete: 400 rejected with completion_output — degraded resend without the field (completion first)"
                        );
                        let degraded = CompleteRequest {
                            output: output.to_string(),
                            session_id: session_id.map(String::from),
                            work_dir: work_dir.map(String::from),
                            completion_output: None,
                        };
                        self.complete_post_once(&path, &degraded)
                            .await
                            .into_result()
                    }
                    other => other.into_result(),
                }
            }
        })
        .await
    }

    /// `POST /api/daemon/tasks/{tid}/complete` 单次请求的分类结果（complete 专用）。
    ///
    /// 不进 `map_status`：400 必须单独分类——`map_status` 把 400 并入 `NetworkFailed` 会让
    /// [`with_terminal_retry`] 对业务拒绝盲退避 6 次、降级无从谈起。401/403/404/409 与
    /// 传输/5xx/解码沿用 `map_status` 的码表与重试语义。
    async fn complete_post_once(&self, path: &str, body: &CompleteRequest) -> CompletePostOutcome {
        // 自建请求先例（rerun_task / download_issue_attachment）：json_send 会把非 2xx 吞进
        // map_status 的裸错误码（body 丢弃），而 400 需要识别状态 + 透传 server message 供降级。
        let mut req = self.http.request(Method::POST, self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        let resp = match req.json(body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                return CompletePostOutcome::Retryable(MulticaError::NetworkFailed(format!(
                    "POST {path} failed: {e}"
                )));
            }
        };
        let status = resp.status();
        if status.is_success() {
            return CompletePostOutcome::Delivered;
        }
        if status == StatusCode::BAD_REQUEST {
            let message = extract_error_message(resp).await;
            return CompletePostOutcome::Rejected(format!(
                "{path}: HTTP 400 Bad Request: {message}"
            ));
        }
        match map_status(path, status) {
            Ok(()) => CompletePostOutcome::Delivered,
            // map_status 的 `_` 分支（5xx 等）= NetworkFailed → 可重试；401/403/404/409 → 立即上抛。
            Err(err @ MulticaError::NetworkFailed(_)) => CompletePostOutcome::Retryable(err),
            Err(err) => CompletePostOutcome::Fatal(err),
        }
    }

    /// `POST /api/daemon/tasks/{tid}/fail` —— 失败终态（接入方案 C7）。
    ///
    /// `failure_reason` 如实传值（`runtime_offline`/`agent_error`/`runtime_recovery`），
    /// 供 server 决定 auto-retry（resume-safe reason 服务端自动重试并带 prior_session_id 续跑）。
    /// 终态回调严格重试。
    pub async fn fail_task(
        &self,
        task_id: &str,
        error: &str,
        failure_reason: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/fail");
        let body = FailRequest {
            error: error.to_string(),
            failure_reason: failure_reason.to_string(),
        };
        self.with_terminal_retry("fail", || async {
            let _: serde_json::Value = self.post_json(&path, &body).await?;
            Ok(())
        })
        .await
    }

    // ===== 执行记录（transcript）实时上报 =====

    /// `POST /api/daemon/tasks/{tid}/messages` —— 执行记录批量上报。
    ///
    /// **单次尝试、不重试**：协议明确批次原子（整批成功或整批丢弃），且 seq 表上无唯一约束，
    /// 重发同 seq 会产生重复行——失败只能丢，下一批用新 seq 继续。
    /// daemon task 维度接口只用 PAT，不带 `X-Workspace-ID`；空批次由调用方跳过。
    pub async fn post_task_messages(
        &self,
        task_id: &str,
        messages: &[TranscriptMessage],
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/messages");
        let body = serde_json::json!({ "messages": messages });
        self.json_send(
            Method::POST,
            &path,
            None,
            &body,
            Some(Duration::from_secs(TRANSCRIPT_REQUEST_TIMEOUT_SECS)),
        )
        .await
        .map(|_| ())
    }

    /// `POST /api/daemon/tasks/{tid}/usage` —— token 用量上报。
    ///
    /// 服务端按 `(task_id, provider, model)` **整体替换**，所以 `usage` 必须是该任务截至当前的
    /// 累计总量（可重复上报）。同样单次尝试、不重试，失败由调用方记日志后丢弃。
    pub async fn post_task_usage(
        &self,
        task_id: &str,
        usage: &[UsageRow],
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/usage");
        let body = serde_json::json!({ "usage": usage });
        self.json_send(
            Method::POST,
            &path,
            None,
            &body,
            Some(Duration::from_secs(TRANSCRIPT_REQUEST_TIMEOUT_SECS)),
        )
        .await
        .map(|_| ())
    }

    /// `POST /api/daemon/tasks/{tid}/session` —— PinTaskSession（接入方案 C8）。
    ///
    /// 写 task 行的 session_id/work_dir（断点续跑依据；执行期 ACP 建立会话后调用）。
    /// 非终态，走一般网络重试（3 次）。
    pub async fn pin_task_session(
        &self,
        task_id: &str,
        session_id: &str,
        work_dir: Option<&str>,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/session");
        let body = PinTaskSessionRequest {
            session_id: session_id.to_string(),
            work_dir: work_dir.map(String::from),
        };
        self.with_network_retry("pin_session", || async {
            let _: serde_json::Value = self.post_json(&path, &body).await?;
            Ok(())
        })
        .await
    }

    /// `PUT /api/issues/{id}` —— 更新 issue 状态（接入方案 D2：码灵完成远程任务后流转 issue 到 done）。
    ///
    /// **码灵作为中介**：完成远程任务（[`complete_task`]）成功后，若该 task 关联了 issue，用码灵自身
    /// PAT（**非** agent 的 task-scoped `auth_token`）把 issue 状态推进到 [`MULTICA_ISSUE_DONE_STATUS`]。
    /// issue 维度接口，path 不含 workspace，靠 `X-Workspace-ID` 头路由（开发设计 4.1）。走一般网络重试
    /// （3 次）；该步骤**失败仅记日志、不阻断任务终态**——complete 已送达、任务已 done，issue 状态推进
    /// 失败由调用方兜底（issue 保持原状，不影响 multica 任务生命周期）。
    ///
    /// multica C6 改造后**纯状态流转**（body 恰为 `{status}`）：交付说明改随 [`complete_task`] 的
    /// `completion_output` 携带，本接口不再写输出键。
    ///
    /// [`complete_task`]: MulticaClient::complete_task
    pub async fn update_issue_status(
        &self,
        workspace_id: &str,
        issue_id: &str,
        status: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/issues/{issue_id}");
        let body = UpdateIssueStatusRequest {
            status: status.to_string(),
        };
        self.with_network_retry("update_issue_status", || async {
            // body 丢弃：issue 状态更新只关心 HTTP 状态（map_status 已校验），无需解码响应。
            let _resp = self
                .json_send(Method::PUT, &path, Some(workspace_id), &body, None)
                .await?;
            Ok(())
        })
        .await
    }

    /// `PUT /api/issues/{id}` —— 只补写 `completion_output`（**不带 status 键**，多 run 场景）。
    ///
    /// 任务终态后的追问 run 补交了交付说明时（首 run fail-open 标 done 时未提取到块），
    /// 由 bridge 按 local_task_id 反查 completed 历史找回 issue 关联后调用（见
    /// `multica/bridge.rs` 的 `relay_late_completion_output`）。服务端字段级 merge：
    /// 未触摸的 `status` 保留现值——迟到写入不把已 reopen/人工流转的 issue 拉回 done；
    /// 单值覆盖语义（每次 run 整体覆盖前值）与首 run 同一 PUT 内写入一致。
    ///
    /// 该步骤失败仅记日志、不重试——迟到的自愈写入，下次补交会整体覆盖。走一般网络重试（3 次）。
    pub async fn update_issue_completion_output(
        &self,
        workspace_id: &str,
        issue_id: &str,
        completion_output: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/issues/{issue_id}");
        let body = UpdateIssueCompletionOutputRequest {
            completion_output: completion_output.to_string(),
        };
        self.with_network_retry("update_issue_completion_output", || async {
            // body 丢弃：同 update_issue_status，只关心 HTTP 状态（map_status 已校验）。
            let _resp = self
                .json_send(Method::PUT, &path, Some(workspace_id), &body, None)
                .await?;
            Ok(())
        })
        .await
    }

    /// `POST /api/daemon/issues/{issue_id}/dpms-story` —— DPMS 需求回写（M5-cl 建立；M5-cn 契约
    /// 定版为关联回传；M5-cu 补产品 ID 作为 join key）。
    ///
    /// 调用方（bridge `report_dpms_story_if_changed`：早回写节点/turn 边界 + 终态兜底）在 claim 快照
    /// 命中回写条件（`dpms_required=true` 且 `system_story_id` 空）且任务记忆 `storyId` 可解析为
    /// 正整数时，把 story id（及可选的 `dpms.productId`）回传 issue——二者是 multica 从 DPMS
    /// 拉取字段所需的 join key（DPMS 查询按产品隔离）；其余字段镜像由 multica 拉取，码灵不回传。
    /// 鉴权与其余 `/api/daemon/*` 相同（PAT bearer + `X-Workspace-ID`，服务端按
    /// issue 所属 ws 做 `requireDaemonWorkspaceAccess` 范围校验）。响应体不消费（只关心 HTTP 状态）。
    ///
    /// 状态语义：200 = 落库（同 id 重放幂等，带字段时刷新）；**409 = 已链接不同 story**——本端点
    /// 专属语义，映射为 [`MulticaError::IssueLinkConflict`]（不重试：服务端不落任何字段，调用方记
    /// 录已发 body 即止）；400/404 = 参数/越权错误，同样不重试。走一般网络重试（仅 NetworkFailed）。
    pub async fn report_issue_dpms_story(
        &self,
        workspace_id: &str,
        issue_id: &str,
        body: &ReportDpmsStoryRequest,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/issues/{issue_id}/dpms-story");
        self.with_network_retry("report_issue_dpms_story", || async {
            // body 丢弃：只关心 HTTP 状态。409 在本端点是「已链接异 id」而非 claim 冲突——
            // 译为 IssueLinkConflict，避免调用方误读（ClaimConflict 语义为任务领取冲突）。
            let _resp = self
                .json_send(Method::POST, &path, Some(workspace_id), body, None)
                .await
                .map_err(|e| match e {
                    MulticaError::ClaimConflict => MulticaError::IssueLinkConflict,
                    other => other,
                })?;
            Ok(())
        })
        .await
    }

    /// `GET /api/issues/{id}` —— issue 详情（[`IssueDetail`] 最小投影，M5-cf 拉取链路）。
    ///
    /// 用途：① 解析自身 issue 的 `parent_issue_id`（父寻址，绑定时未随 claim 响应下发）；
    /// ② 拉取角色轮询父 issue 的 `completion_output`（内网确认 2026-10-24：detail 带
    /// `completion_output`，`*string` + `omitempty`）。PAT 认证 + `X-Workspace-ID` 路由，
    /// 只读接口走一般网络重试；服务端无状态门槛（读侧天然无）。
    pub async fn get_issue(
        &self,
        workspace_id: &str,
        issue_id: &str,
    ) -> Result<IssueDetail, MulticaError> {
        let path = format!("/api/issues/{issue_id}");
        self.with_network_retry("get_issue", || async {
            let resp = self
                .send(Method::GET, &path, Some(workspace_id), None)
                .await?;
            resp.json::<IssueDetail>().await.map_err(|e| {
                MulticaError::NetworkFailed(format!("decode /api/issues detail failed: {e}"))
            })
        })
        .await
    }

    /// 浏览器登录（复刻 `cmd_auth.go:240-358`）：
    ///
    /// 1. 起 IPv4 本地 listener（`127.0.0.1:0`）
    /// 2. open browser → `<app_url>/login?cli_callback=<local>&cli_state=<state>`
    /// 3. 收回调 `?token=<JWT>&state=<state>`（校验 CSRF state）
    /// 4. JWT → `POST /api/tokens` → PAT
    /// 5. PAT → `GET /api/me` 验证
    ///
    /// 返回 `(PAT, UserInfo)`。PAT 永不回显，由调用方落盘（仅暴露 `pat_set`）。
    pub async fn browser_login(
        base_url: &str,
        app_url: &str,
        client_name: &str,
        expires_in_days: u32,
    ) -> Result<(String, UserInfo), MulticaError> {
        // 1. IPv4 listener（cmd_auth.go:246 用 tcp4 避免 IPv6-only 不可达）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| {
                MulticaError::NetworkFailed(format!("bind local callback listener failed: {e}"))
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| MulticaError::NetworkFailed(format!("local listener addr failed: {e}")))?
            .port();
        let callback_url = format!("http://127.0.0.1:{port}/callback");
        // multica Web 根：登录成功后 302 回此处（登录链路已设 session cookie，见 parse_callback_request）。
        let app_root = app_url.trim_end_matches('/').to_string();

        // 2. CSRF state（cmd_auth.go:256-260，16 字节 hex）
        let state = generate_state();

        // 3. 构造 loginURL（cmd_auth.go:262）并开浏览器（cmd_auth.go:295）
        let mut login_url = url::Url::parse(app_url).map_err(|_| MulticaError::NotConfigured)?; // app_url 无效视为未配置
        login_url.set_path("/login");
        {
            let mut q = login_url.query_pairs_mut();
            q.append_pair("cli_callback", &callback_url);
            q.append_pair("cli_state", &state);
        }
        let login_url = login_url.to_string();
        if let Err(e) = open::that(&login_url) {
            tracing::warn!("multica browser_login: open browser failed: {e}");
            // 不阻断——前端可向用户展示 login_url 手动打开（M5 处理）
        }

        // 4. 等待回调（5 分钟超时，cmd_auth.go:300-308）
        let expected_state = state.clone();
        let jwt = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let (stream, _) = listener.accept().await.map_err(|e| {
                    MulticaError::NetworkFailed(format!("accept callback failed: {e}"))
                })?;
                if let Some((token, returned_state)) =
                    parse_callback_request(stream, &app_root).await
                {
                    if returned_state != expected_state {
                        // CSRF 不匹配，忽略并继续等（cmd_auth.go:276 返回 400 但 loop）
                        continue;
                    }
                    return Ok(token);
                }
            }
        })
        .await
        .map_err(|_| {
            MulticaError::NetworkFailed("timed out waiting for authentication".into())
        })??;

        // 5. JWT → PAT
        let login_client = MulticaClient::new(base_url, None)?;
        let pat = login_client
            .create_token(&jwt, client_name, expires_in_days)
            .await?;

        // 6. PAT → verify
        let pat_client = MulticaClient::new(base_url, Some(pat.clone()))?;
        let me = pat_client.verify_pat().await?;

        Ok((pat, me))
    }
}

/// 按状态码映射 multica 错误（开发设计第 5 章码表）。
fn map_status(path: &str, status: StatusCode) -> Result<(), MulticaError> {
    if status.is_success() {
        return Ok(());
    }
    match status.as_u16() {
        401 | 403 => Err(MulticaError::AuthFailed(format!("{path}: HTTP {status}"))),
        404 => Err(MulticaError::TaskNotFound),
        409 => Err(MulticaError::ClaimConflict),
        _ => Err(MulticaError::NetworkFailed(format!(
            "{path}: HTTP {status}"
        ))),
    }
}

/// [`MulticaClient::complete_post_once`] 的分类结果：`/complete` 专用，把 400 从可重试错误里
/// 拆出来（携带 `completion_output` 收到 400 → 先降级重发再谈重试，multica 改造清单 §2.2）。
#[derive(Debug)]
enum CompletePostOutcome {
    /// 2xx：终态已送达。
    Delivered,
    /// 传输/5xx/解码失败（`NetworkFailed`）——按终态语义退避重试。
    Retryable(MulticaError),
    /// 401/403/404/409：确定错误码，重试无意义，立即上抛。
    Fatal(MulticaError),
    /// HTTP 400 业务拒绝（含 server message 透传）——携带 completion_output 时先降级重发。
    Rejected(String),
}

impl CompletePostOutcome {
    /// 折叠为终态重试可消费的 `Result`：Rejected 归入 `NetworkFailed`（维持既有「400 可重试」
    /// 语义，仅降级路径提前；完成回调的失败只进日志不进 UI）。
    fn into_result(self) -> Result<(), MulticaError> {
        match self {
            Self::Delivered => Ok(()),
            Self::Retryable(err) | Self::Fatal(err) => Err(err),
            Self::Rejected(reason) => Err(MulticaError::NetworkFailed(reason)),
        }
    }
}

/// 错误响应体可读 message 的宽容提取（rerun 400 业务错误透传，码灵不猜文案只透传 server 原文）。
///
/// server 错误体形态不保证：优先按 JSON 取 `error`/`message` 字段，取不到回退 body 原文；
/// 截断至 200 字符防超长 body 进错误码 params（前端 toast 展示）。读失败（连接断等）回退空串
/// ——错误路径的锦上添花，不再产生二级错误。
async fn extract_error_message(resp: Response) -> String {
    pick_error_message(&resp.text().await.unwrap_or_default())
}

/// [`extract_error_message`] 的纯函数核（body 文本 → message），单独抽出可直测。
fn pick_error_message(body: &str) -> String {
    const MAX_MESSAGE_CHARS: usize = 200;
    let raw = if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        ["error", "message"]
            .iter()
            .find_map(|key| value.get(key).and_then(|v| v.as_str()))
            .map(str::to_string)
            .unwrap_or_else(|| body.to_string())
    } else {
        body.to_string()
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.chars().take(MAX_MESSAGE_CHARS).collect()
    }
}

/// 生成 CSRF state（16 字节随机 → 32 hex，等价 cmd_auth.go:256-260）。
fn generate_state() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 从 TCP 流解析 `GET /callback?token=<jwt>&state=<state>`。
///
/// 回写成功 HTML（cmd_auth.go:280-281）。返回 `(token, state)`；非 callback/缺参返回 None。
async fn parse_callback_request(
    stream: tokio::net::TcpStream,
    app_root: &str,
) -> Option<(String, String)> {
    let mut buf = [0u8; 4096];
    let mut stream = stream;
    let n = stream.read(&mut buf).await.ok()?;
    let request = std::str::from_utf8(&buf[..n]).ok()?;
    let request_line = request.lines().next()?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || !parts[0].eq_ignore_ascii_case("GET") {
        let _ = stream
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await;
        return None;
    }
    let raw_path = parts[1];
    let query = raw_path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut token = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "token" => token = Some(percent_decode(v)),
                "state" => state = Some(percent_decode(v)),
                _ => {}
            }
        }
    }
    // 登录成功：302 回 multica Web 根。multica-webank CLI 登录链路在 redirectToCliCallback
    // 之前已 onTokenObtained() 设置浏览器 session cookie（login-page.tsx:198-205），故用户 landed
    // 在 multica 已登录界面；码灵后台用捕获的 JWT 换 PAT。不再回写「登录成功」中间 HTML。
    let resp = callback_redirect_response(app_root);
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.flush().await;
    match (token, state) {
        (Some(t), Some(s)) if !t.is_empty() => Some((t, s)),
        _ => None,
    }
}

/// 构造登录 callback 的 302 响应：把浏览器导回 multica Web 根。
///
/// `app_root` 为 multica Web 前端根地址（可信配置来源，非 callback 入参——无开放重定向风险）；
/// 不附带任何 query，捕获到的 JWT 仅用于码灵后台换 PAT，不外泄给 multica Web。
fn callback_redirect_response(app_root: &str) -> String {
    let location = format!("{}/", app_root.trim_end_matches('/'));
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        location
    )
}

/// 简单 percent-decode（处理 `%XX` 与 `+`）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gold_band::dsl::WorkflowDsl;
    use serde_json::json;

    #[test]
    fn callback_redirect_response_is_302_to_app_root_without_query() {
        // 登录成功 callback 应 302 回 multica Web 根（非「登录成功」HTML），且不携带 token。
        let resp = callback_redirect_response("http://localhost:3000");
        assert!(resp.starts_with("HTTP/1.1 302 Found\r\n"), "应为 302 Found");
        assert!(
            resp.contains("Location: http://localhost:3000/\r\n"),
            "Location 指向 multica Web 根，末尾单斜杠"
        );
        // 尾斜杠归一：app_root 末尾无论有无 '/' 都产出同一 Location。
        let resp_trailing = callback_redirect_response("http://localhost:3000/");
        assert!(resp_trailing.contains("Location: http://localhost:3000/\r\n"));
        // 不外泄 JWT：响应不含 token query。
        assert!(!resp.contains("token="));
    }

    #[test]
    fn map_status_translates_http_codes_to_multica_errors() {
        assert!(map_status("/x", StatusCode::OK).is_ok());
        assert!(map_status("/x", StatusCode::NO_CONTENT).is_ok());
        assert!(matches!(
            map_status("/x", StatusCode::UNAUTHORIZED),
            Err(MulticaError::AuthFailed(_))
        ));
        assert!(matches!(
            map_status("/x", StatusCode::FORBIDDEN),
            Err(MulticaError::AuthFailed(_))
        ));
        assert!(matches!(
            map_status("/x", StatusCode::NOT_FOUND),
            Err(MulticaError::TaskNotFound)
        ));
        assert!(matches!(
            map_status("/x", StatusCode::CONFLICT),
            Err(MulticaError::ClaimConflict)
        ));
        assert!(matches!(
            map_status("/x", StatusCode::INTERNAL_SERVER_ERROR),
            Err(MulticaError::NetworkFailed(_))
        ));
    }

    #[test]
    fn workspaces_response_accepts_wrapped_and_bare() {
        let wrapped: WorkspacesResponse =
            serde_json::from_str(r#"{"workspaces":[{"id":"w1","name":"Acme"}]}"#).unwrap();
        assert!(matches!(wrapped, WorkspacesResponse::Wrapped { .. }));

        let bare: WorkspacesResponse =
            serde_json::from_str(r#"[{"id":"w1","name":"Acme"}]"#).unwrap();
        assert!(matches!(bare, WorkspacesResponse::Bare(v) if v.len() == 1));
    }

    #[test]
    fn percent_decode_handles_query_encoding() {
        assert_eq!(percent_decode("abc"), "abc");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%2Bb"), "a+b");
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
    }

    #[test]
    fn terminal_retry_schedule_is_monotonic_exponential() {
        // 锁定 multica postJSONWithRetry 的退避节奏（开发设计 5.x 终态上报）。
        assert_eq!(TERMINAL_RETRY_SCHEDULE_SECS, &[4, 8, 16, 32, 64]);
        for w in TERMINAL_RETRY_SCHEDULE_SECS.windows(2) {
            assert!(w[1] > w[0], "退避序列应单调递增");
        }
    }

    #[test]
    fn create_token_request_serializes_multica_body_keys() {
        let body = CreateTokenRequest {
            name: "Maling (host)".into(),
            expires_in_days: 90,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["name"], "Maling (host)");
        assert_eq!(json["expires_in_days"], 90);
    }

    #[test]
    fn register_request_serializes_multica_body_keys() {
        // 锁定 POST /api/daemon/register body 契约（开发设计 2.2.7 / 4.2）。
        let req = RegisterRequest {
            workspace_id: "ws-1".into(),
            daemon_id: "d-abc".into(),
            device_name: "maling-host".into(),
            cli_version: "0.1.0".into(),
            runtimes: vec![RuntimeSpec {
                // name = 客户端展示名（channel app_name，默认 "Gold Band"），≠ type(=provider)。
                name: "Gold Band".into(),
                runtime_type: "claude-acp".into(),
                version: "0.1.0".into(),
                status: "ready".into(),
            }],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["workspace_id"], "ws-1");
        assert_eq!(json["daemon_id"], "d-abc");
        assert_eq!(json["device_name"], "maling-host");
        assert_eq!(json["cli_version"], "0.1.0");
        // name 是展示名（独立序列化键），type 是 provider 路由键——二者分离。
        assert_eq!(json["runtimes"][0]["name"], "Gold Band");
        assert_eq!(json["runtimes"][0]["type"], "claude-acp");
        assert_eq!(json["runtimes"][0]["status"], "ready");
    }

    #[test]
    fn register_response_extracts_runtime_id_from_runtimes() {
        // 注册响应取回 runtime_id（心跳/领取/终态上报的关键键）。
        let resp: RegisterResponse =
            serde_json::from_str(r#"{"runtimes":[{"id":"rt-xyz"}]}"#).unwrap();
        assert_eq!(resp.runtimes.len(), 1);
        assert_eq!(resp.runtimes[0].id, "rt-xyz");
    }

    #[test]
    fn heartbeat_body_carries_runtime_id_and_batch_flag() {
        // 锁定心跳 body 形状（开发设计 2.6：{runtime_id, supports_batch_import:true}）。
        let body = serde_json::json!({
            "runtime_id": "rt-xyz",
            "supports_batch_import": true,
        });
        assert_eq!(body["runtime_id"], "rt-xyz");
        assert_eq!(body["supports_batch_import"], true);
    }

    #[test]
    fn remote_task_parses_with_optional_fields_missing() {
        // 除 id 外字段均可缺失（不同 server 版本字段集不同），缺字段不阻断解析。
        let task: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert_eq!(task.id, "t-1");
        assert_eq!(task.status, "queued");
        assert!(task.issue_id.is_none());
        assert!(task.auth_token.is_none());
        assert!(task.prior_session_id.is_none());

        // claim 响应字段全集。
        let full: RemoteTask = serde_json::from_str(
            r#"{"id":"t-2","issue_id":"iss-1","status":"dispatched","auth_token":"tok","prior_session_id":"sess-9"}"#,
        ).unwrap();
        assert_eq!(full.issue_id.as_deref(), Some("iss-1"));
        assert_eq!(full.auth_token.as_deref(), Some("tok"));
        assert_eq!(full.prior_session_id.as_deref(), Some("sess-9"));
    }

    #[test]
    fn remote_task_parses_attribution_rerun_lineage() {
        // rerun 新任务携带 attribution.rerun_of_task_id 指回源任务（看板「重新执行」徽标数据源）。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-new","status":"queued","issue_id":"iss-1","attribution":{"rerun_of_task_id":"t-old"}}"#,
        )
        .unwrap();
        assert_eq!(task.id, "t-new");
        assert_eq!(
            task.attribution
                .as_ref()
                .and_then(|a| a.rerun_of_task_id.as_deref()),
            Some("t-old")
        );

        // 普通任务 / 旧 server 无 attribution 键 → None（徽标不渲染，行为与引入前一致）。
        let plain: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(plain.attribution.is_none());
    }

    #[test]
    fn batch_task_status_request_serializes_task_ids() {
        // 锁定 batch-status body 契约：{task_ids: [...]}（上限分片由调用方负责，body 原样透传）。
        let body = BatchTaskStatusRequest {
            task_ids: vec!["t-1".into(), "t-2".into()],
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["task_ids"][0], "t-1");
        assert_eq!(json["task_ids"][1], "t-2");
        assert_eq!(
            json.as_object().unwrap().len(),
            1,
            "body 只有 task_ids 一个键"
        );
    }

    #[test]
    fn task_issue_status_rows_parse_leniently() {
        // 全字段落位：category 是判据、name 仅自定义状态携带。
        let rows: Vec<TaskIssueStatus> = serde_json::from_str(
            r#"[{"task_id":"t-1","task_status":"completed","issue_id":"iss-1","issue_status":"done_2","issue_status_category":"done"}]"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_id, "t-1");
        assert_eq!(rows[0].task_status, "completed");
        assert_eq!(rows[0].issue_status_category.as_deref(), Some("done"));
        assert!(rows[0].issue_status_name.is_none(), "标准分类无 name 键");

        // 自定义状态：issue_status_name 携带（展示优先于分类）。
        let custom: Vec<TaskIssueStatus> = serde_json::from_str(
            r#"[{"task_id":"t-2","issue_status_category":"todo","issue_status_name":"待产品评审"}]"#,
        )
        .unwrap();
        assert_eq!(custom[0].issue_status_name.as_deref(), Some("待产品评审"));

        // 空数组（全部 id 不认识）+ 字段缺省不阻断解析。
        let empty: Vec<TaskIssueStatus> = serde_json::from_str("[]").unwrap();
        assert!(empty.is_empty());
        let minimal: Vec<TaskIssueStatus> = serde_json::from_str(r#"[{"task_id":"t-3"}]"#).unwrap();
        assert_eq!(minimal[0].task_id, "t-3");
        assert!(minimal[0].issue_status_category.is_none());
    }

    #[test]
    fn pick_error_message_prefers_json_fields_and_truncates() {
        // JSON error 字段优先（multica 400 业务错误形态）。
        assert_eq!(
            pick_error_message(r#"{"error":"task is not bound to an issue"}"#),
            "task is not bound to an issue"
        );
        // message 字段次之；非 JSON 回退原文；空白体回退空串（不猜文案）。
        assert_eq!(pick_error_message(r#"{"message":"nope"}"#), "nope");
        assert_eq!(
            pick_error_message("plain text failure"),
            "plain text failure"
        );
        assert_eq!(pick_error_message("   "), "");
        // 超长 body 截断至 200 字符（防 params 膨胀，前端 toast 有界）。
        let long = "x".repeat(500);
        assert_eq!(pick_error_message(&long).chars().count(), 200);
    }

    #[test]
    fn remote_task_reads_thread_name_wire_key() {
        // webank 权威源：AgentTaskResponse.ThreadName → JSON `thread_name`，
        // claim 响应与 pending 列表均带此键。锁定 wire 契约 thread_name → task.title。
        let task: RemoteTask =
            serde_json::from_str(r#"{"id":"t-1","thread_name":"Fix login bug","status":"queued"}"#)
                .unwrap();
        assert_eq!(task.title.as_deref(), Some("Fix login bug"));
    }

    #[test]
    fn remote_task_reads_parent_task_id_lineage() {
        // webank AgentTaskResponse.ParentTaskID → JSON `parent_task_id`（auto-retry 子任务 T' 指向父 T）。
        // 断点续跑方案 §3.3：客户端按它反查父任务本地索引续跑。锁定 wire 契约。
        let child: RemoteTask = serde_json::from_str(
            r#"{"id":"t-child","status":"queued","parent_task_id":"t-parent"}"#,
        )
        .unwrap();
        assert_eq!(child.parent_task_id.as_deref(), Some("t-parent"));
        // 非重试任务（首发）无血缘 → None，缺字段不阻断解析。
        let first: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(first.parent_task_id.is_none());
    }

    // ===== story dev/test 拆分：issue_kind / is_ready 契约（仅提醒展示，不门控——§12.42）=====

    #[test]
    fn remote_task_parses_issue_kind_and_is_ready() {
        // 键名 `issue_kind`（multica 已确认；`kind` 已被任务来源判别符占用，不可复用）。
        // pending / claim / detail 三接口共用 AgentTaskResponse，三处载荷均带此两键。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","issue_kind":"test","is_ready":false}"#,
        )
        .unwrap();
        assert_eq!(task.issue_kind.as_deref(), Some("test"));
        assert_eq!(task.is_ready, Some(false));

        let ready: RemoteTask = serde_json::from_str(
            r#"{"id":"t-2","status":"queued","issue_kind":"test","is_ready":true}"#,
        )
        .unwrap();
        assert_eq!(ready.issue_kind.as_deref(), Some("test"));
        assert_eq!(ready.is_ready, Some(true));
    }

    #[test]
    fn remote_task_missing_issue_fields_parse_as_none() {
        // 旧 server 契约（版本解耦）：两字段皆无 → None/None，不渲染类型徽标与未就绪提醒。
        let legacy: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(legacy.issue_kind.is_none());
        assert!(legacy.is_ready.is_none());
    }

    // ===== DPMS 溯源字段（2026-09-17 daemon 任务接口新增，仅 issue 任务携带）=====

    #[test]
    fn remote_task_parses_dpms_provenance_fields() {
        // wire 契约：release_plan_id / dev_user / test_user / business_story_id / origin_url
        // 五键，列非 NULL 才上 wire（缺 key 而非 0/null），pending / detail / claim 三接口
        // 共用 AgentTaskResponse。纯溯源语义，无门控——不参与 is_ready/issue_kind 判定。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","issue_kind":"dev","is_ready":true,"release_plan_id":538181,"dev_user":"alice,bob","test_user":"carol","business_story_id":674290,"origin_url":"https://dpms.example.com/story/674290"}"#,
        )
        .unwrap();
        assert_eq!(task.release_plan_id, Some(538181));
        assert_eq!(task.dev_user.as_deref(), Some("alice,bob"));
        assert_eq!(task.test_user.as_deref(), Some("carol"));
        assert_eq!(task.business_story_id, Some(674290));
        assert_eq!(
            task.origin_url.as_deref(),
            Some("https://dpms.example.com/story/674290")
        );
    }

    #[test]
    fn remote_task_missing_dpms_fields_parse_as_none() {
        // 旧 server / 非 issue 任务（chat / autopilot / quick-create）：五键全缺省 → 全 None，
        // 不报错、不回退默认值（会话起始输入不拼溯源块）。M5-cl 三键（dpms_required /
        // system_story_id / dpms_product_id）同契约：缺键 = None（dpms_required 灰度语义
        //「缺键 = false」，门条件统一按 == Some(true) 判定）。
        let legacy: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(legacy.release_plan_id.is_none());
        assert!(legacy.dev_user.is_none());
        assert!(legacy.test_user.is_none());
        assert!(legacy.business_story_id.is_none());
        assert!(legacy.origin_url.is_none());
        assert!(legacy.dpms_required.is_none());
        assert!(legacy.system_story_id.is_none());
        assert!(legacy.dpms_product_id.is_none());
    }

    // ===== M5-cl：DPMS 分支门 / 回写 wire 契约 =====

    #[test]
    fn remote_task_parses_dpms_gate_fields() {
        // wire 契约：dpms_required（非 omitempty bool，true/false 都上线）/ system_story_id /
        // dpms_product_id（均 omitempty）。显式 false 也要可解析（回写成功后的 rerun 载荷）。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","dpms_required":true,"system_story_id":889001,"dpms_product_id":100943}"#,
        )
        .unwrap();
        assert_eq!(task.dpms_required, Some(true));
        assert_eq!(task.system_story_id, Some(889001));
        assert_eq!(task.dpms_product_id, Some(100943));
        let rerun: RemoteTask = serde_json::from_str(
            r#"{"id":"t-2","status":"queued","dpms_required":false,"system_story_id":889001}"#,
        )
        .unwrap();
        assert_eq!(rerun.dpms_required, Some(false));
        assert_eq!(rerun.system_story_id, Some(889001));
    }

    #[test]
    fn report_dpms_story_request_serializes_story_id_only() {
        // M5-cn 单字段契约；M5-cu 保持缺省形态逐字节等价（旧角色/记忆未写产品 ID）。
        let body = serde_json::to_value(ReportDpmsStoryRequest {
            dpms_story_id: 889001,
            dpms_product_id: None,
        })
        .unwrap();
        assert_eq!(body, serde_json::json!({"dpms_story_id": 889001}));
    }

    #[test]
    fn report_dpms_story_request_serializes_product_id_when_present() {
        // M5-cu：带产品 ID 时两字段同体上送（(产品, 需求) 是 multica 拉取 DPMS 的 join key）。
        let body = serde_json::to_value(ReportDpmsStoryRequest {
            dpms_story_id: 889001,
            dpms_product_id: Some(889121),
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"dpms_story_id": 889001, "dpms_product_id": 889121})
        );
    }

    #[tokio::test]
    async fn report_issue_dpms_story_posts_to_daemon_issue_endpoint() {
        // 验收：POST /api/daemon/issues/{id}/dpms-story，PAT bearer + X-Workspace-ID 路由，
        // body 单字段上送；200 即成功（响应体不消费）。
        let (base, handle) = mock_http_responses(vec![(
            200,
            r#"{"issue_id":"iss-1","dpms_story_id":889001,"story_id":889001}"#,
        )]);
        let client = test_client(base);
        client
            .report_issue_dpms_story(
                "ws-1",
                "iss-1",
                &ReportDpmsStoryRequest {
                    dpms_story_id: 889001,
                    dpms_product_id: Some(889121),
                },
            )
            .await
            .unwrap();
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request.starts_with("POST /api/daemon/issues/iss-1/dpms-story HTTP/1.1"));
        assert!(request.contains("authorization: Bearer pat-test"));
        assert!(request.contains("x-workspace-id: ws-1"));
        assert_eq!(
            request.split("\r\n\r\n").nth(1).map(str::trim),
            Some(r#"{"dpms_story_id":889001,"dpms_product_id":889121}"#)
        );
    }

    #[tokio::test]
    async fn report_issue_dpms_story_maps_409_without_retry() {
        // 验收：409（已链接异 id）→ IssueLinkConflict（非 ClaimConflict），确定错误码不重试
        // ——mock 只准备一个响应，若重试会 panic（收不到第二个请求）。
        let (base, handle) = mock_http_responses(vec![(
            409,
            r#"{"error":"issue already linked to a different DPMS story"}"#,
        )]);
        let client = test_client(base);
        let error = client
            .report_issue_dpms_story(
                "ws-1",
                "iss-1",
                &ReportDpmsStoryRequest {
                    dpms_story_id: 777001,
                    dpms_product_id: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, MulticaError::IssueLinkConflict));
        assert_eq!(handle.join().unwrap().len(), 1);
    }

    // ===== 父任务完成输出 parent_output（issue 完成输出传递特性，仅 issue 任务 + 父有输出时携带）=====

    #[test]
    fn remote_task_parses_parent_output() {
        // wire 契约：`parent_output` = 直接父 issue 的 completion_output，服务端 claim 时
        // 派生读取父行。omitempty——父无输出/无父/非 issue 任务时键整体缺席。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","issue_kind":"test","is_ready":true,"parent_output":"部署地址: https://t.example.com\n测试要点: 回归登录链路"}"#,
        )
        .unwrap();
        assert_eq!(
            task.parent_output.as_deref(),
            Some("部署地址: https://t.example.com\n测试要点: 回归登录链路")
        );
    }

    #[test]
    fn remote_task_missing_parent_output_parses_as_none() {
        // 旧 server / 父无输出 / 无父 / 非 issue 任务：键缺席 → None（会话起始输入不拼
        // 「上游交付说明」块），与拆分前行为一致（版本解耦）。
        let legacy: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(legacy.parent_output.is_none());
    }

    // ===== issue 直属附件 issue_attachments（multica dev 1be391b6，M5-bu）=====

    #[test]
    fn remote_task_parses_issue_attachments() {
        // wire 契约：issue_attachments = 所属 issue 直属附件（不含评论附件；DPMS 需求文件代理卡
        // 在其中），元素与 chat_message_attachments 同构（id / filename / content_type，后者可空）。
        // pending / detail / claim 三接口共用 AgentTaskResponse。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","issue_attachments":[{"id":"0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0","filename":"多值测试样本.zip","content_type":"application/octet-stream"},{"id":"1a2b3c4d-5e6f-7a8b-9c0d-e1f2a3b4c5d6","filename":"界面截图.png","content_type":""}]}"#,
        )
        .unwrap();
        assert_eq!(
            task.issue_attachments,
            vec![
                IssueAttachment {
                    id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".to_string(),
                    filename: "多值测试样本.zip".to_string(),
                    content_type: Some("application/octet-stream".to_string()),
                },
                IssueAttachment {
                    id: "1a2b3c4d-5e6f-7a8b-9c0d-e1f2a3b4c5d6".to_string(),
                    filename: "界面截图.png".to_string(),
                    content_type: Some(String::new()),
                },
            ]
        );
    }

    #[test]
    fn remote_task_missing_or_empty_issue_attachments_parse_as_empty() {
        // 兼容性契约：server 侧 omitempty——issue 无附件 / 非 issue 任务 / 旧 server 都不带键 →
        // 空数组（缺省不阻断解析，发送路径零下载、零行为变化）。显式空数组同样归一为空。
        let legacy: RemoteTask = serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(legacy.issue_attachments.is_empty());
        let explicit: RemoteTask =
            serde_json::from_str(r#"{"id":"t-1","status":"queued","issue_attachments":[]}"#)
                .unwrap();
        assert!(explicit.issue_attachments.is_empty());
        // content_type 键缺席（旧 server 元素形态）→ None，不阻断同元素其余字段解析。
        let no_mime: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","issue_attachments":[{"id":"a1","filename":"spec.pdf"}]}"#,
        )
        .unwrap();
        assert_eq!(
            no_mime.issue_attachments,
            vec![IssueAttachment {
                id: "a1".to_string(),
                filename: "spec.pdf".to_string(),
                content_type: None,
            }]
        );
    }

    #[test]
    fn pending_list_response_carries_readiness_fields() {
        // pending 列表是看板的数据源：包装形态与裸数组形态都必须带出两字段
        // （类型徽标 / 未就绪提醒依赖它们）。
        let wrapped: TasksListResponse = serde_json::from_str(
            r#"{"tasks":[{"id":"t-1","status":"queued","issue_kind":"test","is_ready":false},{"id":"t-2","status":"queued","issue_kind":"dev"}]}"#,
        )
        .unwrap();
        let TasksListResponse::Wrapped { tasks } = wrapped else {
            panic!("包装形态应解为 Wrapped");
        };
        assert_eq!(tasks[0].issue_kind.as_deref(), Some("test"));
        assert_eq!(tasks[0].is_ready, Some(false));
        assert_eq!(tasks[1].issue_kind.as_deref(), Some("dev"));
        assert_eq!(tasks[1].is_ready, None);

        // 裸数组形态（旧 server 兼容路径）同样解析且缺字段不阻断。
        let bare: TasksListResponse =
            serde_json::from_str(r#"[{"id":"t-1","status":"queued"}]"#).unwrap();
        assert!(matches!(bare, TasksListResponse::Bare(v) if v.len() == 1));
    }

    #[test]
    fn remote_task_requirement_text_picks_source_by_priority() {
        // 镜像 server computeTaskKind 来源互斥优先级：
        // quick-create > chat > comment > autopilot > handoff > issue_description
        // （M5-ca 起皆空即无正文——title 不进正文槽，由 vm 任务名称块承载）。
        let qc: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","quick_create_prompt":"qc-prompt"}"#,
        )
        .unwrap();
        assert_eq!(qc.requirement_text().as_deref(), Some("qc-prompt"));

        let chat: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"T","chat_message":"chat-msg"}"#)
                .unwrap();
        assert_eq!(chat.requirement_text().as_deref(), Some("chat-msg"));

        let comment: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","trigger_comment_content":"plz fix"}"#,
        )
        .unwrap();
        assert_eq!(comment.requirement_text().as_deref(), Some("plz fix"));

        // issue 带 body（webank claim 响应回填 issue_description）→ 取正文，不回退标题。
        let issue_body: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"Login bug","issue_description":"Steps: ...\nExpected: ..."}"#,
        )
        .unwrap();
        assert_eq!(
            issue_body.requirement_text().as_deref(),
            Some("Steps: ...\nExpected: ...")
        );

        // issue 无 body → 无正文（M5-ca 起 title 不再混进正文槽——任务名称由 vm 预填的
        // 专属名称块承载，此处回退只会让无正文任务把标题重复一遍）。
        let issue: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"Login bug"}"#).unwrap();
        assert_eq!(issue.requirement_text(), None);

        // issue_description 排在 handoff 之后：handoff 非空时压过正文。
        let handoff_over_body: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","handoff_note":"handoff here","issue_description":"body"}"#,
        )
        .unwrap();
        assert_eq!(
            handoff_over_body.requirement_text().as_deref(),
            Some("handoff here")
        );

        // 优先级：多来源同时存在取最高优先级（quick_create 压过 chat）。
        let multi: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","quick_create_prompt":"qc","chat_message":"chat"}"#,
        )
        .unwrap();
        assert_eq!(multi.requirement_text().as_deref(), Some("qc"));

        // 空白来源视为缺失，逐个跳过；全部来源缺席 → None（名称块承载标题，见上）。
        let blank: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"Title","quick_create_prompt":"  "}"#)
                .unwrap();
        assert_eq!(blank.requirement_text(), None);
    }

    #[test]
    fn remote_task_requirement_text_decodes_markdown_escapes() {
        // multica web 富文本编辑器（tiptap）保存走 @tiptap/markdown 序列化，正文的 `_`/`*`/`[`
        // 等会被转义成 `\_`/`\*` 存库；web 端渲染时解析回去视觉正常，码灵拿到的是 markdown
        // 源码。码灵把需求正文当**纯文本**消费（composer 预填 + agent 首轮 prompt），必须在
        // requirement_text 边界做一次 CommonMark 转义解码（代码块/代码 span 内的转义是用户
        // 字面输入，保留；`C:\Users` 等非标点前反斜杠不动）。JSON 里 `\\_` 解析后即被测输入 `\_`。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","issue_description":"包名 my\\_app\\_1 部署 dev\\_cluster"}"#,
        )
        .unwrap();
        assert_eq!(
            task.requirement_text().as_deref(),
            Some("包名 my_app_1 部署 dev_cluster"),
        );
    }

    #[test]
    fn remote_task_requirement_text_strips_html_and_decodes_entities() {
        // 同一序列化链路还会产 HTML 岛屿与实体：块级 `<p>` 包裹正文、行内标签、空段落标记
        // `&nbsp;`、`&amp;` 等实体（DPMS 同步的 HTML 正文同构）。web 端 markdown 渲染器正常
        // 显示，码灵纯文本消费需在 requirement_text 边界剥掉 HTML 层、解码实体；`&lt;p&gt;`
        // 这类实体解出的是用户字面文本，不得再当标签二次剥离。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","issue_description":"<p>包名 my\\_app</p><p>&nbsp;</p><p>环境 dev\\_cluster &amp; 预发</p>"}"#,
        )
        .unwrap();
        assert_eq!(
            task.requirement_text().as_deref(),
            Some("包名 my_app\n\u{00A0}\n环境 dev_cluster & 预发\n"),
        );
    }

    #[test]
    fn claim_request_is_empty_object() {
        // 接入方案 B2：body 恒 `{}`（服务端不读请求体，续跑指针由响应回填，断点续跑方案 §3.3）。
        let body = serde_json::to_value(ClaimRequest {}).unwrap();
        assert_eq!(body, serde_json::json!({}));
    }

    #[test]
    fn claim_response_unwraps_task_field() {
        // claim 响应 `{ "task": <RemoteTask> }`（接入方案 B2 / line 578）。
        let resp: ClaimResponse =
            serde_json::from_str(r#"{"task":{"id":"t-1","status":"dispatched"}}"#).unwrap();
        assert_eq!(resp.task.id, "t-1");
    }

    #[test]
    fn get_task_requirement_response_is_bare_remote_task() {
        // 接入方案 B3（claim-at-send 只读取）：GET /runtimes/{rid}/tasks/{tid} 返回**裸** AgentTaskResponse，
        // 直接反序列化成 RemoteTask——与 claim 响应的 `{task:...}` 包裹形状**刻意不同**（claim 是动作、有信封；
        // GET 是只读资源、RESTful 裸对象）。锁定此契约：若 server 误加 `{task:...}` 信封，id 缺失会解析失败。
        let bare: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","status":"queued","thread_name":"Fix login","issue_description":"Steps..."}"#,
        )
        .unwrap();
        assert_eq!(bare.id, "t-1");
        assert_eq!(bare.status, "queued");
        // claim-at-send 只读取：GET 响应不带执行凭证 / 续跑 session（claim-only 富字段）。
        assert!(bare.auth_token.is_none());
        assert!(bare.prior_session_id.is_none());
        // 需求正文已由 resolveTaskRequirementBody 回填 → requirement_text 取正文（预填 composer）。
        assert_eq!(bare.requirement_text().as_deref(), Some("Steps..."));

        // 反向锁定：包裹信封 `{task:{...}}` 不能直接解成 RemoteTask（id 在信封层、对象层缺失）。
        let wrapped = serde_json::from_str::<RemoteTask>(r#"{"task":{"id":"t-1"}}"#);
        assert!(
            wrapped.is_err(),
            "GET 任务详情必须是裸对象，不得包裹 task 信封"
        );
    }

    #[test]
    fn tasks_list_response_accepts_wrapped_and_bare() {
        let wrapped: TasksListResponse =
            serde_json::from_str(r#"{"tasks":[{"id":"t-1","status":"queued"}]}"#).unwrap();
        assert!(matches!(wrapped, TasksListResponse::Wrapped { ref tasks } if tasks.len() == 1));

        let bare: TasksListResponse =
            serde_json::from_str(r#"[{"id":"t-2","status":"queued"}]"#).unwrap();
        assert!(matches!(bare, TasksListResponse::Bare(ref v) if v.len() == 1));
    }

    #[test]
    fn start_request_serializes_force_fresh_session() {
        let normal = serde_json::to_value(StartRequest {
            force_fresh_session: false,
        })
        .unwrap();
        assert_eq!(normal["force_fresh_session"], false);

        let rerun = serde_json::to_value(StartRequest {
            force_fresh_session: true,
        })
        .unwrap();
        assert_eq!(rerun["force_fresh_session"], true);
    }

    #[test]
    fn complete_request_serializes_output_and_optional_fields() {
        // 接入方案 C6 + multica C6 改造：complete body {output, session_id?, work_dir?, completion_output?}
        // （缺失字段不序列化——completion_output 键缺席 = 不动 issue 现值，旧 server 未知键忽略）。
        let minimal = serde_json::to_value(CompleteRequest {
            output: "done".into(),
            session_id: None,
            work_dir: None,
            completion_output: None,
        })
        .unwrap();
        assert_eq!(minimal["output"], "done");
        assert!(minimal.get("session_id").is_none());
        assert!(minimal.get("work_dir").is_none());
        assert!(minimal.get("completion_output").is_none());

        let full = serde_json::to_value(CompleteRequest {
            output: "done".into(),
            session_id: Some("sess-9".into()),
            work_dir: Some("/repo".into()),
            completion_output: Some("交付说明".into()),
        })
        .unwrap();
        assert_eq!(full["session_id"], "sess-9");
        assert_eq!(full["work_dir"], "/repo");
        assert_eq!(full["completion_output"], "交付说明");
    }

    #[test]
    fn fail_request_serializes_error_and_failure_reason() {
        // 接入方案 C7：fail body {error, failure_reason}，如实传 reason 供 server auto-retry 决策。
        let body = serde_json::to_value(FailRequest {
            error: "agent exited 1".into(),
            failure_reason: "agent_error".into(),
        })
        .unwrap();
        assert_eq!(body["error"], "agent exited 1");
        assert_eq!(body["failure_reason"], "agent_error");
    }

    #[test]
    fn pin_task_session_request_omits_work_dir_when_none() {
        // 接入方案 C8：PinTaskSession body {session_id, work_dir?}（work_dir 缺失不序列化）。
        let no_dir = serde_json::to_value(PinTaskSessionRequest {
            session_id: "sess-9".into(),
            work_dir: None,
        })
        .unwrap();
        assert_eq!(no_dir, serde_json::json!({"session_id": "sess-9"}));

        let with_dir = serde_json::to_value(PinTaskSessionRequest {
            session_id: "sess-9".into(),
            work_dir: Some("/repo".into()),
        })
        .unwrap();
        assert_eq!(with_dir["work_dir"], "/repo");
    }

    #[test]
    fn terminal_retry_attempts_count_matches_schedule_plus_one() {
        // 锁定终态重试次数 = schedule.len() + 1（初始尝试 + 每个退避一次重试 = 6，开发设计第 5 章）。
        assert_eq!(TERMINAL_RETRY_SCHEDULE_SECS.len() + 1, 6);
    }

    #[test]
    fn multica_issue_done_status_constant_is_done() {
        // 锁定完成态字面量（接入方案 D2：码灵完成远程任务后流转 issue 到 done）。
        assert_eq!(MULTICA_ISSUE_DONE_STATUS, "done");
    }

    #[test]
    fn update_issue_status_request_serializes_status_key() {
        // 接入方案 D2：PUT /api/issues/{id} body 恰为 {status}——multica C6 改造后 completion_output
        // 改随 /complete 携带，issue PUT 回归纯状态流转（单一写入来源；迟到补写走
        // UpdateIssueCompletionOutputRequest 的单键形态）。
        let body = serde_json::to_value(UpdateIssueStatusRequest {
            status: "done".into(),
        })
        .unwrap();
        assert_eq!(body, serde_json::json!({"status": "done"}));
    }

    #[test]
    fn update_issue_completion_output_request_omits_status_key() {
        // 多 run 迟到补发（终态后追问 run 补交交付说明）：同一 PUT 端点**只写
        // completion_output、不带 status 键**——服务端字段级 merge「未触摸字段保留现值」，
        // 迟到写入不得把 issue（可能已被 reopen/人工流转）强行拉回 done。锁定 body 恰为单键。
        let body = serde_json::to_value(UpdateIssueCompletionOutputRequest {
            completion_output: "部署地址: https://t.example.com".into(),
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"completion_output": "部署地址: https://t.example.com"})
        );
    }

    #[test]
    fn issue_detail_decodes_optional_keys_and_tolerates_absence() {
        // M5-cf 拉取链路 wire 契约：completion_output / parent_issue_id 均可缺失
        // （内网 *string+omitempty：NULL 键整个不下发；顶层无父时 parent_issue_id 为 null）。
        let full = serde_json::from_str::<IssueDetail>(
            r#"{
                "id": "5f0d1b2c",
                "status": "in_progress",
                "parent_issue_id": "7c9e4a1f",
                "completion_output": "部署地址: https://t.example.com"
            }"#,
        )
        .unwrap();
        assert_eq!(
            full,
            IssueDetail {
                status: "in_progress".into(),
                parent_issue_id: Some("7c9e4a1f".into()),
                completion_output: Some("部署地址: https://t.example.com".into()),
            }
        );

        let bare = serde_json::from_str::<IssueDetail>(
            r#"{"id": "5f0d1b2c", "status": "todo", "parent_issue_id": null}"#,
        )
        .unwrap();
        assert_eq!(
            bare,
            IssueDetail {
                status: "todo".into(),
                parent_issue_id: None,
                completion_output: None,
            }
        );
    }

    // ===== /complete 携带 completion_output（multica C6 改造）：降级回路真 socket 集成测试 =====

    /// 裸 TCP mock（`metrics/heartbeat.rs` 同款先例）：按序逐连接返回固定 (status, body)，
    /// `Connection: close` 强制每请求新连接；记录收到的请求原文（headers + body）供断言。
    fn mock_http_responses(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut requests = Vec::new();
            for (status, body) in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                panic!("mock multica server did not receive all requests");
                            }
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(error) => panic!("mock multica server failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut raw = Vec::new();
                let mut buffer = [0_u8; 8192];
                // 读满 headers + Content-Length 字节（JSON body 可能晚于 headers 到达）。
                loop {
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..pos]).to_string();
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                if name.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        if raw.len() >= pos + 4 + content_length {
                            break;
                        }
                    }
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => raw.extend_from_slice(&buffer[..n]),
                        Err(_) => break,
                    }
                }
                requests.push(String::from_utf8_lossy(&raw).to_string());
                let reason = match status {
                    200 => "OK",
                    400 => "Bad Request",
                    404 => "Not Found",
                    500 => "Internal Server Error",
                    _ => "Status",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}"), handle)
    }

    /// 测试用 client：`no_proxy` 防 Windows 系统代理劫持 127.0.0.1（metrics 测试同款处理）。
    fn test_client(base_url: String) -> MulticaClient {
        let mut client = MulticaClient::new(base_url, Some("pat-test".into())).unwrap();
        client.http = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        client
    }

    #[tokio::test]
    async fn post_task_messages_sends_daemon_batch_without_workspace_header() {
        // 契约：daemon task 维度接口、PAT、无 X-Workspace-ID；body 为 {"messages":[...]}，
        // 消息字段按 multica 五种类型序列化（type 改名、None 字段省略）。
        let (base, handle) = mock_http_responses(vec![(200, r#"{"status":"ok"}"#)]);
        let client = test_client(base);
        let mut first = TranscriptMessage::body(
            crate::multica::transcript::model::TYPE_TEXT,
            "hello".to_string(),
        );
        first.seq = 1;
        let mut second =
            TranscriptMessage::tool_use("Bash".into(), serde_json::json!({"command": "ls"}));
        second.seq = 2;
        client
            .post_task_messages("t-1", &[first, second])
            .await
            .expect("messages must be accepted");
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(
            request.starts_with("POST /api/daemon/tasks/t-1/messages"),
            "{request}"
        );
        assert!(
            request.contains(r#""messages":[{"seq":1,"type":"text","content":"hello"}"#),
            "{request}"
        );
        assert!(
            request.contains(r#""type":"tool_use","tool":"Bash","input":{"command":"ls"}"#),
            "{request}"
        );
        assert!(
            !request.to_ascii_lowercase().contains("x-workspace-id"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn post_task_usage_sends_cumulative_rows() {
        // 契约：{"usage":[...]}，七字段（含恒为 0 的 cost_usd_ticks → 服务端按费率估算）。
        let (base, handle) = mock_http_responses(vec![(200, r#"{"status":"ok"}"#)]);
        let client = test_client(base);
        let rows = vec![UsageRow {
            provider: "anthropic".into(),
            model: "claude-sonnet-5-5".into(),
            input_tokens: 30,
            output_tokens: 12,
            cache_read_tokens: 4,
            cache_write_tokens: 2,
            cost_usd_ticks: 0,
        }];
        client
            .post_task_usage("t-1", &rows)
            .await
            .expect("usage must be accepted");
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].starts_with("POST /api/daemon/tasks/t-1/usage"),
            "{}",
            requests[0]
        );
        assert!(
            requests[0].contains(
                r#""usage":[{"provider":"anthropic","model":"claude-sonnet-5-5","input_tokens":30,"output_tokens":12,"cache_read_tokens":4,"cache_write_tokens":2,"cost_usd_ticks":0}]"#
            ),
            "{}",
            requests[0]
        );
    }

    #[tokio::test]
    async fn post_task_messages_maps_404_to_task_not_found() {
        // 404 = 任务已不可见/无权：由 reporter 据此停止继续上报（不再重试）。
        let (base, handle) = mock_http_responses(vec![(404, r#"{"error":"not found"}"#)]);
        let client = test_client(base);
        let mut message =
            TranscriptMessage::body(crate::multica::transcript::model::TYPE_TEXT, "x".into());
        message.seq = 1;
        let error = client
            .post_task_messages("gone", &[message])
            .await
            .expect_err("404 must surface as TaskNotFound");
        assert!(matches!(error, MulticaError::TaskNotFound), "{error:?}");
        let _ = handle.join();
    }

    #[tokio::test]
    async fn complete_task_carries_completion_output_and_succeeds() {
        // 验收 1/5 形态：单请求成功（旧 server 未知键忽略 → 同样 200 路径），body 携带交付说明。
        let (base, handle) = mock_http_responses(vec![(200, "{}")]);
        let client = test_client(base);
        client
            .complete_task(
                "t-1",
                "CRIS-收尾归档",
                Some("sess-9"),
                Some("/repo"),
                Some("交付说明"),
            )
            .await
            .expect("complete must succeed");
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("\"completion_output\":\"交付说明\""));
    }

    #[tokio::test]
    async fn complete_task_degrades_to_fieldless_resend_on_400() {
        // 验收 4 后半（multica 改造清单 §2.2）：携带字段收到 400 → **立即**降级去字段重发（无退避，
        // 不吃满 6 次终态重试），任务必须完成；降级只去 completion_output，既有字段不丢。
        let (base, handle) = mock_http_responses(vec![
            (
                400,
                r#"{"error":"completion_output exceeds 16000 characters"}"#,
            ),
            (200, "{}"),
        ]);
        let client = test_client(base);
        client
            .complete_task("t-1", "out", None, None, Some("超长交付说明"))
            .await
            .expect("degraded resend must complete the task");
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("\"completion_output\""));
        assert!(!requests[1].contains("\"completion_output\""));
        assert!(requests[1].contains("\"output\":\"out\""));
    }

    #[tokio::test]
    async fn complete_post_once_classifies_400_with_server_message() {
        // 400 → Rejected 且透传 server message（供降级告警与最终错误上下文）。
        let (base, handle) = mock_http_responses(vec![(
            400,
            r#"{"error":"completion_output exceeds 16000 characters"}"#,
        )]);
        let client = test_client(base);
        let body = CompleteRequest {
            output: "out".into(),
            session_id: None,
            work_dir: None,
            completion_output: Some("超长".into()),
        };
        match client
            .complete_post_once("/api/daemon/tasks/t-1/complete", &body)
            .await
        {
            CompletePostOutcome::Rejected(reason) => {
                assert!(reason.contains("completion_output exceeds 16000 characters"));
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert_eq!(handle.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn complete_post_once_maps_404_fatal_and_500_retryable() {
        let body = CompleteRequest {
            output: "out".into(),
            session_id: None,
            work_dir: None,
            completion_output: None,
        };
        // 404 → Fatal(TaskNotFound)：确定错误码，重试无意义。
        let (base, handle) = mock_http_responses(vec![(404, "{}")]);
        let client = test_client(base);
        assert!(matches!(
            client
                .complete_post_once("/api/daemon/tasks/t-1/complete", &body)
                .await,
            CompletePostOutcome::Fatal(MulticaError::TaskNotFound)
        ));
        assert_eq!(handle.join().unwrap().len(), 1);

        // 500 → Retryable(NetworkFailed)：终态重试语义继续（服务端幂等，退避后重试）。
        let (base, handle) = mock_http_responses(vec![(500, "{}")]);
        let client = test_client(base);
        assert!(matches!(
            client.complete_post_once("/p", &body).await,
            CompletePostOutcome::Retryable(MulticaError::NetworkFailed(_))
        ));
        assert_eq!(handle.join().unwrap().len(), 1);
    }

    #[test]
    fn update_issue_status_path_is_workspace_scoped_put() {
        // 锁定 PUT path 形状（issue 维度接口，靠 X-Workspace-ID 头路由，path 不含 workspace）。
        // 头注入在 json_send 内，由 HTTP 集成测试覆盖；此处锁定 path。
        let path = format!("/api/issues/{}", "iss-7");
        assert_eq!(path, "/api/issues/iss-7");
    }

    #[test]
    fn multica_client_new_rejects_empty_base_url() {
        // S1：共享 client 后，空 base_url 守卫仍生效（NotConfigured，不构造 client）。
        assert!(matches!(
            MulticaClient::new("", None),
            Err(MulticaError::NotConfigured)
        ));
        assert!(matches!(
            MulticaClient::new("   ", None),
            Err(MulticaError::NotConfigured)
        ));
        // 合法 base_url 构造成功（复用进程级共享 client，无网络副作用）。
        assert!(MulticaClient::new("http://localhost:1", None).is_ok());
    }

    #[test]
    fn multica_client_is_clone() {
        // S1：MulticaClient 可廉价 Clone（http 是 reqwest::Client 的 Arc clone）。
        // 锁定 derive(Clone)——并发扇出需要把 client move 进 spawn 任务（O1 前置）。
        let client = MulticaClient::new("http://localhost:1", Some("mul_x".into())).unwrap();
        let cloned = client.clone();
        assert_eq!(cloned.base_url, client.base_url);
    }

    #[test]
    fn liveness_timeout_is_bounded_under_global_default() {
        // S2：liveness 调用（heartbeat/get_task_status/register_once）用更短的 per-request 超时
        // 覆盖 client 级 30s，使单 tick 在退化网络下快速失败。锁定上界 < 30s 且 > 0。
        assert!(LIVENESS_TIMEOUT_SECS > 0 && LIVENESS_TIMEOUT_SECS < 30);
        assert_eq!(LIVENESS_TIMEOUT_SECS, 10);
    }

    // ===== skill 同步：心跳 ack 契约（multica server/pkg/protocol/messages.go:315-329）=====

    #[test]
    fn heartbeat_ack_parses_full_pending_payloads() {
        // HTTP 响应只含 status + 非空 pending 字段；完整形态四类 pending 齐备。
        let ack: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_local_skills":{"id":"req-1"},"pending_local_skill_imports":[{"id":"req-2","skill_key":"pr-review"}],"pending_update":{},"pending_model_list":{"x":1}}"#,
        )
        .unwrap();
        assert_eq!(ack.status, "ok");
        assert_eq!(
            ack.pending_local_skills.as_ref().map(|p| p.id.as_str()),
            Some("req-1")
        );
        let imports = ack.pending_local_skill_imports.expect("批量导入应解析");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].id, "req-2");
        assert_eq!(imports[0].skill_key, "pr-review");
        // v1 不消费，仅反序列化占位（解析不失败即契约成立）。
        assert!(ack.pending_update.is_some());
        assert!(ack.pending_model_list.is_some());
    }

    // ===== story dev/test 拆分：心跳 ack 就绪变化 diff 契约（R8 方案 1）=====

    #[test]
    fn heartbeat_ack_parses_pending_readiness_changes() {
        // diff 列表形态 [{task_id, is_ready}]，与 ack 现有复数 pending_* 字段同构。
        // 码灵只作刷新信号消费：非空 → remote-tasks-updated 事件 → 重取 pending。
        let ack: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_readiness_changes":[{"task_id":"t-1","is_ready":true},{"task_id":"t-2","is_ready":false}]}"#,
        )
        .unwrap();
        let changes = ack.pending_readiness_changes.expect("diff 应解析");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].task_id, "t-1");
        assert!(changes[0].is_ready);
        assert_eq!(changes[1].task_id, "t-2");
        assert!(!changes[1].is_ready);
    }

    #[test]
    fn heartbeat_ack_without_readiness_changes() {
        // 旧 server / 无变化 → 字段缺失（None），不触发刷新信号。
        let ack: HeartbeatAck = serde_json::from_str(r#"{"status":"ok"}"#).unwrap();
        assert!(ack.pending_readiness_changes.is_none());
    }

    #[test]
    fn heartbeat_ack_tolerates_diverged_readiness_change_shape() {
        // wire 形态尚未定稿（S4 目前是码灵提案）：条目字段与提案不一致时必须退化为「无信号」，
        // 绝不能因这个可选字段让**整条 ack** 解码失败——ack 同时承载 skill 待办派发
        // （loop_.rs `dispatch_pending_skill_work`），解码失败会连带跳过本 tick 的 skill 工作，
        // 且日志只留一句通用的 `multica heartbeat failed (will retry next tick)`，故障无法归因。
        let missing_flag: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_readiness_changes":[{"task_id":"t-1"}]}"#,
        )
        .expect("条目缺 is_ready 不应让整条 ack 解码失败");
        let changes = missing_flag
            .pending_readiness_changes
            .expect("diverged 形态仍应解出 diff");
        assert_eq!(changes.len(), 1, "非空即刷新信号，条目内容不参与判定");

        // 键名风格不一致（camelCase）同样退化为「有变化但内容不可用」——只取「非空」这一事实。
        let camel: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_readiness_changes":[{"taskId":"t-1","isReady":true}]}"#,
        )
        .expect("键名不一致不应让整条 ack 解码失败");
        assert_eq!(camel.pending_readiness_changes.map(|c| c.len()), Some(1));
    }

    #[test]
    fn heartbeat_ack_tolerates_missing_pending_and_unknown_fields() {
        // 无 pending 时字段整体缺失（server 只回非空 pending）+ 未知字段忽略（serde 默认）。
        let ack: HeartbeatAck =
            serde_json::from_str(r#"{"status":"ok","future_field":true}"#).unwrap();
        assert_eq!(ack.status, "ok");
        assert!(ack.pending_local_skills.is_none());
        assert!(ack.pending_local_skill_imports.is_none());
        assert!(ack.pending_update.is_none());
        assert!(ack.pending_model_list.is_none());
    }

    #[test]
    fn heartbeat_ack_ignores_singular_legacy_import_field() {
        // 兼容字段 pending_local_skill_import（单条旧形式）不在结构体上——serde 未知字段忽略；
        // 码灵已声明 supports_batch_import: true，服务端按批量形式下发（批量数组必含同内容首条）。
        let ack: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_local_skill_import":{"id":"req-9","skill_key":"x"}}"#,
        )
        .unwrap();
        assert!(ack.pending_local_skill_imports.is_none());
    }

    #[test]
    fn heartbeat_ack_parses_pending_runtime_assets_and_tolerates_absence() {
        // 角色共享工单引用：id + 建议 action（详情才是权威）；旧 server 不发该字段 → None。
        let ack: HeartbeatAck = serde_json::from_str(
            r#"{"status":"ok","pending_runtime_assets":{"id":"req-42","action":"list"}}"#,
        )
        .unwrap();
        let pending = ack.pending_runtime_assets.expect("pending present");
        assert_eq!(pending.id, "req-42");
        assert_eq!(pending.action, "list");
        // action 是建议字段，缺省容忍（分发器只消费 id）。
        let ack: HeartbeatAck =
            serde_json::from_str(r#"{"status":"ok","pending_runtime_assets":{"id":"req-43"}}"#)
                .unwrap();
        assert_eq!(ack.pending_runtime_assets.unwrap().action, "");

        let legacy: HeartbeatAck =
            serde_json::from_str(r#"{"status":"ok","pending_local_skills":{"id":"r"}}"#).unwrap();
        assert!(legacy.pending_runtime_assets.is_none());
    }

    #[test]
    fn runtime_assets_request_detail_parses_three_work_order_shapes() {
        // list：无 item_id/payload 语义；id/slug/version 等信息性字段被忽略。
        let list: RuntimeAssetsRequest = serde_json::from_str(
            r#"{"id":"r1","action":"list","asset_type":"role","status":"running"}"#,
        )
        .unwrap();
        assert_eq!(list.action, "list");
        assert!(list.payload.is_none());
        // export：action=install + item_id 空 + payload={asset_key}。
        let export: RuntimeAssetsRequest = serde_json::from_value(json!({
            "id": "r2", "action": "install", "asset_type": "role",
            "item_id": "", "slug": "", "version": "",
            "payload": {"asset_key": "pf-my-role"}
        }))
        .unwrap();
        assert!(export.item_id.is_empty());
        // push-install：item_id 非空 + payload 即角色正文。
        let install: RuntimeAssetsRequest = serde_json::from_value(json!({
            "id": "r3", "action": "install", "asset_type": "role",
            "item_id": "item-uuid", "slug": "pf-my-role", "version": "1.2.0",
            "payload": {"name": "pf-my-role", "description": "d",
                        "content": "---\nname: pf-my-role\n---\nbody", "is_builtin": false}
        }))
        .unwrap();
        assert_eq!(install.item_id, "item-uuid");
        assert!(install.payload.is_some());
        // uninstall（M5-cw）：action=uninstall + item_id 恒空 + payload={asset_key}。
        let uninstall: RuntimeAssetsRequest = serde_json::from_value(json!({
            "id": "r4", "action": "uninstall", "asset_type": "role",
            "item_id": "", "slug": "", "version": "", "status": "running",
            "payload": {"asset_key": "pf-mcx1a2"}
        }))
        .unwrap();
        assert_eq!(uninstall.action, "uninstall");
        assert!(uninstall.item_id.is_empty());
        assert!(uninstall.payload.is_some());
    }

    #[test]
    fn runtime_assets_reports_serialize_wire_contract() {
        // list 上报：摘要无正文、type 恒 role、key = 本地 profile.id。
        let list = RuntimeAssetsListReport {
            status: "completed",
            supported: true,
            assets: vec![RuntimeAssetSummary {
                key: "pf-my-role".into(),
                r#type: "role",
                name: "我的角色".into(),
                description: "一句话".into(),
                is_builtin: false,
                updated_at: "2026-10-31T10:00:00Z".into(),
            }],
            error: None,
        };
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["supported"], true);
        assert_eq!(json["assets"][0]["key"], "pf-my-role");
        assert_eq!(json["assets"][0]["type"], "role");
        assert_eq!(json["assets"][0]["is_builtin"], false);
        assert!(json["assets"][0].get("content").is_none());

        // export completed：payload 承载角色正文（交换文档 wire 形态：name=slug、is_builtin snake）。
        let export = RuntimeAssetsInstallReport {
            status: "completed",
            payload: Some(RuntimeAssetPayload::Role(
                gold_band::app::MulticaRoleExchange {
                    name: "pf-my-role".into(),
                    description: "一句话".into(),
                    content: "---\nname: pf-my-role\n---\nbody".into(),
                    is_builtin: false,
                },
            )),
            result: None,
            error: None,
        };
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["payload"]["name"], "pf-my-role");
        assert_eq!(json["payload"]["is_builtin"], false);
        assert!(json["payload"].get("dynamicTemplate").is_none());
        // Option 字段显式 null（与 skill 上报同约定，不加 skip_serializing_if）。
        assert!(json["result"].is_null());

        // export completed（workflow）：payload 承载模板 envelope（isBuiltIn camelCase、
        // 四键形状、绝不携带 modelBindings——multaca 发布校验 MODEL_BINDINGS_PRESENT 双保险）。
        let export = RuntimeAssetsInstallReport {
            status: "completed",
            payload: Some(RuntimeAssetPayload::Workflow(
                gold_band::app::MulticaWorkflowExchange {
                    id: "my-pipeline".into(),
                    name: "我的流水线".into(),
                    is_built_in: true,
                    workflow: WorkflowDsl {
                        version: "0.1".into(),
                        id: "workflow-abc".into(),
                        entry: "dev".into(),
                        control: Default::default(),
                        nodes: Vec::new(),
                        edges: Vec::new(),
                    },
                },
            )),
            result: None,
            error: None,
        };
        let json = serde_json::to_value(&export).unwrap();
        assert_eq!(json["payload"]["id"], "my-pipeline");
        assert_eq!(json["payload"]["isBuiltIn"], true);
        assert_eq!(json["payload"]["workflow"]["entry"], "dev");
        assert!(json["payload"].get("modelBindings").is_none());
        assert!(json["payload"].get("is_builtin").is_none());

        // push-install completed：result.installed_key（= 新本地 profile.id，M5-cz 起字段名
        // 对齐 2026-11-13 协议 §3.3，旧名 installed_name）+ installed_display_name（实际
        // 展示名，multica 反馈 P3）+ outcome（#6 落库形态）；failed：error。
        let install = RuntimeAssetsInstallReport {
            status: "completed",
            payload: None,
            result: Some(RuntimeAssetsReportResult::Install(
                RuntimeAssetsInstallResult {
                    installed_key: "pf-new-id".into(),
                    installed_display_name: "开发工程师".into(),
                    outcome: "created",
                },
            )),
            error: None,
        };
        let json = serde_json::to_value(&install).unwrap();
        assert_eq!(json["result"]["installed_key"], "pf-new-id");
        assert_eq!(json["result"]["installed_display_name"], "开发工程师");
        assert_eq!(json["result"]["outcome"], "created");
        assert!(json["result"].get("installed_name").is_none());
        assert!(json["payload"].is_null());

        // uninstall completed（M5-cw）：result.removed_key（M5-cz 起字段名对齐协议 §3.4，
        // 旧名 uninstalled_key；untagged——wire 即内层对象，multica 对 result 仅透传）；
        // 三动作共用 install result 端点。
        let uninstall = RuntimeAssetsInstallReport {
            status: "completed",
            payload: None,
            result: Some(RuntimeAssetsReportResult::Uninstall(
                RuntimeAssetsUninstallResult {
                    removed_key: "pf-mcx1a2".into(),
                },
            )),
            error: None,
        };
        let json = serde_json::to_value(&uninstall).unwrap();
        assert_eq!(json["result"]["removed_key"], "pf-mcx1a2");
        assert_eq!(json["result"].as_object().unwrap().len(), 1);
        assert!(json["payload"].is_null());

        let failed = RuntimeAssetsInstallReport {
            status: "failed",
            payload: None,
            result: None,
            error: Some("profile not found".into()),
        };
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error"], "profile not found");
    }

    // ===== skill 同步：发现/导入上报 body 契约 =====

    #[test]
    fn local_skill_list_report_serializes_wire_contract() {
        // 发现上报 body：status/skills[]/supported/mcp_supported；key=目录名、root 恒 "provider"。
        let report = LocalSkillListReport {
            status: "completed",
            skills: vec![LocalSkillSummary {
                key: "pr-review".into(),
                name: "PR review".into(),
                description: "Review pull requests".into(),
                source_path: "C:/home/.gold-band/skills/pr-review".into(),
                provider: "claude-acp".into(),
                root: "provider",
                file_count: 2,
            }],
            supported: true,
            mcp_supported: false,
            error: None,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["supported"], true);
        assert_eq!(json["mcp_supported"], false);
        assert_eq!(json["skills"][0]["key"], "pr-review");
        assert_eq!(json["skills"][0]["name"], "PR review");
        assert_eq!(
            json["skills"][0]["source_path"],
            "C:/home/.gold-band/skills/pr-review"
        );
        assert_eq!(json["skills"][0]["provider"], "claude-acp");
        assert_eq!(json["skills"][0]["root"], "provider");
        assert_eq!(json["skills"][0]["file_count"], 2);

        // 失败形态：status=failed + error 文本（skills 为空数组、能力标志仍如实上报）。
        let failed = LocalSkillListReport {
            status: "failed",
            skills: Vec::new(),
            supported: true,
            mcp_supported: false,
            error: Some("scan failed".into()),
        };
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error"], "scan failed");
    }

    #[test]
    fn local_skill_import_report_serializes_wire_contract() {
        // 导入上报 body：status/skill{content=SKILL.md 全文含 frontmatter}/files[]；
        // content 原样透传（multica DB 不强制 body-only，2026-09-09 Q5 修正）。
        let report = LocalSkillImportReport {
            status: "completed",
            skill: Some(LocalSkillBundle {
                name: "PR review".into(),
                description: "Review pull requests".into(),
                content: "---\nname: PR review\nallowed-tools: Read\n---\n\nBody".into(),
                source_path: "C:/home/.gold-band/skills/pr-review".into(),
                provider: "claude-acp".into(),
                files: vec![LocalSkillFile {
                    path: "assets/template.md".into(),
                    content: "# template".into(),
                }],
            }),
            error: None,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["skill"]["name"], "PR review");
        assert_eq!(
            json["skill"]["content"],
            "---\nname: PR review\nallowed-tools: Read\n---\n\nBody"
        );
        assert_eq!(json["skill"]["files"][0]["path"], "assets/template.md");
        assert_eq!(json["skill"]["files"][0]["content"], "# template");

        // 失败形态：skill 缺失 + error 文本（如 skill not found: <key>）。
        let failed = LocalSkillImportReport {
            status: "failed",
            skill: None,
            error: Some("skill not found: deleted-skill".into()),
        };
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error"], "skill not found: deleted-skill");
        assert!(json["skill"].is_null());
    }

    // ===== skill 同步：拉取 REST 契约（multica handler/skill.go:60-72 / 43-53）=====

    #[test]
    fn remote_skill_summary_parses_server_shape() {
        // GET /api/skills（X-Workspace-ID 路由）返回裸数组；summary 不含 content（列表性能刻意省略）。
        let list: Vec<RemoteSkillSummary> = serde_json::from_str(
            r#"[{"id":"sk-1","name":"PR review","description":"Review PRs","updated_at":"2026-09-01T00:00:00Z"},{"id":"sk-2","name":"Empty","description":""}]"#,
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "sk-1");
        assert_eq!(list[0].name, "PR review");
        assert_eq!(list[1].updated_at, None);
    }

    #[test]
    fn remote_skill_detail_parses_server_shape() {
        // GET /api/skills/{id} 返回含 content 的详情（content 可能带 frontmatter——DB 不强制 body-only）。
        let detail: RemoteSkillDetail = serde_json::from_str(
            r#"{"id":"sk-1","name":"PR review","description":"Review PRs","content":"---\nname: PR review\n---\n\nBody"}"#,
        )
        .unwrap();
        assert_eq!(detail.id, "sk-1");
        assert_eq!(detail.name, "PR review");
        assert!(detail.content.starts_with("---\n"));
    }

    #[test]
    fn skill_files_response_accepts_wrapped_and_bare() {
        // GET /api/skills/{id}/files（默认 includeContent=true）的容错：包装 {files:[...]} 或裸数组
        // （照搬 WorkspacesResponse/TasksListResponse 的 untagged 容错惯例）；项含正文，根 SKILL.md
        // 不在列表（由 detail.content 承载）。
        let wrapped: SkillFilesResponse = serde_json::from_str(
            r##"{"files":[{"path":"assets/a.md","size":6,"content":"# tpl"},{"path":"nested/b.md","content":"b"}]}"##,
        )
        .unwrap();
        let SkillFilesResponse::Wrapped { files } = &wrapped else {
            panic!("expected wrapped shape");
        };
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "assets/a.md");
        assert_eq!(files[0].content, "# tpl");
        assert_eq!(files[1].size, 0); // size 缺失时 serde default，正文才是事实源
        assert!(!files.iter().any(|file| file.path == "SKILL.md"));

        let bare: SkillFilesResponse = serde_json::from_str(r#"[]"#).unwrap();
        assert!(matches!(bare, SkillFilesResponse::Bare(ref v) if v.is_empty()));
    }
}
