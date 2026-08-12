// ── MCP Manager ──
// 对标 Zed crates/project/src/context_server_store.rs（精简版）
//
// 职责：
//   1. MCP 服务器配置持久化（settings.json ↔ McpServerConfig[]）
//   2. 添加/保存时的 MCP 协议握手验证（对标 Zed server.start()）
//   3. enabled 开关管理（对标 Zed maintain_servers 的 partition 逻辑）
//
// 不做：
//   - 长期进程管理（Agent 通过 ACP mcpServers 自行管理）
//   - SettingsStore 变更监听（Gold-Band 用 Tauri commands 手动触发）

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{
    McpServerConfig, McpServerHealthResult, McpServerState, McpTransportConfig, OAuthClientConfig,
    SettingsConfig,
};
use crate::process::{ManagedProcessGroup, PROCESS_GROUP_TERMINATION_GRACE, background_command};
use crate::storage::write_json;

/// MCP 协议版本（现代规范统一用日期字符串；stdio / http / sse 三传输共用，
/// 消除协议版本格式割裂）。
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// streamable HTTP 请求头：声明客户端使用的 MCP 协议版本。
const MCP_VERSION_HEADER: &str = "MCP-Protocol-Version";
/// streamable HTTP 会话标识响应头：initialize 返回，后续请求回带。
const MCP_SESSION_HEADER: &str = "mcp-session-id";
/// streamable HTTP Accept 头：服务端可返回 application/json 或 text/event-stream。
const ACCEPT_STREAMABLE: &str = "application/json, text/event-stream";

/// 对标 Zed ContextServerStore — MCP 服务器的中枢管理器
pub struct McpManager {
    settings_path: Utf8PathBuf,
    /// 对标 Zed ContextServerState 状态机 — 缓存每个服务器的运行时状态
    state_cache: RefCell<HashMap<String, McpServerState>>,
}

/// 对标 Zed ServerStatusChangedEvent
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerWithStatus {
    #[serde(flatten)]
    pub config: McpServerConfig,
    pub health_status: Option<String>,
    pub health_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct McpJsonEntry {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    url: Option<String>,
    #[serde(rename = "type", default)]
    transport_type: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<OAuthClientConfig>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "helpMessage", default)]
    help_message: Option<String>,
}

impl McpManager {
    pub fn new(settings_path: Utf8PathBuf) -> Self {
        Self {
            settings_path,
            state_cache: RefCell::new(HashMap::new()),
        }
    }

    // ── 对标 Zed ContextServerStore::configured_server_ids ──

    pub fn list(&self) -> Result<Vec<McpServerWithStatus>> {
        let settings = self.load_settings()?;
        let cache = self.state_cache.borrow();
        Ok(settings
            .context_servers
            .unwrap_or_default()
            .into_iter()
            .map(|config| {
                let (health_status, health_message) = match cache.get(&config.id) {
                    Some(McpServerState::Running { .. }) => (Some("healthy".to_string()), None),
                    Some(McpServerState::Error { message }) => {
                        (Some("unhealthy".to_string()), Some(message.clone()))
                    }
                    Some(McpServerState::AuthRequired { auth_url }) => {
                        (Some("auth_required".to_string()), auth_url.clone())
                    }
                    Some(McpServerState::Stopped) => (Some("stopped".to_string()), None),
                    Some(McpServerState::Starting) => (Some("checking".to_string()), None),
                    None => (None, None),
                };
                McpServerWithStatus {
                    config,
                    health_status,
                    health_message,
                }
            })
            .collect())
    }

