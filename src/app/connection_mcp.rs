//! MCP 连接工具 server（连接→对话 阶段4）。
//!
//! 以内置 streamable-HTTP MCP server 暴露连接工具（list/open/send/read/close），
//! 供 ACP 适配器（claude-code）驱动 agent 连接操控；密钥永不出应用，agent 不重造 SSH。
//! 本模块固定**工具契约 + JSON-RPC 分发**（进程内可测，无需真实 agent）；HTTP 传输与
//! 桌面接线（`ServerConnectionStore` + `TerminalSessionManager`/`SshSessionManager`）在后续
//! 步骤接入。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::app::{
    App, ConnectionRunLocator, ConnectionSpec, TerminalSessionManager, TerminalSessionStatus,
    TerminalSessionView,
};

/// 注入 `WorkerInvocation.mcp_servers` 时 server name。
pub const CONNECTION_MCP_SERVER_NAME: &str = "gold-band-connections";
pub const CONNECTION_MCP_SERVER_VERSION: &str = "0.1.0";
/// 与 `mcp/mod.rs` 的 MCP 协议版本一致。
pub const CONNECTION_MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Agent 需运行连接工具的运行时。每个操作支撑一个 `tools/call`。
/// 由桌面层基于 `ServerConnectionStore` + `TerminalSessionManager`/`SshSessionManager`
/// 实现；测试用假实现注入。
pub trait ConnectionMcpRuntime: Send {
    fn list_connections(&self) -> Result<Vec<ConnectionSpec>>;
    fn open_connection(
        &mut self,
        connection_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<TerminalSessionView>;
    fn send_input(&self, session_id: &str, input: &str) -> Result<()>;
    fn read_output(
        &self,
        session_id: &str,
        from_offset: Option<u64>,
    ) -> Result<ConnectionOutputChunk>;
    fn close_connection(&self, session_id: &str) -> Result<()>;
}

/// `read_output` 的一帧有界输出（复用后端历史视角，不无界；`eof`=true 表示无后续）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionOutputChunk {
    pub text: String,
    pub eof: bool,
    /// 会话已终止时的结构化失败码（如 `terminal.auth-required` / `terminal.ssh-auth-failed`）。
    /// 会话失败后 entry 会被移除、输出历史被清空，若不在此带出原因，agent 只能看到
    /// `eof=true` 而无法区分「OTP 超时」与「连接被拒」。`None` 表示会话仍在或正常结束。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 会话当前状态（`connecting` / `awaiting-auth` / `connected`），随每帧带出——
    /// agent 轮询时无需盲猜「是在等口令」还是「网络慢」。会话已结束时为 `None`
    /// （失败原因见 `errorCode`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// `open_connection` 工具有界等待会话离开 Connecting 的窗口。短于后端
/// `AUTH_PROMPT_TIMEOUT`（120s）：口令等待不计入本窗口（AwaitingAuth 即返回，
/// 由 agent 提醒用户输口令），只覆盖纯网络/握手阶段的等待。
pub const CONNECTION_TOOL_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 命令执行语义：为未以行尾结束的输入补回车（agent 发送即执行）。以 `\r`/`\n`
/// 结尾、或输入本身就是控制字符（如 `\u{3}` 中断堡垒机菜单）时保持原样——
/// 现场事故：agent 发的命令没有行尾，在堡垒机里只「敲了字符没按回车」，永远不会执行。
pub(crate) fn with_exec_newline(input: &str) -> String {
    if input.ends_with('\r') || input.ends_with('\n') {
        return input.to_string();
    }
    // 控制字符结尾（Ctrl+C / Ctrl+D 等裸中断）保持原样，不追加可执行行尾。
    if input
        .as_bytes()
        .last()
        .is_some_and(|last| *last < 0x20 && *last != b'\t')
    {
        return input.to_string();
    }
    format!("{input}\r")
}

/// 有界等待会话离开 `Connecting`，抵达 Connected / AwaitingAuth / 结束。
/// - `Ok(())`：已到有意义状态（调用方按视图状态组织结果）。
/// - `Err(code)`：会话从存活表消失（连接失败被清理），`code` 为结构化失败码。
pub(crate) fn settle_terminal_session(
    manager: &Arc<Mutex<TerminalSessionManager>>,
    session_id: &str,
    timeout: std::time::Duration,
) -> std::result::Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        {
            let guard = manager
                .lock()
                .map_err(|_| "terminal lock poisoned".to_string())?;
            let status = guard
                .list()
                .iter()
                .find(|view| view.session_id == session_id)
                .map(|view| view.status);
            match status {
                // 连接成功，或进入口令等待：都是有意义且可行动的状态。
                Some(TerminalSessionStatus::Connected)
                | Some(TerminalSessionStatus::AwaitingAuth)
                | Some(TerminalSessionStatus::Closed) => return Ok(()),
                // 从存活表消失：连接失败已被清理，带出结构化原因。
                None => {
                    let code = guard
                        .recent_failure_code(session_id)
                        .unwrap_or_else(|| "terminal.connect-timeout".to_string());
                    return Err(code);
                }
                Some(TerminalSessionStatus::Connecting) => {}
            }
        }
        if std::time::Instant::now() >= deadline {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

/// 结构化工具错误：后端只回 `code`，前端/agent 侧本地化文案，不拼对客文案。
#[derive(Debug, Clone)]
pub struct ConnectionToolError {
    pub code: &'static str,
    pub message: String,
}

impl ConnectionToolError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ConnectionToolError {
    fn from(error: anyhow::Error) -> Self {
        Self::new("tool.internal", format!("{error:#}"))
    }
}

/// `tools/list` 的 5 个工具 schema。
pub fn connection_tools_schema() -> Vec<Value> {
    vec![
        tool(
            "list_connections",
            "列出当前对话绑定的可连接服务器（非密钥 spec）。",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "open_connection",
            "打开到指定连接的远程 shell 会话，返回 sessionId。",
            json!({
                "type": "object",
                "properties": {
                    "connectionId": { "type": "string" },
                    "cols": { "type": "integer", "minimum": 1 },
                    "rows": { "type": "integer", "minimum": 1 },
                },
                "required": ["connectionId"],
                "additionalProperties": false,
            }),
        ),
        tool(
            "send_input",
            "向已打开会话发送输入并执行（自动追加回车；以回车/换行结尾或控制字符则原样发送）。会话已结束时返回结构化失败码。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string" },
                    "input": { "type": "string" },
                },
                "required": ["sessionId", "input"],
                "additionalProperties": false,
            }),
        ),
        tool(
            "read_output",
            "读取会话自 fromOffset 起的新输出（有界帧），附带会话状态（connecting/awaiting-auth/connected）。awaiting-auth 表示正在等待用户输入动态口令（Gold Band 已向用户给出输入框）。",
            json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string" },
                    "fromOffset": { "type": "integer", "minimum": 0 },
                },
                "required": ["sessionId"],
                "additionalProperties": false,
            }),
        ),
        tool(
            "close_connection",
            "关闭已打开会话。",
            json!({
                "type": "object",
                "properties": { "sessionId": { "type": "string" } },
                "required": ["sessionId"],
                "additionalProperties": false,
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

/// 分发 `tools/call`，返回 `{"content":[...], "isError":false}` 的 `result`。
pub fn handle_tools_call(
    runtime: &mut dyn ConnectionMcpRuntime,
    name: &str,
    arguments: &Value,
) -> std::result::Result<Value, ConnectionToolError> {
    Ok(match name {
        "list_connections" => {
            let specs = runtime
                .list_connections()
                .map_err(ConnectionToolError::from)?;
            json_result(serde_json::to_string(&specs).unwrap_or_else(|_| "[]".to_string()))
        }
        "open_connection" => {
            let id = required_string(arguments, "connectionId")?;
            let cols = optional_u64(arguments, "cols")?.and_then(|v| u16::try_from(v).ok());
            let rows = optional_u64(arguments, "rows")?.and_then(|v| u16::try_from(v).ok());
            let view = runtime
                .open_connection(id, cols, rows)
                .map_err(ConnectionToolError::from)?;
            // Status-conditional guidance so the agent knows what to do next
            // (the desktop already surfaced an OTP input box for the user, or
            // the agent should start driving the shell).
            let note = match view.status {
                TerminalSessionStatus::AwaitingAuth => Some(
                    "The session is waiting for the one-time passphrase. Gold Band has already surfaced a passphrase input box to the user (in the conversation when this tool opened the session, otherwise over the terminal panel); do NOT ask the user for the passphrase and do NOT send it via send_input. Just tell the user a passphrase is required and poll read_output until the status is connected. If it fails (auth timeout), open_connection can be retried with a fresh code.",
                ),
                TerminalSessionStatus::Connecting => {
                    Some("The session is still connecting; poll read_output to check progress.")
                }
                TerminalSessionStatus::Closed => Some(
                    "The session closed immediately after opening; report the failure code to the user.",
                ),
                TerminalSessionStatus::Connected => None,
            };
            let payload = match note {
                Some(note) => json!({ "session": view, "note": note }),
                None => json!(view),
            };
            json_result(payload.to_string())
        }
        "send_input" => {
            let session_id = required_string(arguments, "sessionId")?;
            let input = required_string(arguments, "input")?;
            runtime
                .send_input(session_id, input)
                .map_err(ConnectionToolError::from)?;
            json_result("ok".to_string())
        }
        "read_output" => {
            let session_id = required_string(arguments, "sessionId")?;
            let from_offset = optional_u64(arguments, "fromOffset")?;
            let chunk = runtime
                .read_output(session_id, from_offset)
                .map_err(ConnectionToolError::from)?;
            json_result(serde_json::to_string(&chunk).unwrap_or_else(|_| "{}".to_string()))
        }
        "close_connection" => {
            let session_id = required_string(arguments, "sessionId")?;
            runtime
                .close_connection(session_id)
                .map_err(ConnectionToolError::from)?;
            json_result("ok".to_string())
        }
        _ => {
            return Err(ConnectionToolError::new(
                "tool.not-found",
                format!("unknown tool `{name}`"),
            ));
        }
    })
}

/// MCP JSON-RPC 入口：`initialize` / `tools/list` / `tools/call` / notifications。
/// notification（无 id）返回 `Value::Null`（fire-and-forget）。
pub fn handle_jsonrpc(request: &Value, runtime: &mut dyn ConnectionMcpRuntime) -> Value {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if method.starts_with("notifications/") {
        return Value::Null;
    }
    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": CONNECTION_MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": CONNECTION_MCP_SERVER_NAME,
                    "version": CONNECTION_MCP_SERVER_VERSION,
                },
            },
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": connection_tools_schema() },
        }),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match handle_tools_call(runtime, name, &arguments) {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(err) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32000,
                        "message": err.message,
                        "data": { "code": err.code },
                    },
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "method not found" },
        }),
    }
}

