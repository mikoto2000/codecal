use codecal_lib::claude::{parse_line, session_key};
use std::path::Path;

const SID: &str = "550e8400-e29b-41d4-a716-446655440000";
#[test]
fn parses_only_activity_metadata_without_retaining_message_bodies() {
    let path = format!("project/{SID}.jsonl");
    let raw = serde_json::json!({"type":"assistant","uuid":"message-1","sessionId":SID,
        "cwd":"C:\\work\\demo", "timestamp":"2026-11-20T19:00:00+09:00", "gitBranch":"main",
        "message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"SECRET"}}]}});
    let e = parse_line(&raw.to_string(), Path::new(&path))
        .unwrap()
        .unwrap();
    assert_eq!(e.source.as_deref(), Some("claude"));
    assert_eq!(e.kind, "activity");
    assert_eq!(e.text.as_deref(), Some("ツール呼び出し"));
    assert_eq!(e.timestamp, 1795168800000);
    assert_eq!(e.details.unwrap().git_branch.as_deref(), Some("main"));
    assert!(!serde_json::to_string(&e.text).unwrap().contains("SECRET"));
}
#[test]
fn validates_session_paths_and_keeps_subagent_parent() {
    assert!(session_key(Path::new("p/invalid.jsonl")).is_err());
    assert!(session_key(Path::new("../escape.jsonl")).is_err());
    let path = format!("p/{SID}/subagents/agent-a1b2c3.jsonl");
    let (id, native, parent) = session_key(Path::new(&path)).unwrap();
    assert_eq!(native, SID);
    assert_eq!(parent.as_deref(), Some(format!("claude:{SID}").as_str()));
    assert_eq!(id, format!("claude:{SID}:agent-a1b2c3"));
}
use codecal_lib::store::Store;
use serde_json::json;
use std::fs;
fn message(id: &str, time: &str, kind: &str) -> String {
    json!({"type":kind,"uuid":id,"sessionId":SID,"cwd":"C:\\work\\demo",
        "gitBranch":"main","timestamp":time,"message":{"content":"SECRET"}})
    .to_string()
        + "\n"
}
#[test]
fn imports_incrementally_into_shared_intervals_and_excludes_idle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let log = root.join("p").join(format!("{SID}.jsonl"));
    fs::write(
        &log,
        message("a", "2026-11-20T23:59:00Z", "user")
            + &message("b", "2026-11-21T00:01:00Z", "assistant")
            + &message("c", "2026-11-21T10:00:00Z", "user")
            + &message("d", "2026-11-21T10:01:00Z", "assistant"),
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_claude(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.projects[0].cwd, "c:/work/demo");
    assert_eq!(a.days.iter().map(|d| d.elapsed_ms).sum::<i64>(), 180000);
    assert_eq!(a.days.len(), 2);
    assert_eq!(s.snapshot("Asia/Tokyo").unwrap().days.len(), 1);
    assert!(a
        .turns
        .iter()
        .all(|t| t.source == "claude" && !t.prompt.contains("SECRET")));
    assert_eq!(s.sync_claude(&root).unwrap().bytes_read, 0);
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), a.turns.len());
}
#[test]
fn missing_and_empty_directories_are_safe() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    assert!(s.sync_claude(&tmp.path().join("missing")).is_err());
    assert_eq!(s.sync_claude(tmp.path()).unwrap().files, 0);
}