    pub fn enabled_servers(&self) -> Result<Vec<McpServerConfig>> {
        let settings = self.load_settings()?;
        Ok(settings
            .context_servers
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.enabled)
            .collect())
    }

    // ── 对标 Zed update_settings_file + maintain_servers ──

    /// 对标 Zed confirm() 中的完整流程：
    ///   1. parse JSON
    ///   2. write settings.json
    ///   3. MCP 协议握手验证（对标 run_server + wait_for_context_server）
    pub fn add(
        &self,
        json_content: &str,
    ) -> Result<(McpServerWithStatus, Vec<McpServerWithStatus>)> {
        let (id, transport, display_name, help_message) = parse_mcp_json(json_content)?;
        let config = McpServerConfig {
            name: display_name.unwrap_or_else(|| id.clone()),
            id,
            enabled: true,
            transport,
            managed: false,
            help_message,
        };
        let mut settings = self.load_settings()?;
        let mut servers = settings.context_servers.unwrap_or_default();
        servers.retain(|s| s.id != config.id);
        servers.push(config.clone());
        settings.context_servers = Some(servers);
        self.save_settings(&settings)?;

        let status = self.verify_server(&config);
        let list = self.list()?;
        Ok((
            McpServerWithStatus {
                config,
                health_status: status.as_ref().ok().map(|_| "healthy".into()),
                health_message: status.as_ref().ok().and_then(|r| r.message.clone()),
            },
            list,
        ))
    }

    /// 对标 add()，但标记为 managed（托管），用户不可删除
    pub fn add_managed(
        &self,
        json_content: &str,
        default_enabled: bool,
    ) -> Result<(McpServerWithStatus, Vec<McpServerWithStatus>)> {
        let (id, transport, display_name, help_message) = parse_mcp_json(json_content)?;
        let mut settings = self.load_settings()?;
        let mut servers = settings.context_servers.unwrap_or_default();
        let enabled = servers
            .iter()
            .find(|s| s.id == id && s.managed)
            .map(|s| s.enabled)
            .unwrap_or(default_enabled);
        let config = McpServerConfig {
            name: display_name.unwrap_or_else(|| id.clone()),
            id,
            enabled,
            transport,
            managed: true,
            help_message,
        };
        servers.retain(|s| s.id != config.id);
        servers.push(config.clone());
        settings.context_servers = Some(servers);
        self.save_settings(&settings)?;

        let status = self.verify_server(&config);
        let list = self.list()?;
        Ok((
            McpServerWithStatus {
                config,
                health_status: status.as_ref().ok().map(|_| "healthy".into()),
                health_message: status.as_ref().ok().and_then(|r| r.message.clone()),
            },
            list,
        ))
    }

    pub fn update(
        &self,
        id: &str,
        json_content: &str,
    ) -> Result<(McpServerWithStatus, Vec<McpServerWithStatus>)> {
        let settings = self.load_settings()?;
        if let Some(s) = settings
            .context_servers
            .as_ref()
            .and_then(|servers| servers.iter().find(|s| s.id == id))
        {
            anyhow::ensure!(
                !s.managed,
                "MCP server `{id}` is managed and cannot be modified"
            );
        }
        let (new_id, transport, display_name, help_message) = parse_mcp_json(json_content)?;
        let config = McpServerConfig {
            name: display_name.unwrap_or_else(|| new_id.clone()),
            id: new_id,
            enabled: true,
            transport,
            managed: false,
            help_message,
        };
        let mut settings = self.load_settings()?;
        let mut servers = settings.context_servers.unwrap_or_default();
        servers.retain(|s| s.id != id && s.id != config.id);
        servers.push(config.clone());
        settings.context_servers = Some(servers);
        self.save_settings(&settings)?;

        let status = self.verify_server(&config);
        let list = self.list()?;
        Ok((
            McpServerWithStatus {
                config,
                health_status: status.as_ref().ok().map(|_| "healthy".into()),
                health_message: status.as_ref().ok().and_then(|r| r.message.clone()),
            },
            list,
        ))
    }

    pub fn delete(&self, id: &str) -> Result<Vec<McpServerWithStatus>> {
        let mut settings = self.load_settings()?;
        // Check managed before mutation — borrow then move
        if let Some(s) = settings
            .context_servers
            .as_ref()
            .and_then(|servers| servers.iter().find(|s| s.id == id))
        {
            anyhow::ensure!(
                !s.managed,
                "MCP server `{id}` is managed and cannot be deleted"
            );
        }
        let mut servers = settings.context_servers.unwrap_or_default();
        servers.retain(|s| s.id != id);
        settings.context_servers = Some(servers);
        self.save_settings(&settings)?;
        self.list()
    }

    pub fn toggle(&self, id: &str, enabled: bool) -> Result<Vec<McpServerWithStatus>> {
        let mut settings = self.load_settings()?;
        let mut servers = settings.context_servers.unwrap_or_default();
        if let Some(s) = servers.iter_mut().find(|s| s.id == id) {
            s.enabled = enabled;
        }
        settings.context_servers = Some(servers);
        self.save_settings(&settings)?;
        self.list()
    }

    // ── 对标 Zed run_server + wait_for_context_server ──

    pub fn check_health(&self, id: &str) -> Result<McpServerHealthResult> {
        let settings = self.load_settings()?;
        let config = settings
            .context_servers
            .as_ref()
            .and_then(|servers| servers.iter().find(|s| s.id == id))
            .with_context(|| format!("MCP server `{id}` not found"))?;
        let result = self.verify_server(config)?;
        // 更新状态缓存
        let mut cache = self.state_cache.borrow_mut();
        let new_state = if result.status == "healthy" {
            McpServerState::Running {
                tools: result.tools.clone(),
            }
        } else if result.status == "auth_required" {
            McpServerState::AuthRequired {
                auth_url: result.auth_url.clone(),
            }
        } else {
            McpServerState::Error {
                message: result
                    .message
                    .clone()
                    .unwrap_or_else(|| "unknown error".into()),
            }
        };
        cache.insert(id.to_string(), new_state);
        Ok(result)
    }

    /// 手动刷新指定服务器的健康状态（对标 Zed wait_for_context_server）
    pub fn refresh_health(&self, id: &str) -> Result<McpServerHealthResult> {
        self.check_health(id)
    }

    /// 清除指定服务器的缓存状态（对标 Zed 的 invalidate）
    pub fn invalidate_health(&self, id: &str) {
        self.state_cache.borrow_mut().remove(id);
    }

    /// 拉取 MCP 服务器的工具列表（tools/list）
    pub fn list_tools(&self, id: &str) -> Result<Vec<crate::config::ToolInfo>> {
        let settings = self.load_settings()?;
        let config = settings
            .context_servers
            .as_ref()
            .and_then(|servers| servers.iter().find(|s| s.id == id))
            .with_context(|| format!("MCP server `{id}` not found"))?;
        match &config.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                fetch_stdio_tools(command, args, env)
            }
            McpTransportConfig::Http { url, headers, .. } => fetch_http_tools(url, headers),
            McpTransportConfig::Sse { url, headers } => fetch_sse_tools(url, headers),
        }
    }

    /// 对标 Zed server.start() — Stdio 发送 MCP initialize 请求; HTTP/SSE 实际请求
    fn verify_server(&self, config: &McpServerConfig) -> Result<McpServerHealthResult> {
        match &config.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                verify_stdio_server(command, args, env)
            }
            McpTransportConfig::Http {
                url,
                headers,
                oauth,
            } => verify_http_server(url, headers, oauth),
            McpTransportConfig::Sse { url, headers } => verify_sse_server(url, headers),
        }
    }

    // ── System Prompt ──

    // ── ACP 序列化 ──

    /// 解析当前启用的 MCP 配置，供 ACP session/new 或 session/load 使用。
    ///
    /// 会话启动路径只负责传递用户配置，不执行网络请求或子进程探活；健康状态由
    /// `check_health` 等诊断入口独立维护，避免诊断生命周期阻塞会话生命周期。
    pub fn configured_acp_mcp_servers(&self) -> Result<Vec<Value>> {
        Ok(self
            .enabled_servers()?
            .into_iter()
            .map(|server| mcp_server_to_acp_json(&server))
            .collect())
    }

    // ── private ──

    fn load_settings(&self) -> Result<SettingsConfig> {
        crate::storage::load_settings_file(&self.settings_path)
    }

    fn save_settings(&self, settings: &SettingsConfig) -> Result<()> {
        write_json(&self.settings_path, settings)
    }
}

fn name_value_entries(entries: &BTreeMap<String, String>) -> Vec<Value> {
    entries
        .iter()
        .map(|(name, value)| {
            serde_json::json!({
                "name": name,
                "value": value,
            })
        })
        .collect()
}

fn mcp_server_to_acp_json(server: &McpServerConfig) -> Value {
    match &server.transport {
        McpTransportConfig::Stdio { command, args, env } => {
            serde_json::json!({
                "name": server.name,
                "command": command,
                "args": args,
                "env": name_value_entries(env),
            })
        }
        McpTransportConfig::Http { url, headers, .. } => {
            serde_json::json!({
                "type": "http",
                "name": server.name,
                "url": url,
                "headers": name_value_entries(headers),
            })
        }
        McpTransportConfig::Sse { url, headers } => {
            serde_json::json!({
                "type": "sse",
                "name": server.name,
                "url": url,
                "headers": name_value_entries(headers),
            })
        }
    }
}