/// 构建注入 `WorkerInvocation.mcp_servers` 的连接 server 配置（streamable-HTTP）。
/// `headers: []` 必须显式携带：ACP SDK 的 `session/new` 参数校验会**静默丢弃**缺
/// `headers` 的 http 条目（现场事故：内置 server 带 `headers: []` 正常注册，本
/// server 缺失该字段导致 agent 侧工具从未出现）。空数组即「无附加头」，语义安全。
pub fn connection_mcp_server_config(url: &str) -> Value {
    json!({
        "type": "http",
        "name": CONNECTION_MCP_SERVER_NAME,
        "url": url,
        "headers": [],
    })
}

fn json_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

/// 桌面层 `ConnectionMcpRuntime`：把连接工具接到 `App`（连接解析与凭据）+
/// 共享 `TerminalSessionManager`（会话 open/write/close）+ 后端输出历史。
/// 供进程内 MCP server 工具直接调用；会话与用户 UI 共享同一 manager（可审阅/接管）。
pub struct AppConnectionMcpRuntime {
    app: App,
    /// 绑定到该对话的连接（`RunState.connection_specs`）；只有它们可被打开。
    bound_specs: Vec<ConnectionSpec>,
    manager: Arc<Mutex<TerminalSessionManager>>,
    history: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// 发起本次调用的对话 run。打开会话时写入会话条目，口令提示据此把输入界面
    /// 路由回该对话（会话内输入框），而不是终端面板弹窗。
    origin: ConnectionRunLocator,
}

