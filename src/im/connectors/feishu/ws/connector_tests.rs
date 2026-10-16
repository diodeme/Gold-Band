use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use super::super::api::test_server::{fixture_server, test_client};
use super::super::*;
use super::*;
use super::{FRAME_METHOD_DATA, Header};

const APP_ID: &str = "cli_fixture_app";
const OWNER: &str = "ou_fixture_owner";
const RECEIVE_FIXTURE: &str = include_str!("../fixtures/im_message_receive_v1.json");

fn test_codec() -> ImActionTokenCodec {
    ImActionTokenCodec::new(vec![7; 32]).unwrap()
}

#[tokio::test]
async fn cancelling_transport_drops_its_pending_event_handler() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cancellation = CancellationToken::new();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (connection, mut server) =
        tokio::join!(Connection::open(&url, 7, &cancellation), accept(&listener),);
    let connection = connection.unwrap().unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let released = CancellationToken::new();
    let task = tokio::spawn({
        let cancellation = cancellation.clone();
        let entered = Arc::clone(&entered);
        let released = released.clone();
        async move {
            connection
                .run(
                    7,
                    &mut FeishuClientConfig::default(),
                    &cancellation,
                    move |event| {
                        let entered = Arc::clone(&entered);
                        let released = released.clone();
                        async move {
                            let _on_exit = released.drop_guard();
                            entered.notify_one();
                            std::future::pending::<()>().await;
                            drop(event);
                        }
                    },
                )
                .await
        }
    });
    send_frame(&mut server, &event_frame(1, "blocked", "{}")).await;
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), released.cancelled())
        .await
        .expect("pending handler is dropped with its owning connection");
    let _ = server.close(None).await;
}

type Started = (
    TcpListener,
    Arc<FeishuConnector>,
    mpsc::Receiver<ImConnectorEvent>,
    CancellationToken,
    tokio::task::JoinHandle<Result<(), ImIntegrationError>>,
);

fn config() -> ResolvedImChannelConfig {
    ResolvedImChannelConfig {
        public_identity: APP_ID.to_owned(),
        credential_fields: BTreeMap::from([
            (FEISHU_APP_SECRET_FIELD.to_owned(), "secret".to_owned()),
            (FEISHU_OPEN_ID_FIELD.to_owned(), OWNER.to_owned()),
            (FEISHU_BRAND_FIELD.to_owned(), "feishu".to_owned()),
        ]),
        locale: ImLocale::En,
    }
}

fn session_responses(ws_url: &str, client_config: Value) -> Vec<(u16, String)> {
    let token = json!({ "code": 0, "tenant_access_token": "t-1", "expire": 7200 });
    let bot = json!({
        "code": 0,
        "bot": { "activate_status": 2, "app_name": "Gold Band", "open_id": "ou_bot" },
    });
    let endpoint = json!({
        "code": 0,
        "data": { "URL": ws_url, "ClientConfig": client_config },
    });
    [token, bot, endpoint]
        .into_iter()
        .map(|body| (200, body.to_string()))
        .collect()
}

async fn start(
    client_config: Value,
    generation: u64,
    prepare: impl FnOnce(&FeishuConnector),
) -> Started {
    start_with_http(client_config, generation, prepare, Vec::new())
        .await
        .0
}

async fn start_with_http(
    client_config: Value,
    generation: u64,
    prepare: impl FnOnce(&FeishuConnector),
    extra_responses: Vec<(u16, String)>,
) -> (Started, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_url = format!(
        "ws://{}/ws?service_id=7&ticket=hidden",
        listener.local_addr().unwrap()
    );
    let mut responses = session_responses(&ws_url, client_config);
    responses.extend(extra_responses);
    let (open, requests) = fixture_server(responses).await;
    let connector = FeishuConnector::for_test(
        FeishuHostTable::for_test(open.clone(), open),
        test_client(),
        test_codec(),
    );
    prepare(&connector);
    let connector = Arc::new(connector);
    let (events_tx, events_rx) = mpsc::channel(16);
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let connector = Arc::clone(&connector);
        let cancellation = cancellation.clone();
        async move {
            connector
                .connect(config(), generation, events_tx, cancellation)
                .await
        }
    });
    (
        (listener, connector, events_rx, cancellation, task),
        requests,
    )
}