// ── MCP Protocol Handshake（对标 Zed server.start()） ──

/// 构建标准 MCP initialize 请求（Stdio 和 HTTP 共用）
fn build_initialize_request() -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "gold-band",
                "version": crate::domain::VERSION,
            }
        }
    })
}

/// 解析 MCP initialize 响应，返回健康检查结果
fn parse_initialize_response(response_text: &str) -> Result<McpServerHealthResult> {
    let response: Value =
        serde_json::from_str(response_text.trim()).context("invalid JSON response from server")?;

    if let Some(err) = response.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("server returned error: {msg}")
    }

    let result = response
        .get("result")
        .context("unexpected response format: missing 'result' field")?;

    let version = result
        .get("protocolVersion")
        .map(|v| {
            v.as_str()
                .map(String::from)
                .unwrap_or_else(|| v.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    Ok(McpServerHealthResult {
        status: "healthy".into(),
        message: Some(format!("MCP handshake successful (protocol v{version})")),
        auth_url: None,
        needs_client_secret: None,
        tools: Vec::new(),
    })
}

fn jsonrpc_ids_match(expected: &Value, actual: &Value) -> bool {
    if expected == actual {
        return true;
    }
    let expected = expected
        .as_u64()
        .or_else(|| expected.as_str()?.parse::<u64>().ok());
    let actual = actual
        .as_u64()
        .or_else(|| actual.as_str()?.parse::<u64>().ok());
    expected.is_some() && expected == actual
}

fn is_jsonrpc_response_for(value: &Value, expected_id: &Value) -> bool {
    value.get("method").is_none()
        && (value.get("result").is_some() || value.get("error").is_some())
        && value
            .get("id")
            .is_some_and(|actual_id| jsonrpc_ids_match(expected_id, actual_id))
}

/// 按 SSE framing 增量读取事件，忽略请求/通知及其他响应，直到收到目标 JSON-RPC id。
/// 多个 `data:` 行按 SSE 标准使用换行拼接；不会等待整个 HTTP body EOF。
fn read_sse_jsonrpc_response(
    reader: impl BufRead,
    expected_id: &Value,
    label: &str,
) -> Result<String> {
    let mut data_lines = Vec::new();
    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read {label} event stream"))?;
        let line = line.strip_suffix('\r').unwrap_or(&line);
        if line.is_empty() {
            if data_lines.is_empty() {
                continue;
            }
            let payload = data_lines.join("\n");
            data_lines.clear();
            let value: Value = serde_json::from_str(&payload)
                .with_context(|| format!("invalid JSON-RPC event in {label} stream"))?;
            if is_jsonrpc_response_for(&value, expected_id) {
                return Ok(payload);
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            data_lines.push(value.strip_prefix(' ').unwrap_or(value).to_string());
        }
    }
    bail!("{label} event stream closed before JSON-RPC response id {expected_id}")
}

fn verify_stdio_server(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<McpServerHealthResult> {
    let mut cmd = background_command(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = ManagedProcessGroup::spawn(&mut cmd)
        .with_context(|| format!("failed to start command: {command}"))?;

    let mut stdin = child.take_stdin().context("failed to capture stdin")?;
    let stdout = child.take_stdout().context("failed to capture stdout")?;

    // 对标 Zed: 发送 MCP initialize 请求
    let request_line = serde_json::to_string(&build_initialize_request())? + "\n";
    stdin
        .write_all(request_line.as_bytes())
        .context("failed to send initialize request")?;
    stdin.flush().context("failed to flush stdin")?;
    drop(stdin);

    // 对标 Zed: 读取响应（带 10s 超时保护 + 多行处理）
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(text) => {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        let _ = tx.send(Ok(trimmed));
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
        let _ = tx.send(Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "server closed stdout without responding",
        )));
    });
    let response_line = rx
        .recv_timeout(Duration::from_secs(10))
        .context("health check timed out")?
        .context("failed to read server response")?;

    let _ = child.terminate(PROCESS_GROUP_TERMINATION_GRACE);

    parse_initialize_response(&response_line)
}

/// streamable HTTP 单次请求的返回。
enum StreamableOutcome {
    /// 与请求 id 匹配的 JSON-RPC 响应文本。
    Jsonrpc(String),
    /// 服务端返回 401，已通过 OAuth discovery 解析出授权信息。
    AuthRequired(McpServerHealthResult),
    /// 服务端已终止当前 session；调用方必须重新 initialize 后重试。
    SessionExpired,
}

enum StreamableNotificationOutcome {
    Accepted,
    SessionExpired,
}

/// MCP Streamable HTTP 会话客户端。
/// 封装单端点 POST、`MCP-Protocol-Version` 头、`mcp-session-id` 会话流转，
/// 以及 application/json / text/event-stream 双形态响应解析。
struct StreamableHttpClient {
    client: reqwest::blocking::Client,
    url: String,
    headers: BTreeMap<String, String>,
    oauth: Option<OAuthClientConfig>,
    session_id: Option<String>,
    protocol_version: String,
}

impl StreamableHttpClient {
    fn new(
        url: &str,
        headers: &BTreeMap<String, String>,
        oauth: &Option<OAuthClientConfig>,
    ) -> Result<Self> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("invalid URL: must start with http:// or https://");
        }
        let client = reqwest::blocking::Client::builder()
            // POST 在 301/302 下可能被降级为 GET；要求配置最终 MCP endpoint，避免方法语义漂移。
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .context("failed to create HTTP client")?;
        Ok(Self {
            client,
            url: url.to_string(),
            headers: headers.clone(),
            oauth: oauth.clone(),
            session_id: None,
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        })
    }

    fn reset_session(&mut self) {
        self.session_id = None;
        self.protocol_version = MCP_PROTOCOL_VERSION.to_string();
    }

    fn adopt_negotiated_protocol_version(&mut self, initialize_response: &str) -> Result<()> {
        let response: Value = serde_json::from_str(initialize_response)
            .context("invalid JSON response from server")?;
        let version = response
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .context("initialize response missing protocolVersion")?;
        self.protocol_version = version.to_string();
        Ok(())
    }

    fn apply_headers(
        &self,
        mut req: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        req = req.header(MCP_VERSION_HEADER, self.protocol_version.as_str());
        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }
        if let Some(session_id) = &self.session_id {
            req = req.header(MCP_SESSION_HEADER, session_id);
        }
        req
    }

    fn http_error(resp: reqwest::blocking::Response, label: &str) -> anyhow::Error {
        let status = resp.status();
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body = resp.text().unwrap_or_default();
        body.truncate(body.floor_char_boundary(512));
        if status.is_redirection() {
            return anyhow::anyhow!(
                "{label} endpoint redirected with {status} to {}; configure the final MCP endpoint URL",
                location.as_deref().unwrap_or("an unknown location")
            );
        }
        if body.trim().is_empty() {
            anyhow::anyhow!("{label} returned HTTP {status}")
        } else {
            anyhow::anyhow!("{label} returned HTTP {status}: {}", body.trim())
        }
    }

    /// 发送 JSON-RPC request。SSE 响应按 event 增量读取并等待匹配 request id。
    fn send(&mut self, request: &Value) -> Result<StreamableOutcome> {
        let expected_id = request
            .get("id")
            .cloned()
            .context("streamable HTTP request missing JSON-RPC id")?;
        let label = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("MCP request");
        let body = serde_json::to_string(request).context("failed to serialize request")?;
        let req = self.apply_headers(
            self.client
                .post(&self.url)
                .header("Content-Type", "application/json")
                .header("Accept", ACCEPT_STREAMABLE)
                .body(body),
        );
        let had_session = self.session_id.is_some();

        let resp = match req.send() {
            Ok(resp) => resp,
            Err(e) if e.is_connect() => bail!("cannot connect to server: {e}"),
            Err(e) if e.is_timeout() => bail!("connection timed out"),
            Err(e) => bail!("HTTP request failed: {e}"),
        };

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let has_static_auth = self
                .headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("authorization"));
            if has_static_auth {
                bail!("server returned 401 — check your Authorization header");
            }
            return Ok(StreamableOutcome::AuthRequired(try_oauth_discovery(
                &self.url,
                &self.oauth,
            )?));
        }
        if resp.status() == reqwest::StatusCode::NOT_FOUND && had_session {
            self.reset_session();
            return Ok(StreamableOutcome::SessionExpired);
        }
        if !resp.status().is_success() {
            return Err(Self::http_error(resp, label));
        }

        // initialize 成功响应缓存 session-id，后续请求自动回带。
        if self.session_id.is_none() {
            if let Some(sid) = resp
                .headers()
                .get(MCP_SESSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
            {
                self.session_id = Some(sid);
            }
        }

        let content_type = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let payload = if content_type.contains("text/event-stream") {
            read_sse_jsonrpc_response(BufReader::new(resp), &expected_id, label)?
        } else if content_type.contains("application/json") {
            let body = resp.text().context("failed to read response body")?;
            let payload = body.trim().to_string();
            let value: Value = serde_json::from_str(&payload)
                .with_context(|| format!("invalid JSON-RPC response for {label}"))?;
            if !is_jsonrpc_response_for(&value, &expected_id) {
                bail!("{label} returned a JSON-RPC message with an unexpected id or shape");
            }
            payload
        } else {
            bail!("{label} returned unsupported Content-Type `{content_type}`");
        };
        Ok(StreamableOutcome::Jsonrpc(payload))
    }

    /// 发送一个 JSON-RPC 通知（无 id、无响应）。
    /// 用于 MCP 规范要求的 initialize 后 `notifications/initialized`。
    fn notify(&mut self, method: &str) -> Result<StreamableNotificationOutcome> {
        let body = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        let req = self.apply_headers(
            self.client
                .post(&self.url)
                .header("Content-Type", "application/json")
                .header("Accept", ACCEPT_STREAMABLE)
                .body(serde_json::to_string(&body)?),
        );
        let had_session = self.session_id.is_some();
        let resp = req.send()?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND && had_session {
            self.reset_session();
            return Ok(StreamableNotificationOutcome::SessionExpired);
        }
        if !status.is_success() {
            return Err(Self::http_error(resp, method));
        }
        Ok(StreamableNotificationOutcome::Accepted)
    }

    /// 显式释放短生命周期健康检查/工具发现创建的服务端 session。
    fn terminate(&mut self) -> Result<()> {
        let Some(session_id) = self.session_id.take() else {
            return Ok(());
        };
        let mut req = self
            .client
            .delete(&self.url)
            .header(MCP_VERSION_HEADER, self.protocol_version.as_str())
            .header(MCP_SESSION_HEADER, session_id);
        for (key, value) in &self.headers {
            req = req.header(key.as_str(), value.as_str());
        }
        let resp = req.send()?;
        if resp.status().is_success()
            || matches!(
                resp.status(),
                reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
            )
        {
            return Ok(());
        }
        Err(Self::http_error(resp, "session termination"))
    }
}

