//! Integration tests for `read_session_tokens` — extracting token totals
//! from ACP session snapshot and timeline files.

use std::io::Write;

use gold_band::acp::events::read_session_tokens;
use tempfile::TempDir;

fn session_path(dir: &TempDir) -> camino::Utf8PathBuf {
    camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf())
        .unwrap()
        .join("acp.session.json")
}

#[test]
fn reads_token_data_from_snapshot_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("acp.snapshot.json"),
        r#"{
        "adapterId":"test","adapterDisplayName":"Test","cwd":".",
        "status":"completed","restored":false,"capabilities":{},
        "createdAt":"2026-01-01T00:00:00","updatedAt":"2026-01-01T00:00:00",
        "inputTokens":5000,"outputTokens":2000,"cachedReadTokens":1000,"totalTokens":8000
    }"#,
    )
    .unwrap();
    let (i, o, cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!((i, o, cr, t), (5000, 2000, 1000, 8000));
}

#[test]
fn no_files_returns_all_zeros() {
    let dir = TempDir::new().unwrap();
    let (i, o, cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!((i, o, cr, t), (0, 0, 0, 0));
}

#[test]
fn reads_usage_update_from_timeline() {
    let dir = TempDir::new().unwrap();
    let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":888,"outputTokens":222,"cachedReadTokens":100,"totalTokens":1210}}}}"#).unwrap();
    let (i, o, cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!((i, o, cr, t), (888, 222, 100, 1210));
}

#[test]
fn timeline_takes_max_values() {
    let dir = TempDir::new().unwrap();
    let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":100,"outputTokens":50,"totalTokens":150}}}}"#).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":999,"outputTokens":20,"totalTokens":1019}}}}"#).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":500,"outputTokens":80,"totalTokens":580}}}}"#).unwrap();
    let (i, o, _cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!(i, 999);
    assert_eq!(o, 80);
    assert_eq!(t, 1019);
}

#[test]
fn ignores_non_usage_events() {
    let dir = TempDir::new().unwrap();
    let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
    writeln!(
        f,
        r#"{{"item":{{"kind":"userTextDelta","content":"hello"}}}}"#
    )
    .unwrap();
    writeln!(f, r#"{{"item":{{"kind":"availableCommands"}}}}"#).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":777,"outputTokens":33,"totalTokens":810}}}}"#).unwrap();
    let (i, o, _cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!(i, 777);
    assert_eq!(o, 33);
    assert_eq!(t, 810);
}

#[test]
fn snapshot_overridden_by_higher_timeline() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("acp.snapshot.json"),
        r#"{
        "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
        "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
        "inputTokens":100,"outputTokens":50,"cachedReadTokens":10,"totalTokens":160
    }"#,
    )
    .unwrap();
    let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":500,"outputTokens":200,"cachedReadTokens":50,"totalTokens":750}}}}"#).unwrap();
    let (i, o, cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!(i, 500);
    assert_eq!(o, 200);
    assert_eq!(cr, 50);
    assert_eq!(t, 750);
}

#[test]
fn snapshot_preserved_when_timeline_lower() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("acp.snapshot.json"),
        r#"{
        "adapterId":"t","adapterDisplayName":"T","cwd":".","status":"ok",
        "restored":false,"capabilities":{},"createdAt":"","updatedAt":"",
        "inputTokens":1000,"outputTokens":500,"cachedReadTokens":200,"totalTokens":1700
    }"#,
    )
    .unwrap();
    let mut f = std::fs::File::create(dir.path().join("acp.timeline.jsonl")).unwrap();
    writeln!(f, r#"{{"item":{{"kind":"usageUpdate","inputTokens":100,"outputTokens":50,"totalTokens":150}}}}"#).unwrap();
    let (i, o, cr, t) = read_session_tokens(&session_path(&dir));
    assert_eq!(i, 1000);
    assert_eq!(o, 500);
    assert_eq!(cr, 200);
    assert_eq!(t, 1700);
}
