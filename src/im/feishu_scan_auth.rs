//! Feishu / Lark scan-to-create: the RFC 8628 device-code registration flow
//! (`/oauth/v1/app/registration`) plus a credential self-check. The state
//! machine mirrors `wecom_scan_auth.rs`; the brand switch to Lark happens at
//! most once, when the poll response reports `tenant_brand=lark`.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::Compression;
use flate2::write::GzEncoder;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::json;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::config::FeishuScanAuthConfig;
use crate::im::connectors::feishu::api::{
    FEISHU_PROFILE_SCOPES, FEISHU_REQUIRED_SCOPES, FeishuBrand, FeishuHostTable, FeishuOpenApi,
    feishu_http_client,
};
use crate::im::connectors::feishu::{
    FEISHU_APP_SECRET_FIELD, FEISHU_BRAND_FIELD, FEISHU_OPEN_ID_FIELD,
};
use crate::im::{ImErrorCode, ImIntegrationError};

const REGISTRATION_PATH: &str = "oauth/v1/app/registration";
const REGISTRATION_ARCHETYPE: &str = "PersonalAgent";
const REGISTRATION_AUTH_METHOD: &str = "client_secret";
const REGISTRATION_USER_INFO: &str = "open_id";
const RECEIVE_MESSAGE_EVENT: &str = "im.message.receive_v1";
const CARD_ACTION_CALLBACK: &str = "card.action.trigger";
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(600);
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const MAX_CONSECUTIVE_NETWORK_ERRORS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FeishuScanAuthError {
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
    #[error("scan authorization was denied")]
    Denied,
    #[error("scanned app is missing required scopes")]
    ScopeMissing,
    #[error("scanned app credentials were rejected")]
    AuthenticationRequired,
    #[error("scanned app bot is not usable")]
    ConfigInvalid,
    #[error("scanned app long connection conflicts with another client")]
    ConnectionConflict,
}

impl FeishuScanAuthError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "IM_SCAN_CANCELLED",
            Self::Expired => "IM_SCAN_EXPIRED",
            Self::Timeout => "IM_SCAN_TIMEOUT",
            Self::NetworkUnavailable => "IM_SCAN_NETWORK_UNAVAILABLE",
            Self::ProtocolInvalid => "IM_SCAN_PROTOCOL_INVALID",
            Self::SessionInvalid => "IM_SCAN_SESSION_INVALID",
            Self::SessionConflict => "IM_SCAN_SESSION_CONFLICT",
            Self::Denied => ImErrorCode::ScanDenied.as_str(),
            Self::ScopeMissing => ImErrorCode::ScopeMissing.as_str(),
            Self::AuthenticationRequired => ImErrorCode::AuthenticationRequired.as_str(),
            Self::ConfigInvalid => ImErrorCode::ConfigInvalid.as_str(),
            Self::ConnectionConflict => ImErrorCode::ConnectionConflict.as_str(),
        }
    }

    fn from_self_check(error: ImIntegrationError) -> Self {
        match error.code {
            ImErrorCode::AuthenticationRequired | ImErrorCode::CredentialUnavailable => {
                Self::AuthenticationRequired
            }
            ImErrorCode::ScopeMissing => Self::ScopeMissing,
            ImErrorCode::ConfigInvalid => Self::ConfigInvalid,
            ImErrorCode::ConnectionConflict => Self::ConnectionConflict,
            ImErrorCode::NetworkUnavailable | ImErrorCode::RateLimited => Self::NetworkUnavailable,
            _ => Self::ProtocolInvalid,
        }
    }
}

/// A started registration: the QR URL to render and the device code to poll.
#[derive(Clone, PartialEq, Eq)]
pub struct FeishuScanAuthorization {
    pub device_code: String,
    pub qr_url: String,
    pub interval: Duration,
    pub expires_in: Duration,
}

impl fmt::Debug for FeishuScanAuthorization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeishuScanAuthorization")
            .field("interval", &self.interval)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// App credentials created by the scan; `open_id` is the scanning user.
