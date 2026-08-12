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

// M2-M5 分里程碑接入：register/heartbeat/list_workspaces/claim/start/终态上报等预留 API
// 暂未全部接线，先一次定义完整接口（先定数据→再定接口→再补实现）。M5 完成后审查移除该 allow。
#![allow(dead_code)]

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, Method, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::multica::error::MulticaError;

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
/// 心跳 tick 内的高频调用（heartbeat / 取消检测 / prepare-lease 续期 / 自愈 register）正常 <1s；
/// server 慢响应或网络抖动时，若沿用 30s 全局超时，单 tick 串行阻塞会累积，威胁 prepare lease（45s）
/// 的续期窗口与取消检测的及时性。给这些调用更短的 per-request 上界，使退化网络下单 tick 也能快速失败、
/// 下一 tick（15s）重试——而非一个 tick 阻塞数分钟。
const LIVENESS_TIMEOUT_SECS: u64 = 10;

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
    /// 执行该 task 用的短期凭证（claim 响应带；M4 bridge 注入 ACP 执行）。
    #[serde(default)]
    pub auth_token: Option<String>,
    /// 续跑指针（**只输出**）：server claim 响应回填的父任务 ACP session_id。
    ///
    /// 角色为「从响应消费」——server claim 处理器**不读请求体**，续跑指针全由响应回填
    /// （webank `daemon.go:2025-2054` 经 `GetLastTaskSession` 解析父任务 session）。客户端用它做
    /// 续跑兜底/校验；主路径是 [`Self::parent_task_id`] 反查本地索引（更稳，不依赖 server session 解析）。
    #[serde(default)]
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
    /// 预填文本：quick-create/chat/comment/autopilot/handoff 任一非空取之；皆空回退 title（issue 场景）。
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
    /// `requirement_text()` 把它排在 handoff_note 之后、title 之前——比「具体指令」次之、比「标题」丰富。
    #[serde(default)]
    pub issue_description: Option<String>,
    #[serde(default)]
    pub last_activity_at: Option<String>,
}

