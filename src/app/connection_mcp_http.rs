//! MCP streamable-HTTP 传输端点（连接→对话 阶段4）。
//!
//! 为连接工具 server 提供 `mcp-session-id` / `MCP-Protocol-Version` 的 HTTP 边界。
//! 本模块固定**可测的 HTTP 契约**：请求解析 + HTTP 响应（注入 `ConnectionMcpRuntime` 分发）。
//! 真正的 `TcpListener` 循环是薄胶水（见 [`serve`]）；claude-code 端到端握手留待真实 agent 验收。

use std::io::{BufRead, Result as IoResult};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::app::{ConnectionMcpRuntime, ConnectionMcpRuntimeFactory, handle_jsonrpc};

/// 连接工具 server 的 HTTP 端点路径。
pub const MCP_ENDPOINT_PATH: &str = "/mcp";

/// 生成的 session id 前缀。
const SESSION_PREFIX: &str = "connection-mcp-";

static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 解析出的最小 HTTP/1.1 请求（仅 localhost endpoint 所需字段）。
#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// MCP endpoint 的 HTTP 响应。
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub content_type: String,
    /// `initialize` 成功时下发 `mcp-session-id`；其余请求不设置。
    pub set_session: Option<String>,
}

/// 从 HTTP/1.1 请求流解析 `HttpRequest`（请求行 + headers + Content-Length body）。
pub fn parse_http_request(reader: &mut impl BufRead) -> Result<HttpRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        bail!("empty request line");
    }
    let mut parts = request_line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or_default();
            }
            headers.push((name, value));
        }
    }
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .context("failed to read body")?;
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

/// 分发一个已解析的 HTTP 请求到 MCP JSON-RPC，返回 HTTP 响应。
/// 纯函数：可测试（无需 socket）。
pub fn respond(runtime: &mut dyn ConnectionMcpRuntime, request: &HttpRequest) -> HttpResponse {
    if request_path_without_query(request) != MCP_ENDPOINT_PATH {
        return plain(404, "not found");
    }
    match request.method.as_str() {
        "POST" => respond_post(runtime, &request.body),
        "DELETE" => HttpResponse {
            status: 200,
            body: String::new(),
            content_type: "application/json".to_string(),
            set_session: None,
        },
        _ => plain(405, "method not allowed"),
    }
}

fn respond_post(runtime: &mut dyn ConnectionMcpRuntime, body: &str) -> HttpResponse {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return plain(400, "invalid JSON-RPC body"),
    };
    // Notifications (no id) are fire-and-forget: 202 Accepted, no body.
    let is_notification = value
        .get("method")
        .and_then(Value::as_str)
        .map(|method| method.starts_with("notifications/"))
        .unwrap_or_default();
    if is_notification {
        return HttpResponse {
            status: 202,
            body: String::new(),
            content_type: "application/json".to_string(),
            set_session: None,
        };
    }
    let response = handle_jsonrpc(&value, runtime);
    let is_initialize = value.get("method").and_then(Value::as_str) == Some("initialize");
    HttpResponse {
        status: 200,
        body: serde_json::to_string(&response).unwrap_or_default(),
        content_type: "application/json".to_string(),
        set_session: is_initialize.then(new_session_id),
    }
}

fn plain(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.to_string(),
        content_type: "text/plain".to_string(),
        set_session: None,
    }
}

fn new_session_id() -> String {
    let n = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{SESSION_PREFIX}{n}")
}

fn request_path_without_query(request: &HttpRequest) -> &str {
    request.path.split('?').next().unwrap_or(&request.path)
}

/// 解析 `path` 中的 `key=value` 查询参数（不做百分号解码——`taskId`/`runId` 是内部
/// 生成的 `task-NNN`/`run-NNN` 形式标识符，从不含需要转义的字符）。
fn query_param<'a>(request: &'a HttpRequest, key: &str) -> Option<&'a str> {
    let query = request.path.split_once('?').map(|(_, query)| query)?;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