async fn accept(listener: &TcpListener) -> WebSocketStream<TcpStream> {
    let (stream, _) = listener.accept().await.unwrap();
    tokio_tungstenite::accept_async(stream).await.unwrap()
}

fn header(key: &str, value: &str) -> Header {
    Header {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}

fn event_frame(seq_id: u64, message_id: &str, payload: &str) -> Frame {
    Frame {
        seq_id,
        log_id: seq_id,
        service: 7,
        method: FRAME_METHOD_DATA,
        headers: vec![header("type", "event"), header("message_id", message_id)],
        payload_encoding: None,
        payload_type: None,
        payload: Some(payload.as_bytes().to_vec()),
        log_id_new: None,
    }
}

async fn send_frame(server: &mut WebSocketStream<TcpStream>, frame: &Frame) {
    server
        .send(Message::Binary(encode_frame(frame).into()))
        .await
        .unwrap();
}

async fn next_data_frame(server: &mut WebSocketStream<TcpStream>) -> Frame {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), server.next())
            .await
            .expect("frame in time")
            .expect("socket open")
            .expect("valid message");
        if let Message::Binary(bytes) = message {
            let frame = decode_frame(&bytes).expect("decodable frame");
            if frame.method == FRAME_METHOD_DATA {
                return frame;
            }
        }
    }
}

async fn next_event(events: &mut mpsc::Receiver<ImConnectorEvent>) -> ImConnectorEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("event in time")
        .expect("event channel open")
}

fn card_event(event_id: &str) -> String {
    json!({
        "schema": "2.0",
        "header": {
            "event_id": event_id,
            "event_type": "card.action.trigger",
            "create_time": "1791959702123",
            "app_id": APP_ID,
            "tenant_key": "tenant_fixture",
        },
        "event": {
            "operator": { "open_id": OWNER },
            "action": {
                "value": {
                    "deliveryId": "delivery-1",
                    "actionToken": "signed-token",
                    "actionIndex": 0,
                },
                "tag": "button",
                "name": "gb_action",
            },
            "context": {
                "open_message_id": "om_card",
                "open_chat_id": "oc_fixture_p2p",
            },
        },
    })
    .to_string()
}