impl AppConnectionMcpRuntime {
    pub fn new(
        app: App,
        bound_specs: Vec<ConnectionSpec>,
        manager: Arc<Mutex<TerminalSessionManager>>,
        history: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        origin: ConnectionRunLocator,
    ) -> Self {
        Self {
            app,
            bound_specs,
            manager,
            history,
            origin,
        }
    }

    fn session_is_open(&self, session_id: &str) -> bool {
        self.manager
            .lock()
            .ok()
            .map(|guard| {
                guard.list().iter().any(|view| {
                    view.session_id == session_id && view.status != TerminalSessionStatus::Closed
                })
            })
            .unwrap_or(false)
    }
}

/// 每次 HTTP 请求按 `(taskId, runId)` 现读 `run.connection_specs` 并构建一个新的
/// [`AppConnectionMcpRuntime`]。`AppConnectionMcpRuntime` 本身无跨调用状态——会话状态
/// 全部活在共享的 `manager`/`history`（desktop 单例），绑定集合每次现读也天然正确
/// （绑定变化立即生效）——所以按请求现建 runtime 既简单又和「全局单端点、按 run 现读
/// 绑定」的设计一致：天然支持多对话并发、互不干扰，无需为每个 run 维护独立 runtime
/// 生命周期。
pub struct ConnectionMcpRuntimeFactory {
    app: App,
    manager: Arc<Mutex<TerminalSessionManager>>,
    history: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Resolves a URL `projectId` to the workspace-scoped paths that own the
    /// run's data. Runs live under per-workspace data dirs
    /// (`~/<data>/projects/<workspace-project-id>/…`), so the endpoint — built
    /// on the global app — cannot find them without this resolver (field
    /// incident: initialize answered 400 "failed to load run" for every
    /// conversation workspace, so agents never got the tools).
    workspace_paths_resolver:
        Arc<dyn Fn(&str) -> Result<crate::storage::GoldBandPaths> + Send + Sync>,
}

