use std::io::{BufRead, Write};
use std::time::Duration;

use camino::Utf8PathBuf;
use gold_band::acp::connection::{AdapterConnection, AdapterShutdownReason};
use gold_band::config::AcpAdapterConfig;
use serde_json::{Value, json};

const BACKGROUND_ENV: &str = "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS";

fn with_adapter(provider: &str, check: impl FnOnce(&AdapterConnection)) {
    let temp = tempfile::tempdir().unwrap();
    let config = AcpAdapterConfig {
        command: std::env::current_exe()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
        args: ["--ignored", "--exact", "adapter_fixture", "--nocapture"]
            .map(str::to_string)
            .to_vec(),
        // The display name must not determine the policy.
        display_name: "Claude ACP".to_string(),
        env: [
            ("GOLD_BAND_POLICY_FIXTURE".to_string(), "1".to_string()),
            (BACKGROUND_ENV.to_string(), "0".to_string()),
            ("DIAGNOSTIC_SENTINEL".to_string(), "preserved".to_string()),
        ]
        .into(),
    };
    let original = serde_json::to_value(&config).unwrap();
    let connection = AdapterConnection::spawn_standalone(
        provider,
        &config,
        &Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap(),
        false,
        false,
    )
    .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&connection)));
    connection.shutdown(AdapterShutdownReason::StandaloneRelease);
    assert_eq!(serde_json::to_value(config).unwrap(), original);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn request(connection: &AdapterConnection, method: &str, params: Value) -> Value {
    let request = connection.begin_request(method, params).unwrap();
    request.recv_timeout(Duration::from_secs(10)).unwrap()["result"].clone()
}

#[test]
fn claude_launch_disables_background_without_rewriting_user_configuration() {
    with_adapter("claude-acp", |connection| {
        let result = request(connection, "fixture/environment", json!({}));
        assert_eq!(result["background"], "1");
        assert_eq!(result["diagnostic"], "preserved");
    });
}

#[test]
fn claude_session_creation_and_restore_disable_monitor_and_preserve_options() {
    with_adapter("claude-acp", |connection| {
        for method in [
            "session/new",
            "session/load",
            "session/resume",
            "session/fork",
        ] {
            let params = json!({
                "cwd": "/fixture", "sessionId": "original", "mcpServers": [],
                "_meta": {
                    "systemPrompt": {"append": "stable rules"},
                    "claudeCode": {"options": {
                        "disallowedTools": ["WebSearch"], "maxTurns": 7,
                        "env": {"DIAGNOSTIC_SENTINEL": "session", BACKGROUND_ENV: "0"},
                        "settings": {"env": {"OTHER": "keep", BACKGROUND_ENV: "0"}}
                    }}
                }
            });
            let result = request(connection, method, params);
            let params = &result["params"];
            let options = &params["_meta"]["claudeCode"]["options"];
            assert_eq!(
                options["disallowedTools"],
                json!(["WebSearch", "Monitor"]),
                "{method}"
            );
            assert_eq!(options["maxTurns"], 7);
            assert_eq!(options["env"]["DIAGNOSTIC_SENTINEL"], "session");
            assert_eq!(options["env"][BACKGROUND_ENV], "1");
            assert_eq!(options["settings"]["env"][BACKGROUND_ENV], "1");
            assert_eq!(options["settings"]["env"]["OTHER"], "keep");
            assert_eq!(params["_meta"]["systemPrompt"]["append"], "stable rules");
            assert_eq!(params["sessionId"], "original");
            let again = request(connection, method, params.clone());
            assert_eq!(again["params"], *params, "policy must be idempotent");
        }
        let result = request(
            connection,
            "session/new",
            json!({"cwd": "/fixture", "mcpServers": []}),
        );
        assert_eq!(
            result["params"]["_meta"]["claudeCode"]["options"]["disallowedTools"],
            json!(["Monitor"])
        );
        let params = json!({"sessionId": "original", "prompt": []});
        assert_eq!(
            request(connection, "session/prompt", params.clone())["params"],
            params
        );
    });
}

#[test]
fn other_agents_do_not_receive_claude_runtime_restrictions() {
    with_adapter("codex-acp", |connection| {
        let params = json!({"cwd": "/fixture", "mcpServers": []});
        let result = request(connection, "session/new", params.clone());
        assert_eq!(result["background"], "0");
        assert_eq!(result["params"], params);
    });
}

