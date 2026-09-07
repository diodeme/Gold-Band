use std::io::{BufRead, Write};
use std::sync::{Arc, Barrier, Mutex};
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
fn acquired_connection_survives_idle_cleanup_before_resume() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let manager = AdapterConnectionManager::default();
    let mut config = fixture_config();
    config
        .env
        .insert("GOLD_BAND_TEST_RESUME".into(), "1".into());
    let initial = manager
        .get_or_spawn("lease-regression", &config, workspace.clone(), false, false)
        .unwrap();
    initial.initialize_once(|| Ok(json!({}))).unwrap();
    let observer = Arc::clone(&initial);
    drop(initial);
    let resolution = manager
        .get_or_spawn_with_outcome("lease-regression", &config, workspace, false, false)
        .unwrap();
    assert_eq!(resolution.outcome.as_str(), "reused");
    let connection = resolution.connection;
    assert_eq!(connection.generation(), observer.generation());
    assert!(
        !connection
            .initialize_once(|| panic!("must reuse initialization"))
            .unwrap()
            .performed
    );
    // No prompt or session binding exists yet, just like the restore window.
    manager.prune_idle_connections(Duration::ZERO, 0);
    let survived = !connection.is_transport_closed();
    if !survived {
        connection.shutdown(AdapterShutdownReason::StandaloneRelease);
    }
    assert!(
        survived,
        "idle cleanup closed a connection already handed to its caller"
    );
    let response = connection
        .begin_request("session/resume", json!({"sessionId":"existing-session"}))
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(response.get("result").is_some());
    drop(connection);
    manager.prune_idle_connections(Duration::ZERO, 0);
    assert!(
        observer.is_transport_closed(),
        "released connection must remain reclaimable"
    );
}

#[test]
fn concurrent_borrowers_block_capacity_eviction_until_last_drop() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let manager = Arc::new(AdapterConnectionManager::default());
    let config = fixture_config();
    let first = manager
        .get_or_spawn("borrowers", &config, workspace.clone(), false, false)
        .unwrap();
    let observer = Arc::clone(&first);
    let borrowed = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let worker = {
        let manager = Arc::clone(&manager);
        let borrowed = Arc::clone(&borrowed);
        let release = Arc::clone(&release);
        std::thread::spawn(move || {
            let second = manager
                .get_or_spawn("borrowers", &config, workspace, false, false)
                .unwrap();
            borrowed.wait();
            release.wait();
            drop(second);
        })
    };
    borrowed.wait();
    drop(first);
    manager.prune_idle_connections(Duration::from_secs(600), 0);
    let survived = !observer.is_transport_closed();
    release.wait();
    worker.join().unwrap();
    manager.prune_idle_connections(Duration::from_secs(600), 0);
    assert!(survived);
    assert!(observer.is_transport_closed());
}

#[test]
fn explicit_close_and_replacement_do_not_wait_for_connection_use() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let manager = AdapterConnectionManager::default();
    let config = fixture_config();
    let old = manager
        .get_or_spawn("replacement", &config, workspace.clone(), false, false)
        .unwrap();
    assert!(manager.evict_if_current(
        &AdapterConnectionKey::new("replacement", workspace.clone()),
        &old,
        AdapterShutdownReason::InitializationFailed,
    ));
    assert!(old.is_transport_closed());
    let replacement = manager
        .get_or_spawn("replacement", &config, workspace.clone(), false, false)
        .unwrap();
    assert_ne!(old.generation(), replacement.generation());
    drop(old);
    manager.prune_idle_connections(Duration::ZERO, 0);
    let survived = !replacement.is_transport_closed();
    manager
        .close_workspace_connections_bounded(&workspace, Duration::ZERO)
        .unwrap();
    assert!(
        survived,
        "old guard must not release replacement protection"
    );
    assert!(
        replacement.is_transport_closed(),
        "explicit close must still work"
    );
}

#[test]
fn acquire_racing_with_prune_never_returns_an_evicted_connection() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let manager = Arc::new(AdapterConnectionManager::default());
    let rendezvous = Arc::new(Barrier::new(2));
    let rounds = 12;
    let worker = {
        let manager = Arc::clone(&manager);
        let rendezvous = Arc::clone(&rendezvous);
        std::thread::spawn(move || {
            for _ in 0..rounds {
                rendezvous.wait();
                manager.prune_idle_connections(Duration::ZERO, 0);
                rendezvous.wait();
            }
        })
    };
    let mut all_open = true;
    for _ in 0..rounds {
        rendezvous.wait();
        let connection =
            manager.get_or_spawn("race", &fixture_config(), workspace.clone(), false, false);
        rendezvous.wait();
        all_open &= connection
            .as_ref()
            .is_ok_and(|connection| !connection.is_transport_closed());
        drop(connection);
    }
    worker.join().unwrap();
    manager.prune_idle_connections(Duration::ZERO, 0);
    assert!(all_open);
}

#[test]
fn early_error_releases_connection_use_without_manual_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let manager = AdapterConnectionManager::default();
    let mut observer = None;
    let result = (|| -> anyhow::Result<()> {
        let connection =
            manager.get_or_spawn("early-error", &fixture_config(), workspace, false, false)?;
        observer = Some(Arc::clone(&connection));
        anyhow::bail!("simulated failure before session setup");
    })();
    assert!(result.is_err());
    manager.prune_idle_connections(Duration::ZERO, 0);
    assert!(observer.unwrap().is_transport_closed());
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
    // Keep an observer, not a use guard: this test exercises actual eviction.
    let observer = Arc::clone(&connection);
    drop(connection);
    let connection = observer;
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
        .find(|line| {
            line.contains("event=\"acp_connection_closed\"")
                && line.contains(&format!("connection_generation={generation} "))
        })
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
        let connection = manager
            .get_or_spawn(provider, &fixture_config(), workspace.clone(), false, false)
            .unwrap();
        Arc::clone(&connection)
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
    let replacement_use = manager
        .get_or_spawn("config-changed", &config, workspace.clone(), false, false)
        .unwrap();
    let replacement = Arc::clone(&replacement_use);
    drop(replacement_use);
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
        let identity = format!("connection_generation={} ", connection.generation());
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
    let eof_identity = format!("connection_generation={} ", eof.generation());
    assert!(log.lines().any(|line| line.contains(&eof_identity)
        && line.contains("event=\"acp_connection_close_observed\"")
        && line.contains("reason=\"standalone-release\"")));
    assert!(log.lines().any(|line| line.contains(&eof_identity)
        && line.contains("event=\"acp_adapter_exit_status\"")
        && line.contains("exit_code=23")));
    assert!(log.contains("event=\"acp_connection_request_rejected\""));
    assert!(log.contains("event=\"acp_session_close_requested\""));
    let capacity_identity = format!("connection_generation={} ", capacity.generation());
    let capacity_snapshot = log
        .lines()
        .find(|line| {
            line.contains(&capacity_identity)
                && line.contains("pending_requests=12")
                && line.contains("pending_methods_truncated=true")
        })
        .unwrap();
    assert_eq!(capacity_snapshot.matches("session/resume").count(), 8);
    let workspace_identity = format!("connection_generation={} ", workspace_close.generation());
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
        if frame["method"] == "session/resume"
            && std::env::var("GOLD_BAND_TEST_RESUME").as_deref() != Ok("1")
        {
            continue;
        }
        println!(
            "{}",
            json!({"jsonrpc": "2.0", "id": frame["id"], "result": {}})
        );
        std::io::stdout().flush().unwrap();
    }
}
