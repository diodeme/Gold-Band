//! Feishu Open Platform HTTP client: tenant token cache, message creation,
//! self-check reads and the long-connection endpoint. Secrets, tokens and the
//! endpoint query (`access_key`, `ticket`) never reach `Debug` output or logs.

use std::fmt;
use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use reqwest::{Client, Method, StatusCode, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::im::{ImDeliveryReceipt, ImErrorCode, ImIntegrationError};

pub(crate) const FEISHU_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const FEISHU_ACCOUNTS_URL: &str = "https://accounts.feishu.cn/";
const FEISHU_OPEN_URL: &str = "https://open.feishu.cn/";
const LARK_ACCOUNTS_URL: &str = "https://accounts.larksuite.com/";
const LARK_OPEN_URL: &str = "https://open.larksuite.com/";
const TENANT_TOKEN_PATH: &str = "open-apis/auth/v3/tenant_access_token/internal";
const BOT_INFO_PATH: &str = "open-apis/bot/v3/info";
const APP_SCOPES_PATH: &str = "open-apis/application/v6/scopes";
const MESSAGES_PATH: &str = "open-apis/im/v1/messages";
const WS_ENDPOINT_PATH: &str = "callback/ws/endpoint";
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(300);
const BOT_ACTIVE_STATUS: i64 = 2;
const SCOPE_GRANTED: i64 = 1;
/// Tenant token rejected as invalid or expired; refreshed once and retried.
const TOKEN_INVALID_CODES: [i64; 2] = [99_991_661, 99_991_663];
const RATE_LIMITED_CODES: [i64; 2] = [99_991_400, 230_020];
/// PATCH can no longer replace a recalled, aged-out or deleted message.
const MESSAGE_UPDATE_UNAVAILABLE_CODES: [i64; 3] = [230_011, 230_031, 230_110];
const MESSAGE_PERMISSION_MISSING: i64 = 230_027;
const WS_AUTH_FAILED: i64 = 514;
const WS_INTERNAL_ERROR: i64 = 1_000_040_343;
const WS_EXCEED_CONN_LIMIT: i64 = 1_000_040_350;
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(120);
const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(120);
const DEFAULT_RECONNECT_NONCE: Duration = Duration::from_secs(30);

/// Scopes the scan-created app must hold; mirrored in the registration addons.
pub(crate) const FEISHU_REQUIRED_SCOPES: [&str; 3] = [
    "im:message:send_as_bot",
    "im:message.p2p_msg:readonly",
    "im:message:update",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeishuBrand {
    Feishu,
    Lark,
}

impl FeishuBrand {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "feishu" => Some(Self::Feishu),
            "lark" => Some(Self::Lark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuHosts {
    pub accounts: Url,
    pub open: Url,
}

/// Brand → host mapping. Production uses the official domains; tests point
/// both brands at local fixture servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuHostTable {
    feishu: FeishuHosts,
    lark: FeishuHosts,
}

impl FeishuHostTable {
    pub fn production() -> Self {
        let parse = |value: &str| Url::parse(value).expect("static Feishu host URL is valid");
        Self {
            feishu: FeishuHosts {
                accounts: parse(FEISHU_ACCOUNTS_URL),
                open: parse(FEISHU_OPEN_URL),
            },
            lark: FeishuHosts {
                accounts: parse(LARK_ACCOUNTS_URL),
                open: parse(LARK_OPEN_URL),
            },
        }
    }

    pub fn hosts(&self, brand: FeishuBrand) -> &FeishuHosts {
        match brand {
            FeishuBrand::Feishu => &self.feishu,
            FeishuBrand::Lark => &self.lark,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(feishu: Url, lark: Url) -> Self {
        Self {
            feishu: FeishuHosts {
                accounts: feishu.clone(),
                open: feishu,
            },
            lark: FeishuHosts {
                accounts: lark.clone(),
                open: lark,
            },
        }
    }
}

pub(crate) fn feishu_http_client() -> Result<Client, ImIntegrationError> {
    Client::builder()
        .timeout(FEISHU_REQUEST_TIMEOUT)
        .build()
        .map_err(|_| ImIntegrationError::permanent(ImErrorCode::NetworkUnavailable))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuBotInfo {
    pub app_name: String,
    pub open_id: String,
}

/// Server-issued long-connection tuning, all durations in seconds on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeishuClientConfig {
    pub ping_interval: Duration,
    /// Negative means unlimited reconnect attempts.
    pub reconnect_count: i64,
    pub reconnect_interval: Duration,
    pub reconnect_nonce: Duration,
}

impl Default for FeishuClientConfig {
    fn default() -> Self {
        Self {
            ping_interval: DEFAULT_PING_INTERVAL,
            reconnect_count: -1,
            reconnect_interval: DEFAULT_RECONNECT_INTERVAL,
            reconnect_nonce: DEFAULT_RECONNECT_NONCE,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RawClientConfig {
    #[serde(rename = "PingInterval")]
    ping_interval: Option<i64>,
    #[serde(rename = "ReconnectCount")]
    reconnect_count: Option<i64>,
    #[serde(rename = "ReconnectInterval")]
    reconnect_interval: Option<i64>,
    #[serde(rename = "ReconnectNonce")]
    reconnect_nonce: Option<i64>,
}

impl FeishuClientConfig {
    /// Applies server values; non-positive durations keep the previous value.
    pub(crate) fn merge(&mut self, raw: &RawClientConfig) {
        let seconds = |value: Option<i64>| {
            value
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| *value > 0)
                .map(Duration::from_secs)
        };
        if let Some(value) = seconds(raw.ping_interval) {
            self.ping_interval = value;
        }
        if let Some(value) = raw.reconnect_count {
            self.reconnect_count = value;
        }
        if let Some(value) = seconds(raw.reconnect_interval) {
            self.reconnect_interval = value;
        }
        if let Some(value) = seconds(raw.reconnect_nonce) {
            self.reconnect_nonce = value;
        }
    }
}

pub struct FeishuWsEndpoint {
    pub url: Url,
    pub service_id: i32,
    pub config: FeishuClientConfig,
}

impl fmt::Debug for FeishuWsEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeishuWsEndpoint")
            .field("host", &self.url.host_str())
            .field("service_id", &self.service_id)
            .field("config", &self.config)
            .finish()
    }
}

struct CachedToken {
    value: String,
    refresh_at: Instant,
}

/// One app's Open API session. The token cache is single-flight: concurrent
/// callers wait on the same refresh instead of each requesting a token.
pub struct FeishuOpenApi {
    client: Client,
    open: Url,
    app_id: String,
    app_secret: String,
    token: Mutex<Option<CachedToken>>,
}

impl fmt::Debug for FeishuOpenApi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeishuOpenApi")
            .field("host", &self.open.host_str())
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    code: i64,
    data: Option<T>,
}

#[derive(Deserialize)]
struct TokenResponse {
    code: i64,
    tenant_access_token: Option<String>,
    expire: Option<u64>,
}

#[derive(Deserialize)]
struct BotInfoResponse {
    code: i64,
    bot: Option<RawBotInfo>,
}

#[derive(Deserialize)]
struct RawBotInfo {
    activate_status: Option<i64>,
    app_name: Option<String>,
    open_id: Option<String>,
}

#[derive(Deserialize)]
struct ScopesData {
    #[serde(default)]
    scopes: Vec<RawScope>,
}

#[derive(Deserialize)]
struct RawScope {
    scope_name: String,
    grant_status: i64,
}

#[derive(Deserialize)]
struct WsEndpointData {
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "ClientConfig", default)]
    client_config: RawClientConfig,
}

#[derive(Deserialize)]
struct MessageData {
    message_id: Option<String>,
    chat_id: Option<String>,
}

impl FeishuOpenApi {
    pub fn new(
        client: Client,
        open: Url,
        app_id: impl Into<String>,
        app_secret: impl Into<String>,
    ) -> Self {
        Self {
            client,
            open,
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            token: Mutex::new(None),
        }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Bot identity; an inactive bot cannot receive or send messages.
    pub async fn bot_info(&self) -> Result<FeishuBotInfo, ImIntegrationError> {
        let response: BotInfoResponse = self.authorized(Method::GET, BOT_INFO_PATH, None).await?;
        ensure_success(response.code)?;
        let bot = response
            .bot
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
        if bot.activate_status != Some(BOT_ACTIVE_STATUS) {
            return Err(ImIntegrationError::permanent(ImErrorCode::ConfigInvalid));
        }
        Ok(FeishuBotInfo {
            app_name: non_empty(bot.app_name)?,
            open_id: non_empty(bot.open_id)?,
        })
    }

    /// Required scopes that the tenant has not granted yet.
    pub async fn missing_scopes(
        &self,
        required: &[&str],
    ) -> Result<Vec<String>, ImIntegrationError> {
        let response: Envelope<ScopesData> =
            self.authorized(Method::GET, APP_SCOPES_PATH, None).await?;
        ensure_success(response.code)?;
        let granted = response
            .data
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?
            .scopes
            .into_iter()
            .filter(|scope| scope.grant_status == SCOPE_GRANTED)
            .map(|scope| scope.scope_name)
            .collect::<std::collections::HashSet<_>>();
        Ok(required
            .iter()
            .filter(|scope| !granted.contains(**scope))
            .map(|scope| (*scope).to_owned())
            .collect())
    }

    /// Pulls a fresh long-connection URL; the URL embeds one-shot credentials.
    pub async fn ws_endpoint(&self) -> Result<FeishuWsEndpoint, ImIntegrationError> {
        let url = self.endpoint(WS_ENDPOINT_PATH)?;
        let request = self
            .client
            .post(url)
            .header("locale", "zh")
            .json(&json!({ "AppID": self.app_id, "AppSecret": self.app_secret }));
        let response: Envelope<WsEndpointData> = send_json(request).await?;
        match response.code {
            0 => {}
            WS_AUTH_FAILED => {
                return Err(
                    ImIntegrationError::permanent(ImErrorCode::AuthenticationRequired)
                        .with_platform_code(WS_AUTH_FAILED),
                );
            }
            WS_EXCEED_CONN_LIMIT => {
                return Err(
                    ImIntegrationError::permanent(ImErrorCode::ConnectionConflict)
                        .with_platform_code(WS_EXCEED_CONN_LIMIT),
                );
            }
            WS_INTERNAL_ERROR => {
                return Err(
                    ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None)
                        .with_platform_code(WS_INTERNAL_ERROR),
                );
            }
            code => {
                return Err(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid)
                    .with_platform_code(code));
            }
        }
        let data = response
            .data
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
        let url = Url::parse(&non_empty(data.url)?)
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
        let service_id = url
            .query_pairs()
            .find(|(key, _)| key == "service_id")
            .and_then(|(_, value)| value.parse::<i32>().ok())
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
        let mut config = FeishuClientConfig::default();
        config.merge(&data.client_config);
        Ok(FeishuWsEndpoint {
            url,
            service_id,
            config,
        })
    }

    /// Sends one message to a user identified by `open_id`.
    pub async fn create_message(
        &self,
        open_id: &str,
        msg_type: &str,
        content: &str,
        delivery_id: &str,
    ) -> Result<ImDeliveryReceipt, ImIntegrationError> {
        // The platform deduplicates this key for one hour. Derive it from the
        // durable delivery so retries (including token refresh) reuse it.
        let uuid =
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, delivery_id.as_bytes()).to_string();
        let body = json!({ "receive_id": open_id, "msg_type": msg_type, "content": content, "uuid": uuid });
        let response: Envelope<MessageData> = self
            .authorized(
                Method::POST,
                &format!("{MESSAGES_PATH}?receive_id_type=open_id"),
                Some(body),
            )
            .await?;
        ensure_success(response.code)?;
        let data = response
            .data
            .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))?;
        Ok(ImDeliveryReceipt {
            platform_message_id: Some(non_empty(data.message_id)?),
            platform_chat_id: non_empty(data.chat_id)?,
            update_token_ref: None,
        })
    }

    /// Replaces an interactive card delivered by this app.
    pub async fn patch_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Result<(), ImIntegrationError> {
        let body = json!({ "content": content });
        let response: Envelope<Value> = self
            .authorized(
                Method::PATCH,
                &format!("{MESSAGES_PATH}/{message_id}"),
                Some(body),
            )
            .await?;
        if MESSAGE_UPDATE_UNAVAILABLE_CODES.contains(&response.code) {
            return Err(
                ImIntegrationError::permanent(ImErrorCode::MessageUpdateUnavailable)
                    .with_platform_code(response.code),
            );
        }
        ensure_success(response.code)?;
        Ok(())
    }

    async fn authorized<T: DeserializeOwned + HasCode>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, ImIntegrationError> {
        let mut refreshed = false;
        loop {
            let token = self.tenant_token().await?;
            let mut request = self
                .client
                .request(method.clone(), self.endpoint(path)?)
                .bearer_auth(&token);
            if let Some(body) = &body {
                request = request.json(body);
            }
            let response: T = send_json(request).await?;
            if !refreshed && TOKEN_INVALID_CODES.contains(&response.code()) {
                refreshed = true;
                self.invalidate_token(&token).await;
                continue;
            }
            return Ok(response);
        }
    }

    async fn tenant_token(&self) -> Result<String, ImIntegrationError> {
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref()
            && Instant::now() < token.refresh_at
        {
            return Ok(token.value.clone());
        }
        let request = self
            .client
            .post(self.endpoint(TENANT_TOKEN_PATH)?)
            .json(&json!({ "app_id": self.app_id, "app_secret": self.app_secret }));
        let response: TokenResponse = send_json(request).await?;
        if response.code != 0 {
            return Err(
                ImIntegrationError::permanent(ImErrorCode::AuthenticationRequired)
                    .with_platform_code(response.code),
            );
        }
        let value = non_empty(response.tenant_access_token)?;
        let lifetime = Duration::from_secs(response.expire.unwrap_or_default());
        let usable = if lifetime > TOKEN_REFRESH_MARGIN * 2 {
            lifetime - TOKEN_REFRESH_MARGIN
        } else {
            lifetime / 2
        };
        *cached = Some(CachedToken {
            value: value.clone(),
            refresh_at: Instant::now() + usable,
        });
        Ok(value)
    }

    async fn invalidate_token(&self, rejected: &str) {
        let mut cached = self.token.lock().await;
        if cached.as_ref().is_some_and(|token| token.value == rejected) {
            *cached = None;
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, ImIntegrationError> {
        self.open
            .join(path)
            .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))
    }
}

