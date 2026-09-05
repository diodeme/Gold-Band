use std::io::{BufRead, Write};
use std::sync::Mutex;
use std::time::Duration;

use camino::Utf8PathBuf;
use gold_band::acp::connection::{
    AdapterConnectionKey, AdapterConnectionManager, AdapterShutdownReason,
};
use gold_band::config::AcpAdapterConfig;
use serde_json::json;

fn fixture_config() -> AcpAdapterConfig {
    AcpAdapterConfig {
        command: std::env::current_exe()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
        args: ["--ignored", "--exact", "adapter_fixture", "--nocapture"]
            .map(str::to_string)
            .to_vec(),
        display_name: "Connection logging fixture".to_string(),
        env: [(
            "GOLD_BAND_CONNECTION_LOG_FIXTURE".to_string(),
            "1".to_string(),
        )]
        .into_iter()
        .collect(),
    }
}

#[test]
fn idle_close_is_diagnosable_at_info_level_without_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("runtime.log");
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(std::fs::File::create(&log_path).unwrap()))
        .init();
    let manager = AdapterConnectionManager::default();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let connection = manager
        .get_or_spawn(
            "fixture-provider",
            &fixture_config(),
            workspace.clone(),
            false,
            false,
        )
        .unwrap();
    let pid = connection.pid();
    let generation = connection.generation();
    let pending = connection
        .begin_request(
            "session/resume",
            json!({"sessionId": "fixture-session", "secret": "DO_NOT_LOG_PAYLOAD"}),
        )
        .unwrap();
    manager.prune_idle_connections(Duration::ZERO, usize::MAX);
    assert!(connection.is_transport_closed());
    assert!(pending.recv_timeout(Duration::from_secs(1)).is_err());
    assert!(
        connection
            .begin_request("session/prompt", json!({"prompt": "DO_NOT_LOG_PAYLOAD"}))
            .is_err()
    );

    let log = std::fs::read_to_string(&log_path).unwrap();
    let close = log
        .lines()
        .find(|line| line.contains("event=\"acp_connection_closed\""))
        .expect("idle eviction must leave a closure cause in the default INFO log");
    for field in [
        "reason=\"idle-ttl\"".to_string(),
        format!("pid={pid}"),
        format!("connection_generation={generation}"),
        "pending_requests=1".to_string(),
        "active_prompts=0".to_string(),
        "session/resume".to_string(),
    ] {
        assert!(close.contains(&field), "missing {field}: {close}");
    }
    assert!(!log.contains("DO_NOT_LOG_PAYLOAD"));

    let spawn = |provider: &str| {
        manager
            .get_or_spawn(provider, &fixture_config(), workspace.clone(), false, false)
            .unwrap()
    };
    let capacity = spawn("capacity");
    let requests = (0..12)
        .map(|_| capacity.begin_request("session/resume", json!({})).unwrap())
        .collect::<Vec<_>>();
    manager.prune_idle_connections(Duration::from_secs(600), 0);
    drop(requests);
    let workspace_close = spawn("workspace-close");
    let active_prompt = workspace_close.begin_prompt("active-fixture").unwrap();
    let route = workspace_close.register_session_route("active-fixture");
    manager
        .close_workspace_connections_bounded(&workspace, Duration::from_secs(1))
        .unwrap();
    drop(active_prompt);
    drop(route);
    let provider_close = spawn("provider-close");
    manager
        .close_provider_connections_bounded("provider-close", Duration::from_secs(1))
        .unwrap();
    let all_close = spawn("all-close");
    all_close
        .close_session_bounded("fixture-session", Duration::from_secs(5))
        .unwrap();
    manager
        .close_all_connections_bounded(Duration::from_secs(1))
        .unwrap();
    let initialization = spawn("initialization");
    assert!(manager.evict_if_current(
        &AdapterConnectionKey::new("initialization", workspace.clone()),
        &initialization,
        AdapterShutdownReason::InitializationFailed,
    ));

    let config_changed = spawn("config-changed");
    let mut config = fixture_config();
    config
        .env
        .insert("CONFIG_REVISION".to_string(), "2".to_string());
    let replacement = manager
        .get_or_spawn("config-changed", &config, workspace.clone(), false, false)
        .unwrap();
    assert_ne!(config_changed.generation(), replacement.generation());
    replacement.shutdown(AdapterShutdownReason::StandaloneRelease);

    let eof = spawn("unexpected-exit");
    let request = eof.begin_request("fixture/exit", json!({})).unwrap();
    assert!(matches!(
        request.recv_timeout(Duration::from_secs(5)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while eof.try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < exit_deadline,
            "fixture did not exit"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    eof.shutdown(AdapterShutdownReason::StandaloneRelease);

    let log = std::fs::read_to_string(&log_path).unwrap();
    for (connection, reason) in [
        (&capacity, "idle-capacity"),
        (&workspace_close, "workspace-close"),
        (&provider_close, "provider-close"),
        (&all_close, "all-connections-close"),
        (&initialization, "initialization-failed"),
        (&config_changed, "config-changed"),
        (&replacement, "standalone-release"),
        (&eof, "stdout-eof"),
    ] {
        let identity = format!("connection_generation={}", connection.generation());
        let events = log
            .lines()
            .filter(|line| {
                line.contains(&identity) && line.contains("event=\"acp_connection_closed\"")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            events.len(),
            1,
            "one first closure per connection: {events:?}"
        );
        assert!(
            events[0].contains(&format!("reason=\"{reason}\"")),
            "{}",
            events[0]
        );
    }
    let eof_identity = format!("connection_generation={}", eof.generation());
    assert!(log.lines().any(|line| line.contains(&eof_identity)
        && line.contains("event=\"acp_connection_close_observed\"")
        && line.contains("reason=\"standalone-release\"")));
    assert!(log.lines().any(|line| line.contains(&eof_identity)
        && line.contains("event=\"acp_adapter_exit_status\"")
        && line.contains("exit_code=23")));
    assert!(log.contains("event=\"acp_connection_request_rejected\""));
    assert!(log.contains("event=\"acp_session_close_requested\""));
    let capacity_identity = format!("connection_generation={}", capacity.generation());
    let capacity_snapshot = log
        .lines()
        .find(|line| {
            line.contains(&capacity_identity)
                && line.contains("pending_requests=12")
                && line.contains("pending_methods_truncated=true")
        })
        .unwrap();
    assert_eq!(capacity_snapshot.matches("session/resume").count(), 8);
    let workspace_identity = format!("connection_generation={}", workspace_close.generation());
    assert!(log.lines().any(|line| line.contains(&workspace_identity)
        && line.contains("event=\"acp_connection_draining\"")
        && line.contains("active_prompts=1")
        && line.contains("session_routes=1")));
    assert!(!log.contains("DO_NOT_LOG_PAYLOAD"));
}

// The adapter is another copy of this test binary, not an installed provider.
#[test]
#[ignore]
fn adapter_fixture() {
    if std::env::var("GOLD_BAND_CONNECTION_LOG_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    println!();
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let frame: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        if frame["method"] == "fixture/exit" {
            std::process::exit(23);
        }
        if frame["method"] == "session/resume" {
            continue;
        }
        println!(
            "{}",
            json!({"jsonrpc": "2.0", "id": frame["id"], "result": {}})
        );
        std::io::stdout().flush().unwrap();
    }
}