#[derive(Clone, PartialEq, Eq)]
pub struct FeishuScanCredentials {
    pub app_id: String,
    pub app_secret: String,
    pub open_id: String,
    pub brand: FeishuBrand,
}

impl FeishuScanCredentials {
    /// Splits the credentials into the channel's public identity (the app id)
    /// and the secret fields stored in the OS credential store.
    pub fn into_credential_fields(self) -> (String, BTreeMap<String, String>) {
        let fields = BTreeMap::from([
            (FEISHU_APP_SECRET_FIELD.to_owned(), self.app_secret),
            (FEISHU_OPEN_ID_FIELD.to_owned(), self.open_id),
            (
                FEISHU_BRAND_FIELD.to_owned(),
                self.brand.as_str().to_owned(),
            ),
        ]);
        (self.app_id, fields)
    }
}

impl fmt::Debug for FeishuScanCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FeishuScanCredentials")
            .field("app_id", &self.app_id)
            .field("brand", &self.brand)
            .finish_non_exhaustive()
    }
}

/// The app the Feishu registration page should target.
///
/// `SelectOrCreate` intentionally omits both `clientID` and `createOnly`: the
/// launcher then presents its native existing-app list and the create entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeishuRegistrationTarget<'a> {
    SelectOrCreate,
    Existing(&'a str),
}

#[derive(Clone)]
pub struct FeishuScanAuthClient {
    client: Client,
    hosts: FeishuHostTable,
    config: FeishuScanAuthConfig,
    app_name: String,
    slow_down_step: Duration,
}

impl FeishuScanAuthClient {
    pub fn new(
        config: &FeishuScanAuthConfig,
        app_name: impl Into<String>,
    ) -> Result<Self, FeishuScanAuthError> {
        let app_name = app_name.into();
        if app_name.trim().is_empty() || config.app_description_suffix.trim().is_empty() {
            return Err(FeishuScanAuthError::ConfigInvalid);
        }
        Ok(Self {
            client: feishu_http_client().map_err(|_| FeishuScanAuthError::NetworkUnavailable)?,
            hosts: FeishuHostTable::production(),
            config: config.clone(),
            app_name: app_name.trim().to_owned(),
            slow_down_step: SLOW_DOWN_STEP,
        })
    }

    /// Starts a registration for the selected target. No target is forced into
    /// a create-only flow, so the platform can present its native app list.
    pub async fn begin(
        &self,
        target: FeishuRegistrationTarget<'_>,
        cancellation: &CancellationToken,
    ) -> Result<FeishuScanAuthorization, FeishuScanAuthError> {
        let response = self
            .registration(
                FeishuBrand::Feishu,
                &[
                    ("action", "begin"),
                    ("archetype", REGISTRATION_ARCHETYPE),
                    ("auth_method", REGISTRATION_AUTH_METHOD),
                    ("request_user_info", REGISTRATION_USER_INFO),
                ],
                cancellation,
            )
            .await?;
        let device_code = non_empty(response.device_code)?;
        let mut qr_url = Url::parse(&non_empty(response.verification_uri_complete)?)
            .map_err(|_| FeishuScanAuthError::ProtocolInvalid)?;
        if qr_url.scheme() != "https" {
            return Err(FeishuScanAuthError::ProtocolInvalid);
        }
        {
            let mut query = qr_url.query_pairs_mut();
            query
                .append_pair("source", self.config.source.trim())
                .append_pair("name", &self.app_name)
                .append_pair(
                    "desc",
                    &format!(
                        "{} {}",
                        self.app_name,
                        self.config.app_description_suffix.trim()
                    ),
                )
                .append_pair("addons", &encoded_addons()?);
            if let FeishuRegistrationTarget::Existing(app_id) = target {
                query.append_pair("clientID", app_id);
            }
        }
        Ok(FeishuScanAuthorization {
            device_code,
            qr_url: qr_url.into(),
            interval: response
                .interval
                .map_or(DEFAULT_POLL_INTERVAL, Duration::from_secs),
            expires_in: response
                .expires_in
                .map_or(DEFAULT_SESSION_TTL, Duration::from_secs),
        })
    }

