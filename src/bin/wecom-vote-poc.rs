use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use gold_band::app::App;
use gold_band::im::connectors::wecom::WECOM_WEBSOCKET_ENDPOINT;
use gold_band::im::{ImChannelKind, ImCredentialStore, OsImCredentialStore};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

const WECOM_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const POC_WAIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const WECOM_SECRET_FIELD: &str = "secret";
const QUESTION_KEY: &str = "permission_choice";
const SENSITIVE_JSON_KEYS: [&str; 7] = [
    "aibotid",
    "bot_id",
    "chatid",
    "msgid",
    "req_id",
    "response_url",
    "userid",
];

#[derive(Debug, Parser)]
struct Args {
    /// Directory for redacted callback JSON evidence.
    #[arg(long)]
    capture_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let current_dir = Utf8PathBuf::from_path_buf(std::env::current_dir()?)
        .map_err(|_| anyhow::anyhow!("working directory must be valid UTF-8"))?;
    let app = App::new(current_dir);
    let settings = app.load_settings()?;
    let channel = settings
        .im_integrations
        .channels
        .iter()
        .find(|channel| channel.kind == ImChannelKind::WeCom)
        .context("WeCom channel is not configured")?;
    if !channel.enabled {
        bail!("WeCom channel is disabled");
    }
    let binding = channel
        .binding
        .as_ref()
        .context("WeCom channel has no private binding")?;
    let credential_ref = channel
        .credential_ref
        .as_deref()
        .context("WeCom channel has no credential reference")?;
    let credentials = OsImCredentialStore
        .load(ImChannelKind::WeCom, credential_ref)
        .map_err(|error| anyhow::anyhow!(error.code()))?;
    let secret = credentials
        .fields
        .get(WECOM_SECRET_FIELD)
        .context("WeCom credential is missing its secret field")?;

    let capture_dir = args
        .capture_dir
        .unwrap_or_else(|| std::env::temp_dir().join("gold-band-wecom-vote-interaction-poc"));
    std::fs::create_dir_all(&capture_dir).context("create WeCom PoC capture directory")?;

    let mobile_task_id = poc_task_id("mobile");
    let pc_task_id = poc_task_id("pc");
    let mobile_card = vote_poc_card(&mobile_task_id, "手机端");
    let pc_card = vote_poc_card(&pc_task_id, "PC 端");

    let (mut socket, _) = tokio_tungstenite::connect_async(WECOM_WEBSOCKET_ENDPOINT)
        .await
        .context("connect to WeCom WebSocket")?;
    subscribe(&mut socket, &channel.public_identity, secret).await?;
    send_poc_card(&mut socket, &binding.conversation_id, mobile_card.clone()).await?;
    send_poc_card(&mut socket, &binding.conversation_id, pc_card.clone()).await?;
    println!("Both vote_interaction PoC cards were sent.");
    println!("Select different options on mobile and PC, then submit each card.");
    println!(
        "Redacted callbacks will be written under {}",
        capture_dir.display()
    );

    let expected_tasks = HashSet::from([mobile_task_id.clone(), pc_task_id.clone()]);
    let mut completed_tasks = HashSet::new();
    let mut update_requests = HashSet::new();
    let deadline = tokio::time::Instant::now() + POC_WAIT_TIMEOUT;

