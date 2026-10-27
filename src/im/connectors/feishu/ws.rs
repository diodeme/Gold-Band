//! pbbp2 long-connection framing (§6.3): prost declarations of the Feishu
//! `Frame`/`Header` wire types, ping/ACK construction, fragment reassembly and
//! reconnect pacing. Only project types leave this module; logs may carry the
//! frame type, method, length and generation but never payloads or URL queries.

mod connection;
pub(super) use connection::{Connection, EventReply, InboundEvent};

#[cfg(test)]
mod connector_tests;

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use prost::Message;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::Instant;
use uuid::Uuid;

use super::api::FeishuClientConfig;

const FRAME_METHOD_CONTROL: i32 = 0;
const FRAME_METHOD_DATA: i32 = 1;
const HEADER_TYPE: &str = "type";
const HEADER_MESSAGE_ID: &str = "message_id";
const HEADER_SUM: &str = "sum";
const HEADER_SEQ: &str = "seq";
const HEADER_BIZ_RT: &str = "biz_rt";
const TYPE_PING: &str = "ping";
const TYPE_PONG: &str = "pong";
const TYPE_EVENT: &str = "event";
const ACK_CODE_OK: u16 = 200;
const ACK_CODE_FAILED: u16 = 500;
const FRAGMENT_TTL: Duration = Duration::from_secs(10);
const MAX_PENDING_FRAGMENTED_MESSAGES: usize = 32;
const MAX_FRAGMENTS_PER_MESSAGE: usize = 64;
const FEISHU_CALLBACK_BUDGET: Duration = Duration::from_millis(2_500);
const MAX_PENDING_ACTION_RESPONSES: usize = 128;

#[derive(Clone, PartialEq, Message)]
struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

/// pbbp2 `Frame`. Fields 1–4 are proto2 `required` and must always be encoded.
#[derive(Clone, PartialEq, Message)]
struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Ping,
    Pong,
    Event,
    Other,
}

impl FrameKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ping => TYPE_PING,
            Self::Pong => TYPE_PONG,
            Self::Event => TYPE_EVENT,
            Self::Other => "other",
        }
    }
}

impl Frame {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.as_str())
    }

    fn kind(&self) -> FrameKind {
        match (self.method, self.header(HEADER_TYPE)) {
            (FRAME_METHOD_CONTROL, Some(TYPE_PING)) => FrameKind::Ping,
            (FRAME_METHOD_CONTROL, Some(TYPE_PONG)) => FrameKind::Pong,
            (FRAME_METHOD_DATA, Some(TYPE_EVENT)) => FrameKind::Event,
            _ => FrameKind::Other,
        }
    }

    fn payload_len(&self) -> usize {
        self.payload.as_ref().map_or(0, Vec::len)
    }
}