    pub async fn poll(
        &self,
        authorization: &FeishuScanAuthorization,
        cancellation: &CancellationToken,
    ) -> Result<FeishuScanCredentials, FeishuScanAuthError> {
        let deadline = Instant::now() + authorization.expires_in;
        let mut interval = authorization.interval;
        let mut brand = FeishuBrand::Feishu;
        let mut consecutive_network_errors = 0_u8;
        let mut wait = true;
        loop {
            if wait {
                if Instant::now() + interval >= deadline {
                    return Err(FeishuScanAuthError::Timeout);
                }
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(FeishuScanAuthError::Cancelled),
                    _ = tokio::time::sleep(interval) => {}
                }
            }
            wait = true;
            let response = match self
                .registration(
                    brand,
                    &[
                        ("action", "poll"),
                        ("device_code", &authorization.device_code),
                    ],
                    cancellation,
                )
                .await
            {
                Ok(response) => {
                    consecutive_network_errors = 0;
                    response
                }
                Err(FeishuScanAuthError::NetworkUnavailable)
                    if consecutive_network_errors < MAX_CONSECUTIVE_NETWORK_ERRORS =>
                {
                    consecutive_network_errors += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let reported_brand = response
                .user_info
                .as_ref()
                .and_then(|info| info.tenant_brand.as_deref())
                .and_then(FeishuBrand::parse);
            if reported_brand == Some(FeishuBrand::Lark) && brand == FeishuBrand::Feishu {
                brand = FeishuBrand::Lark;
                wait = false;
                continue;
            }
            if let (Some(app_id), Some(app_secret)) = (
                response.client_id.filter(|value| !value.trim().is_empty()),
                response
                    .client_secret
                    .filter(|value| !value.trim().is_empty()),
            ) {
                let open_id = non_empty(response.user_info.and_then(|info| info.open_id))?;
                return Ok(FeishuScanCredentials {
                    app_id,
                    app_secret,
                    open_id,
                    brand,
                });
            }
            match response.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => interval += self.slow_down_step,
                Some("access_denied") => return Err(FeishuScanAuthError::Denied),
                Some("expired_token") => return Err(FeishuScanAuthError::Expired),
                _ => return Err(FeishuScanAuthError::ProtocolInvalid),
            }
        }
    }

    /// Proves the scanned app is usable: active bot, required scopes granted
    /// and the long connection endpoint reachable.
    pub async fn self_check(
        &self,
        credentials: &FeishuScanCredentials,
    ) -> Result<(), FeishuScanAuthError> {
        let api = FeishuOpenApi::new(
            self.client.clone(),
            self.hosts.hosts(credentials.brand).open.clone(),
            credentials.app_id.clone(),
            credentials.app_secret.clone(),
        );
        api.bot_info()
            .await
            .map_err(FeishuScanAuthError::from_self_check)?;
        let missing = api
            .missing_scopes(&FEISHU_REQUIRED_SCOPES)
            .await
            .map_err(FeishuScanAuthError::from_self_check)?;
        if !missing.is_empty() {
            return Err(FeishuScanAuthError::ScopeMissing);
        }
        api.ws_endpoint()
            .await
            .map_err(FeishuScanAuthError::from_self_check)?;
        Ok(())
    }

    async fn registration(
        &self,
        brand: FeishuBrand,
        form: &[(&str, &str)],
        cancellation: &CancellationToken,
    ) -> Result<RegistrationResponse, FeishuScanAuthError> {
        let url = self
            .hosts
            .hosts(brand)
            .accounts
            .join(REGISTRATION_PATH)
            .map_err(|_| FeishuScanAuthError::ProtocolInvalid)?;
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(FeishuScanAuthError::Cancelled),
            response = self.client.post(url).form(form).send() => {
                response.map_err(|_| FeishuScanAuthError::NetworkUnavailable)?
            }
        };
        // RFC 8628 reports pending / slow_down / denial as HTTP 400 bodies.
        if response.status().is_server_error() {
            return Err(FeishuScanAuthError::NetworkUnavailable);
        }
        tokio::select! {
            _ = cancellation.cancelled() => Err(FeishuScanAuthError::Cancelled),
            body = response.json() => body.map_err(|_| FeishuScanAuthError::ProtocolInvalid),
        }
    }

    #[cfg(test)]
    fn for_test(hosts: FeishuHostTable, app_name: &str) -> Self {
        Self {
            client: crate::im::connectors::feishu::api::test_server::test_client(),
            hosts,
            config: FeishuScanAuthConfig {
                source: "maling".to_owned(),
                app_description_suffix: "remote intervention".to_owned(),
            },
            app_name: app_name.to_owned(),
            slow_down_step: Duration::from_millis(20),
        }
    }
}

