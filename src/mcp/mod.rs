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
use std::process::{Command, Stdio};
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
use crate::storage::write_json;

const MCP_PROTOCOL_VERSION: u64 = 1;

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

    pub fn to_acp_mcp_servers(&self) -> Result<Vec<Value>> {
        let servers = self.enabled_servers()?;
        let mut cache = self.state_cache.borrow_mut();
        Ok(servers
            .into_iter()
            .filter(|s| {
                let is_healthy = match cache.get(&s.id) {
                    Some(McpServerState::Running { .. }) => true,
                    Some(_) => false,
                    None => {
                        // 缓存未命中 → 运行健康检查并更新缓存
                        match self.verify_server(s) {
                            Ok(r) if r.status == "healthy" => {
                                cache.insert(
                                    s.id.clone(),
                                    McpServerState::Running {
                                        tools: r.tools.clone(),
                                    },
                                );
                                true
                            }
                            Ok(r) => {
                                let state = if r.status == "auth_required" {
                                    McpServerState::AuthRequired {
                                        auth_url: r.auth_url.clone(),
                                    }
                                } else {
                                    McpServerState::Error {
                                        message: r
                                            .message
                                            .unwrap_or_else(|| "unknown error".into()),
                                    }
                                };
                                cache.insert(s.id.clone(), state);
                                false
                            }
                            Err(e) => {
                                cache.insert(
                                    s.id.clone(),
                                    McpServerState::Error {
                                        message: e.to_string(),
                                    },
                                );
                                false
                            }
                        }
                    }
                };
                is_healthy
            })
            .map(|s| mcp_server_to_acp_json(&s))
            .collect())
    }

    // ── private ──

    fn load_settings(&self) -> Result<SettingsConfig> {
        if !self.settings_path.exists() {
            return Ok(SettingsConfig::default());
        }
        crate::storage::read_json(&self.settings_path)
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
            "clientCapabilities": {},
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
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Ok(McpServerHealthResult {
        status: "healthy".into(),
        message: Some(format!("MCP handshake successful (protocol v{version})")),
        auth_url: None,
        needs_client_secret: None,
        tools: Vec::new(),
    })
}

fn verify_stdio_server(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<McpServerHealthResult> {
    let mut cmd = Command::new(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to start command: {command}"))?;

    let mut stdin = child.stdin.take().context("failed to capture stdin")?;
    let stdout = child.stdout.take().context("failed to capture stdout")?;

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

    let _ = child.kill();
    let _ = child.wait();

    parse_initialize_response(&response_line)
}

fn verify_http_server(
    url: &str,
    headers: &BTreeMap<String, String>,
    oauth: &Option<OAuthClientConfig>,
) -> Result<McpServerHealthResult> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("invalid URL: must start with http:// or https://");
    }

    let initialize_body = serde_json::to_string(&build_initialize_request())
        .context("failed to serialize initialize request")?;

    let mut req = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(initialize_body);

    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }

    let response = req.send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status == 401 {
                let has_static_auth = headers
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("authorization"));
                if has_static_auth {
                    bail!("server returned 401 — check your Authorization header")
                }
                return try_oauth_discovery(url, oauth);
            }
            // 对标 Zed: 解析 MCP initialize 响应
            let body = resp.text().context("failed to read response body")?;
            parse_initialize_response(&body)
        }
        Err(e) => {
            if e.is_connect() {
                bail!("cannot connect to server: {e}")
            } else if e.is_timeout() {
                bail!("connection timed out")
            } else {
                bail!("HTTP request failed: {e}")
            }
        }
    }
}

fn verify_sse_server(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<McpServerHealthResult> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("invalid URL: must start with http:// or https://");
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?;

    let mut req = client.get(url).header("Accept", "text/event-stream");

    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }

    match req.send() {
        Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
            Ok(McpServerHealthResult {
                status: "healthy".into(),
                message: Some("SSE server reachable".into()),
                auth_url: None,
                needs_client_secret: None,
                tools: Vec::new(),
            })
        }
        Ok(resp) => {
            bail!("SSE server returned {}", resp.status())
        }
        Err(e) if e.is_connect() => {
            bail!("cannot connect to SSE server: {e}")
        }
        Err(e) if e.is_timeout() => {
            bail!("SSE connection timed out")
        }
        Err(e) => {
            bail!("SSE request failed: {e}")
        }
    }
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
    let mut cmd = Command::new(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to start command: {command}"))?;

    let mut stdin = child.stdin.take().context("failed to capture stdin")?;
    let stdout = child.stdout.take().context("failed to capture stdout")?;

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

        // Step 2: tools/list
        let tools_line = serde_json::to_string(&build_tools_list_request())? + "\n";
        stdin.write_all(tools_line.as_bytes())?;
        stdin.flush()?;
        drop(stdin);

        let tools_response = recv_jsonrpc_response(&rx, 2, Duration::from_secs(10), "tools/list")?;
        parse_tools_list_response(&tools_response)
    })();

    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();

    result
}

fn fetch_http_tools(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Vec<crate::config::ToolInfo>> {
    let tools_body = serde_json::to_string(&build_tools_list_request())?;
    let mut req = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(tools_body);
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = req.send().context("tools/list HTTP request failed")?;
    let body = resp.text().context("failed to read tools/list response")?;
    parse_tools_list_response(&body)
}

fn fetch_sse_tools(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Vec<crate::config::ToolInfo>> {
    let base_url = url::Url::parse(url).context("invalid SSE URL")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("failed to create HTTP client")?;

    // Step 1: GET SSE endpoint — read the first chunk for the endpoint URL
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

    // Step 2: Spawn reader thread to keep SSE connection alive and collect
    // incoming events. The MCP SSE session is tied to this GET connection.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut leftover = String::new();
        let mut buf = [0u8; 4096];
        loop {
            match resp.read(&mut buf) {
                Ok(n) if n > 0 => {
                    leftover.push_str(&String::from_utf8_lossy(&buf[..n]));
                    // Emit complete lines; keep incomplete tail in leftover
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

    // Step 3: POST initialize
    post_sse_json(&client, &endpoint_url, headers, &build_initialize_request())
        .context("failed to POST initialize to SSE endpoint")?;
    rx.recv_timeout(Duration::from_secs(10))
        .context("no initialize response from SSE stream")?;

    // Step 4: POST tools/list
    post_sse_json(&client, &endpoint_url, headers, &build_tools_list_request())
        .context("failed to POST tools/list to SSE endpoint")?;
    let tools_raw = rx
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
            _ => McpTransportConfig::Http {
                url,
                headers: entry.headers,
                oauth: entry.oauth,
            },
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
    [Console]::Out.WriteLine('{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}')
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
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}'
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
