//! Socket ownership and pbbp2 dispatch. Callers receive an event payload and
//! a single-use reply handle, never a wire frame or WebSocket sink.

use std::future::Future;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use super::super::api::RawClientConfig;
use super::super::network_unavailable;
use super::{
    Duration, FEISHU_CALLBACK_BUDGET, FeishuAckResult, FeishuClientConfig, FragmentBuffer, Frame,
    FrameKind, Instant, ack_frame, decode_frame, encode_frame, ping_frame,
};
use crate::im::ImIntegrationError;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const ACK_CAPACITY: usize = 64;

pub(crate) struct Connection {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    service_id: i32,
}

pub(crate) struct InboundEvent {
    pub payload: Vec<u8>,
    pub reply: EventReply,
}

/// The received frame and ACK sender stay inside the transport. Consuming
/// this handle prevents one handler from sending two ACKs for an event.
pub(crate) struct EventReply {
    frame: Frame,
    sender: mpsc::Sender<Frame>,
    received_at: Instant,
}

impl EventReply {
    pub fn deadline(&self) -> Instant {
        self.received_at + FEISHU_CALLBACK_BUDGET
    }

    pub async fn send(self, result: FeishuAckResult) {
        let ack = ack_frame(self.frame, self.received_at.elapsed(), &result);
        let _ = self.sender.send(ack).await;
    }
}

impl Connection {
    pub async fn open(
        url: &str,
        service_id: i32,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self>, ImIntegrationError> {
        let (socket, _) = tokio::select! {
            _ = cancellation.cancelled() => return Ok(None),
            connected = tokio::time::timeout(HANDSHAKE_TIMEOUT, tokio_tungstenite::connect_async(url)) => {
                connected.map_err(|_| network_unavailable())?.map_err(|_| network_unavailable())?
            }
        };
        Ok(Some(Self { socket, service_id }))
    }

    pub async fn run<F, Fut>(
        mut self,
        generation: u64,
        client_config: &mut FeishuClientConfig,
        cancellation: &CancellationToken,
        handle_event: F,
    ) -> Result<(), ImIntegrationError>
    where
        F: Fn(InboundEvent) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (ack_tx, mut ack_rx) = mpsc::channel::<Frame>(ACK_CAPACITY);
        // Every handler belongs to this socket, including card callbacks.
        let mut handlers = JoinSet::new();
        let mut fragments = FragmentBuffer::default();
        let mut last_inbound = Instant::now();
        let mut last_ping: Option<Instant> = None;
        let mut next_ping = Instant::now();
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = self.socket.close(None).await;
                    return Ok(());
                }
                _ = tokio::time::sleep_until(next_ping) => {
                    if last_ping.is_some_and(|sent| last_inbound < sent) {
                        tracing::warn!(generation, "Feishu long connection went silent");
                        return Err(network_unavailable());
                    }
                    let now = Instant::now();
                    self.socket.send(Message::Binary(encode_frame(&ping_frame(self.service_id)).into()))
                        .await.map_err(|_| network_unavailable())?;
                    last_ping = Some(now);
                    next_ping = now + client_config.ping_interval;
                }
                Some(ack) = ack_rx.recv() => {
                    self.socket.send(Message::Binary(encode_frame(&ack).into()))
                        .await.map_err(|_| network_unavailable())?;
                }
                Some(_) = handlers.join_next(), if !handlers.is_empty() => {}
                message = self.socket.next() => {
                    let message = message.ok_or_else(network_unavailable)?.map_err(|_| network_unavailable())?;
                    last_inbound = Instant::now();
                    match message {
                        Message::Binary(bytes) => {
                            let Some(mut frame) = decode_frame(&bytes) else {
                                tracing::warn!(generation, len = bytes.len(), "Feishu frame could not be decoded");
                                continue;
                            };
                            let kind = frame.kind();
                            tracing::trace!(generation, kind = kind.as_str(), method = frame.method, len = frame.payload_len(), "Feishu frame received");
                            match kind {
                                FrameKind::Pong => match frame.payload.as_deref().map(serde_json::from_slice::<RawClientConfig>) {
                                    Some(Ok(raw)) => client_config.merge(&raw),
                                    _ => tracing::debug!(generation, "Feishu pong without a usable client config"),
                                },
                                FrameKind::Event => {
                                    let Some(payload) = fragments.accept(&frame, Instant::now()) else { continue };
                                    frame.payload = None;
                                    let event = InboundEvent {
                                        payload,
                                        reply: EventReply { frame, sender: ack_tx.clone(), received_at: Instant::now() },
                                    };
                                    handlers.spawn(handle_event(event));
                                }
                                FrameKind::Ping | FrameKind::Other => {}
                            }
                        }
                        Message::Close(_) => return Err(network_unavailable()),
                        _ => {}
                    }
                }
            }
        }
    }
}