#[derive(Deserialize)]
struct RegistrationResponse {
    device_code: Option<String>,
    verification_uri_complete: Option<String>,
    interval: Option<u64>,
    expires_in: Option<u64>,
    client_id: Option<String>,
    client_secret: Option<String>,
    user_info: Option<RegistrationUserInfo>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct RegistrationUserInfo {
    open_id: Option<String>,
    tenant_brand: Option<String>,
}

/// Least-privilege preset for the confirm page: `preset:false` drops the
/// platform template, then only the declared scopes, event and callback apply.
/// Encoding is fixed by the platform: JSON → gzip → base64url without padding.
fn encoded_addons() -> Result<String, FeishuScanAuthError> {
    // Profile scopes are requested but optional: the self-check never needs them.
    let scopes = [
        FEISHU_REQUIRED_SCOPES.as_slice(),
        FEISHU_PROFILE_SCOPES.as_slice(),
    ]
    .concat();
    let addons = json!({
        "preset": false,
        "scopes": { "tenant": scopes },
        "events": { "items": { "tenant": [RECEIVE_MESSAGE_EVENT] } },
        "callbacks": { "items": [CARD_ACTION_CALLBACK] },
    });
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(addons.to_string().as_bytes())
        .map_err(|_| FeishuScanAuthError::ProtocolInvalid)?;
    let compressed = encoder
        .finish()
        .map_err(|_| FeishuScanAuthError::ProtocolInvalid)?;
    Ok(URL_SAFE_NO_PAD.encode(compressed))
}

fn non_empty(value: Option<String>) -> Result<String, FeishuScanAuthError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(FeishuScanAuthError::ProtocolInvalid)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;
    use serde_json::Value;

    use super::*;
    use crate::im::connectors::feishu::api::test_server::fixture_server;

    const TOKEN_OK: &str = r#"{"code":0,"tenant_access_token":"t-1","expire":7200}"#;
    const BOT_OK: &str =
        r#"{"code":0,"bot":{"activate_status":2,"app_name":"Gold Band","open_id":"ou_bot"}}"#;
    const SCOPES_ALL: &str = r#"{"code":0,"data":{"scopes":[{"scope_name":"im:message:send_as_bot","grant_status":1},{"scope_name":"im:message.p2p_msg:readonly","grant_status":1},{"scope_name":"im:message:update","grant_status":1}]}}"#;
    const SCOPES_PARTIAL: &str = r#"{"code":0,"data":{"scopes":[{"scope_name":"im:message:send_as_bot","grant_status":1}]}}"#;
    const WS_OK: &str = r#"{"code":0,"data":{"URL":"wss://example.test/ws?device_id=d&service_id=1&access_key=k&ticket=t","ClientConfig":{"PingInterval":90}}}"#;
    const BEGIN_OK: &str = r#"{"device_code":"dc-1","verification_uri_complete":"https://accounts.feishu.cn/page/register?user_code=U1","interval":0,"expires_in":30}"#;

    fn client_for(url: Url) -> FeishuScanAuthClient {
        client_for_named(url, "MALING")
    }

    fn client_for_named(url: Url, app_name: &str) -> FeishuScanAuthClient {
        FeishuScanAuthClient::for_test(FeishuHostTable::for_test(url.clone(), url), app_name)
    }