fn finish_streamable_session<T>(client: &mut StreamableHttpClient, result: Result<T>) -> Result<T> {
    if let Err(error) = client.terminate() {
        tracing::debug!(%error, "failed to terminate short-lived MCP session");
    }
    result
}

fn verify_http_server(
    url: &str,
    headers: &BTreeMap<String, String>,
    oauth: &Option<OAuthClientConfig>,
) -> Result<McpServerHealthResult> {
    let mut client = StreamableHttpClient::new(url, headers, oauth)?;
    let result = (|| match client.send(&build_initialize_request())? {
        StreamableOutcome::AuthRequired(result) => Ok(result),
        StreamableOutcome::Jsonrpc(payload) => {
            let health = parse_initialize_response(&payload)?;
            client.adopt_negotiated_protocol_version(&payload)?;
            Ok(health)
        }
        StreamableOutcome::SessionExpired => {
            bail!("initialize unexpectedly reported an expired MCP session")
        }
    })();
    finish_streamable_session(&mut client, result)
}

/// 一次旧式 SSE 会话：GET 端点拿到 message endpoint，并启动后台线程持续读取事件流。
struct SseSession {
    client: reqwest::blocking::Client,
    endpoint: String,
    events: mpsc::Receiver<String>,
}

/// 建立 SSE 会话：GET `url` → 解析 `event: endpoint` → 启动事件流读取线程。
fn open_sse_session(url: &str, headers: &BTreeMap<String, String>) -> Result<SseSession> {
    let base_url = url::Url::parse(url).context("invalid SSE URL")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create HTTP client")?;

    let mut req = client.get(url).header("Accept", "text/event-stream");
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let mut resp = req.send().context("failed to connect to SSE endpoint")?;
    if !resp.status().is_success() {
        bail!("SSE endpoint returned {}", resp.status());
    }

    use std::io::Read;
    let mut buf = [0u8; 8192];
    let n = resp
        .read(&mut buf)
        .context("failed to read SSE handshake")?;
    let sse_body = String::from_utf8_lossy(&buf[..n]);
    let endpoint_url = discover_sse_endpoint(&sse_body, &base_url)
        .context("SSE handshake did not contain an endpoint event")?;

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut leftover = String::new();
        let mut buf = [0u8; 4096];
        loop {
            match resp.read(&mut buf) {
                Ok(n) if n > 0 => {
                    leftover.push_str(&String::from_utf8_lossy(&buf[..n]));
                    while let Some(nl) = leftover.find('\n') {
                        let line = leftover[..nl].trim().to_string();
                        leftover = leftover[nl + 1..].to_string();
                        if let Some(data) = line.strip_prefix("data:") {
                            let payload = data.trim().to_string();
                            if !payload.is_empty() && tx.send(payload).is_err() {
                                return;
                            }
                        }
                    }
                }
                _ => return,
            }
        }
    });

    Ok(SseSession {
        client,
        endpoint: endpoint_url,
        events: rx,
    })
}