#[tokio::test]
async fn owner_message_is_acked_and_observes_the_private_binding() {
    let (listener, _connector, mut events, cancellation, task) = start(json!({}), 1, |_| {}).await;
    let mut server = accept(&listener).await;
    assert_eq!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected {
            generation: 1,
            identity: ImConnectionIdentity {
                bot_id: APP_ID.to_owned(),
                display_name: "Gold Band".to_owned(),
            },
        }
    );

    let stranger = RECEIVE_FIXTURE.replace(
        "\"open_id\": \"ou_fixture_owner\"",
        "\"open_id\": \"ou_stranger\"",
    );
    let foreign_app = RECEIVE_FIXTURE.replace(APP_ID, "cli_other_app");
    for (seq, payload) in [
        (1, stranger.as_str()),
        (2, foreign_app.as_str()),
        (3, RECEIVE_FIXTURE),
    ] {
        send_frame(&mut server, &event_frame(seq, &format!("m-{seq}"), payload)).await;
    }

    let mut acked = Vec::new();
    for _ in 0..3 {
        let ack = next_data_frame(&mut server).await;
        assert!(ack.header("biz_rt").expect("biz_rt").parse::<u64>().is_ok());
        let body: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
        assert_eq!(body, json!({ "code": 200 }));
        acked.push(ack.seq_id);
    }
    acked.sort_unstable();
    assert_eq!(acked, vec![1, 2, 3]);

    assert_eq!(
        next_event(&mut events).await,
        ImConnectorEvent::BindingObserved {
            generation: 1,
            binding: ImObservedBinding {
                destination_id: OWNER.to_owned(),
                conversation_id: "oc_fixture_p2p".to_owned(),
                actor_id: OWNER.to_owned(),
                is_private: true,
            },
        }
    );
    assert!(events.try_recv().is_err(), "only the owner message binds");

    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn fragmented_event_is_reassembled_and_acked_once() {
    let (listener, _connector, mut events, cancellation, task) = start(json!({}), 1, |_| {}).await;
    let mut server = accept(&listener).await;
    next_event(&mut events).await;

    let (head, tail) = RECEIVE_FIXTURE.split_at(RECEIVE_FIXTURE.len() / 2);
    for (seq, part) in [(0_u64, head), (1, tail)] {
        let mut frame = event_frame(10 + seq, "fragmented", part);
        frame.headers.push(header("sum", "2"));
        frame.headers.push(header("seq", &seq.to_string()));
        send_frame(&mut server, &frame).await;
    }

    assert_eq!(next_data_frame(&mut server).await.seq_id, 11);
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::BindingObserved { .. }
    ));
    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn card_callback_replies_within_the_generation_budget() {
    let (listener, connector, mut events, cancellation, task) = start(json!({}), 1, |_| {}).await;
    let mut server = accept(&listener).await;
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected { .. }
    ));

    send_frame(
        &mut server,
        &event_frame(20, "m-card", &card_event("ev-card")),
    )
    .await;
    let response_context = match next_event(&mut events).await {
        ImConnectorEvent::InboundAction {
            response_context, ..
        } => response_context,
        other => panic!("expected inbound action, got {other:?}"),
    };
    connector
        .respond_to_action(
            response_context,
            ImActionResponse {
                state: crate::im::ImActionResponseState::Handled,
                resolved_by: Some(crate::im::ImResolvedBy::Channel(ImChannelKind::Feishu)),
                outcome: None,
                delivery: None,
            },
        )
        .await
        .unwrap();

    let ack = next_data_frame(&mut server).await;
    let body: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
    let encoded = body["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let reply: Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(reply["toast"]["type"], "success");
    assert_eq!(reply["card"]["type"], "raw");
    assert!(
        connector
            .pending_responses
            .lock()
            .unwrap()
            .get(&1)
            .is_none_or(|table| table.len() == 0),
        "a completed callback response must leave no pending entry"
    );

    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn slow_card_callback_gets_a_processing_toast_and_late_reply_is_discarded() {
    let (listener, connector, mut events, cancellation, task) = start(json!({}), 1, |_| {}).await;
    let mut server = accept(&listener).await;
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected { .. }
    ));

    send_frame(
        &mut server,
        &event_frame(21, "m-slow", &card_event("ev-slow")),
    )
    .await;
    let response_context = match next_event(&mut events).await {
        ImConnectorEvent::InboundAction {
            response_context, ..
        } => response_context,
        other => panic!("expected inbound action, got {other:?}"),
    };

    let ack = next_data_frame(&mut server).await;
    let body: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(body["data"].as_str().unwrap())
        .unwrap();
    let reply: Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(reply["toast"]["content"], "Processing");
    assert!(reply.get("card").is_none());

    connector
        .respond_to_action(
            response_context,
            ImActionResponse {
                state: crate::im::ImActionResponseState::Handled,
                resolved_by: Some(crate::im::ImResolvedBy::Channel(ImChannelKind::Feishu)),
                outcome: None,
                delivery: None,
            },
        )
        .await
        .unwrap();
    assert!(
        connector
            .pending_responses
            .lock()
            .unwrap()
            .get(&1)
            .is_none_or(|table| table.len() == 0),
        "a timed-out callback response must be discarded"
    );

    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn callback_timeout_is_completed_by_durable_worker_patch() {
    use crate::im::{
        IM_PAYLOAD_VERSION, ImDeliveryWorker, ImNotificationKind, ImRepository,
        ImResolutionDispatch, ImResolutionOutcome, ImResolvedBy,
        InterventionResolutionNotification,
    };
    let ((listener, connector, mut events, cancellation, task), requests) = start_with_http(
        json!({}),
        1,
        |_| {},
        vec![(200, json!({"code": 0, "data": {}}).to_string())],
    )
    .await;
    let mut server = accept(&listener).await;
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected { .. }
    ));
    send_frame(
        &mut server,
        &event_frame(25, "m-durable", &card_event("ev-durable")),
    )
    .await;
    let context = match next_event(&mut events).await {
        ImConnectorEvent::InboundAction {
            response_context, ..
        } => response_context,
        other => panic!("expected inbound, got {other:?}"),
    };
    let ack = next_data_frame(&mut server).await;
    let body: Value = serde_json::from_slice(ack.payload.as_deref().unwrap()).unwrap();
    let reply: Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(body["data"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["toast"]["content"], "Processing");
    assert!(reply.get("card").is_none());

    let temp = tempfile::tempdir().unwrap();
    let repository = Arc::new(ImRepository::new(
        camino::Utf8PathBuf::from_path_buf(temp.path().join("im.db")).unwrap(),
    ));
    let mut source = card::tests::delivery(
        ImNotificationKind::Permission,
        vec![
            crate::app::intervention::InterventionAllowedAction::PermissionOption {
                option_id: "allow".into(),
                name: "Allow".into(),
                permission_kind: crate::app::intervention::PermissionActionKind::AllowOnce,
            },
        ],
    );
    source.refresh_id();
    repository.enqueue(&source, 100).unwrap();
    let source = repository.delivery(&source.delivery_id).unwrap().unwrap();
    let claim = repository
        .claim_due(ImChannelKind::Feishu, 100, Duration::from_secs(30), 1)
        .unwrap()
        .pop()
        .unwrap();
    repository
        .mark_sent(
            &source.delivery_id,
            ImChannelKind::Feishu,
            claim.attempt_count,
            Some(&ImDeliveryBinding {
                channel: ImChannelKind::Feishu,
                delivery_id: source.delivery_id.clone(),
                platform_message_id: "om_card".into(),
                platform_chat_id: "oc_private".into(),
                update_token_ref: None,
            }),
            100,
        )
        .unwrap();
    let outcome = ImResolutionOutcome::PermissionOption {
        option_id: "allow".into(),
    };
    let resolved_by = ImResolvedBy::Channel(ImChannelKind::Feishu);
    let response = ImActionResponse {
        state: crate::im::ImActionResponseState::Handled,
        resolved_by: Some(resolved_by),
        outcome: Some(outcome.clone()),
        delivery: Some(source.clone()),
    };
    // The callback slot is gone, but the durable result must still be delivered.
    connector
        .respond_to_action(context, response.clone())
        .await
        .unwrap();
    let mut terminal = source.clone();
    terminal.canonical_event_id = crate::im::resolution_event_id(&source.canonical_event_id);
    terminal.refresh_id();
    terminal.payload = ImDeliveryPayload::InterventionResolution {
        version: IM_PAYLOAD_VERSION,
        resolution: InterventionResolutionNotification {
            canonical_event_id: terminal.canonical_event_id.clone(),
            source_event_id: source.canonical_event_id.clone(),
            notification_kind: source.notification_kind,
            locator: serde_json::from_value(json!({"projectId":"p"})).unwrap(),
            display_ref: source.display_ref,
            resolved_by,
            outcome,
        },
    };
    repository
        .enqueue_resolution_and_expire_pending_source(
            &source.canonical_event_id,
            &terminal,
            ImResolutionDispatch::Send,
            101,
        )
        .unwrap();
    let worker = ImDeliveryWorker::new(ImChannelKind::Feishu, repository, connector);
    assert_eq!(worker.dispatch_once(102).await.unwrap(), 1);
    let captured = requests.lock().unwrap().clone();
    assert_eq!(captured.len(), 4, "only the session setup and one PATCH");
    assert!(captured[3].starts_with("PATCH /open-apis/im/v1/messages/om_card "));
    let patch: Value = serde_json::from_str(captured[3].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let patched_card: Value = serde_json::from_str(patch["content"].as_str().unwrap()).unwrap();
    let expected = card::action_reply(&response, ImLocale::En, &test_codec());
    assert_eq!(patched_card, expected["card"]["data"]);
    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn card_callback_response_is_discarded_when_generation_changes() {
    let (listener, connector, mut events, cancellation, task) = start(json!({}), 1, |_| {}).await;
    let mut server = accept(&listener).await;
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected { .. }
    ));

    send_frame(
        &mut server,
        &event_frame(22, "m-generation", &card_event("ev-generation")),
    )
    .await;
    let response_context = match next_event(&mut events).await {
        ImConnectorEvent::InboundAction {
            response_context, ..
        } => response_context,
        other => panic!("expected inbound action, got {other:?}"),
    };
    connector.advance_generation(2);
    connector
        .respond_to_action(
            response_context,
            ImActionResponse {
                state: crate::im::ImActionResponseState::Handled,
                resolved_by: None,
                outcome: None,
                delivery: None,
            },
        )
        .await
        .unwrap();
    assert!(
        connector
            .pending_responses
            .lock()
            .unwrap()
            .get(&1)
            .is_none(),
        "an old generation must not retain callback replies"
    );

    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn silent_connection_schedules_a_reconnect_and_drops_the_send_handle() {
    let (listener, connector, mut events, cancellation, task) =
        start(json!({ "PingInterval": 1 }), 1, |_| {}).await;
    let _server = accept(&listener).await;
    assert!(matches!(
        next_event(&mut events).await,
        ImConnectorEvent::Connected { .. }
    ));
    assert!(connector.active_api().is_ok());

    assert_eq!(
        next_event(&mut events).await,
        ImConnectorEvent::ReconnectScheduled {
            generation: 1,
            error: network_unavailable(),
        }
    );
    assert!(connector.active_api().is_err());
    cancellation.cancel();
    assert_eq!(task.await.unwrap(), Ok(()));
}

#[tokio::test]
async fn stale_generation_never_installs_a_send_handle() {
    let (listener, connector, mut events, _cancellation, task) =
        start(json!({}), 1, |connector| connector.advance_generation(2)).await;
    let _server = accept(&listener).await;
    assert_eq!(task.await.unwrap(), Ok(()));
    assert!(
        events.try_recv().is_err(),
        "a stale session must not report Connected"
    );
    assert!(connector.active_api().is_err());
}

#[tokio::test]
async fn missing_credentials_fail_permanently_before_any_request() {
    let connector = FeishuConnector::new(test_codec());
    let mut config = config();
    config.credential_fields.remove(FEISHU_OPEN_ID_FIELD);
    let (events, _rx) = mpsc::channel(1);
    let error = connector
        .connect(config, 1, events, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ImErrorCode::CredentialUnavailable);
    assert!(!error.retryable);
}

#[test]
fn p2_capabilities_are_interactive_and_updatable() {
    let connector = FeishuConnector::new(test_codec());
    let capabilities = connector.capabilities();
    assert!(capabilities.proactive_delivery && capabilities.private_chat);
    assert!(capabilities.card_actions);
    assert_eq!(capabilities.message_update, ImMessageUpdateMode::Anytime);
    assert_eq!(capabilities.interaction.permission_max_options, 20);
    assert_eq!(capabilities.interaction.max_questions, 5);
    assert_eq!(
        capabilities.interaction.single_choice_max_options,
        ImOptionLimit {
            single_question: 20,
            multi_question: 20,
        }
    );
    assert_eq!(
        capabilities.interaction.multi_choice,
        Some(ImMultiChoiceCapability {
            max_options: 4,
            within_multi_question: true,
        })
    );
    assert_eq!(
        capabilities.interaction.free_text,
        Some(ImFreeTextCapability {
            max_chars: 500,
            multiline: true,
        })
    );
    assert_eq!(
        capabilities.interaction.custom_answer,
        ImCustomAnswerSupport::Supported { max_chars: 100 }
    );
    assert!(capabilities.interaction.decline);
    assert_eq!(connector.active_api().err(), Some(network_unavailable()));
}