impl ConnectionMcpRuntimeFactory {
    pub fn new(
        app: App,
        manager: Arc<Mutex<TerminalSessionManager>>,
        history: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        workspace_paths_resolver: Arc<
            dyn Fn(&str) -> Result<crate::storage::GoldBandPaths> + Send + Sync,
        >,
    ) -> Self {
        Self {
            app,
            manager,
            history,
            workspace_paths_resolver,
        }
    }

    /// 现读 `run.connection_specs`，构建该 run 作用域的 runtime。连接解析走基础
    /// App（用户级凭据/连接存储，工作区无关）；run 数据经 resolver 定位到该工作区。
    /// run 不存在或读取失败时返回错误（调用方映射为 HTTP 400/404，不崩溃）。
    pub fn build_for_run(
        &self,
        project_id: &str,
        task_id: &str,
        run_id: &str,
    ) -> Result<AppConnectionMcpRuntime> {
        let paths = (self.workspace_paths_resolver)(project_id)
            .with_context(|| format!("failed to resolve workspace for project `{project_id}`"))?;
        let run: crate::runtime::RunState =
            crate::storage::read_json(&paths.run_file(task_id, run_id))
                .with_context(|| format!("failed to load run `{task_id}/{run_id}`"))?;
        Ok(AppConnectionMcpRuntime::new(
            self.app.clone_for_background(),
            run.connection_specs,
            Arc::clone(&self.manager),
            Arc::clone(&self.history),
            ConnectionRunLocator {
                project_id: project_id.to_string(),
                task_id: task_id.to_string(),
                run_id: run_id.to_string(),
            },
        ))
    }
}

impl ConnectionMcpRuntime for AppConnectionMcpRuntime {
    fn list_connections(&self) -> Result<Vec<ConnectionSpec>> {
        Ok(self.bound_specs.clone())
    }

    fn open_connection(
        &mut self,
        connection_id: &str,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<TerminalSessionView> {
        // 会话级作用域：只能打开绑定到该对话的连接。
        if !self.bound_specs.iter().any(|spec| spec.id == connection_id) {
            return Err(anyhow!("connection.not-found: {connection_id}"));
        }
        let request = self
            .app
            .resolve_ssh_request(connection_id)?
            .with_origin(self.origin.clone());
        let view = {
            let guard = self
                .manager
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            guard
                .open_remote(request)
                .with_context(|| format!("open_connection `{connection_id}` failed"))?
        };
        // 有界等待首个有意义状态（连接成功 / 等待口令 / 失败），让工具结果对 agent
        // 可行动：Connecting 直接返回会让 agent 无从知晓该提醒用户输 OTP 还是轮询。
        let settled = settle_terminal_session(
            &self.manager,
            &view.session_id,
            CONNECTION_TOOL_SETTLE_TIMEOUT,
        );
        match settled {
            // Connected / AwaitingAuth：返回当前视图（状态由调用方转成指引）。
            Ok(()) => Ok(view),
            // Failed：把结构化失败码作为工具错误抛给 agent（如 terminal.auth-required）。
            Err(failure_code) => Err(anyhow!("{failure_code}")),
        }
    }

    fn send_input(&self, session_id: &str, input: &str) -> Result<()> {
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(with_exec_newline(input).as_bytes());
        let mut guard = self
            .manager
            .lock()
            .map_err(|_| anyhow!("terminal lock poisoned"))?;
        if let Err(error) = guard.write(session_id, &encoded) {
            // 会话消失时带出此前的结构化失败码（如 terminal.auth-required），
            // agent 据此自动重试 open_connection，而不是拿到裸 session-not-found。
            let reason = guard
                .recent_failure_code(session_id)
                .map(|code| format!(" (session ended earlier: {code})"))
                .unwrap_or_default();
            return Err(anyhow!("terminal.session-not-found{reason}")
                .context(format!("send_input `{session_id}` failed")));
        }
        Ok(())
    }

    fn read_output(
        &self,
        session_id: &str,
        from_offset: Option<u64>,
    ) -> Result<ConnectionOutputChunk> {
        let text = {
            let history = self
                .history
                .lock()
                .map_err(|_| anyhow!("history lock poisoned"))?;
            let buffer = history.get(session_id).cloned().unwrap_or_default();
            let offset = from_offset.unwrap_or(0) as usize;
            if offset <= buffer.len() {
                String::from_utf8_lossy(&buffer[offset..]).into_owned()
            } else {
                String::new()
            }
        };
        let open = self.session_is_open(session_id);
        let status = open
            .then(|| {
                self.manager.lock().ok().and_then(|guard| {
                    guard
                        .list()
                        .iter()
                        .find(|view| view.session_id == session_id)
                        .map(|view| match view.status {
                            TerminalSessionStatus::Connecting => "connecting",
                            TerminalSessionStatus::AwaitingAuth => "awaiting-auth",
                            TerminalSessionStatus::Connected => "connected",
                            TerminalSessionStatus::Closed => "closed",
                        })
                        .map(str::to_string)
                })
            })
            .flatten();
        // A terminated session's entry is removed; recover the reason it died so
        // the agent can act on it instead of just seeing a bare `eof`.
        let error_code = if open {
            None
        } else {
            self.manager
                .lock()
                .ok()
                .and_then(|guard| guard.recent_failure_code(session_id))
        };
        Ok(ConnectionOutputChunk {
            text,
            eof: !open,
            status,
            error_code,
        })
    }

    fn close_connection(&self, session_id: &str) -> Result<()> {
        let mut guard = self
            .manager
            .lock()
            .map_err(|_| anyhow!("terminal lock poisoned"))?;
        guard
            .close(session_id)
            .with_context(|| format!("close_connection `{session_id}` failed"))
    }
}

fn required_string<'a>(
    arguments: &'a Value,
    key: &str,
) -> std::result::Result<&'a str, ConnectionToolError> {
    // Agents occasionally send snake_case despite the camelCase schema (observed
    // in the field: `connection_id`); accept both rather than failing the call.
    let snake = key
        .chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                vec!['_', c.to_ascii_lowercase()]
            } else {
                vec![c]
            }
        })
        .collect::<String>();
    for candidate in [key, snake.as_str()] {
        if let Some(value) = arguments.get(candidate).and_then(Value::as_str) {
            return Ok(value);
        }
    }
    Err(ConnectionToolError::new(
        "tool.bad-arguments",
        format!("missing string `{key}`"),
    ))
}

