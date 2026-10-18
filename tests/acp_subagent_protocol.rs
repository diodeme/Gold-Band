//! Opt-in, local-model-only probe for the official Claude ACP adapter.
//! Run through scripts/diagnostics/verify-claude-subagents.mjs, never directly.
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use gold_band::acp::connection::{AdapterConnection, AdapterShutdownReason};
use gold_band::config::AcpAdapterConfig;
use serde_json::{Value, json};

fn request(connection: &AdapterConnection, method: &str, params: Value) -> Value {
    let response = connection
        .begin_request(method, params)
        .unwrap()
        .recv_timeout(Duration::from_secs(45))
        .unwrap();
    assert!(response.get("error").is_none(), "{method}: {response}");
    response["result"].clone()
}

#[test]
#[ignore = "requires the isolated official adapter and local HTTP fixture"]
fn installed_claude_subagent_protocol_probe() {
    let root = std::env::var("GOLD_BAND_SUBAGENT_ROOT").expect("run the diagnostic script");
    let api = std::env::var("GOLD_BAND_SUBAGENT_API").unwrap();
    assert!(
        api.starts_with("http://127.0.0.1:"),
        "local fixture required"
    );
    for mode in std::env::var("GOLD_BAND_SUBAGENT_MODES")
        .unwrap()
        .split(',')
    {
        // Outside the repository so ancestor project instructions cannot leak in.
        let temporary = tempfile::tempdir().unwrap();
        let workspace = Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).unwrap();
        let config = AcpAdapterConfig {
            command: std::env::var("GOLD_BAND_SUBAGENT_NODE").unwrap(),
            args: vec![
                std::env::var("GOLD_BAND_SUBAGENT_SCRIPT").unwrap(),
                "--relay".into(),
                std::env::var("GOLD_BAND_SUBAGENT_ENTRY").unwrap(),
                workspace.to_string(),
                mode.into(),
            ],
            display_name: "Isolated subagent protocol probe".into(),
            env: [
                ("GOLD_BAND_SUBAGENT_API".into(), api.clone()),
                ("GOLD_BAND_SUBAGENT_ROOT".into(), root.clone()),
            ]
            .into(),
        };
        let connection = AdapterConnection::spawn_standalone(
            "protocol-probe",
            &config,
            &workspace,
            false,
            false,
        )
        .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Deliberately send initialize by hand, without initialize_once: this
            // probes the adapter protocol, not Gold Band's existing no-background
            // execution policy. Production capability negotiation is unchanged.
            request(
                &connection,
                "initialize",
                json!({"protocolVersion": 1,
                "clientCapabilities": if mode == "off" { json!({}) } else { json!({"subagents": {}}) },
                "clientInfo": {"name": "gold-band-isolated-probe", "version": "1"}}),
            );
            let created = request(
                &connection,
                "session/new",
                json!({"cwd": workspace,
                "mcpServers": [], "_meta": {"claudeCode": {"options": {
                    "tools": ["Agent", "SendMessage", "Bash"], "settingSources": [],
                    "allowedTools": ["Agent", "SendMessage"],
                    "settings": if mode == "permission" { json!({"permissions": {"ask": ["Bash"]}}) } else { json!({}) },
                    "permissionMode": if mode == "permission" { "default" } else { "bypassPermissions" },
                    "allowDangerouslySkipPermissions": true, "persistSession": matches!(mode, "resume" | "replay"),
                    "model": "claude-sonnet-4-6", "maxTurns": 6,
                    "thinking": {"type": "disabled"}
                }}}}),
            );
            let session = created["sessionId"]
                .as_str()
                .expect("session/new missing id");
            let mut routes = vec![connection.register_session_route(session)];
            let mut children = Vec::<String>::new();
            let prompt = connection
                .begin_request(
                    "session/prompt",
                    json!({"sessionId": session,
                "prompt": [{"type": "text", "text": format!("SUBAGENT_PROBE:{mode}")}]}),
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut cancelled = false;
            let mut permissions = 0;
            loop {
                let mut discovered = Vec::new();
                for route in &routes {
                    while let Ok(frame) = route.try_recv() {
                        if frame["params"]["update"]["sessionUpdate"] == "subagent_update" {
                            let child = frame["params"]["update"]["sessionId"]
                                .as_str()
                                .unwrap()
                                .to_owned();
                            if !children.contains(&child) && !discovered.contains(&child) {
                                discovered.push(child);
                            }
                        }
                        if frame["method"] == "session/request_permission" {
                            assert_ne!(
                                frame["params"]["sessionId"], session,
                                "permission must belong to child"
                            );
                            permissions += 1;
                            connection
                                .send_response(
                                    frame["id"].clone(),
                                    json!({"outcome": {"outcome": "cancelled"}}),
                                )
                                .unwrap();
                        }
                    }
                }
                for child in discovered {
                    routes.push(connection.register_session_route(&child));
                    children.push(child);
                }
                if mode == "cancel" && !children.is_empty() && !cancelled {
                    connection
                        .send_notification("session/cancel", json!({"sessionId": session}))
                        .unwrap();
                    cancelled = true;
                }
                match prompt.recv_timeout(Duration::from_millis(20)) {
                    Ok(response) => {
                        assert_eq!(
                            response["result"]["stopReason"],
                            if mode == "cancel" {
                                "cancelled"
                            } else {
                                "end_turn"
                            },
                            "{mode}: {response}"
                        );
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        assert!(Instant::now() < deadline, "{mode}: prompt timed out")
                    }
                    Err(error) => panic!("{mode}: prompt disconnected: {error}"),
                }
            }
            if mode == "permission" {
                assert!(permissions > 0, "missing child permission request");
            }
            request(&connection, "session/close", json!({"sessionId": session}));
            if mode == "replay" {
                request(
                    &connection,
                    "session/load",
                    json!({"sessionId": session,
                    "cwd": workspace, "mcpServers": [], "_meta": {"claudeCode": {"options": {
                        "settingSources": [], "tools": ["Agent"], "persistSession": true,
                        "model": "claude-sonnet-4-6", "thinking": {"type": "disabled"}
                    }}}}),
                );
                request(&connection, "session/close", json!({"sessionId": session}));
            }
        }));
        connection.shutdown(AdapterShutdownReason::StandaloneRelease);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}