fn verify_sse_server(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<McpServerHealthResult> {
    let session = open_sse_session(url, headers)?;
    post_sse_json(
        &session.client,
        &session.endpoint,
        headers,
        &build_initialize_request(),
    )?;
    let init = session
        .events
        .recv_timeout(Duration::from_secs(10))
        .context("no initialize response from SSE stream")?;
    parse_initialize_response(&init)
}

// ── MCP tools/list ──

fn build_tools_list_request() -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    })
}

fn jsonrpc_response_id(response_text: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(response_text.trim()).ok()?;
    value
        .get("id")
        .and_then(|id| id.as_u64().or_else(|| id.as_str()?.parse::<u64>().ok()))
}

fn recv_jsonrpc_response(
    rx: &mpsc::Receiver<std::io::Result<String>>,
    expected_id: u64,
    timeout: Duration,
    label: &str,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("{label} timed out");
        }
        let remaining = deadline.saturating_duration_since(now);
        let text = match rx.recv_timeout(remaining) {
            Ok(Ok(text)) => text,
            Ok(Err(e)) => {
                return Err(e).with_context(|| format!("failed to read {label} response"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => bail!("{label} timed out"),
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("{label} response stream closed"),
        };
        if jsonrpc_response_id(&text) == Some(expected_id) {
            return Ok(text);
        }
    }
}

fn parse_tools_list_response(response_text: &str) -> Result<Vec<crate::config::ToolInfo>> {
    let response: Value = serde_json::from_str(response_text.trim())
        .context("invalid JSON response for tools/list")?;
    if let Some(err) = response.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("server returned error for tools/list: {msg}")
    }
    let tools = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Value::as_array)
        .context("unexpected tools/list response format")?;
    tools
        .iter()
        .map(|t| {
            Ok(crate::config::ToolInfo {
                name: t
                    .get("name")
                    .and_then(Value::as_str)
                    .context("tool missing name")?
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(Value::as_str)
                    .map(String::from),
                input_schema: t.get("inputSchema").cloned(),
            })
        })
        .collect()
}

fn fetch_stdio_tools(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Vec<crate::config::ToolInfo>> {
    let mut cmd = background_command(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = ManagedProcessGroup::spawn(&mut cmd)
        .with_context(|| format!("failed to start command: {command}"))?;

    let mut stdin = child.take_stdin().context("failed to capture stdin")?;
    let stdout = child.take_stdout().context("failed to capture stdout")?;

    let (tx, rx) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(text) => {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        if tx.send(Ok(trimmed)).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
        let _ = tx.send(Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "server closed stdout",
        )));
    });

    let result = (|| -> Result<Vec<crate::config::ToolInfo>> {
        // Step 1: initialize
        let init_line = serde_json::to_string(&build_initialize_request())? + "\n";
        stdin.write_all(init_line.as_bytes())?;
        stdin.flush()?;
        let init_response = recv_jsonrpc_response(&rx, 1, Duration::from_secs(10), "initialize")?;
        parse_initialize_response(&init_response).context("initialize failed")?;

        // MCP 规范：initialize 后发 notifications/initialized（无 id、无响应）
        let initialized_line = serde_json::to_string(
            &serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        )? + "\n";
        stdin.write_all(initialized_line.as_bytes())?;
        stdin.flush()?;

        // Step 2: tools/list
        let tools_line = serde_json::to_string(&build_tools_list_request())? + "\n";
        stdin.write_all(tools_line.as_bytes())?;
        stdin.flush()?;
        drop(stdin);

        let tools_response = recv_jsonrpc_response(&rx, 2, Duration::from_secs(10), "tools/list")?;
        parse_tools_list_response(&tools_response)
    })();

    let _ = child.terminate(PROCESS_GROUP_TERMINATION_GRACE);
    let _ = stdout_reader.join();

    result
}