    fn authorization(interval: Duration, expires_in: Duration) -> FeishuScanAuthorization {
        FeishuScanAuthorization {
            device_code: "dc-1".to_owned(),
            qr_url: "https://accounts.feishu.cn/page/register".to_owned(),
            interval,
            expires_in,
        }
    }

    fn quick() -> FeishuScanAuthorization {
        authorization(Duration::from_millis(1), Duration::from_secs(10))
    }

    fn query(url: &str) -> Vec<(String, String)> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    fn param<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    #[tokio::test]
    async fn begin_uses_channel_name_and_least_privilege_addons() {
        let (url, requests) =
            fixture_server(vec![(200, BEGIN_OK.into()), (200, BEGIN_OK.into())]).await;
        let client = client_for(url);
        let cancellation = CancellationToken::new();

        let created = client
            .begin(FeishuRegistrationTarget::SelectOrCreate, &cancellation)
            .await
            .unwrap();
        assert_eq!(created.device_code, "dc-1");
        assert_eq!(created.interval, Duration::ZERO);
        assert_eq!(created.expires_in, Duration::from_secs(30));
        let pairs = query(&created.qr_url);
        assert_eq!(param(&pairs, "user_code"), Some("U1"));
        assert_eq!(param(&pairs, "source"), Some("maling"));
        assert_eq!(param(&pairs, "name"), Some("MALING"));
        assert_eq!(param(&pairs, "desc"), Some("MALING remote intervention"));
        assert_eq!(param(&pairs, "createOnly"), None);
        assert_eq!(param(&pairs, "clientID"), None);
        assert!(param(&pairs, "from").is_none() && param(&pairs, "tp").is_none());

        let encoded = param(&pairs, "addons").unwrap();
        assert!(!encoded.contains('=') && !encoded.contains('+') && !encoded.contains('/'));
        let mut json = String::new();
        GzDecoder::new(URL_SAFE_NO_PAD.decode(encoded).unwrap().as_slice())
            .read_to_string(&mut json)
            .unwrap();
        let addons: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            addons,
            json!({
                "preset": false,
                "scopes": { "tenant": [
                    "im:message:send_as_bot",
                    "im:message.p2p_msg:readonly",
                    "im:message:update",
                    "contact:contact.base:readonly",
                    "contact:user.base:readonly"
                ] },
                "events": { "items": { "tenant": ["im.message.receive_v1"] } },
                "callbacks": { "items": ["card.action.trigger"] },
            })
        );

