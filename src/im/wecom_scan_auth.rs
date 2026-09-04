use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

const WECOM_SCAN_BASE_URL: &str = "https://work.weixin.qq.com";
const WECOM_SCAN_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub const WECOM_SCAN_POLL_INTERVAL: Duration = Duration::from_secs(3);
pub const WECOM_SCAN_SESSION_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeComScanAuthorization {
    pub scode: String,
    pub auth_url: String,
}

#[derive(PartialEq, Eq)]
pub struct WeComScanCredentials {
    pub bot_id: String,
    pub secret: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WeComScanAuthError {
    #[error("scan authorization was cancelled")]
    Cancelled,
    #[error("scan authorization expired")]
    Expired,
    #[error("scan authorization timed out")]
    Timeout,
    #[error("scan authorization network is unavailable")]
    NetworkUnavailable,
    #[error("scan authorization protocol response is invalid")]
    ProtocolInvalid,
    #[error("scan authorization session is invalid")]
    SessionInvalid,
    #[error("scan authorization session is already being polled")]
    SessionConflict,
}

impl WeComScanAuthError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "IM_SCAN_CANCELLED",
            Self::Expired => "IM_SCAN_EXPIRED",
            Self::Timeout => "IM_SCAN_TIMEOUT",
            Self::NetworkUnavailable => "IM_SCAN_NETWORK_UNAVAILABLE",
            Self::ProtocolInvalid => "IM_SCAN_PROTOCOL_INVALID",
            Self::SessionInvalid => "IM_SCAN_SESSION_INVALID",
            Self::SessionConflict => "IM_SCAN_SESSION_CONFLICT",
        }
    }
}

#[derive(Clone)]
pub struct WeComScanAuthClient {
    client: Client,
    base_url: Url,
    source: String,
    platform_code: u8,
    poll_interval: Duration,
    session_ttl: Duration,
}

impl WeComScanAuthClient {
    pub fn new(source: impl Into<String>) -> Result<Self, WeComScanAuthError> {
        let client = Client::builder()
            .timeout(WECOM_SCAN_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| WeComScanAuthError::NetworkUnavailable)?;
        let base_url =
            Url::parse(WECOM_SCAN_BASE_URL).map_err(|_| WeComScanAuthError::ProtocolInvalid)?;
        Ok(Self {
            client,
            base_url,
            source: source.into(),
            platform_code: current_platform_code(),
            poll_interval: WECOM_SCAN_POLL_INTERVAL,
            session_ttl: WECOM_SCAN_SESSION_TTL,
        })
    }