/// 全局单端点 + URL 带 run 作用域：按请求的 `?taskId=..&runId=..` 现读
/// `run.connection_specs` 构建 runtime，再分发。天然支持多对话并发、互不干扰
/// （详见 [`ConnectionMcpRuntimeFactory`]）。缺少/无法解析的 `taskId`/`runId`
/// 或该 run 不存在都返回 400，不崩溃。
pub fn respond_scoped(
    factory: &ConnectionMcpRuntimeFactory,
    request: &HttpRequest,
) -> HttpResponse {
    if request_path_without_query(request) != MCP_ENDPOINT_PATH {
        return plain(404, "not found");
    }
    let Some(project_id) = query_param(request, "projectId") else {
        return plain(400, "missing `projectId` query parameter");
    };
    let Some(task_id) = query_param(request, "taskId") else {
        return plain(400, "missing `taskId` query parameter");
    };
    let Some(run_id) = query_param(request, "runId") else {
        return plain(400, "missing `runId` query parameter");
    };
    let mut runtime = match factory.build_for_run(project_id, task_id, run_id) {
        Ok(runtime) => runtime,
        Err(error) => return plain(400, &format!("{error:#}")),
    };
    respond(&mut runtime, request)
}

/// 构建注入 `mcp_servers` 时用的按 run 作用域 URL。`projectId` 必带：run 数据
/// 按工作区数据目录分片存放，端点（全局 App）须经它解析出正确的工作区路径。
pub fn connection_mcp_url(port: u16, project_id: &str, task_id: &str, run_id: &str) -> String {
    format!(
        "http://127.0.0.1:{port}{MCP_ENDPOINT_PATH}?projectId={project_id}&taskId={task_id}&runId={run_id}"
    )
}