        let reauthorized = client
            .begin(
                FeishuRegistrationTarget::Existing("cli_existing"),
                &cancellation,
            )
            .await
            .unwrap();
        let pairs = query(&reauthorized.qr_url);
        assert_eq!(param(&pairs, "clientID"), Some("cli_existing"));
        assert_eq!(param(&pairs, "createOnly"), None);

        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("POST /oauth/v1/app/registration"));
        for field in [
            "action=begin",
            "archetype=PersonalAgent",
            "auth_method=client_secret",
            "request_user_info=open_id",
        ] {
            assert!(requests[0].contains(field), "missing {field}");
        }
    }

    #[tokio::test]
    async fn begin_uses_the_default_channel_product_name() {
        let (url, _) = fixture_server(vec![(200, BEGIN_OK.into())]).await;
        let created = client_for_named(url, "Gold Band")
            .begin(
                FeishuRegistrationTarget::SelectOrCreate,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let pairs = query(&created.qr_url);
        assert_eq!(param(&pairs, "name"), Some("Gold Band"));
        assert_eq!(param(&pairs, "desc"), Some("Gold Band remote intervention"));
        assert_eq!(param(&pairs, "createOnly"), None);
        assert_eq!(param(&pairs, "clientID"), None);
    }

    #[tokio::test]
    async fn begin_rejects_a_non_https_qr_url_and_missing_device_code() {
        let (url, _) = fixture_server(vec![
            (
                200,
                r#"{"device_code":"dc","verification_uri_complete":"http://evil.test/x"}"#.into(),
            ),
            (
                200,
                r#"{"verification_uri_complete":"https://accounts.feishu.cn/x"}"#.into(),
            ),
        ])
        .await;
        let client = client_for(url);
        let cancellation = CancellationToken::new();
        for _ in 0..2 {
            assert_eq!(
                client
                    .begin(FeishuRegistrationTarget::SelectOrCreate, &cancellation)
                    .await
                    .unwrap_err(),
                FeishuScanAuthError::ProtocolInvalid
            );
        }
    }

    #[tokio::test]
    async fn poll_waits_through_pending_and_slow_down_then_returns_credentials() {
        let (url, requests) = fixture_server(vec![
            (400, r#"{"error":"authorization_pending"}"#.into()),
            (400, r#"{"error":"slow_down"}"#.into()),
            (
                200,
                r#"{"client_id":"cli_new","client_secret":"s3cret","user_info":{"open_id":"ou_owner","tenant_brand":"feishu"}}"#
                    .into(),
            ),
        ])
        .await;
        let credentials = client_for(url)
            .poll(&quick(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            credentials,
            FeishuScanCredentials {
                app_id: "cli_new".to_owned(),
                app_secret: "s3cret".to_owned(),
                open_id: "ou_owner".to_owned(),
                brand: FeishuBrand::Feishu,
            }
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests
                .iter()
                .all(|request| request.contains("action=poll")
                    && request.contains("device_code=dc-1"))
        );
    }

    #[tokio::test]
    async fn poll_switches_to_lark_hosts_once() {
        let (feishu, feishu_requests) = fixture_server(vec![(
            400,
            r#"{"error":"authorization_pending","user_info":{"tenant_brand":"lark"}}"#.into(),
        )])
        .await;
        let (lark, lark_requests) = fixture_server(vec![(
            200,
            r#"{"client_id":"cli_lark","client_secret":"s","user_info":{"open_id":"ou_l","tenant_brand":"lark"}}"#
                .into(),
        )])
        .await;
        let client =
            FeishuScanAuthClient::for_test(FeishuHostTable::for_test(feishu, lark), "MALING");
        let credentials = client
            .poll(
                &authorization(Duration::from_millis(300), Duration::from_secs(60)),
                &CancellationToken::new(),
            )
            .await;
        let credentials = credentials.unwrap();
        assert_eq!(credentials.brand, FeishuBrand::Lark);
        assert_eq!(credentials.app_id, "cli_lark");
        assert_eq!(feishu_requests.lock().unwrap().len(), 1);
        assert_eq!(lark_requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn poll_maps_terminal_errors() {
        for (body, expected) in [
            (r#"{"error":"access_denied"}"#, FeishuScanAuthError::Denied),
            (r#"{"error":"expired_token"}"#, FeishuScanAuthError::Expired),
            (
                r#"{"error":"invalid_grant"}"#,
                FeishuScanAuthError::ProtocolInvalid,
            ),
            (
                r#"{"client_id":"cli","client_secret":"s","user_info":{}}"#,
                FeishuScanAuthError::ProtocolInvalid,
            ),
        ] {
            let (url, _) = fixture_server(vec![(400, body.into())]).await;
            assert_eq!(
                client_for(url)
                    .poll(&quick(), &CancellationToken::new())
                    .await
                    .unwrap_err(),
                expected,
                "{body}"
            );
        }
    }

    #[tokio::test]
    async fn poll_tolerates_two_consecutive_network_errors() {
        let success = r#"{"client_id":"cli","client_secret":"s","user_info":{"open_id":"ou"}}"#;
        let (url, _) = fixture_server(vec![
            (502, "{}".into()),
            (503, "{}".into()),
            (200, success.into()),
        ])
        .await;
        assert!(
            client_for(url)
                .poll(&quick(), &CancellationToken::new())
                .await
                .is_ok()
        );

        let (url, _) = fixture_server(vec![
            (502, "{}".into()),
            (502, "{}".into()),
            (502, "{}".into()),
        ])
        .await;
        assert_eq!(
            client_for(url)
                .poll(&quick(), &CancellationToken::new())
                .await
                .unwrap_err(),
            FeishuScanAuthError::NetworkUnavailable
        );
    }

    #[tokio::test]
    async fn poll_stops_at_the_deadline_and_on_cancellation() {
        let (url, requests) = fixture_server(vec![]).await;
        let client = client_for(url);
        assert_eq!(
            client
                .poll(
                    &authorization(Duration::from_secs(5), Duration::from_secs(1)),
                    &CancellationToken::new(),
                )
                .await
                .unwrap_err(),
            FeishuScanAuthError::Timeout
        );
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            client
                .poll(
                    &authorization(Duration::from_secs(1), Duration::from_secs(60)),
                    &cancellation,
                )
                .await
                .unwrap_err(),
            FeishuScanAuthError::Cancelled
        );
        assert!(requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn self_check_requires_scopes_and_the_long_connection_endpoint() {
        let credentials = FeishuScanCredentials {
            app_id: "cli_app".to_owned(),
            app_secret: "s".to_owned(),
            open_id: "ou".to_owned(),
            brand: FeishuBrand::Feishu,
        };
        let (url, requests) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (200, BOT_OK.into()),
            (200, SCOPES_ALL.into()),
            (200, WS_OK.into()),
        ])
        .await;
        client_for(url).self_check(&credentials).await.unwrap();
        assert_eq!(requests.lock().unwrap().len(), 4);

        let (url, _) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (200, BOT_OK.into()),
            (200, SCOPES_PARTIAL.into()),
        ])
        .await;
        assert_eq!(
            client_for(url).self_check(&credentials).await.unwrap_err(),
            FeishuScanAuthError::ScopeMissing
        );

        let (url, _) = fixture_server(vec![
            (200, TOKEN_OK.into()),
            (200, BOT_OK.into()),
            (200, SCOPES_ALL.into()),
            (200, r#"{"code":1000040350,"msg":"limit"}"#.into()),
        ])
        .await;
        assert_eq!(
            client_for(url).self_check(&credentials).await.unwrap_err(),
            FeishuScanAuthError::ConnectionConflict
        );

        let (url, _) =
            fixture_server(vec![(400, r#"{"code":10014,"msg":"bad secret"}"#.into())]).await;
        assert_eq!(
            client_for(url).self_check(&credentials).await.unwrap_err(),
            FeishuScanAuthError::AuthenticationRequired
        );
    }

    #[test]
    fn debug_output_hides_secrets_and_device_codes() {
        let credentials = FeishuScanCredentials {
            app_id: "cli_app".to_owned(),
            app_secret: "top-secret".to_owned(),
            open_id: "ou_person".to_owned(),
            brand: FeishuBrand::Lark,
        };
        let debug = format!("{credentials:?}");
        assert!(!debug.contains("top-secret") && !debug.contains("ou_person"));
        let debug = format!("{:?}", quick());
        assert!(!debug.contains("dc-1"));
    }

    #[test]
    fn credential_fields_match_what_the_connector_reads() {
        let (app_id, fields) = FeishuScanCredentials {
            app_id: "cli_app".to_owned(),
            app_secret: "top-secret".to_owned(),
            open_id: "ou_person".to_owned(),
            brand: FeishuBrand::Lark,
        }
        .into_credential_fields();
        assert_eq!(app_id, "cli_app");
        assert_eq!(fields[FEISHU_APP_SECRET_FIELD], "top-secret");
        assert_eq!(fields[FEISHU_OPEN_ID_FIELD], "ou_person");
        assert_eq!(
            FeishuBrand::parse(&fields[FEISHU_BRAND_FIELD]),
            Some(FeishuBrand::Lark)
        );
        assert_eq!(fields.len(), 3);
    }

    #[test]
    fn error_codes_reuse_the_shared_im_codes() {
        assert_eq!(FeishuScanAuthError::Denied.code(), "IM_SCAN_DENIED");
        assert_eq!(FeishuScanAuthError::ScopeMissing.code(), "IM_SCOPE_MISSING");
        assert_eq!(FeishuScanAuthError::Expired.code(), "IM_SCAN_EXPIRED");
    }
}