#[test]
fn malformed_claude_options_fail_before_transport_without_poisoning_connection() {
    with_adapter("claude-acp", |connection| {
        for options in [json!({"disallowedTools": "Monitor"}), json!({"env": []})] {
            let result = connection.begin_request(
                "session/new",
                json!({
                    "_meta": {"claudeCode": {"options": options}}
                }),
            );
            let error = result.err().expect("invalid options must be rejected");
            assert!(
                error
                    .to_string()
                    .starts_with("acp.invalid-session-options:")
            );
        }
        assert_eq!(
            request(connection, "fixture/environment", json!({}))["background"],
            "1"
        );
    });
}

#[test]
#[ignore]
fn installed_claude_policy_probe() {
    let Ok(adapter_entry) = std::env::var("GOLD_BAND_POLICY_ADAPTER_ENTRY") else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    std::fs::create_dir_all(workspace.join(".claude")).unwrap();
    std::fs::write(
        workspace.join(".claude/settings.json"),
        json!({"env": {BACKGROUND_ENV: "0"}}).to_string(),
    )
    .unwrap();
    let config = AcpAdapterConfig {
        command: std::env::var("GOLD_BAND_POLICY_NODE").unwrap(),
        args: vec![adapter_entry],
        display_name: "Isolated installed adapter probe".to_string(),
        env: [
            (BACKGROUND_ENV.to_string(), "0".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                workspace.join("config").to_string(),
            ),
            (
                "ANTHROPIC_BASE_URL".to_string(),
                std::env::var("GOLD_BAND_POLICY_API").unwrap(),
            ),
            (
                "ANTHROPIC_API_KEY".to_string(),
                "local-fixture-only".to_string(),
            ),
            (
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                "local-fixture-only".to_string(),
            ),
            ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), String::new()),
            (
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
                "1".to_string(),
            ),
            ("CLAUDE_CODE_USE_BEDROCK".to_string(), "0".to_string()),
            ("CLAUDE_CODE_USE_VERTEX".to_string(), "0".to_string()),
            ("CLAUDE_CODE_USE_FOUNDRY".to_string(), "0".to_string()),
        ]
        .into(),
    };
    let connection =
        AdapterConnection::spawn_standalone("claude-acp", &config, &workspace, false, false)
            .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        request(
            &connection,
            "initialize",
            json!({"protocolVersion": 1,
            "clientCapabilities": {}, "clientInfo": {"name": "policy-probe", "version": "1"}}),
        );
        for mode in ["synchronous", "background", "monitor", "subagent"] {
            let created = request(
                &connection,
                "session/new",
                json!({"cwd": workspace,
                "mcpServers": [], "_meta": {"claudeCode": {"options": {
                    "tools": ["Bash", "Monitor", "Agent"], "permissionMode": "bypassPermissions",
                    "allowDangerouslySkipPermissions": true, "persistSession": false,
                    "model": "claude-sonnet-4-6", "maxTurns": 3
                }}}}),
            );
            let session_id = created["sessionId"].as_str().expect("session/new failed");
            let prompt = connection
                .begin_request(
                    "session/prompt",
                    json!({"sessionId": session_id,
                "prompt": [{"type": "text", "text": format!("POLICY_PROBE:{mode}")}]}),
                )
                .unwrap();
            let response = prompt.recv_timeout(Duration::from_secs(90)).unwrap();
            assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
            request(
                &connection,
                "session/close",
                json!({"sessionId": session_id}),
            );
        }
        assert!(!workspace.join("monitor-must-not-run").exists());
    }));
    connection.shutdown(AdapterShutdownReason::StandaloneRelease);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
#[ignore]
fn adapter_fixture() {
    if std::env::var("GOLD_BAND_POLICY_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    println!();
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!(
            "{}",
            json!({"jsonrpc": "2.0", "id": frame["id"], "result": {
                "params": frame["params"],
                "background": std::env::var(BACKGROUND_ENV).ok(),
                "diagnostic": std::env::var("DIAGNOSTIC_SENTINEL").ok(),
            }})
        );
        std::io::stdout().flush().unwrap();
    }
}