fn fetch_http_tools(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Vec<crate::config::ToolInfo>> {
    let mut client = StreamableHttpClient::new(url, headers, &None)?;
    let result = (|| {
        for attempt in 0..2 {
            client.reset_session();
            let init = match client.send(&build_initialize_request())? {
                StreamableOutcome::AuthRequired(_) => bail!("server requires authentication"),
                StreamableOutcome::Jsonrpc(init) => init,
                StreamableOutcome::SessionExpired => {
                    bail!("initialize unexpectedly reported an expired MCP session")
                }
            };
            parse_initialize_response(&init).context("initialize failed")?;
            client.adopt_negotiated_protocol_version(&init)?;

            // MCP 规范：initialize 后必须发 notifications/initialized，服务端才转入正常请求状态。
            if matches!(
                client.notify("notifications/initialized")?,
                StreamableNotificationOutcome::SessionExpired
            ) {
                if attempt == 0 {
                    continue;
                }
                bail!("MCP session expired repeatedly during initialization");
            }

            match client.send(&build_tools_list_request())? {
                StreamableOutcome::AuthRequired(_) => bail!("server requires authentication"),
                StreamableOutcome::Jsonrpc(tools) => return parse_tools_list_response(&tools),
                StreamableOutcome::SessionExpired if attempt == 0 => continue,
                StreamableOutcome::SessionExpired => {
                    bail!("MCP session expired repeatedly during tools/list")
                }
            }
        }
        unreachable!("streamable HTTP retry loop always returns")
    })();
    finish_streamable_session(&mut client, result)
}

fn fetch_sse_tools(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Vec<crate::config::ToolInfo>> {
    let session = open_sse_session(url, headers)?;
    post_sse_json(
        &session.client,
        &session.endpoint,
        headers,
        &build_initialize_request(),
    )
    .context("failed to POST initialize to SSE endpoint")?;
    session
        .events
        .recv_timeout(Duration::from_secs(10))
        .context("no initialize response from SSE stream")?;
    // MCP 规范：initialize 后发 notifications/initialized（通知无响应，SSE 不产生事件）
    let initialized_notification =
        serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    post_sse_json(
        &session.client,
        &session.endpoint,
        headers,
        &initialized_notification,
    )?;
    post_sse_json(
        &session.client,
        &session.endpoint,
        headers,
        &build_tools_list_request(),
    )
    .context("failed to POST tools/list to SSE endpoint")?;
    let tools_raw = session
        .events
        .recv_timeout(Duration::from_secs(10))
        .context("no tools/list response from SSE stream")?;
    parse_tools_list_response(&tools_raw)
}

/// POST JSON-RPC request to an SSE message endpoint; 202 Accepted is normal
fn post_sse_json(
    client: &reqwest::blocking::Client,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: &Value,
) -> Result<()> {
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body)?);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = req.send()?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 202 {
        bail!("POST returned {}", status);
    }
    Ok(())
}

/// Parse SSE handshake text to find the `event: endpoint` → `data: <path>` pair
fn discover_sse_endpoint(body: &str, base_url: &url::Url) -> Option<String> {
    let mut current_event: Option<&str> = None;
    for line in body.lines() {
        if let Some(event_type) = line.strip_prefix("event:") {
            current_event = Some(event_type.trim());
        } else if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if current_event == Some("endpoint") && !data.is_empty() {
                return base_url.join(data).ok().map(|u| u.to_string());
            }
            current_event = None;
        }
    }
    None
}

