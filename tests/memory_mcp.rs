use camino::Utf8PathBuf;
use gold_band::{
    memory::{Entry, MemoryService, Scope, WriteCommand},
    storage::{GoldBandPaths, write_json},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Stdio},
    sync::mpsc,
    time::Duration,
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn unbound_memory_stdio_supports_protocol_diagnostics_and_rejects_tool_calls() {
    let mut command = gold_band::process::background_command(env!("CARGO_BIN_EXE_gold-band"));
    command.arg(gold_band::memory::mcp::FLAG);
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut request = |id: u64, method: &str, params: Value| -> Value {
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0", "id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        loop {
            let value: Value = serde_json::from_str(
                &rx.recv_timeout(Duration::from_secs(20))
                    .expect("MCP response timeout"),
            )
            .unwrap();
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{value}");
                if method == "initialize" {
                    writeln!(
                        input,
                        "{}",
                        json!({"jsonrpc":"2.0", "method":"notifications/initialized"})
                    )
                    .unwrap();
                    input.flush().unwrap();
                }
                return value["result"].clone();
            }
        }
    };

    let init = request(
        1,
        "initialize",
        json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"memory-diagnostic-test","version":"1"}}),
    );
    assert!(init["capabilities"].get("tools").is_some());
    let tools = request(2, "tools/list", json!({}));
    assert_eq!(tools["tools"].as_array().unwrap().len(), 2);
    let read = request(
        3,
        "tools/call",
        json!({"name":"memory_read","arguments":{}}),
    );
    assert_eq!(read["isError"], true);
    let error: Value = serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["code"], "memory.context-required");

    drop(request);
    drop(input);
    drop(child);
    reader.join().unwrap();
}

#[test]
fn memory_stdio_tools_share_durable_state_and_reject_stale_revisions() {
    let temp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let mut paths = GoldBandPaths::new(root.join("workspace"));
    paths.user_gold_band_root = root.join("data");
    paths.runtime_root = paths
        .user_gold_band_root
        .join("projects")
        .join(&paths.project_id);
    paths.provision_project_manifest().unwrap();
    write_json(&paths.task_file("task-1"), &json!({"id":"task-1"})).unwrap();
    let service =
        MemoryService::new(paths.clone(), &paths.project_id, Some("task-1".into())).unwrap();
    let base = json!({
        "name": gold_band::memory::mcp::SERVER_NAME,
        "command": env!("CARGO_BIN_EXE_gold-band"),
        "args": [gold_band::memory::mcp::FLAG],
        "env": []
    });
    let config = gold_band::memory::mcp::bind_session_config(
        &base,
        &paths,
        "task-1",
        gold_band::config::DesktopLanguage::En,
    )
    .unwrap();
    let binding: Value = serde_json::from_str(config["args"][1].as_str().unwrap()).unwrap();
    assert!(
        binding.get("wb").is_none(),
        "release channel must come from the compiled channel source, not MCP launch data"
    );
    let mut command = gold_band::process::background_command(env!("CARGO_BIN_EXE_gold-band"));
    command.args(
        config["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap()),
    );
    let mut child = ChildGuard(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut request = |id: u64, method: &str, params: Value| -> Value {
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0", "id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        loop {
            let value: Value = serde_json::from_str(
                &rx.recv_timeout(Duration::from_secs(20))
                    .expect("MCP response timeout"),
            )
            .unwrap();
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{value}");
                return value["result"].clone();
            }
        }
    };
    let init = request(
        1,
        "initialize",
        json!({"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"memory-test","version":"1"}}),
    );
    assert!(init["capabilities"].get("tools").is_some());
    let tools = request(2, "tools/list", json!({}));
    assert_eq!(tools["tools"].as_array().unwrap().len(), 2);
    let args = json!({"scope":"task","key":"plan","expectedRevision":null,"entry":{"key":"plan","value":"B2","desc":"plan"}});
    let write = request(
        3,
        "tools/call",
        json!({"name":"memory_write","arguments":args}),
    );
    assert_eq!(write["isError"], false);
    let snapshot = service.read().unwrap();
    assert_eq!(
        snapshot
            .effective
            .iter()
            .find(|record| record.entry.key == "plan")
            .unwrap()
            .entry
            .value,
        "B2"
    );
    let revision = snapshot.task[0].revision.clone();
    service
        .write(WriteCommand {
            scope: Scope::Task,
            key: "plan".into(),
            expected_revision: Some(revision.clone()),
            entry: Some(Entry {
                key: "plan".into(),
                value: "B3".into(),
                desc: "plan".into(),
            }),
        })
        .unwrap();
    let stale = request(
        4,
        "tools/call",
        json!({"name":"memory_write","arguments":{"scope":"task","key":"plan","expectedRevision":revision,"entry":{"key":"plan","value":"stale","desc":"plan"}}}),
    );
    assert_eq!(stale["isError"], true);
    let read = request(
        5,
        "tools/call",
        json!({"name":"memory_read","arguments":{}}),
    );
    let data: Value = serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        data["effective"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["key"] == "plan")
            .unwrap()["value"],
        "B3"
    );
    drop(request);
    drop(input);
    drop(child);
    reader.join().unwrap();
}