    while completed_tasks != expected_tasks {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for both PoC callbacks");
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .context("wait for WeCom PoC callback")?
            .context("WeCom WebSocket closed")?
            .context("read WeCom WebSocket frame")?;
        let Some(value) = parse_json_message(&message)? else {
            continue;
        };
        if let Some(req_id) = value
            .pointer("/headers/req_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            && update_requests.remove(&req_id)
        {
            assert_platform_success(&value)?;
            continue;
        }
        if value.get("cmd").and_then(Value::as_str) != Some("aibot_event_callback")
            || value
                .pointer("/body/event/eventtype")
                .and_then(Value::as_str)
                != Some("template_card_event")
        {
            continue;
        }

        let Some(task_id) = callback_task_id(&value) else {
            println!("Unroutable template callback:\n{}", redact_json(&value));
            continue;
        };
        if !expected_tasks.contains(&task_id) || completed_tasks.contains(&task_id) {
            continue;
        }

        let redacted = redact_json(&value);
        let capture_path = capture_dir.join(format!("{task_id}.json"));
        std::fs::write(&capture_path, serde_json::to_vec_pretty(&redacted)?)?;
        println!(
            "Captured redacted callback for {task_id}: {}",
            capture_path.display()
        );
        println!("{}", redacted);

        let callback_req_id = value
            .pointer("/headers/req_id")
            .and_then(Value::as_str)
            .context("callback has no req_id")?
            .to_owned();
        let selected_option_id = callback_selected_option_id(&value)
            .with_context(|| "callback has no unique selected option for the vote question")?;
        let update = vote_poc_update(&task_id, &selected_option_id);
        socket
            .send(Message::Text(
                json!({
                    "cmd": "aibot_respond_update_msg",
                    "headers": { "req_id": callback_req_id },
                    "body": {
                        "response_type": "update_template_card",
                        "template_card": update,
                    },
                })
                .to_string()
                .into(),
            ))
            .await
            .context("send WeCom PoC card update")?;
        update_requests.insert(callback_req_id);
        completed_tasks.insert(task_id);
    }

    while !update_requests.is_empty() {
        let message = tokio::time::timeout(WECOM_REQUEST_TIMEOUT, socket.next())
            .await
            .context("wait for WeCom PoC update ACK")?
            .context("WeCom WebSocket closed")?
            .context("read WeCom WebSocket frame")?;
        let Some(value) = parse_json_message(&message)? else {
            continue;
        };
        let Some(req_id) = value
            .pointer("/headers/req_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if update_requests.remove(&req_id) {
            assert_platform_success(&value)?;
        }
    }

    println!("Both callbacks and vote_interaction updates completed.");
    Ok(())
}

fn poc_task_id(device: &str) -> String {
    format!(
        "goldband_vote_poc_{}_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        device
    )
}

fn vote_poc_card(task_id: &str, device: &str) -> Value {
    json!({
        "chatid": "", // replaced by the caller with the bound private conversation
        "msgtype": "template_card",
        "template_card": {
            "card_type": "vote_interaction",
            "source": { "desc": "Gold Band PoC" },
            "main_title": {
                "title": format!("权限审批 PoC（{device}）"),
                "desc": "验证 vote_interaction 回调契约"
            },
            "sub_title_text": "请选择不同选项后提交",
            "horizontal_content_list": [
                { "keyname": "用途", "value": "协议验证" },
                { "keyname": "端", "value": device },
            ],
            "checkbox": {
                "question_key": QUESTION_KEY,
                "mode": 0,
                "option_list": [
                    { "id": "0", "text": "拒绝", "is_checked": true },
                    { "id": "1", "text": "允许一次" },
                    { "id": "2", "text": "本会话允许" },
                ],
            },
            "submit_button": {
                "text": "提交",
                "key": format!("{task_id}:submit"),
            },
            "task_id": task_id,
        },
    })
}

fn vote_poc_update(task_id: &str, selected_option_id: &str) -> Value {
    let options = ["0", "1", "2"]
        .into_iter()
        .map(|option_id| {
            json!({
                "id": option_id,
                "text": match option_id {
                    "0" => "拒绝",
                    "1" => "允许一次",
                    _ => "本会话允许",
                },
                "is_checked": option_id == selected_option_id,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "card_type": "vote_interaction",
        "main_title": {
            "title": "PoC 已提交",
            "desc": "回调契约已捕获"
        },
        "checkbox": {
            "question_key": QUESTION_KEY,
            "mode": 0,
            "disable": true,
            "option_list": options,
        },
        "submit_button": {
            "text": "已提交",
            "key": format!("{task_id}:submit"),
        },
        "task_id": task_id,
    })
}

async fn subscribe<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    bot_id: &str,
    secret: &str,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let req_id = Uuid::new_v4().to_string();
    socket
        .send(Message::Text(
            json!({
                "cmd": "aibot_subscribe",
                "headers": { "req_id": req_id.clone() },
                "body": { "bot_id": bot_id, "secret": secret },
            })
            .to_string()
            .into(),
        ))
        .await
        .context("send WeCom subscribe frame")?;
    let response = wait_matching_request(socket, &req_id).await?;
    assert_platform_success(&response)
}

async fn send_poc_card<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    conversation_id: &str,
    mut body: Value,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    body["chatid"] = Value::String(conversation_id.to_owned());
    let req_id = Uuid::new_v4().to_string();
    socket
        .send(Message::Text(
            json!({
                "cmd": "aibot_send_msg",
                "headers": { "req_id": req_id.clone() },
                "body": body,
            })
            .to_string()
            .into(),
        ))
        .await
        .context("send WeCom vote_interaction PoC card")?;
    let response = wait_matching_request(socket, &req_id).await?;
    assert_platform_success(&response)
}

async fn wait_matching_request<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    req_id: &str,
) -> Result<Value>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let deadline = tokio::time::Instant::now() + WECOM_REQUEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            bail!("timed out waiting for WeCom request response");
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .context("wait for WeCom request response")?
            .context("WeCom WebSocket closed")?
            .context("read WeCom WebSocket frame")?;
        let Some(value) = parse_json_message(&message)? else {
            continue;
        };
        if value.pointer("/headers/req_id").and_then(Value::as_str) == Some(req_id) {
            return Ok(value);
        }
    }
}