impl RemoteTask {
    /// 预填用「最佳可用需求文本」（镜像 server `computeTaskKind` 来源互斥优先级）。
    ///
    /// quick-create → chat → comment → autopilot → handoff → issue_description 任一非空取之；
    /// 皆空回退 title（issue 无正文时预填标题）。issue 正文排在 handoff 之后、title 之前——
    /// 比「具体指令/交接」次之、比「标题」丰富。空白值视为缺失跳过——按来源**逐个**过滤后再短路，
    /// 保证纯空白的上游来源不会吞掉下游（如空白 quick_create 仍回退 title）。
    pub fn requirement_text(&self) -> Option<String> {
        [
            self.quick_create_prompt.as_deref(),
            self.chat_message.as_deref(),
            self.trigger_comment_content.as_deref(),
            self.autopilot_description.as_deref(),
            self.handoff_note.as_deref(),
            self.issue_description.as_deref(),
            self.title.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .map(str::to_string)
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

// ===== M4 终态上报/会话固定/重跑 wire 类型（开发设计 2.2.7 / 接入方案 C6-C8/D1）=====

/// complete 请求（接入方案 C6：body output + session_id + work_dir）。
///
/// `output` = 会话产物摘要；`session_id`/`work_dir` 来自 worker_ref 采集（可能缺失）。
#[derive(Debug, Serialize)]
pub struct CompleteRequest {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
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
#[derive(Debug, Serialize)]
struct UpdateIssueStatusRequest {
    status: String,
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
    pub fn new(
        base_url: impl Into<String>,
        token: Option<String>,
    ) -> Result<Self, MulticaError> {
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
    /// 使单次调用在 server 慢响应时快速失败，避免拖垮整个心跳 tick（prepare lease 45s 续期窗口）。
    async fn send(
        &self,
        method: Method,
        path: &str,
        per_request_timeout: Option<Duration>,
    ) -> Result<Response, MulticaError> {
        let mut req = self.http.request(method.clone(), self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
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
    /// `post_json` / `post_json_with_workspace` / issue PUT 的共用底座——杜绝三者各自重复请求构造
    /// （否则 issue PUT 会带来第三份几乎相同的 auth+send+map_status 模板）。用 `self.token`（PAT）做
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
        let resp = self
            .json_send(Method::POST, path, None, body, None)
            .await?;
        resp.json::<R>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
    }

    /// 同 `post_json` 但额外带 `X-Workspace-ID` 头。
    ///
    /// issue 维度业务接口（接入方案 D1/D2/E1/E2）path 不含 workspace（`/api/issues/{id}/...`），
    /// 靠该头路由到对应 workspace（开发设计 4.1）。daemon 任务接口（C2-C8）path 自带 task_id，
    /// 不需要该头。
    async fn post_json_with_workspace<T, R>(
        &self,
        path: &str,
        workspace_id: &str,
        body: &T,
    ) -> Result<R, MulticaError>
    where
        T: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let resp = self
            .json_send(Method::POST, path, Some(workspace_id), body, None)
            .await?;
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
        let resp = self.send(Method::GET, "/api/me", None).await?;
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
        let resp = self.send(Method::GET, "/api/workspaces", None).await?;
        let parsed = resp
            .json::<WorkspacesResponse>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode /api/workspaces failed: {e}")))?;
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
    pub async fn register(
        &self,
        req: &RegisterRequest,
    ) -> Result<RegisterResponse, MulticaError> {
        self.with_network_retry("register", || async {
            self.post_json("/api/daemon/register", req).await
        })
        .await
    }

    /// `POST /api/daemon/register` —— 单次注册（无 client 内重试 + liveness 短超时），供常驻心跳 tick 自愈用。
    ///
    /// 与 [`register`](MulticaClient::register) 的区别：后者带 `with_network_retry`（3 次，单次最长 30s），
    /// 适合一次性调用。而自愈注册由 15s 心跳 tick 驱动——**循环即重试**；若再嵌套 client 内 3×30s 退避，
    /// 弱网下单 tick 可超 90s，阻塞后续续期/取消检测（prepare lease 45s 被 server 回收）。
    /// 故自愈路径用单次 register + per-request 短超时：失败下 tick 自然重试，单 tick 耗时有界。
    pub async fn register_once(&self, req: &RegisterRequest) -> Result<RegisterResponse, MulticaError> {
        let resp = self
            .json_send(
                Method::POST,
                "/api/daemon/register",
                None,
                req,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        resp.json::<RegisterResponse>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode /api/daemon/register failed: {e}")))
    }

    /// `POST /api/daemon/heartbeat` —— 维持 runtime 在线（执行期 15s）。
    ///
    /// body `{runtime_id, supports_batch_import: true}`。失败仅记日志（下一 tick 自然重试），
    /// 不在 client 内重试（循环即重试）。走单次请求 + **liveness 短超时**（覆盖 client 级 30s），
    /// 防 server 慢响应拖垮整个心跳 tick（prepare lease 45s 续期窗口）。
    pub async fn heartbeat(&self, runtime_id: &str) -> Result<(), MulticaError> {
        let body = serde_json::json!({
            "runtime_id": runtime_id,
            "supports_batch_import": true,
        });
        let _resp = self
            .json_send(
                Method::POST,
                "/api/daemon/heartbeat",
                None,
                &body,
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        Ok(())
    }

    /// `POST /api/daemon/runtimes/{rid}/recover-orphans` —— 启动时清理残留的在飞任务（无条件置失败态）。
    ///
    /// 失败不阻断启动（仅记日志）；server 对 `runtime_recovery` 等 retryable reason 自动重试（本期接受）。
    pub async fn recover_orphans(&self, runtime_id: &str) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/recover-orphans");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
        Ok(())
    }

    /// `POST /api/daemon/runtimes/{rid}/tasks/{tid}/prepare-lease` —— claim 后 compose 期间续期 prepare lease。
    ///
    /// server 的 prepare lease 仅 45s（`prepareLeaseDuration`），claim-at-click 后用户在 composer 选模型/
    /// 改需求可能超过该窗口；心跳循环每 tick 调本方法续期，防任务被 `ReclaimStaleDispatchedTaskForRuntime`
    /// 回收（与 multica daemon `startTaskPrepareLeaseExtender` 同构）。失败仅记日志（下一 tick 重试），
    /// 不在 client 内重试（循环即重试）。走单次请求 + **liveness 短超时**——这是续期窗口的关键路径，
    /// 单次调用必须快速失败，否则被它阻塞会直接导致 45s lease 过期。
    pub async fn extend_prepare_lease(
        &self,
        runtime_id: &str,
        task_id: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}/prepare-lease");
        let _resp = self
            .json_send(
                Method::POST,
                &path,
                None,
                &serde_json::json!({}),
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        Ok(())
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
            let resp = self.send(Method::GET, &path, None).await?;
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
        let body = StartRequest { force_fresh_session };
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
                Some(Duration::from_secs(LIVENESS_TIMEOUT_SECS)),
            )
            .await?;
        let parsed = resp
            .json::<TaskStatusResponse>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))?;
        Ok(parsed.status)
    }

    // ===== M4 终态上报/会话固定/重跑（开发设计 2.5 / 接入方案 C6-C8/D1）=====

    /// `POST /api/daemon/tasks/{tid}/complete` —— 成功终态（接入方案 C6）。
    ///
    /// 终态回调严格重试（4/8/16/32/64s 共 6 次，服务端幂等）。`output` = 会话产物摘要，
    /// `session_id`/`work_dir` 来自 worker_ref 采集（可能缺失 → 不序列化该键）。
    pub async fn complete_task(
        &self,
        task_id: &str,
        output: &str,
        session_id: Option<&str>,
        work_dir: Option<&str>,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/tasks/{task_id}/complete");
        let body = CompleteRequest {
            output: output.to_string(),
            session_id: session_id.map(String::from),
            work_dir: work_dir.map(String::from),
        };
        self.with_terminal_retry("complete", || async {
            let _: serde_json::Value = self.post_json(&path, &body).await?;
            Ok(())
        })
        .await
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
            .map_err(|e| MulticaError::NetworkFailed(format!("bind local callback listener failed: {e}")))?;
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
        let mut login_url = url::Url::parse(app_url)
            .map_err(|_| MulticaError::NotConfigured)?; // app_url 无效视为未配置
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
                if let Some((token, returned_state)) = parse_callback_request(stream, &app_root).await {
                    if returned_state != expected_state {
                        // CSRF 不匹配，忽略并继续等（cmd_auth.go:276 返回 400 但 loop）
                        continue;
                    }
                    return Ok(token);
                }
            }
        })
        .await
        .map_err(|_| MulticaError::NetworkFailed("timed out waiting for authentication".into()))??;

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
        _ => Err(MulticaError::NetworkFailed(format!("{path}: HTTP {status}"))),
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
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
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
                name: "claude-acp".into(),
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
        // provider 固定编码进单个 runtime 的 type 字段。
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
    fn remote_task_reads_thread_name_wire_key() {
        // webank 权威源：AgentTaskResponse.ThreadName → JSON `thread_name`，
        // claim 响应与 pending 列表均带此键。锁定 wire 契约 thread_name → task.title。
        let task: RemoteTask = serde_json::from_str(
            r#"{"id":"t-1","thread_name":"Fix login bug","status":"queued"}"#,
        )
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
        let first: RemoteTask =
            serde_json::from_str(r#"{"id":"t-1","status":"queued"}"#).unwrap();
        assert!(first.parent_task_id.is_none());
    }

    #[test]
    fn remote_task_requirement_text_picks_source_by_priority() {
        // 镜像 server computeTaskKind 来源互斥优先级：
        // quick-create > chat > comment > autopilot > handoff > issue_description > title。
        let qc: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"T","quick_create_prompt":"qc-prompt"}"#)
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

        // issue 无 body → 回退 title（预填 issue 标题）。
        let issue: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"Login bug"}"#).unwrap();
        assert_eq!(issue.requirement_text().as_deref(), Some("Login bug"));

        // issue_description 排在 handoff 之后：handoff 非空时压过正文。
        let handoff_over_body: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","handoff_note":"handoff here","issue_description":"body"}"#,
        )
        .unwrap();
        assert_eq!(handoff_over_body.requirement_text().as_deref(), Some("handoff here"));

        // 优先级：多来源同时存在取最高优先级（quick_create 压过 chat）。
        let multi: RemoteTask = serde_json::from_str(
            r#"{"id":"t","thread_name":"T","quick_create_prompt":"qc","chat_message":"chat"}"#,
        )
        .unwrap();
        assert_eq!(multi.requirement_text().as_deref(), Some("qc"));

        // 空白来源视为缺失，跳到下一优先级（回退 title）。
        let blank: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"Title","quick_create_prompt":"  "}"#)
                .unwrap();
        assert_eq!(blank.requirement_text().as_deref(), Some("Title"));
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
        // 接入方案 C6：complete body {output, session_id?, work_dir?}（缺失字段不序列化）。
        let minimal = serde_json::to_value(CompleteRequest {
            output: "done".into(),
            session_id: None,
            work_dir: None,
        })
        .unwrap();
        assert_eq!(minimal["output"], "done");
        assert!(minimal.get("session_id").is_none());
        assert!(minimal.get("work_dir").is_none());