fn ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: FRAME_METHOD_CONTROL,
        headers: vec![Header {
            key: HEADER_TYPE.to_owned(),
            value: TYPE_PING.to_owned(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

/// Outcome of handling one data frame, echoed back as the frame ACK.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FeishuAckResult {
    Handled(Option<Value>),
    Failed,
}

/// The ACK is the received frame plus a positive `biz_rt` header (handling time
/// in ms) and a `{code, data?}` payload whose `data` is base64 JSON.
fn ack_frame(mut frame: Frame, elapsed: Duration, result: &FeishuAckResult) -> Frame {
    let payload = match result {
        FeishuAckResult::Handled(None) => json!({ "code": ACK_CODE_OK }),
        FeishuAckResult::Handled(Some(data)) => json!({
            "code": ACK_CODE_OK,
            "data": BASE64.encode(data.to_string()),
        }),
        FeishuAckResult::Failed => json!({ "code": ACK_CODE_FAILED }),
    };
    frame.headers.retain(|header| header.key != HEADER_BIZ_RT);
    frame.headers.push(Header {
        key: HEADER_BIZ_RT.to_owned(),
        value: elapsed.as_millis().to_string(),
    });
    frame.payload = Some(payload.to_string().into_bytes());
    frame
}

/// Callback replies belonging to one connection generation. The waiter removes
/// an entry on completion or timeout, so the map never grows beyond in-flight
/// callbacks. A stale generation has its own map and is dropped with the socket.
#[derive(Debug)]
pub(crate) struct PendingActionResponses {
    generation: u64,
    responses: std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

impl PendingActionResponses {
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            responses: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn open(&self, request_id: &str) -> Option<oneshot::Receiver<Value>> {
        let mut responses = self
            .responses
            .lock()
            .expect("Feishu callback response lock poisoned");
        if responses.len() >= MAX_PENDING_ACTION_RESPONSES {
            return None;
        }
        let (sender, receiver) = oneshot::channel();
        responses.insert(request_id.to_owned(), sender);
        Some(receiver)
    }

    pub(crate) fn take(&self, request_id: &str) -> Option<oneshot::Sender<Value>> {
        self.responses
            .lock()
            .expect("Feishu callback response lock poisoned")
            .remove(request_id)
    }

    pub(crate) fn send(&self, request_id: &str, response: Value) -> bool {
        self.take(request_id)
            .is_some_and(|sender| sender.send(response).is_ok())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.responses
            .lock()
            .expect("Feishu callback response lock poisoned")
            .len()
    }
}

fn encode_frame(frame: &Frame) -> Vec<u8> {
    frame.encode_to_vec()
}

fn decode_frame(bytes: &[u8]) -> Option<Frame> {
    Frame::decode(bytes).ok()
}

struct PendingFragments {
    parts: Vec<Option<Vec<u8>>>,
    received: usize,
    first_seen: Instant,
}

/// Reassembles `sum > 1` payloads by `message_id`; bounded in message count,
/// fragments per message and lifetime so a lost fragment cannot grow memory.
#[derive(Default)]
struct FragmentBuffer {
    pending: HashMap<String, PendingFragments>,
}

impl FragmentBuffer {
    /// Returns the full payload once complete. Unfragmented frames pass through.
    fn accept(&mut self, frame: &Frame, now: Instant) -> Option<Vec<u8>> {
        let payload = frame.payload.clone().unwrap_or_default();
        let sum = frame
            .header(HEADER_SUM)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if sum <= 1 {
            return Some(payload);
        }
        let seq = frame.header(HEADER_SEQ)?.parse::<usize>().ok()?;
        let message_id = frame.header(HEADER_MESSAGE_ID)?;
        if sum > MAX_FRAGMENTS_PER_MESSAGE || seq >= sum {
            return None;
        }
        self.expire(now);
        if !self.pending.contains_key(message_id)
            && self.pending.len() >= MAX_PENDING_FRAGMENTED_MESSAGES
            && let Some(oldest) = self
                .pending
                .iter()
                .min_by_key(|(_, entry)| entry.first_seen)
                .map(|(key, _)| key.clone())
        {
            self.pending.remove(&oldest);
        }
        let entry = self
            .pending
            .entry(message_id.to_owned())
            .or_insert_with(|| PendingFragments {
                parts: vec![None; sum],
                received: 0,
                first_seen: now,
            });
        if entry.parts.len() != sum {
            self.pending.remove(message_id);
            return None;
        }
        if entry.parts[seq].is_none() {
            entry.parts[seq] = Some(payload);
            entry.received += 1;
        }
        if entry.received < sum {
            return None;
        }
        let entry = self.pending.remove(message_id)?;
        Some(entry.parts.into_iter().flatten().flatten().collect())
    }

    fn expire(&mut self, now: Instant) {
        self.pending
            .retain(|_, entry| now.duration_since(entry.first_seen) < FRAGMENT_TTL);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pending.len()
    }
}

/// The first retry waits a random slice of `ReconnectNonce` so many clients do
/// not reconnect in lockstep; later retries wait `ReconnectInterval`.
pub(crate) fn reconnect_delay(attempt: u32, config: &FeishuClientConfig) -> Duration {
    if attempt == 0 {
        let nonce_ms = u64::try_from(config.reconnect_nonce.as_millis()).unwrap_or(u64::MAX);
        if nonce_ms == 0 {
            return Duration::ZERO;
        }
        let jitter = Uuid::new_v4().as_u128() % u128::from(nonce_ms);
        return Duration::from_millis(u64::try_from(jitter).unwrap_or(0));
    }
    config.reconnect_interval
}

/// Whether another reconnect attempt is allowed (`ReconnectCount < 0` is unlimited).
pub(crate) fn reconnect_allowed(attempt: u32, config: &FeishuClientConfig) -> bool {
    config.reconnect_count < 0 || i64::from(attempt) < config.reconnect_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::connectors::feishu::api::RawClientConfig;

    fn data_frame(headers: &[(&str, &str)], payload: &[u8]) -> Frame {
        Frame {
            seq_id: 7,
            log_id: 9,
            service: 33,
            method: FRAME_METHOD_DATA,
            headers: headers
                .iter()
                .map(|(key, value)| Header {
                    key: (*key).to_owned(),
                    value: (*value).to_owned(),
                })
                .collect(),
            payload_encoding: None,
            payload_type: None,
            payload: Some(payload.to_vec()),
            log_id_new: Some("log-new".into()),
        }
    }

    #[test]
    fn frame_round_trips_with_official_tags() {
        let frame = data_frame(&[("type", "event"), ("message_id", "m1")], b"{}");
        let bytes = encode_frame(&frame);
        // tag 1 varint, tag 2 varint, tag 3 varint, tag 4 varint, tag 5 length-delimited.
        assert_eq!(&bytes[..2], &[0x08, 7]);
        assert_eq!(&bytes[2..4], &[0x10, 9]);
        assert_eq!(&bytes[4..6], &[0x18, 33]);
        assert_eq!(&bytes[6..8], &[0x20, 1]);
        assert_eq!(bytes[8], 0x2a);
        assert_eq!(decode_frame(&bytes), Some(frame));
    }

    #[test]
    fn ping_frame_encodes_required_zero_fields() {
        let frame = ping_frame(33);
        let bytes = encode_frame(&frame);
        assert_eq!(&bytes[..8], &[0x08, 0, 0x10, 0, 0x18, 33, 0x20, 0]);
        let decoded = decode_frame(&bytes).unwrap();
        assert_eq!(decoded.kind(), FrameKind::Ping);
        assert_eq!(decoded.service, 33);
    }

    #[test]
    fn pong_payload_updates_client_config_and_ignores_non_positive_values() {
        let mut config = FeishuClientConfig::default();
        let raw: RawClientConfig = serde_json::from_str(
            r#"{"PingInterval":90,"ReconnectCount":3,"ReconnectInterval":0,"ReconnectNonce":25}"#,
        )
        .unwrap();
        config.merge(&raw);
        assert_eq!(config.ping_interval, Duration::from_secs(90));
        assert_eq!(config.reconnect_count, 3);
        assert_eq!(
            config.reconnect_interval,
            FeishuClientConfig::default().reconnect_interval
        );
        assert_eq!(config.reconnect_nonce, Duration::from_secs(25));
    }

    #[test]
    fn ack_carries_positive_biz_rt_and_base64_result() {
        let frame = data_frame(&[("type", "event"), ("biz_rt", "-3")], b"{\"x\":1}");
        let result = json!({ "toast": { "type": "info", "content": "ok" } });
        let ack = ack_frame(
            frame,
            Duration::from_millis(42),
            &FeishuAckResult::Handled(Some(result.clone())),
        );
        assert_eq!(ack.header("biz_rt"), Some("42"));
        assert_eq!(ack.headers.iter().filter(|h| h.key == "biz_rt").count(), 1);
        let payload: Value = serde_json::from_slice(ack.payload.as_ref().unwrap()).unwrap();
        assert_eq!(payload["code"], 200);
        let data = BASE64.decode(payload["data"].as_str().unwrap()).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&data).unwrap(), result);
        assert_eq!(ack.seq_id, 7);

        let failed = ack_frame(
            data_frame(&[("type", "event")], b"{}"),
            Duration::ZERO,
            &FeishuAckResult::Failed,
        );
        let payload: Value = serde_json::from_slice(failed.payload.as_ref().unwrap()).unwrap();
        assert_eq!(payload, json!({ "code": 500 }));
    }

    #[tokio::test(start_paused = true)]
    async fn fragments_reassemble_in_any_order_and_expire() {
        let mut buffer = FragmentBuffer::default();
        let now = Instant::now();
        let second = data_frame(
            &[
                ("type", "event"),
                ("message_id", "m1"),
                ("sum", "2"),
                ("seq", "1"),
            ],
            b"world",
        );
        let first = data_frame(
            &[
                ("type", "event"),
                ("message_id", "m1"),
                ("sum", "2"),
                ("seq", "0"),
            ],
            b"hello ",
        );
        assert_eq!(buffer.accept(&second, now), None);
        assert_eq!(buffer.accept(&first, now), Some(b"hello world".to_vec()));
        assert_eq!(buffer.len(), 0);

        assert_eq!(buffer.accept(&second, now), None);
        buffer.expire(now + FRAGMENT_TTL);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.accept(&first, now + FRAGMENT_TTL), None);
    }

    #[tokio::test(start_paused = true)]
    async fn fragment_buffer_is_bounded() {
        let mut buffer = FragmentBuffer::default();
        let start = Instant::now();
        for index in 0..(MAX_PENDING_FRAGMENTED_MESSAGES + 5) {
            let id = format!("m{index}");
            let frame = data_frame(
                &[
                    ("type", "event"),
                    ("message_id", &id),
                    ("sum", "2"),
                    ("seq", "0"),
                ],
                b"x",
            );
            let at = start + Duration::from_millis(index as u64);
            assert_eq!(buffer.accept(&frame, at), None);
        }
        assert_eq!(buffer.len(), MAX_PENDING_FRAGMENTED_MESSAGES);
        let oversized = data_frame(
            &[
                ("type", "event"),
                ("message_id", "big"),
                ("sum", &(MAX_FRAGMENTS_PER_MESSAGE + 1).to_string()),
                ("seq", "0"),
            ],
            b"x",
        );
        assert_eq!(buffer.accept(&oversized, start), None);
        assert_eq!(buffer.len(), MAX_PENDING_FRAGMENTED_MESSAGES);
    }

    #[test]
    fn reconnect_delay_uses_nonce_then_interval() {
        let config = FeishuClientConfig {
            ping_interval: Duration::from_secs(90),
            reconnect_count: 2,
            reconnect_interval: Duration::from_secs(90),
            reconnect_nonce: Duration::from_secs(25),
        };
        for _ in 0..32 {
            assert!(reconnect_delay(0, &config) < Duration::from_secs(25));
        }
        assert_eq!(reconnect_delay(1, &config), Duration::from_secs(90));
        assert!(reconnect_allowed(1, &config));
        assert!(!reconnect_allowed(2, &config));
        assert!(reconnect_allowed(
            u32::MAX,
            &FeishuClientConfig {
                reconnect_count: -1,
                ..config
            }
        ));
    }

    #[test]
    fn pending_action_responses_are_bounded_and_removed_on_completion() {
        let table = PendingActionResponses::new(7);
        let mut receivers = Vec::new();
        for index in 0..MAX_PENDING_ACTION_RESPONSES {
            receivers.push(
                table
                    .open(&format!("request-{index}"))
                    .expect("callback slot"),
            );
        }
        assert!(table.open("request-overflow").is_none());
        assert_eq!(table.len(), MAX_PENDING_ACTION_RESPONSES);

        assert!(table.send("request-0", serde_json::json!({ "toast": {} })));
        drop(receivers.remove(0));
        assert_eq!(table.len(), MAX_PENDING_ACTION_RESPONSES - 1);
        assert!(!table.send("request-0", serde_json::json!({ "toast": {} })));
        assert_eq!(table.generation(), 7);
    }
}
