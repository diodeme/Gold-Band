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
use std::time::Duration;

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{
    McpServerConfig, McpServerHealthResult, McpServerState, McpTransportConfig, OAuthClientConfig,
    SettingsConfig,
};
use crate::process::background_command;
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
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<OAuthClientConfig>,
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
        let (id, transport) = parse_mcp_json(json_content)?;
        let config = McpServerConfig {
            name: id.clone(),
            id,
            enabled: true,
            transport,
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

    pub fn update(
        &self,
        id: &str,
        json_content: &str,
    ) -> Result<(McpServerWithStatus, Vec<McpServerWithStatus>)> {
        let (new_id, transport) = parse_mcp_json(json_content)?;
        let config = McpServerConfig {
            name: new_id.clone(),
            id: new_id,
            enabled: true,
            transport,
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

    /// 对标 Zed server.start() — Stdio 发送 MCP initialize 请求; HTTP 实际请求
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
            .map(|s| match &s.transport {
                McpTransportConfig::Stdio { command, args, env } => {
                    serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "transport": "stdio",
                        "command": command,
                        "args": args,
                        "env": env,
                    })
                }
                McpTransportConfig::Http {
                    url,
                    headers,
                    oauth,
                } => {
                    let mut json = serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "transport": "http",
                        "url": url,
                        "headers": headers,
                    });
                    if let Some(o) = oauth {
                        json["oauth"] = serde_json::json!({
                            "clientId": o.client_id,
                            "clientSecret": o.client_secret,
                        });
                    }
                    json
                }
            })
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
    let mut cmd = background_command(command);
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

fn parse_mcp_json(json_content: &str) -> Result<(String, McpTransportConfig)> {
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
    let transport = if let Some(url) = entry.url {
        McpTransportConfig::Http {
            url,
            headers: entry.headers,
            oauth: entry.oauth,
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
    Ok((id, transport))
}