trait HasCode {
    fn code(&self) -> i64;
}

impl<T> HasCode for Envelope<T> {
    fn code(&self) -> i64 {
        self.code
    }
}

impl HasCode for BotInfoResponse {
    fn code(&self) -> i64 {
        self.code
    }
}

/// Feishu returns JSON bodies on 4xx as well, so the body is always parsed;
/// only transport failures, 5xx and 429 become retryable.
async fn send_json<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, ImIntegrationError> {
    let response = request
        .send()
        .await
        .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?;
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs);
        return Err(ImIntegrationError::retryable(
            ImErrorCode::RateLimited,
            retry_after,
        ));
    }
    if status.is_server_error() {
        return Err(ImIntegrationError::retryable(
            ImErrorCode::NetworkUnavailable,
            None,
        ));
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| ImIntegrationError::retryable(ImErrorCode::NetworkUnavailable, None))?;
    serde_json::from_slice(&body)
        .map_err(|_| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))
}

fn ensure_success(code: i64) -> Result<(), ImIntegrationError> {
    match code {
        0 => Ok(()),
        MESSAGE_PERMISSION_MISSING => {
            Err(ImIntegrationError::permanent(ImErrorCode::ScopeMissing).with_platform_code(code))
        }
        code if RATE_LIMITED_CODES.contains(&code) => Err(ImIntegrationError::retryable(
            ImErrorCode::RateLimited,
            None,
        )
        .with_platform_code(code)),
        code if TOKEN_INVALID_CODES.contains(&code) => Err(ImIntegrationError::permanent(
            ImErrorCode::AuthenticationRequired,
        )
        .with_platform_code(code)),
        code => {
            Err(ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid)
                .with_platform_code(code))
        }
    }
}

