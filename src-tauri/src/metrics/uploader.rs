use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow, ensure};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::collector::{
    BatchDisposition, CollectorCommand, OutboxItem, RejectedEvent, run_collector_actor,
};
use super::metrics_log;

const CLAIM_LEASE_SECONDS: i64 = 30;
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_RETRY_DELAY_SECONDS: i64 = 15 * 60;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricsBatchRequest {
    events: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricsBatchEnvelope {
    data: MetricsBatchResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricsBatchResponse {
    #[serde(default)]
    accepted_event_ids: Vec<String>,
    #[serde(default)]
    duplicate_event_ids: Vec<String>,
    #[serde(default)]
    rejected: Vec<RejectedResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RejectedResponse {
    event_id: String,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error: Option<RejectedError>,
}

#[derive(Debug, Deserialize)]
struct RejectedError {
    code: String,
}

impl RejectedResponse {
    fn into_event(self) -> Result<RejectedEvent> {
        let error_code = self
            .error_code
            .or_else(|| self.error.map(|error| error.code))
            .filter(|code| !code.trim().is_empty())
            .ok_or_else(|| anyhow!("rejected metrics event is missing error code"))?;
        Ok(RejectedEvent {
            event_id: self.event_id,
            error_code,
        })
    }
}

pub(super) fn start(
    database_path: PathBuf,
    endpoint: String,
    api_key: String,
) -> mpsc::Sender<CollectorCommand> {
    let (sender, receiver) = mpsc::channel(2048);
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = run_collector_actor(&database_path, receiver, |code, error| {
            metrics_log(&format!("[lifecycle-metrics] code={code} error={error}"));
        }) {
            metrics_log(&format!(
                "[lifecycle-metrics] code=METRICS_COLLECTOR_START_FAILED error={error}"
            ));
        }
    });
    tauri::async_runtime::spawn(run_uploader(sender.clone(), endpoint, api_key));
    sender
}

async fn run_uploader(sender: mpsc::Sender<CollectorCommand>, endpoint: String, api_key: String) {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let owner = format!("desktop-{}", uuid::Uuid::new_v4().simple());
    let mut next_cleanup_at = 0;
    loop {
        let now = chrono::Utc::now().timestamp();
        if now >= next_cleanup_at {
            if sender
                .send(CollectorCommand::Cleanup {
                    now_epoch_seconds: now,
                })
                .await
                .is_err()
            {
                return;
            }
            next_cleanup_at = now.saturating_add(24 * 60 * 60);
        }
        let items = match claim(&sender, &owner, now).await {
            Ok(items) => items,
            Err(error) => {
                metrics_log(&format!(
                    "[lifecycle-metrics] code=METRICS_OUTBOX_CLAIM_FAILED error={error}"
                ));
                tokio::time::sleep(IDLE_POLL_INTERVAL).await;
                continue;
            }
        };
        if items.is_empty() {
            tokio::time::sleep(IDLE_POLL_INTERVAL).await;
            continue;
        }

        let event_ids = items
            .iter()
            .map(|item| item.event_id.clone())
            .collect::<Vec<_>>();
        match send_batch(&client, &endpoint, &api_key, &items).await {
            Ok(disposition) => {
                let (reply, received) = oneshot::channel();
                let command = CollectorCommand::ApplyDisposition {
                    owner: owner.clone(),
                    disposition,
                    now_epoch_seconds: chrono::Utc::now().timestamp(),
                    reply,
                };
                if sender.send(command).await.is_err() {
                    return;
                }
                if let Err(error) = receive_collector(received).await {
                    metrics_log(&format!(
                        "[lifecycle-metrics] code=METRICS_OUTBOX_ACK_FAILED error={error}"
                    ));
                }
            }
            Err(error) => {
                let pause_upload = error.pause_upload;
                let retry_at = if pause_upload {
                    now
                } else {
                    now.saturating_add(retry_delay_seconds(&items))
                };
                let (reply, received) = oneshot::channel();
                let command = CollectorCommand::Retry {
                    owner: owner.clone(),
                    event_ids,
                    next_attempt_at: retry_at,
                    error_code: error.code,
                    reply,
                };
                if sender.send(command).await.is_err() {
                    return;
                }
                if let Err(error) = receive_collector(received).await {
                    metrics_log(&format!(
                        "[lifecycle-metrics] code=METRICS_OUTBOX_RETRY_FAILED error={error}"
                    ));
                }
                if pause_upload {
                    metrics_log(
                        "[lifecycle-metrics] code=METRICS_UPLOAD_PAUSED reason=authentication",
                    );
                    return;
                }
            }
        }
    }
}

async fn claim(
    sender: &mpsc::Sender<CollectorCommand>,
    owner: &str,
    now_epoch_seconds: i64,
) -> Result<Vec<OutboxItem>> {
    let (reply, received) = oneshot::channel();
    sender
        .send(CollectorCommand::Claim {
            owner: owner.to_string(),
            now_epoch_seconds,
            lease_seconds: CLAIM_LEASE_SECONDS,
            limit: super::METRICS_BATCH_LIMIT,
            reply,
        })
        .await
        .map_err(|_| anyhow!("metrics collector stopped"))?;
    receive_collector(received).await
}

async fn receive_collector<T>(received: oneshot::Receiver<Result<T>>) -> Result<T> {
    received
        .await
        .map_err(|_| anyhow!("metrics collector dropped reply"))?
}

#[derive(Debug)]
struct UploadError {
    code: String,
    pause_upload: bool,
}

async fn send_batch(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    items: &[OutboxItem],
) -> Result<BatchDisposition, UploadError> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let events = items
        .iter()
        .map(|item| serde_json::from_str::<Value>(&item.payload_json))
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(|_| UploadError {
            code: "METRICS_OUTBOX_PAYLOAD_INVALID".to_string(),
            pause_upload: false,
        })?;
    let body = MetricsBatchRequest { events };
    metrics_log(&format!(
        "[lifecycle-metrics] request requestId={request_id} eventCount={}",
        items.len()
    ));
    let response = client
        .post(endpoint)
        .header("X-Maling-Report-Key", api_key)
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| UploadError {
            code: "METRICS_NETWORK_FAILED".to_string(),
            pause_upload: false,
        })?;
    let status = response.status();
    if !status.is_success() {
        metrics_log(&format!(
            "[lifecycle-metrics] response requestId={request_id} status={status}"
        ));
        return Err(UploadError {
            code: retry_code(status),
            pause_upload: is_auth_failure(status),
        });
    }
    let response = response
        .json::<MetricsBatchEnvelope>()
        .await
        .map_err(|_| UploadError {
            code: "METRICS_RESPONSE_INVALID".to_string(),
            pause_upload: false,
        })?
        .data;
    let disposition = BatchDisposition {
        accepted_event_ids: response.accepted_event_ids,
        duplicate_event_ids: response.duplicate_event_ids,
        rejected: response
            .rejected
            .into_iter()
            .map(RejectedResponse::into_event)
            .collect::<Result<Vec<_>>>()
            .map_err(|_| UploadError {
                code: "METRICS_RESPONSE_INVALID".to_string(),
                pause_upload: false,
            })?,
    };
    validate_disposition(items, &disposition).map_err(|_| UploadError {
        code: "METRICS_RESPONSE_INCOMPLETE".to_string(),
        pause_upload: false,
    })?;
    metrics_log(&format!(
        "[lifecycle-metrics] response requestId={request_id} status={status} accepted={} duplicate={} rejected={}",
        disposition.accepted_event_ids.len(),
        disposition.duplicate_event_ids.len(),
        disposition.rejected.len()
    ));
    Ok(disposition)
}