    pub async fn generate(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<WeComScanAuthorization, WeComScanAuthError> {
        let mut url = self.endpoint("/ai/qc/generate")?;
        url.query_pairs_mut()
            .append_pair("source", self.source.trim())
            .append_pair("plat", &self.platform_code.to_string());
        let response: GenerateResponse = self.get_json(url, cancellation).await?;
        let data = response.data.ok_or(WeComScanAuthError::ProtocolInvalid)?;
        let scode = non_empty(data.scode)?;
        let auth_url = non_empty(data.auth_url)?;
        let parsed_auth_url =
            Url::parse(&auth_url).map_err(|_| WeComScanAuthError::ProtocolInvalid)?;
        if parsed_auth_url.scheme() != "https"
            || parsed_auth_url.host_str() != Some("work.weixin.qq.com")
        {
            return Err(WeComScanAuthError::ProtocolInvalid);
        }
        Ok(WeComScanAuthorization { scode, auth_url })
    }

    pub async fn poll(
        &self,
        scode: &str,
        cancellation: &CancellationToken,
    ) -> Result<WeComScanCredentials, WeComScanAuthError> {
        let started_at = Instant::now();
        let mut consecutive_network_errors = 0_u8;
        loop {
            if started_at.elapsed() >= self.session_ttl {
                return Err(WeComScanAuthError::Timeout);
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(WeComScanAuthError::Cancelled),
                _ = tokio::time::sleep(self.poll_interval) => {}
            }
            let mut url = self.endpoint("/ai/qc/query_result")?;
            url.query_pairs_mut().append_pair("scode", scode);
            let response: QueryResponse = match self.get_json(url, cancellation).await {
                Ok(response) => {
                    consecutive_network_errors = 0;
                    response
                }
                Err(WeComScanAuthError::NetworkUnavailable) if consecutive_network_errors < 2 => {
                    consecutive_network_errors += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let status = response
                .data
                .as_ref()
                .and_then(|data| data.status.as_deref())
                .or(response.status.as_deref())
                .unwrap_or_default();
            match status {
                "success" => {
                    let bot = response
                        .data
                        .and_then(|data| data.bot_info)
                        .ok_or(WeComScanAuthError::ProtocolInvalid)?;
                    return Ok(WeComScanCredentials {
                        bot_id: non_empty(bot.botid)?,
                        secret: non_empty(bot.secret)?,
                    });
                }
                "expired" => return Err(WeComScanAuthError::Expired),
                _ => {}
            }
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, WeComScanAuthError> {
        self.base_url
            .join(path)
            .map_err(|_| WeComScanAuthError::ProtocolInvalid)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
        cancellation: &CancellationToken,
    ) -> Result<T, WeComScanAuthError> {
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(WeComScanAuthError::Cancelled),
            response = self.client.get(url).send() => response.map_err(|_| WeComScanAuthError::NetworkUnavailable)?,
        };
        if response.status() != StatusCode::OK {
            return Err(if response.status().is_server_error() {
                WeComScanAuthError::NetworkUnavailable
            } else {
                WeComScanAuthError::ProtocolInvalid
            });
        }
        tokio::select! {
            _ = cancellation.cancelled() => Err(WeComScanAuthError::Cancelled),
            body = response.json() => body.map_err(|_| WeComScanAuthError::ProtocolInvalid),
        }
    }

    #[cfg(test)]
    fn with_test_endpoint(
        source: impl Into<String>,
        base_url: Url,
        poll_interval: Duration,
        session_ttl: Duration,
    ) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(1))
                .no_proxy()
                .build()
                .unwrap(),
            base_url,
            source: source.into(),
            platform_code: 2,
            poll_interval,
            session_ttl,
        }
    }
}

#[derive(Deserialize)]
struct GenerateResponse {
    data: Option<GenerateData>,
}

#[derive(Deserialize)]
struct GenerateData {
    scode: Option<String>,
    auth_url: Option<String>,
}

#[derive(Deserialize)]
struct QueryResponse {
    status: Option<String>,
    data: Option<QueryData>,
}

#[derive(Deserialize)]
struct QueryData {
    status: Option<String>,
    bot_info: Option<BotInfo>,
}

#[derive(Deserialize)]
struct BotInfo {
    botid: Option<String>,
    secret: Option<String>,
}

fn non_empty(value: Option<String>) -> Result<String, WeComScanAuthError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(WeComScanAuthError::ProtocolInvalid)
}

const fn current_platform_code() -> u8 {
    if cfg!(target_os = "macos") {
        1
    } else if cfg!(target_os = "windows") {
        2
    } else if cfg!(target_os = "linux") {
        3
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    async fn fixture_server(responses: Vec<&'static str>) -> (Url, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        tokio::spawn(async move {
            for body in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = vec![0_u8; 4096];
                let read = socket.read(&mut buffer).await.unwrap();
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (Url::parse(&format!("http://{address}/")).unwrap(), requests)
    }

    #[tokio::test]
    async fn generate_uses_configured_source_and_official_query_shape() {
        let (base_url, requests) = fixture_server(vec![
            r#"{"data":{"scode":"code-1","auth_url":"https://work.weixin.qq.com/ai/qc/c?s=code-1"}}"#,
        ])
        .await;
        let client = WeComScanAuthClient::with_test_endpoint(
            "halo",
            base_url,
            Duration::from_millis(1),
            Duration::from_secs(1),
        );
        let result = client.generate(&CancellationToken::new()).await.unwrap();
        assert_eq!(result.scode, "code-1");
        let request = requests.lock().unwrap()[0].clone();
        assert!(request.starts_with("GET /ai/qc/generate?source=halo&plat=2 "));
    }

    #[tokio::test]
    async fn poll_accepts_pending_then_success_without_exposing_credentials_in_debug() {
        let (base_url, requests) = fixture_server(vec![
            r#"{"data":{"status":"pending"}}"#,
            r#"{"data":{"status":"success","bot_info":{"botid":"bot-1","secret":"secret-1"}}}"#,
        ])
        .await;
        let client = WeComScanAuthClient::with_test_endpoint(
            "halo",
            base_url,
            Duration::from_millis(1),
            Duration::from_secs(1),
        );
        let credentials = client
            .poll("code-1", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(credentials.bot_id, "bot-1");
        assert_eq!(credentials.secret, "secret-1");
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn poll_is_cancellable_while_waiting() {
        let client = WeComScanAuthClient::with_test_endpoint(
            "halo",
            Url::parse("http://127.0.0.1:9/").unwrap(),
            Duration::from_secs(60),
            Duration::from_secs(120),
        );
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            client.poll("code-1", &cancellation).await,
            Err(WeComScanAuthError::Cancelled)
        ));
    }
}