fn non_empty(value: Option<String>) -> Result<String, ImIntegrationError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ImIntegrationError::permanent(ImErrorCode::ProtocolInvalid))
}

#[cfg(test)]
pub(crate) mod test_server {
    use std::sync::{Arc, Mutex};

    use reqwest::Url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One canned `(status, body)` per accepted connection; captures raw requests.
    pub(crate) async fn fixture_server(
        responses: Vec<(u16, String)>,
    ) -> (Url, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        tokio::spawn(async move {
            for (status, body) in responses {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0_u8; 16 * 1024];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).to_string());
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        (Url::parse(&format!("http://{address}/")).unwrap(), requests)
    }

    pub(crate) fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .no_proxy()
            .build()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{fixture_server, test_client};
    use super::*;

    const TOKEN_OK: &str = r#"{"code":0,"tenant_access_token":"t-1","expire":7200}"#;

    #[tokio::test]
    async fn truncated_response_body_is_retryable_but_invalid_json_is_permanent() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{\"code\":").await.unwrap();
            socket.shutdown().await.unwrap();
        });
        let result = send_json::<Value>(test_client().get(format!("http://{address}/"))).await;
        server.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, ImErrorCode::NetworkUnavailable);
        assert!(error.retryable);

        let (url, _) = fixture_server(vec![(200, "{broken".into())]).await;
        let error = send_json::<Value>(test_client().get(url))
            .await
            .unwrap_err();
        assert_eq!(error.code, ImErrorCode::ProtocolInvalid);
        assert!(!error.retryable);
    }

    fn api(url: Url) -> FeishuOpenApi {
        FeishuOpenApi::new(test_client(), url, "cli_app", "app-secret")
    }

    #[tokio::test]
    async fn token_is_cached_across_calls_and_message_uses_open_id() {
        let (url, requests) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (
                200,
                r#"{"code":0,"data":{"message_id":"om_1","chat_id":"oc_1"}}"#.into(),
            ),
            (
                200,
                r#"{"code":0,"data":{"message_id":"om_2","chat_id":"oc_1"}}"#.into(),
            ),
        ])
        .await;
        let api = api(url);
        let receipt = api
            .create_message("ou_1", "interactive", "{}", "delivery-1")
            .await
            .unwrap();
        assert_eq!(receipt.platform_message_id.as_deref(), Some("om_1"));
        assert_eq!(receipt.platform_chat_id, "oc_1");
        api.create_message("ou_1", "interactive", "{}", "delivery-1")
            .await
            .unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("POST /open-apis/auth/v3/tenant_access_token/internal "));
        assert!(requests[1].starts_with("POST /open-apis/im/v1/messages?receive_id_type=open_id "));
        assert!(
            requests[1]
                .to_ascii_lowercase()
                .contains("authorization: bearer t-1")
        );
        assert!(requests[2].starts_with("POST /open-apis/im/v1/messages"));
        let body = |request: &str| -> Value {
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
        };
        let first = body(&requests[1]);
        let uuid = first["uuid"]
            .as_str()
            .expect("message retries require a stable deduplication key");
        assert!(uuid.len() <= 50);
        assert_eq!(first["uuid"], body(&requests[2])["uuid"]);
    }

    #[tokio::test]
    async fn invalid_token_is_refreshed_once() {
        let (url, requests) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (400, r#"{"code":99991663,"msg":"invalid"}"#.into()),
            (
                200,
                r#"{"code":0,"tenant_access_token":"t-2","expire":7200}"#.into(),
            ),
            (
                200,
                r#"{"code":0,"data":{"message_id":"om_1","chat_id":"oc_1"}}"#.into(),
            ),
        ])
        .await;
        api(url)
            .create_message("ou_1", "text", "{}", "delivery-1")
            .await
            .unwrap();
        let requests = requests.lock().unwrap();
        assert!(
            requests[3]
                .to_ascii_lowercase()
                .contains("authorization: bearer t-2")
        );
        let body = |request: &str| -> Value {
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
        };
        assert_eq!(body(&requests[1])["uuid"], body(&requests[3])["uuid"]);
    }

    #[tokio::test]
    async fn identical_content_in_different_deliveries_has_distinct_message_keys() {
        let message = r#"{"code":0,"data":{"message_id":"om_1","chat_id":"oc_1"}}"#;
        let (url, requests) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (200, message.into()),
            (200, message.into()),
        ])
        .await;
        let api = api(url);
        api.create_message("ou_1", "text", "{}", "delivery-1")
            .await
            .unwrap();
        api.create_message("ou_1", "text", "{}", "delivery-2")
            .await
            .unwrap();
        let requests = requests.lock().unwrap();
        let body = |request: &str| -> Value {
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap()
        };
        assert_ne!(body(&requests[1])["uuid"], body(&requests[2])["uuid"]);
    }

    #[tokio::test]
    async fn response_body_timeout_is_retryable() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{")
                .await
                .unwrap();
            let _ = released.await;
        });
        let error = send_json::<Value>(
            test_client()
                .get(format!("http://{address}/"))
                .timeout(Duration::from_millis(100)),
        )
        .await
        .unwrap_err();
        let _ = release.send(());
        server.await.unwrap();
        assert_eq!(error.code, ImErrorCode::NetworkUnavailable);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn message_patch_updates_the_delivered_card() {
        let (url, requests) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (200, r#"{"code":0,"msg":"success","data":{}}"#.into()),
        ])
        .await;
        api(url)
            .patch_message("om_message", r#"{"schema":"2.0"}"#)
            .await
            .unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[1].starts_with("PATCH /open-apis/im/v1/messages/om_message "));
        assert!(requests[1].contains(r#""content":"{\"schema\":\"2.0\"}""#));
    }

    #[tokio::test]
    async fn patch_errors_distinguish_unavailable_messages_from_retry_and_invalid_cards() {
        for (code, expected, retryable) in [
            (230011, ImErrorCode::MessageUpdateUnavailable, false),
            (230031, ImErrorCode::MessageUpdateUnavailable, false),
            (230110, ImErrorCode::MessageUpdateUnavailable, false),
            (230020, ImErrorCode::RateLimited, true),
            (230027, ImErrorCode::ScopeMissing, false),
            (230099, ImErrorCode::ProtocolInvalid, false),
        ] {
            let (url, _) = fixture_server(vec![
                (200, TOKEN_OK.into()),
                (400, json!({"code": code}).to_string()),
            ])
            .await;
            let error = api(url)
                .patch_message("om_message", "{}")
                .await
                .unwrap_err();
            assert_eq!(error.code, expected, "platform code {code}");
            assert_eq!(error.retryable, retryable);
            assert_eq!(error.platform_code, Some(code));
        }
    }

    #[tokio::test]
    async fn token_failure_requires_authentication_and_4xx_bodies_are_parsed() {
        let (url, _) =
            fixture_server(vec![(400, r#"{"code":10014,"msg":"bad secret"}"#.into())]).await;
        let error = api(url).bot_info().await.unwrap_err();
        assert_eq!(error.code, ImErrorCode::AuthenticationRequired);
        assert_eq!(error.platform_code, Some(10014));
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn rate_limit_and_server_errors_are_retryable() {
        let (url, _) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (429, "{}".into()),
            (503, "{}".into()),
        ])
        .await;
        let api = api(url);
        let error = api
            .create_message("ou_1", "text", "{}", "delivery-1")
            .await
            .unwrap_err();
        assert_eq!(error.code, ImErrorCode::RateLimited);
        assert!(error.retryable);
        let error = api
            .create_message("ou_1", "text", "{}", "delivery-1")
            .await
            .unwrap_err();
        assert_eq!(error.code, ImErrorCode::NetworkUnavailable);
        assert!(error.retryable);
    }

    #[tokio::test]
    async fn bot_info_requires_active_bot_and_scopes_report_missing() {
        let (url, _) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (
                200,
                r#"{"code":0,"bot":{"activate_status":2,"app_name":"Gold Band","open_id":"ou_bot"}}"#
                    .into(),
            ),
            (
                200,
                r#"{"code":0,"data":{"scopes":[{"scope_name":"im:message:send_as_bot","grant_status":1},{"scope_name":"im:message:update","grant_status":0}]}}"#
                    .into(),
            ),
            (
                200,
                r#"{"code":0,"bot":{"activate_status":1,"app_name":"x","open_id":"ou_bot"}}"#
                    .into(),
            ),
        ])
        .await;
        let api = api(url);
        let bot = api.bot_info().await.unwrap();
        assert_eq!(bot.app_name, "Gold Band");
        assert_eq!(
            api.missing_scopes(&FEISHU_REQUIRED_SCOPES).await.unwrap(),
            vec![
                "im:message.p2p_msg:readonly".to_owned(),
                "im:message:update".to_owned()
            ]
        );
        assert_eq!(
            api.bot_info().await.unwrap_err().code,
            ImErrorCode::ConfigInvalid
        );
    }

    #[tokio::test]
    async fn ws_endpoint_parses_config_and_maps_platform_codes() {
        let (url, requests) = fixture_server(vec![
            (
                200,
                r#"{"code":0,"data":{"URL":"wss://example.test/ws?device_id=d1&service_id=33&access_key=k&ticket=t","ClientConfig":{"PingInterval":90,"ReconnectCount":-1,"ReconnectInterval":90,"ReconnectNonce":25}}}"#
                    .into(),
            ),
            (200, r#"{"code":514,"msg":"auth"}"#.into()),
            (200, r#"{"code":1000040350,"msg":"limit"}"#.into()),
            (200, r#"{"code":1000040343,"msg":"internal"}"#.into()),
            (200, r#"{"code":1,"msg":"busy"}"#.into()),
        ])
        .await;
        let api = api(url);
        let endpoint = api.ws_endpoint().await.unwrap();
        assert_eq!(endpoint.service_id, 33);
        assert_eq!(endpoint.config.ping_interval, Duration::from_secs(90));
        assert_eq!(endpoint.config.reconnect_nonce, Duration::from_secs(25));
        let debug = format!("{endpoint:?}");
        assert!(!debug.contains("access_key") && !debug.contains("ticket"));
        assert!(requests.lock().unwrap()[0].contains(r#""AppID":"cli_app""#));

        let auth = api.ws_endpoint().await.unwrap_err();
        assert_eq!(auth.code, ImErrorCode::AuthenticationRequired);
        let conflict = api.ws_endpoint().await.unwrap_err();
        assert_eq!(conflict.code, ImErrorCode::ConnectionConflict);
        assert!(!conflict.retryable);
        assert!(api.ws_endpoint().await.unwrap_err().retryable);
        let busy = api.ws_endpoint().await.unwrap_err();
        assert!(!busy.retryable);
        assert_eq!(busy.platform_code, Some(1));
    }

    #[test]
    fn debug_output_never_contains_the_secret() {
        let api = FeishuOpenApi::new(
            test_client(),
            Url::parse("https://open.feishu.cn/").unwrap(),
            "cli_app",
            "app-secret",
        );
        assert!(!format!("{api:?}").contains("app-secret"));
    }
}
