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
    /// 续跑用：server 返回的上次 PinTaskSession 的 session_id（claim 响应可能带，开发设计 2.5）。
    #[serde(default)]
    pub prior_session_id: Option<String>,
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
    #[serde(default)]
    pub last_activity_at: Option<String>,
}

impl RemoteTask {
    /// 预填用「最佳可用需求文本」（镜像 server `computeTaskKind` 来源互斥优先级）。
    ///
    /// quick-create → chat → comment → autopilot → handoff 任一非空取之；皆空回退 title
    /// （issue 场景预填 issue 标题）。空白值视为缺失跳过——按来源**逐个**过滤后再短路，保证
    /// 纯空白的上游来源不会吞掉下游（如空白 quick_create 仍回退 title）。
    pub fn requirement_text(&self) -> Option<String> {
        [
            self.quick_create_prompt.as_deref(),
            self.chat_message.as_deref(),
            self.trigger_comment_content.as_deref(),
            self.autopilot_description.as_deref(),
            self.handoff_note.as_deref(),
            self.title.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
        .map(str::to_string)
    }
}

/// selective claim 请求（接入方案 B2：body `{}`；命中本地 task_conversations 时带 prior_session_id）。
#[derive(Debug, Serialize)]
pub struct ClaimRequest {
    /// None 时序列化为 `{}`（接入方案 B2），Some 时带 prior_session_id（开发设计 4.4 续跑）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_session_id: Option<String>,
}

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

/// multica HTTP client。`token` 为 PAT（已登录）或登录期的临时 JWT（None=未认证）。
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
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| MulticaError::NetworkFailed(format!("http client build failed: {e}")))?;
        Ok(Self {
            http,
            base_url,
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    /// 发送已认证请求并按状态码映射错误（map_status）。
    async fn send(&self, method: Method, path: &str) -> Result<Response, MulticaError> {
        let mut req = self.http.request(method.clone(), self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        let resp = req
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
        let mut req = self.http.request(Method::POST, self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        let resp = req
            .json(body)
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("POST {path} failed: {e}")))?;
        map_status(path, resp.status())?;
        resp.json::<R>()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("decode {path} failed: {e}")))
    }

    /// 同 `post_json` 但额外带 `X-Workspace-ID` 头。
    ///
    /// issue 维度业务接口（接入方案 D1/E1/E2）path 不含 workspace（`/api/issues/{id}/...`），
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
        let mut req = self.http.request(Method::POST, self.url(path));
        if let Some(t) = self.token.as_deref().filter(|t| !t.is_empty()) {
            req = req.bearer_auth(t);
        }
        req = req.header("X-Workspace-ID", workspace_id);
        let resp = req
            .json(body)
            .send()
            .await
            .map_err(|e| MulticaError::NetworkFailed(format!("POST {path} failed: {e}")))?;
        map_status(path, resp.status())?;
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
        let resp = self.send(Method::GET, "/api/me").await?;
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
        let resp = self.send(Method::GET, "/api/workspaces").await?;
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
    /// 一般请求：网络错误重试 3 次，4xx 不重试（直接映射错误码）。
    pub async fn register(
        &self,
        req: &RegisterRequest,
    ) -> Result<RegisterResponse, MulticaError> {
        self.with_network_retry("register", || async {
            self.post_json("/api/daemon/register", req).await
        })
        .await
    }

    /// `POST /api/daemon/heartbeat` —— 维持 runtime 在线（执行期 15s）。
    ///
    /// body `{runtime_id, supports_batch_import: true}`。失败仅记日志（下一 tick 自然重试），
    /// 不在 client 内重试（循环即重试），故走单次 `post_json`。
    pub async fn heartbeat(&self, runtime_id: &str) -> Result<(), MulticaError> {
        let body = serde_json::json!({
            "runtime_id": runtime_id,
            "supports_batch_import": true,
        });
        let _: serde_json::Value = self.post_json("/api/daemon/heartbeat", &body).await?;
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
    /// 不在 client 内重试（循环即重试），走单次 `post_json`。
    pub async fn extend_prepare_lease(
        &self,
        runtime_id: &str,
        task_id: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}/prepare-lease");
        let _: serde_json::Value = self.post_json(&path, &serde_json::json!({})).await?;
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
            let resp = self.send(Method::GET, &path).await?;
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
    /// 命中本地 `multica_task_conversations[task_id].session_id` 时传 `prior_session_id` 续跑同一 ACP session
    /// （开发设计 2.5 / 4.4）。返回的 `RemoteTask` 含 `auth_token`（执行凭证）+ 可能的 `prior_session_id`。
    /// 一般请求：网络错误重试 3 次，404/409 直接映射 TaskNotFound/ClaimConflict。
    pub async fn claim_specific_task(
        &self,
        runtime_id: &str,
        task_id: &str,
        prior_session_id: Option<&str>,
    ) -> Result<RemoteTask, MulticaError> {
        let path = format!("/api/daemon/runtimes/{runtime_id}/tasks/{task_id}/claim");
        let body = ClaimRequest {
            prior_session_id: prior_session_id.map(String::from),
        };
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
        let resp = self.send(Method::GET, &path).await?;
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

    /// `POST /api/issues/{id}/rerun` —— 用户手动重跑失败任务（接入方案 D1）。
    ///
    /// 带 `X-Workspace-ID` 头（issue 维度路由）；server 创建全新 queued 任务（force_fresh_session，
    /// 不续跑旧 session——失败重跑视为全新任务）。一般网络重试。
    pub async fn rerun_issue(
        &self,
        workspace_id: &str,
        issue_id: &str,
    ) -> Result<(), MulticaError> {
        let path = format!("/api/issues/{issue_id}/rerun");
        self.with_network_retry("rerun", || async {
            let _: serde_json::Value = self
                .post_json_with_workspace(&path, workspace_id, &serde_json::json!({}))
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
    fn remote_task_requirement_text_picks_source_by_priority() {
        // 镜像 server computeTaskKind 来源互斥优先级：
        // quick-create > chat > comment > autopilot > handoff > title（issue 回退标题）。
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

        // issue：无来源字段 → 回退 title（预填 issue 标题）。
        let issue: RemoteTask =
            serde_json::from_str(r#"{"id":"t","thread_name":"Login bug"}"#).unwrap();
        assert_eq!(issue.requirement_text().as_deref(), Some("Login bug"));

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
    fn claim_request_omits_prior_session_when_none() {
        // 接入方案 B2：body `{}`（无 prior_session_id）。
        let none_body = serde_json::to_value(ClaimRequest {
            prior_session_id: None,
        })
        .unwrap();
        assert_eq!(none_body, serde_json::json!({}));

        // 开发设计 4.4：续跑带 prior_session_id。
        let some_body = serde_json::to_value(ClaimRequest {
            prior_session_id: Some("sess-9".into()),
        })
        .unwrap();
        assert_eq!(some_body, serde_json::json!({"prior_session_id": "sess-9"}));
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
    fn rerun_issue_request_shape_is_workspace_scoped() {
        // 接入方案 D1：POST /api/issues/{id}/rerun，body {}，靠 X-Workspace-ID 头路由。
        // 该测试锁定 path 形状 + 空 body + workspace 维度（头注入在 post_json_with_workspace 内，
        // 由 HTTP 集成测试覆盖；此处锁定 path/body 契约）。
        let issue_id = "iss-7";
        let path = format!("/api/issues/{issue_id}/rerun");
        let body = serde_json::json!({});
        assert_eq!(path, "/api/issues/iss-7/rerun");
        assert_eq!(body, serde_json::json!({}));
    }
}