fn optional_u64(
    arguments: &Value,
    key: &str,
) -> std::result::Result<Option<u64>, ConnectionToolError> {
    for candidate in [key] {
        match arguments.get(candidate) {
            None => continue,
            Some(value) => {
                return value.as_u64().map(Some).ok_or_else(|| {
                    ConnectionToolError::new(
                        "tool.bad-arguments",
                        format!("`{key}` must be an integer"),
                    )
                });
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        ConnectionAuthMethod, ConnectionEnvironment, TerminalSessionKind, TerminalSessionStatus,
    };

    #[derive(Default)]
    struct FakeRuntime {
        connections: Vec<ConnectionSpec>,
        opened: Vec<String>,
        outputs: std::collections::HashMap<String, ConnectionOutputChunk>,
    }

    impl ConnectionMcpRuntime for FakeRuntime {
        fn list_connections(&self) -> Result<Vec<ConnectionSpec>> {
            Ok(self.connections.clone())
        }
        fn open_connection(
            &mut self,
            connection_id: &str,
            _cols: Option<u16>,
            _rows: Option<u16>,
        ) -> Result<TerminalSessionView> {
            self.opened.push(connection_id.to_string());
            Ok(TerminalSessionView {
                session_id: format!("sess-{connection_id}"),
                kind: TerminalSessionKind::Remote,
                cwd: None,
                label: Some(connection_id.to_string()),
                status: TerminalSessionStatus::Connected,
                origin: None,
            })
        }
        fn send_input(&self, _session_id: &str, _input: &str) -> Result<()> {
            Ok(())
        }
        fn read_output(
            &self,
            session_id: &str,
            _from_offset: Option<u64>,
        ) -> Result<ConnectionOutputChunk> {
            Ok(self
                .outputs
                .get(session_id)
                .cloned()
                .unwrap_or(ConnectionOutputChunk {
                    text: String::new(),
                    eof: true,
                    status: None,
                    error_code: None,
                }))
        }
        fn close_connection(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn spec() -> ConnectionSpec {
        ConnectionSpec {
            id: "conn-1".to_string(),
            name: "wb-prod".to_string(),
            host: "10.0.0.5".to_string(),
            port: 22,
            username: "app".to_string(),
            auth_method: ConnectionAuthMethod::PublicKey,
            env: ConnectionEnvironment::Prod,
            jump_ref: None,
        }
    }

    fn call_req(id: u64, name: &str, arguments: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } })
    }

    #[test]
    fn initialize_reports_tools_capability_and_server_info() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
            &mut rt,
        );
        assert_eq!(resp["id"], 1);
        assert_eq!(
            resp["result"]["protocolVersion"],
            CONNECTION_MCP_PROTOCOL_VERSION
        );
        assert_eq!(
            resp["result"]["serverInfo"]["name"],
            CONNECTION_MCP_SERVER_NAME
        );
        assert_eq!(resp["result"]["capabilities"]["tools"], json!({}));
    }

    #[test]
    fn tools_list_returns_the_five_connection_tools() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            &mut rt,
        );
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_connections",
                "open_connection",
                "send_input",
                "read_output",
                "close_connection"
            ]
        );
    }

    #[test]
    fn tools_call_list_connections_returns_non_secret_specs() {
        let mut rt = FakeRuntime::default();
        rt.connections = vec![spec()];
        let resp = handle_jsonrpc(&call_req(3, "list_connections", json!({})), &mut rt);
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Vec<ConnectionSpec> = serde_json::from_str(text).unwrap();
        assert_eq!(parsed[0].name, "wb-prod");
        assert_eq!(parsed[0].host, "10.0.0.5");
    }

    #[test]
    fn tools_call_open_connection_returns_session_view() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(
            &call_req(4, "open_connection", json!({ "connectionId": "conn-1" })),
            &mut rt,
        );
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(rt.opened, vec!["conn-1".to_string()]);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("sess-conn-1"));
    }

    #[test]
    fn tools_call_read_output_returns_bounded_chunk() {
        let mut rt = FakeRuntime::default();
        rt.outputs.insert(
            "sess-1".to_string(),
            ConnectionOutputChunk {
                text: "ok".to_string(),
                eof: true,
                status: None,
                error_code: None,
            },
        );
        let resp = handle_jsonrpc(
            &call_req(5, "read_output", json!({ "sessionId": "sess-1" })),
            &mut rt,
        );
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"text\":\"ok\""));
    }

    #[test]
    fn tools_call_reports_unknown_tool_as_structured_error() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(&call_req(6, "deploy", json!({})), &mut rt);
        assert_eq!(resp["error"]["code"], -32000);
        assert_eq!(resp["error"]["data"]["code"], "tool.not-found");
    }

    #[test]
    fn tools_call_accepts_snake_case_argument_aliases() {
        // Observed in the field: agents send `connection_id` despite the
        // camelCase schema. The call must go through rather than bounce.
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(
            &call_req(8, "open_connection", json!({ "connection_id": "conn-1" })),
            &mut rt,
        );
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(rt.opened, vec!["conn-1".to_string()]);
    }

    #[test]
    fn tools_call_reports_bad_arguments_as_structured_error() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(&call_req(7, "open_connection", json!({})), &mut rt);
        assert_eq!(resp["error"]["data"]["code"], "tool.bad-arguments");
    }

    #[test]
    fn notifications_are_ignored() {
        let mut rt = FakeRuntime::default();
        let resp = handle_jsonrpc(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            &mut rt,
        );
        assert!(resp.is_null());
    }

    #[test]
    fn server_config_is_http_named_and_points_at_url() {
        let config = connection_mcp_server_config("http://127.0.0.1:9000/mcp");
        assert_eq!(config["type"], "http");
        assert_eq!(config["name"], CONNECTION_MCP_SERVER_NAME);
        assert_eq!(config["url"], "http://127.0.0.1:9000/mcp");
        // `headers` must be present (even empty): the ACP SDK's session/new
        // validation silently drops http entries that lack it, so the agent
        // would never see the tools (field incident, 2026-09-17).
        assert_eq!(config["headers"], serde_json::json!([]));
    }

    // ── AppConnectionMcpRuntime ──

    use std::sync::Mutex as StdMutex;

    use crate::app::ServerConnectionInput;
    use crate::app::credentials::CredentialStore;

    #[derive(Default)]
    struct MemCredentialStore(StdMutex<std::collections::HashMap<String, String>>);

    impl CredentialStore for MemCredentialStore {
        fn store_secret(&self, connection_id: &str, secret: &str) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .insert(connection_id.to_string(), secret.to_string());
            Ok(())
        }
        fn get_secret(&self, connection_id: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(connection_id).cloned())
        }
        fn delete_secret(&self, connection_id: &str) -> Result<()> {
            self.0.lock().unwrap().remove(connection_id);
            Ok(())
        }
        fn has_secret(&self, connection_id: &str) -> Result<bool> {
            Ok(self.0.lock().unwrap().contains_key(connection_id))
        }
    }

    static HOME_VAR_COUNTER: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn runtime_harness() -> (AppConnectionMcpRuntime, String, tempfile::TempDir) {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let repo_root =
            camino::Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        // Isolate this test's user-level config dir (the connections store is
        // user-scoped, not repo-scoped) to avoid cross-test pollution.
        let counter = HOME_VAR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let home_env_var: &'static str =
            Box::leak(format!("GOLD_BAND_TEST_HOME_CONNECTION_MCP_{counter}").into_boxed_str());
        let home_dir = temp.path().join("home");
        // SAFETY: the env var name is unique per test (see above), so setting it
        // cannot race with any other test's home resolution.
        unsafe {
            std::env::set_var(home_env_var, &home_dir);
        }
        let path_config = crate::storage::StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var,
        };
        let app = App::with_config_and_path_config(
            repo_root,
            crate::config::RuntimeConfig::default(),
            path_config,
        )
        .with_credential_store(Arc::new(MemCredentialStore::default()));
        // Password auth with a stored secret (OS credential store stubbed in-memory).
        let input = ServerConnectionInput {
            name: "wb".to_string(),
            host: "127.0.0.1".to_string(),
            port: 1, // unreachable: open_remote returns a Connecting view immediately
            username: "ops".to_string(),
            auth_method: ConnectionAuthMethod::Password,
            key_path: None,
            jump_ref: None,
            env: ConnectionEnvironment::Test,
            secret: Some("s3cret".to_string()),
            clear_secret: false,
        };
        let connection = app.create_server_connection(input).unwrap();
        let manager = Arc::new(Mutex::new(TerminalSessionManager::new(
            Arc::new(|_| {}),
            temp.path().join("known_hosts"),
        )));
        let history = Arc::new(Mutex::new(HashMap::new()));
        let runtime = AppConnectionMcpRuntime::new(
            app,
            vec![connection.spec()],
            manager,
            history,
            ConnectionRunLocator {
                project_id: "project-a".to_string(),
                task_id: "task-1".to_string(),
                run_id: "run-1".to_string(),
            },
        );
        (runtime, connection.id, temp)
    }

    #[test]
    fn factory_builds_runtime_scoped_to_the_run_bound_connections() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let repo_root =
            camino::Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        let counter = HOME_VAR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let home_env_var: &'static str = Box::leak(
            format!("GOLD_BAND_TEST_HOME_CONNECTION_MCP_FACTORY_{counter}").into_boxed_str(),
        );
        // SAFETY: unique env var name per test (see runtime_harness above).
        unsafe {
            std::env::set_var(home_env_var, temp.path().join("home"));
        }
        let path_config = crate::storage::StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var,
        };
        let app = App::with_config_and_path_config(
            repo_root,
            crate::config::RuntimeConfig::default(),
            path_config,
        );
        let run = crate::runtime::RunState {
            version: crate::domain::VERSION.to_string(),
            id: "run-001".to_string(),
            task_id: "task-001".to_string(),
            task_uuid: None,
            status: crate::domain::RunStatus::Running,
            outcome: None,
            started_at: "2026-09-09T00:00:00Z".to_string(),
            updated_at: "2026-09-09T00:00:00Z".to_string(),
            workflow_snapshot: "workflow.snapshot.json".to_string(),
            current_round: None,
            current_node: None,
            current_attempt: None,
            new_rounds_opened: 0,
            pause_reason: None,
            uuid: None,
            last_executed_node: None,
            worktree: None,
            execution: Default::default(),
            connection_specs: vec![ConnectionSpec {
                id: "conn-9".to_string(),
                name: "wb-prod".to_string(),
                host: "10.0.0.9".to_string(),
                port: 22,
                username: "ops".to_string(),
                auth_method: ConnectionAuthMethod::PublicKey,
                env: ConnectionEnvironment::Prod,
                jump_ref: None,
            }],
        };
        crate::storage::write_json(&app.paths.run_file("task-001", "run-001"), &run).unwrap();

        let manager = Arc::new(Mutex::new(TerminalSessionManager::new(
            Arc::new(|_| {}),
            temp.path().join("known_hosts"),
        )));
        let history = Arc::new(Mutex::new(HashMap::new()));
        let resolver_paths = app.paths.clone();
        let factory = ConnectionMcpRuntimeFactory::new(
            app,
            manager,
            history,
            Arc::new(move |_| Ok(resolver_paths.clone())),
        );

        let runtime = match factory.build_for_run("project-x", "task-001", "run-001") {
            Ok(runtime) => runtime,
            Err(error) => panic!("build_for_run failed: {error:#}"),
        };
        let specs = runtime.list_connections().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "wb-prod");
        // 每次 HTTP 请求按 (projectId, taskId, runId) 现建 runtime，origin 必须
        // 精确等于请求定位：open_connection 用它给会话打来源标记，口令输入据此
        // 路由回发起对话。
        assert_eq!(
            runtime.origin,
            ConnectionRunLocator {
                project_id: "project-x".to_string(),
                task_id: "task-001".to_string(),
                run_id: "run-001".to_string(),
            }
        );

        match factory.build_for_run("project-x", "task-001", "run-missing") {
            Ok(_) => panic!("expected build_for_run to fail for a missing run"),
            Err(error) => assert!(error.to_string().contains("run-missing")),
        }
    }

    #[test]
    fn runtime_lists_only_bound_specs() {
        let (runtime, _id, _temp) = runtime_harness();
        let specs = runtime.list_connections().unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "wb");
    }

    #[test]
    fn runtime_open_connection_rejects_unbound_connection() {
        let (mut runtime, _id, _temp) = runtime_harness();
        let err = runtime
            .open_connection("conn-unknown", None, None)
            .unwrap_err();
        assert!(err.to_string().contains("connection.not-found"));
    }

    #[test]
    fn with_exec_newline_appends_carriage_return_only_when_missing() {
        // 阻断点 C 的回归：命令缺行尾在堡垒机里永远不会执行（现场事故）。
        assert_eq!(with_exec_newline("ls /data"), "ls /data\r");
        assert_eq!(with_exec_newline("ls /data\r"), "ls /data\r");
        assert_eq!(with_exec_newline("ls /data\n"), "ls /data\n");
        // 控制字符（如 Ctrl+C 中断菜单）不被追加行尾。
        assert_eq!(with_exec_newline("\u{3}"), "\u{3}");
    }

    #[test]
    fn send_input_on_a_dead_session_surfaces_the_structured_failure_code() {
        // 阻断点 B 的回归：agent 需要知道会话死于 auth-required 才会自动重试。
        let (runtime, _id, _temp) = runtime_harness();
        runtime
            .manager
            .lock()
            .unwrap()
            .record_terminal_failure("sess-dead", "terminal.auth-required");

        let error = runtime.send_input("sess-dead", "ls /data").unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("terminal.session-not-found")
                && message.contains("terminal.auth-required"),
            "failure message carries both the fact and the reason: {message}"
        );
    }

    #[test]
    fn read_output_for_an_ended_session_reports_status_none_and_failure_code() {
        let (runtime, _id, _temp) = runtime_harness();
        runtime
            .history
            .lock()
            .unwrap()
            .insert("sess-dead2".to_string(), b"partial".to_vec());
        runtime
            .manager
            .lock()
            .unwrap()
            .record_terminal_failure("sess-dead2", "terminal.auth-required");

        let chunk = runtime.read_output("sess-dead2", None).unwrap();
        assert_eq!(chunk.text, "partial");
        assert!(chunk.eof);
        assert_eq!(chunk.status, None);
        assert_eq!(chunk.error_code.as_deref(), Some("terminal.auth-required"));
    }

    #[test]
    fn runtime_open_connection_propagates_structured_failure_for_unreachable_host() {
        // 127.0.0.1:1 refuses immediately: the settle window must observe the
        // failure and surface the structured code instead of returning a dead
        // Connecting view for the agent to poll forever.
        let (mut runtime, id, _temp) = runtime_harness();
        let error = runtime.open_connection(&id, None, None).unwrap_err();
        assert!(
            error.to_string().contains("terminal."),
            "structured failure code in message: {error}"
        );
    }

    #[test]
    fn settle_terminal_session_errors_immediately_for_an_unknown_session() {
        let manager = Arc::new(Mutex::new(TerminalSessionManager::new(
            Arc::new(|_| {}),
            std::env::temp_dir().join("connection-mcp-settle-known-hosts"),
        )));
        let result = settle_terminal_session(
            &manager,
            "sess-unknown",
            std::time::Duration::from_millis(50),
        );
        assert_eq!(result.unwrap_err(), "terminal.connect-timeout");
    }

    #[test]
    fn runtime_read_output_returns_history_chunk_and_eof_for_closed_session() {
        let (runtime, _id, _temp) = runtime_harness();
        runtime
            .history
            .lock()
            .unwrap()
            .insert("sess-x".to_string(), b"hello world".to_vec());
        let chunk = runtime.read_output("sess-x", Some(6)).unwrap();
        assert_eq!(chunk.text, "world");
        assert!(chunk.eof, "session not open, so output is exhausted");
    }

    #[test]
    fn runtime_send_and_close_unknown_session_are_errors() {
        let (runtime, _id, _temp) = runtime_harness();
        assert!(runtime.send_input("nope", "ls").is_err());
        assert!(runtime.close_connection("nope").is_err());
    }

    #[test]
    fn runtime_read_output_surfaces_the_terminal_failure_code() {
        // A terminated session's entry is removed, so the agent would otherwise
        // only see a bare `eof`. The reason it died must still reach the tool.
        let (runtime, _id, _temp) = runtime_harness();
        runtime
            .manager
            .lock()
            .unwrap()
            .record_terminal_failure("sess-dead", "terminal.auth-required");

        let chunk = runtime.read_output("sess-dead", None).unwrap();
        assert!(chunk.eof, "a dead session has no further output");
        assert_eq!(chunk.error_code.as_deref(), Some("terminal.auth-required"));
    }

    #[test]
    fn runtime_read_output_for_a_live_session_has_no_error_code() {
        // Simulate a live session through the settle-independent path: output
        // history present, failure code absent — read_output must not attach one.
        let (runtime, _id, _temp) = runtime_harness();
        runtime
            .history
            .lock()
            .unwrap()
            .insert("sess-live".to_string(), b"boot ok".to_vec());
        let chunk = runtime.read_output("sess-live", None).unwrap();
        assert_eq!(chunk.text, "boot ok");
        assert!(chunk.error_code.is_none());
    }
}