/// 对标 Zed resolve_start_failure → OAuth discovery
fn try_oauth_discovery(
    url: &str,
    oauth: &Option<OAuthClientConfig>,
) -> Result<McpServerHealthResult> {
    // 尝试发现 OAuth metadata（GET /.well-known/oauth-authorization-server）
    let server_url: url::Url = url::Url::parse(url).context("invalid server URL")?;
    let discovery_url = format!(
        "{}://{}:{}/.well-known/oauth-authorization-server",
        server_url.scheme(),
        server_url.host_str().unwrap_or("localhost"),
        server_url
            .port()
            .unwrap_or(if server_url.scheme() == "https" {
                443
            } else {
                80
            })
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to create HTTP client")?;

    match client.get(&discovery_url).send() {
        Ok(discovery_resp) => {
            if let Ok(metadata) = discovery_resp.json::<Value>() {
                let auth_endpoint = metadata
                    .get("authorization_endpoint")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if auth_endpoint.is_empty() {
                    return Ok(McpServerHealthResult {
                        status: "auth_required".into(),
                        message: Some("server requires OAuth authentication".into()),
                        auth_url: None,
                        needs_client_secret: None,
                        tools: Vec::new(),
                    });
                }

                // 对标 Zed: 检查是否有预注册 client_id
                let needs_secret = oauth.as_ref().is_some_and(|o| o.client_secret.is_none());
                Ok(McpServerHealthResult {
                    status: "auth_required".into(),
                    message: Some(
                        "server requires OAuth authentication — click to authenticate".into(),
                    ),
                    auth_url: Some(auth_endpoint.to_string()),
                    needs_client_secret: Some(needs_secret),
                    tools: Vec::new(),
                })
            } else {
                Ok(McpServerHealthResult {
                    status: "auth_required".into(),
                    message: Some("server returned 401 — OAuth authentication required".into()),
                    auth_url: None,
                    needs_client_secret: None,
                    tools: Vec::new(),
                })
            }
        }
        Err(_) => {
            // 对标 Zed: 无 OAuth discovery，但仍返回 401 → 需要认证但无法自动发现
            let needs_secret = oauth.as_ref().is_some_and(|o| o.client_secret.is_none());
            Ok(McpServerHealthResult {
                status: "auth_required".into(),
                message: Some("server returned 401 — OAuth may be required".into()),
                auth_url: None,
                needs_client_secret: Some(needs_secret),
                tools: Vec::new(),
            })
        }
    }
}

// ── JSON Parser（对标 Zed parse_input / parse_http_input） ──

fn parse_mcp_json(
    json_content: &str,
) -> Result<(String, McpTransportConfig, Option<String>, Option<String>)> {
    let stripped: String = json_content
        .lines()
        .filter(|line| !line.trim().starts_with("///"))
        .collect::<Vec<_>>()
        .join("\n");
    let value: BTreeMap<String, McpJsonEntry> = serde_json::from_str(&stripped)
        .or_else(|_| serde_json_lenient::from_str(&stripped))
        .context("invalid MCP server JSON")?;
    anyhow::ensure!(
        value.len() == 1,
        "Expected exactly one server configuration"
    );
    let (id, entry) = value.into_iter().next().unwrap();
    let display_name = entry.name.filter(|n| !n.is_empty());
    let help_message = entry.help_message.filter(|m| !m.is_empty());
    let transport = if let Some(url) = entry.url {
        match entry.transport_type.as_deref() {
            Some("sse") => McpTransportConfig::Sse {
                url,
                headers: entry.headers,
            },
            // streamable-http 与 http 同属 streamable HTTP 传输；缺省也按 http 处理
            Some("streamable-http") | Some("http") | None => McpTransportConfig::Http {
                url,
                headers: entry.headers,
                oauth: entry.oauth,
            },
            Some(other) => bail!("unsupported transport type: {other}"),
        }
    } else {
        McpTransportConfig::Stdio {
            command: entry
                .command
                .context("command is required for stdio transport")?,
            args: entry.args,
            env: entry.env,
        }
    };
    Ok((id, transport, display_name, help_message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::read_json;
    use std::fs;
    use std::io::{Cursor, Read, Write};
    use std::net::{TcpListener, TcpStream};

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 1024];
        let header_end = loop {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0, "client closed before sending HTTP headers");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0, "client closed before sending HTTP body");
            bytes.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8(bytes).unwrap()
    }

    fn write_http_response(
        stream: &mut TcpStream,
        status: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) {
        write!(stream, "HTTP/1.1 {status}\r\n").unwrap();
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").unwrap();
        }
        write!(
            stream,
            "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();
    }

    fn settings_path(temp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(temp.path().join("settings.json")).unwrap()
    }

    #[test]
    fn managed_upsert_preserves_existing_enabled_state() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = settings_path(&temp);
        let initial = SettingsConfig {
            context_servers: Some(vec![McpServerConfig {
                id: "managed-code-graph".into(),
                name: "Old".into(),
                enabled: false,
                transport: McpTransportConfig::Stdio {
                    command: "missing-old-command".into(),
                    args: Vec::new(),
                    env: BTreeMap::new(),
                },
                managed: true,
                help_message: None,
            }]),
            ..SettingsConfig::default()
        };
        write_json(&settings_path, &initial).unwrap();

        let manager = McpManager::new(settings_path.clone());
        manager
            .add_managed(
                r#"{
                  "managed-code-graph": {
                    "command": "missing-new-command",
                    "name": "Code Graph",
                    "helpMessage": "Open the graph console before use"
                  }
                }"#,
                true,
            )
            .unwrap();

        let settings: SettingsConfig = read_json(&settings_path).unwrap();
        let server = settings
            .context_servers
            .unwrap()
            .into_iter()
            .find(|s| s.id == "managed-code-graph")
            .unwrap();
        assert!(!server.enabled);
        assert!(server.managed);
        assert_eq!(server.name, "Code Graph");
        assert_eq!(
            server.help_message.as_deref(),
            Some("Open the graph console before use")
        );
        match server.transport {
            McpTransportConfig::Stdio { command, .. } => {
                assert_eq!(command, "missing-new-command");
            }
            _ => panic!("expected stdio transport"),
        }
    }

    #[test]
    fn managed_insert_uses_channel_default_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = settings_path(&temp);
        let manager = McpManager::new(settings_path.clone());

        manager
            .add_managed(
                r#"{
                  "disabled-by-channel": {
                    "command": "missing-command",
                    "name": "Disabled By Channel"
                  }
                }"#,
                false,
            )
            .unwrap();

        let settings: SettingsConfig = read_json(&settings_path).unwrap();
        let server = settings.context_servers.unwrap().pop().unwrap();
        assert_eq!(server.id, "disabled-by-channel");
        assert!(!server.enabled);
        assert!(server.managed);
    }

    #[test]
    fn parses_sse_transport_name_and_help_message() {
        let (id, transport, name, help_message) = parse_mcp_json(
            r#"{
              "code-graph": {
                "type": "sse",
                "url": "https://example.test/mcp/sse",
                "headers": { "Authorization": "Bearer token" },
                "name": "Code Graph",
                "helpMessage": "Use this after project indexing finishes"
              }
            }"#,
        )
        .unwrap();

        assert_eq!(id, "code-graph");
        assert_eq!(name.as_deref(), Some("Code Graph"));
        assert_eq!(
            help_message.as_deref(),
            Some("Use this after project indexing finishes")
        );
        match transport {
            McpTransportConfig::Sse { url, headers } => {
                assert_eq!(url, "https://example.test/mcp/sse");
                assert_eq!(
                    headers.get("Authorization").map(String::as_str),
                    Some("Bearer token")
                );
            }
            _ => panic!("expected sse transport"),
        }
    }

    #[test]
    fn parses_streamable_http_transport() {
        let (id, transport, _, _) = parse_mcp_json(
            r#"{
              "code-graph": {
                "type": "streamable-http",
                "url": "https://example.test/mcp"
              }
            }"#,
        )
        .unwrap();

        assert_eq!(id, "code-graph");
        match transport {
            McpTransportConfig::Http { url, .. } => assert_eq!(url, "https://example.test/mcp"),
            _ => panic!("expected http transport for streamable-http"),
        }
    }

    #[test]
    fn sse_reader_assembles_multiline_data_and_waits_for_matching_response() {
        let events = concat!(
            ": keepalive\r\n\r\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{}}\r\n\r\n",
            "event: message\r\n",
            "data: {\"jsonrpc\":\"2.0\",\r\n",
            "data: \"id\":\"1\",\"result\":{}}\r\n\r\n",
        );

        let payload =
            read_sse_jsonrpc_response(Cursor::new(events), &serde_json::json!(1), "initialize")
                .unwrap();
        let value: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["id"], "1");
    }

    #[test]
    fn streamable_http_returns_matching_sse_response_before_connection_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            assert!(request.starts_with("POST /mcp HTTP/1.1"));
            stream
                .write_all(
                    concat!(
                        "HTTP/1.1 200 OK\r\n",
                        "Content-Type: text/event-stream\r\n",
                        "Connection: close\r\n\r\n",
                        "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\"}}\r\n\r\n",
                    )
                    .as_bytes(),
            )
            .unwrap();
            stream.flush().unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(12));
        });

        let mut client = StreamableHttpClient::new(&url, &BTreeMap::new(), &None).unwrap();
        let outcome = client.send(&build_initialize_request());
        release_tx.send(()).unwrap();
        server.join().unwrap();
        let response = match outcome.unwrap() {
            StreamableOutcome::Jsonrpc(response) => response,
            _ => panic!("expected JSON-RPC response"),
        };

        assert_eq!(jsonrpc_response_id(&response), Some(1));
    }

    #[test]
    fn streamable_http_reinitializes_expired_session_and_sends_delete() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for step in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_http_request(&mut stream);
                let request_lower = request.to_ascii_lowercase();
                match step {
                    0 => {
                        assert!(!request_lower.contains("mcp-session-id:"));
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            &[
                                ("Content-Type", "application/json"),
                                ("Mcp-Session-Id", "expired-session"),
                            ],
                            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}"#,
                        );
                    }
                    1 => {
                        assert!(request_lower.contains("mcp-session-id: expired-session"));
                        assert!(request_lower.contains("mcp-protocol-version: 2025-03-26"));
                        write_http_response(&mut stream, "404 Not Found", &[], "");
                    }
                    2 => {
                        assert!(!request_lower.contains("mcp-session-id:"));
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            &[
                                ("Content-Type", "application/json"),
                                ("Mcp-Session-Id", "active-session"),
                            ],
                            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}"#,
                        );
                    }
                    3 => {
                        assert!(request_lower.contains("mcp-session-id: active-session"));
                        write_http_response(&mut stream, "202 Accepted", &[], "");
                    }
                    4 => {
                        assert!(request_lower.contains("mcp-session-id: active-session"));
                        write_http_response(
                            &mut stream,
                            "200 OK",
                            &[("Content-Type", "application/json")],
                            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"lookup","description":"Lookup","inputSchema":{"type":"object"}}]}}"#,
                        );
                    }
                    5 => {
                        assert!(request.starts_with("DELETE /mcp HTTP/1.1"));
                        assert!(request_lower.contains("mcp-session-id: active-session"));
                        write_http_response(&mut stream, "200 OK", &[], "");
                    }
                    _ => unreachable!(),
                }
            }
        });

        let tools = fetch_http_tools(&url, &BTreeMap::new()).unwrap();
        server.join().unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "lookup");
    }

    #[test]
    fn serializes_servers_to_acp_mcp_schema() {
        let stdio = McpServerConfig {
            id: "stdio-id".into(),
            name: "Stdio Server".into(),
            enabled: true,
            transport: McpTransportConfig::Stdio {
                command: "node".into(),
                args: vec!["server.js".into()],
                env: BTreeMap::from([("API_KEY".into(), "secret".into())]),
            },
            managed: false,
            help_message: None,
        };
        assert_eq!(
            mcp_server_to_acp_json(&stdio),
            serde_json::json!({
                "name": "Stdio Server",
                "command": "node",
                "args": ["server.js"],
                "env": [{"name": "API_KEY", "value": "secret"}],
            })
        );

        let http = McpServerConfig {
            id: "http-id".into(),
            name: "HTTP Server".into(),
            enabled: true,
            transport: McpTransportConfig::Http {
                url: "https://example.test/mcp".into(),
                headers: BTreeMap::from([("Authorization".into(), "Bearer token".into())]),
                oauth: Some(OAuthClientConfig {
                    client_id: "client".into(),
                    client_secret: Some("secret".into()),
                }),
            },
            managed: false,
            help_message: None,
        };
        assert_eq!(
            mcp_server_to_acp_json(&http),
            serde_json::json!({
                "type": "http",
                "name": "HTTP Server",
                "url": "https://example.test/mcp",
                "headers": [{"name": "Authorization", "value": "Bearer token"}],
            })
        );

        let sse = McpServerConfig {
            id: "sse-id".into(),
            name: "SSE Server".into(),
            enabled: true,
            transport: McpTransportConfig::Sse {
                url: "https://example.test/mcp/sse".into(),
                headers: BTreeMap::new(),
            },
            managed: false,
            help_message: None,
        };
        assert_eq!(
            mcp_server_to_acp_json(&sse),
            serde_json::json!({
                "type": "sse",
                "name": "SSE Server",
                "url": "https://example.test/mcp/sse",
                "headers": [],
            })
        );
    }

    #[test]
    fn configured_acp_servers_include_enabled_entries_without_health_checks() {
        let temp = tempfile::tempdir().unwrap();
        let settings_path = settings_path(&temp);
        write_json(
            &settings_path,
            &SettingsConfig {
                context_servers: Some(vec![
                    McpServerConfig {
                        id: "enabled-unreachable".into(),
                        name: "Enabled Unreachable".into(),
                        enabled: true,
                        transport: McpTransportConfig::Stdio {
                            command: "gold-band-command-that-does-not-exist".into(),
                            args: vec!["--serve".into()],
                            env: BTreeMap::new(),
                        },
                        managed: false,
                        help_message: None,
                    },
                    McpServerConfig {
                        id: "disabled-server".into(),
                        name: "Disabled Server".into(),
                        enabled: false,
                        transport: McpTransportConfig::Http {
                            url: "https://disabled.example.test/mcp".into(),
                            headers: BTreeMap::new(),
                            oauth: None,
                        },
                        managed: false,
                        help_message: None,
                    },
                ]),
                ..SettingsConfig::default()
            },
        )
        .unwrap();

        let servers = McpManager::new(settings_path)
            .configured_acp_mcp_servers()
            .unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["name"], "Enabled Unreachable");
        assert_eq!(
            servers[0]["command"],
            "gold-band-command-that-does-not-exist"
        );
    }

    #[test]
    fn stdio_tools_list_waits_for_response_after_initialize() {
        let temp = tempfile::tempdir().unwrap();
        let (command, args) = stdio_fixture_command(&temp);

        let tools = fetch_stdio_tools(&command, &args, &BTreeMap::new()).unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "lookup");
        assert_eq!(tools[0].description.as_deref(), Some("Lookup"));
        assert_eq!(
            tools[0].input_schema.as_ref().and_then(|s| s.get("type")),
            Some(&serde_json::json!("object"))
        );
    }

    #[cfg(windows)]
    fn stdio_fixture_command(temp: &tempfile::TempDir) -> (String, Vec<String>) {
        let script = temp.path().join("mcp-fixture.ps1");
        fs::write(
            &script,
            r#"
while ($null -ne ($line = [Console]::In.ReadLine())) {
  if ($line -like '*initialize*') {
    [Console]::Out.WriteLine('{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}')
    [Console]::Out.Flush()
  } elseif ($line -like '*tools/list*') {
    [Console]::Out.WriteLine('{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"lookup","description":"Lookup","inputSchema":{"type":"object"}}]}}')
    [Console]::Out.Flush()
    break
  }
}
"#,
        )
        .unwrap();
        (
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script.to_string_lossy().into_owned(),
            ],
        )
    }

    #[cfg(not(windows))]
    fn stdio_fixture_command(temp: &tempfile::TempDir) -> (String, Vec<String>) {
        let script = temp.path().join("mcp-fixture.sh");
        fs::write(
            &script,
            r#"
while IFS= read -r line; do
  case "$line" in
    *initialize*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}'
      ;;
    *tools/list*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"lookup","description":"Lookup","inputSchema":{"type":"object"}}]}}'
      break
      ;;
  esac
done
"#,
        )
        .unwrap();
        ("sh".into(), vec![script.to_string_lossy().into_owned()])
    }
}