fn validate_disposition(items: &[OutboxItem], disposition: &BatchDisposition) -> Result<()> {
    let expected = items
        .iter()
        .map(|item| item.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let actual = disposition
        .accepted_event_ids
        .iter()
        .chain(&disposition.duplicate_event_ids)
        .map(String::as_str)
        .chain(
            disposition
                .rejected
                .iter()
                .map(|event| event.event_id.as_str()),
        )
        .collect::<Vec<_>>();
    ensure!(
        actual.len() == expected.len(),
        "response event count mismatch"
    );
    ensure!(
        actual.iter().copied().collect::<BTreeSet<_>>() == expected,
        "response event ids mismatch"
    );
    Ok(())
}

fn retry_code(status: StatusCode) -> String {
    format!("METRICS_HTTP_{}", status.as_u16())
}

fn is_auth_failure(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

fn retry_delay_seconds(items: &[OutboxItem]) -> i64 {
    let attempts = items
        .iter()
        .map(|item| item.attempt_count)
        .max()
        .unwrap_or(1)
        .min(8);
    2_i64
        .saturating_pow(attempts)
        .clamp(2, MAX_RETRY_DELAY_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    fn item(id: &str) -> OutboxItem {
        OutboxItem {
            event_id: id.to_string(),
            payload_json: "{}".to_string(),
            attempt_count: 1,
        }
    }

    #[test]
    fn disposition_requires_exactly_one_result_per_claimed_event() {
        let items = vec![item("a"), item("b"), item("c")];
        let valid = BatchDisposition {
            accepted_event_ids: vec!["a".to_string()],
            duplicate_event_ids: vec!["b".to_string()],
            rejected: vec![RejectedEvent {
                event_id: "c".to_string(),
                error_code: "METRICS_FIELD_INVALID".to_string(),
            }],
        };
        assert!(validate_disposition(&items, &valid).is_ok());

        let mut missing = valid.clone();
        missing.rejected.clear();
        assert!(validate_disposition(&items, &missing).is_err());

        let mut duplicate = valid;
        duplicate.accepted_event_ids.push("b".to_string());
        assert!(validate_disposition(&items, &duplicate).is_err());
    }

    #[test]
    fn authentication_failures_pause_uploader_but_server_failures_retry() {
        assert!(is_auth_failure(StatusCode::UNAUTHORIZED));
        assert!(is_auth_failure(StatusCode::FORBIDDEN));
        assert!(!is_auth_failure(StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_auth_failure(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[tokio::test]
    async fn real_http_batch_parses_partial_server_disposition() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "metrics request timed out");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("metrics test server failed: {error}"),
                }
            };
            let mut request = [0_u8; 8192];
            let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("x-maling-report-key: key")
            );
            assert!(request.contains("\"eventId\":\"accepted\""));
            let body = r#"{"code":200,"msg":"","ok":true,"data":{"acceptedEventIds":["accepted"],"duplicateEventIds":["duplicate"],"rejected":[{"eventId":"rejected","error":{"code":"METRICS_FIELD_INVALID"}}]}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let items = ["accepted", "duplicate", "rejected"]
            .into_iter()
            .map(|id| OutboxItem {
                event_id: id.to_string(),
                payload_json: format!(r#"{{"eventId":"{id}"}}"#),
                attempt_count: 1,
            })
            .collect::<Vec<_>>();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let disposition = send_batch(
            &client,
            &format!("http://{address}/api/client-report/metrics/batch"),
            "key",
            &items,
        )
        .await
        .unwrap();
        handle.join().unwrap();
        assert_eq!(disposition.accepted_event_ids, vec!["accepted"]);
        assert_eq!(disposition.duplicate_event_ids, vec!["duplicate"]);
        assert_eq!(disposition.rejected[0].event_id, "rejected");
        assert_eq!(disposition.rejected[0].error_code, "METRICS_FIELD_INVALID");
    }
}