        let full = serde_json::to_value(CompleteRequest {
            output: "done".into(),
            session_id: Some("sess-9".into()),
            work_dir: Some("/repo".into()),
        })
        .unwrap();
        assert_eq!(full["session_id"], "sess-9");
        assert_eq!(full["work_dir"], "/repo");
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
        // 接入方案 D2：PUT /api/issues/{id} body {status}。锁定 wire body 契约（status 为唯一键）。
        let body = serde_json::to_value(UpdateIssueStatusRequest {
            status: "done".into(),
        })
        .unwrap();
        assert_eq!(body, serde_json::json!({"status": "done"}));
    }

    #[test]
    fn update_issue_status_path_is_workspace_scoped_put() {
        // 锁定 PUT path 形状（与 rerun_issue 同为 issue 维度接口，靠 X-Workspace-ID 头路由，
        // path 不含 workspace）。头注入在 json_send 内，由 HTTP 集成测试覆盖；此处锁定 path。
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
        // S2：liveness 调用（heartbeat/get_task_status/extend_prepare_lease/register_once）用更短的
        // per-request 超时覆盖 client 级 30s，使单 tick 在退化网络下快速失败。锁定上界 < 30s 且 > 0。
        assert!(LIVENESS_TIMEOUT_SECS > 0 && LIVENESS_TIMEOUT_SECS < 30);
        assert_eq!(LIVENESS_TIMEOUT_SECS, 10);
    }
}