/// 启动 localhost MCP endpoint 的 accept 循环。`stop` 置位后退出。
/// 每个连接独立线程处理；每次请求按 URL 的 `taskId`/`runId` 现建 run 作用域 runtime
/// （见 [`respond_scoped`]），天然支持多对话并发。
pub fn serve(
    listener: TcpListener,
    factory: Arc<ConnectionMcpRuntimeFactory>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let factory = Arc::clone(&factory);
                std::thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &factory) {
                        tracing::debug!(%error, "connection MCP HTTP handler error");
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle_connection(
    mut stream: std::net::TcpStream,
    factory: &ConnectionMcpRuntimeFactory,
) -> Result<()> {
    let mut reader = std::io::BufReader::new(stream.try_clone().context("stream clone")?);
    let request = parse_http_request(&mut reader)?;
    let response = respond_scoped(factory, &request);
    write_response(&mut stream, &response)?;
    Ok(())
}

fn write_response(stream: &mut std::net::TcpStream, response: &HttpResponse) -> IoResult<()> {
    use std::io::Write;
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        304 => "Not Modified",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    write!(stream, "HTTP/1.1 {} {}\r\n", response.status, reason)?;
    write!(
        stream,
        "Content-Type: {}\r\nContent-Length: {}\r\n",
        response.content_type,
        response.body.len()
    )?;
    if let Some(session) = &response.set_session {
        write!(stream, "mcp-session-id: {session}\r\n")?;
    }
    write!(stream, "Connection: close\r\n\r\n")?;
    if !response.body.is_empty() {
        stream.write_all(response.body.as_bytes())?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        ConnectionAuthMethod, ConnectionEnvironment, ConnectionSpec, TerminalSessionKind,
        TerminalSessionStatus, TerminalSessionView,
    };
    use serde_json::json;

    #[derive(Default)]
    struct FakeRuntime {
        connections: Vec<ConnectionSpec>,
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
            _session_id: &str,
            _from_offset: Option<u64>,
        ) -> Result<crate::app::ConnectionOutputChunk> {
            Ok(crate::app::ConnectionOutputChunk {
                text: String::new(),
                eof: true,
                status: None,
                error_code: None,
            })
        }
        fn close_connection(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn request(method: &str, path: &str, body: &str) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers: Vec::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn initialize_returns_json_and_sets_mcp_session_id() {
        let mut rt = FakeRuntime::default();
        let resp = respond(
            &mut rt,
            &request(
                "POST",
                "/mcp",
                &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }).to_string(),
            ),
        );
        assert_eq!(resp.status, 200);
        assert_eq!(resp.content_type, "application/json");
        assert!(
            resp.set_session
                .as_deref()
                .unwrap_or("")
                .starts_with("connection-mcp-")
        );
        assert!(resp.body.contains("protocolVersion"));
        assert!(resp.body.contains("gold-band-connections"));
    }

    #[test]
    fn notification_is_accepted_without_body() {
        let mut rt = FakeRuntime::default();
        let resp = respond(
            &mut rt,
            &request(
                "POST",
                "/mcp",
                &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
            ),
        );
        assert_eq!(resp.status, 202);
        assert!(resp.body.is_empty());
        assert!(resp.set_session.is_none());
    }

    #[test]
    fn tools_list_returns_five_connection_tools() {
        let mut rt = FakeRuntime::default();
        let resp = respond(
            &mut rt,
            &request(
                "POST",
                "/mcp",
                &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }).to_string(),
            ),
        );
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("\"list_connections\""));
        assert!(resp.body.contains("\"close_connection\""));
    }

    #[test]
    fn tools_call_list_connections_returns_specs_json() {
        let mut rt = FakeRuntime::default();
        rt.connections = vec![ConnectionSpec {
            id: "conn-1".to_string(),
            name: "wb".to_string(),
            host: "10.0.0.5".to_string(),
            port: 22,
            username: "ops".to_string(),
            auth_method: ConnectionAuthMethod::PublicKey,
            env: ConnectionEnvironment::Prod,
            jump_ref: None,
        }];
        let resp = respond(&mut rt, &request(
            "POST",
            "/mcp",
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "list_connections", "arguments": {} } }).to_string(),
        ));
        assert_eq!(resp.status, 200);
        let body: Value = serde_json::from_str(&resp.body).unwrap();
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        let specs: Vec<ConnectionSpec> = serde_json::from_str(text).unwrap();
        assert_eq!(specs[0].name, "wb");
        assert_eq!(specs[0].host, "10.0.0.5");
    }

    #[test]
    fn unknown_tool_returns_jsonrpc_error_in_json() {
        let mut rt = FakeRuntime::default();
        let resp = respond(&mut rt, &request(
            "POST",
            "/mcp",
            &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "deploy", "arguments": {} } }).to_string(),
        ));
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("\"tool.not-found\""));
    }

    #[test]
    fn wrong_path_returns_404() {
        let mut rt = FakeRuntime::default();
        let resp = respond(&mut rt, &request("POST", "/other", "{}"));
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn invalid_json_body_returns_400() {
        let mut rt = FakeRuntime::default();
        let resp = respond(&mut rt, &request("POST", "/mcp", "not-json"));
        assert_eq!(resp.status, 400);
    }

    #[test]
    fn delete_session_returns_200() {
        let mut rt = FakeRuntime::default();
        let resp = respond(&mut rt, &request("DELETE", "/mcp", ""));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn parse_http_request_reads_method_path_and_body() {
        let bytes =
            b"POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        let mut reader = std::io::BufReader::new(&bytes[..]);
        let req = parse_http_request(&mut reader).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/mcp");
        assert_eq!(req.body, "{}");
    }

    #[test]
    fn parse_http_request_preserves_query_string_in_path() {
        let bytes =
            b"POST /mcp?taskId=task-001&runId=run-001 HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
        let mut reader = std::io::BufReader::new(&bytes[..]);
        let req = parse_http_request(&mut reader).unwrap();
        assert_eq!(req.path, "/mcp?taskId=task-001&runId=run-001");
    }

    #[test]
    fn query_param_extracts_task_and_run_ids() {
        let req = request("POST", "/mcp?taskId=task-001&runId=run-002", "{}");
        assert_eq!(query_param(&req, "taskId"), Some("task-001"));
        assert_eq!(query_param(&req, "runId"), Some("run-002"));
        assert_eq!(query_param(&req, "missing"), None);
    }

    #[test]
    fn request_path_without_query_strips_query_string() {
        let req = request("POST", "/mcp?taskId=task-001", "{}");
        assert_eq!(request_path_without_query(&req), "/mcp");
    }

    #[test]
    fn connection_mcp_url_embeds_port_project_task_and_run() {
        let url = connection_mcp_url(9000, "proj-1", "task-001", "run-002");
        assert_eq!(
            url,
            "http://127.0.0.1:9000/mcp?projectId=proj-1&taskId=task-001&runId=run-002"
        );
    }

    /// Isolates the given test's user-level config dir (the default `App`
    /// otherwise resolves against the real user's `~/.gold-band`, which a test
    /// must never touch even for reads).
    fn isolated_factory(
        home_env_var: &'static str,
    ) -> (ConnectionMcpRuntimeFactory, tempfile::TempDir) {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let repo_root =
            camino::Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        // SAFETY: caller passes a name unique to its test.
        unsafe {
            std::env::set_var(home_env_var, temp.path().join("home"));
        }
        let path_config = crate::storage::StoragePathConfig {
            app_key: "gold-band",
            config_dir_name: ".gold-band",
            home_env_var,
        };
        let app = crate::app::App::with_config_and_path_config(
            repo_root,
            crate::config::RuntimeConfig::default(),
            path_config,
        );
        let manager = Arc::new(std::sync::Mutex::new(
            crate::app::TerminalSessionManager::new(
                Arc::new(|_| {}),
                temp.path().join("known_hosts"),
            ),
        ));
        let history = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let resolver_paths = app.paths.clone();
        (
            ConnectionMcpRuntimeFactory::new(
                app,
                manager,
                history,
                Arc::new(move |_| Ok(resolver_paths.clone())),
            ),
            temp,
        )
    }

    #[test]
    fn respond_scoped_returns_400_when_task_or_run_query_param_is_missing() {
        let (factory, _temp) =
            isolated_factory("GOLD_BAND_TEST_HOME_CONNECTION_MCP_HTTP_MISSING_QUERY");
        let resp = respond_scoped(&factory, &request("POST", "/mcp", "{}"));
        assert_eq!(resp.status, 400);
        assert!(resp.body.contains("projectId"));

        let resp = respond_scoped(&factory, &request("POST", "/mcp?projectId=proj-1", "{}"));
        assert_eq!(resp.status, 400);
        assert!(resp.body.contains("taskId"));

        let resp = respond_scoped(
            &factory,
            &request("POST", "/mcp?projectId=proj-1&taskId=task-001", "{}"),
        );
        assert_eq!(resp.status, 400);
        assert!(resp.body.contains("runId"));
    }

    #[test]
    fn respond_scoped_returns_400_for_a_run_that_does_not_exist() {
        let (factory, _temp) =
            isolated_factory("GOLD_BAND_TEST_HOME_CONNECTION_MCP_HTTP_MISSING_RUN");
        let resp = respond_scoped(
            &factory,
            &request(
                "POST",
                "/mcp?projectId=proj-1&taskId=task-none&runId=run-none",
                "{}",
            ),
        );
        assert_eq!(resp.status, 400);
    }

    // ── socket-level integration: real TCP + real HTTP client ──
    //
    // Drives `serve()` over an actual loopback socket with `reqwest::blocking`
    // (the same HTTP client stack the project's own MCP client uses in
    // `mcp/mod.rs`). This is the closest verification short of a real ACP
    // adapter: it proves the wire format (status, Content-Type,
    // mcp-session-id, JSON-RPC body) is well-formed HTTP, not just that our
    // in-process `respond()` function returns the right struct.

    #[test]
    fn serve_handles_initialize_tools_list_and_tools_call_over_real_tcp() {
        use crate::app::{
            App, ConnectionAuthMethod as AppConnectionAuthMethod,
            ConnectionEnvironment as AppConnectionEnvironment, ConnectionSpec as AppConnectionSpec,
            TerminalSessionManager,
        };

        let temp = tempfile::TempDir::new().expect("tempdir");
        let repo_root =
            camino::Utf8PathBuf::from_path_buf(temp.path().join("repo")).expect("utf8 path");
        // Isolate this test's user-level config dir (the app default is the
        // real user's ~/.gold-band, which must never be touched by a test).
        let home_env_var: &'static str = "GOLD_BAND_TEST_HOME_CONNECTION_MCP_HTTP_SERVE";
        // SAFETY: this test runs alone against this specific env var name.
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
            connection_specs: vec![AppConnectionSpec {
                id: "conn-1".to_string(),
                name: "wb".to_string(),
                host: "10.0.0.5".to_string(),
                port: 22,
                username: "ops".to_string(),
                auth_method: AppConnectionAuthMethod::PublicKey,
                env: AppConnectionEnvironment::Prod,
                jump_ref: None,
            }],
        };
        crate::storage::write_json(&app.paths.run_file("task-001", "run-001"), &run).unwrap();

        let manager = Arc::new(std::sync::Mutex::new(TerminalSessionManager::new(
            Arc::new(|_| {}),
            temp.path().join("known_hosts"),
        )));
        let history = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let resolver_paths = app.paths.clone();
        let factory = Arc::new(ConnectionMcpRuntimeFactory::new(
            app,
            manager,
            history,
            Arc::new(move |_| Ok(resolver_paths.clone())),
        ));

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_for_server = Arc::clone(&stop);
        let server = std::thread::spawn(move || serve(listener, factory, stop_for_server));

        // `.no_proxy()`: macOS system proxy settings (not just env vars) can
        // otherwise route a loopback request through a corporate proxy that
        // has no route to 127.0.0.1 and answers 503.
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("build http client");
        let url = connection_mcp_url(port, "proj-1", "task-001", "run-001");

        let init_resp = client
            .post(&url)
            .header(
                "MCP-Protocol-Version",
                crate::app::CONNECTION_MCP_PROTOCOL_VERSION,
            )
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }))
            .send()
            .expect("initialize request");
        assert_eq!(init_resp.status(), 200);
        assert_eq!(
            init_resp.headers().get("content-type").unwrap(),
            "application/json"
        );
        let session_id = init_resp
            .headers()
            .get("mcp-session-id")
            .expect("mcp-session-id header")
            .to_str()
            .unwrap()
            .to_string();
        let init_body: Value = init_resp.json().expect("initialize json body");
        assert_eq!(
            init_body["result"]["serverInfo"]["name"],
            "gold-band-connections"
        );

        let list_resp = client
            .post(&url)
            .header("mcp-session-id", &session_id)
            .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .send()
            .expect("tools/list request");
        assert_eq!(list_resp.status(), 200);
        let list_body: Value = list_resp.json().expect("tools/list json body");
        let names = list_body["result"]["tools"]
            .as_array()
            .unwrap()
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

        let call_resp = client
            .post(&url)
            .header("mcp-session-id", &session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "list_connections", "arguments": {} },
            }))
            .send()
            .expect("tools/call request");
        assert_eq!(call_resp.status(), 200);
        let call_body: Value = call_resp.json().expect("tools/call json body");
        let text = call_body["result"]["content"][0]["text"].as_str().unwrap();
        let specs: Vec<ConnectionSpec> = serde_json::from_str(text).unwrap();
        assert_eq!(specs[0].name, "wb");

        let notify_resp = client
            .post(&url)
            .json(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .send()
            .expect("notification request");
        assert_eq!(notify_resp.status(), 202);
        assert!(notify_resp.bytes().unwrap().is_empty());

        stop.store(true, Ordering::Relaxed);
        // Nudge the accept loop past its blocking poll so the thread joins promptly.
        let _ = std::net::TcpStream::connect(format!("127.0.0.1:{port}"));
        server.join().expect("server thread joins");
    }
}