fn assert_platform_success(value: &Value) -> Result<()> {
    let code = value
        .get("errcode")
        .and_then(Value::as_i64)
        .context("WeCom response has no top-level errcode")?;
    if code == 0 {
        return Ok(());
    }
    bail!("WeCom platform error {code}");
}

fn parse_json_message(message: &Message) -> Result<Option<Value>> {
    match message {
        Message::Text(text) => serde_json::from_str(text.as_str())
            .map(Some)
            .with_context(|| "parse WeCom text frame"),
        Message::Binary(bytes) => serde_json::from_slice(bytes)
            .map(Some)
            .with_context(|| "parse WeCom binary frame"),
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_) => Ok(None),
    }
}

fn callback_task_id(value: &Value) -> Option<String> {
    value
        .pointer("/body/event/template_card_event/task_id")
        .or_else(|| value.pointer("/body/event/task_id"))
        .and_then(Value::as_str)
        .filter(|task_id| !task_id.trim().is_empty())
        .map(str::to_owned)
}

fn callback_selected_option_id(value: &Value) -> Option<String> {
    let selections = value
        .pointer("/body/event/template_card_event/selected_items/selected_item")?
        .as_array()?
        .iter()
        .filter(|item| item.get("question_key").and_then(Value::as_str) == Some(QUESTION_KEY))
        .filter_map(|item| {
            let option_ids = item.pointer("/option_ids/option_id")?.as_array()?;
            if option_ids.len() != 1 {
                return None;
            }
            option_ids
                .first()
                .and_then(Value::as_str)
                .filter(|option_id| !option_id.trim().is_empty())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    if selections.len() == 1 {
        selections.into_iter().next()
    } else {
        None
    }
}

fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let redacted = if SENSITIVE_JSON_KEYS.contains(&key.as_str()) {
                        Value::String("<redacted>".to_owned())
                    } else {
                        redact_json(value)
                    };
                    (key.clone(), redacted)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poc_card_uses_official_vote_interaction_shape() {
        let card = vote_poc_card("goldband_vote_poc_test", "手机端");
        let template_card = &card["template_card"];
        assert_eq!(template_card["card_type"], "vote_interaction");
        assert_eq!(template_card["checkbox"]["question_key"], QUESTION_KEY);
        assert_eq!(template_card["checkbox"]["mode"], 0);
        assert_eq!(
            template_card["checkbox"]["option_list"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            template_card["submit_button"]["key"],
            "goldband_vote_poc_test:submit"
        );
        assert_eq!(template_card["task_id"], "goldband_vote_poc_test");
        assert!(template_card.get("button_list").is_none());
    }

    #[test]
    fn poc_update_remains_a_disabled_vote_card() {
        let update = vote_poc_update("goldband_vote_poc_test", "1");
        assert_eq!(update["card_type"], "vote_interaction");
        assert_eq!(update["checkbox"]["disable"], true);
        assert_eq!(update["checkbox"]["option_list"][0]["is_checked"], false);
        assert_eq!(update["checkbox"]["option_list"][1]["is_checked"], true);
        assert_eq!(update["checkbox"]["option_list"][2]["is_checked"], false);
        assert_eq!(
            update["submit_button"]["key"],
            "goldband_vote_poc_test:submit"
        );
        assert_eq!(update["task_id"], "goldband_vote_poc_test");
    }

    #[test]
    fn captured_callback_redacts_known_identity_fields() {
        let redacted = redact_json(&json!({
            "headers": { "req_id": "secret-request" },
            "body": {
                "aibotid": "bot-identity",
                "msgid": "platform-event",
                "response_url": "https://example.test/callback",
                "from": { "userid": "actor" },
                "event": { "template_card_event": { "task_id": "task" } }
            }
        }));
        assert_eq!(redacted["headers"]["req_id"], "<redacted>");
        assert_eq!(redacted["body"]["msgid"], "<redacted>");
        assert_eq!(redacted["body"]["from"]["userid"], "<redacted>");
        assert_eq!(redacted["body"]["aibotid"], "<redacted>");
        assert_eq!(redacted["body"]["response_url"], "<redacted>");
        assert_eq!(
            redacted["body"]["event"]["template_card_event"]["task_id"],
            "task"
        );
    }

    #[test]
    fn callback_selection_requires_one_matching_question_and_option() {
        let callback = json!({
            "body": {
                "event": {
                    "template_card_event": {
                        "selected_items": {
                            "selected_item": [
                                {
                                    "question_key": QUESTION_KEY,
                                    "option_ids": { "option_id": ["2"] }
                                }
                            ]
                        }
                    }
                }
            }
        });
        assert_eq!(callback_selected_option_id(&callback).as_deref(), Some("2"));
    }
}
