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
