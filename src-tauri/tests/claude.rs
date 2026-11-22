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
#[test]
fn incomplete_oversized_tail_waits_for_newline_and_recovers() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let log = root.join("p").join(format!("{SID}.jsonl"));
    fs::write(&log, vec![b'x'; 1048600]).unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    assert_eq!(s.sync_claude(&root).unwrap().bytes_read, 0);
    let mut f = fs::OpenOptions::new().append(true).open(&log).unwrap();
    write!(
        f,
        "\n{}{}",
        message("a", "2026-11-20T10:00:00Z", "user"),
        message("b", "2026-11-20T10:01:00Z", "assistant")
    )
    .unwrap();
    s.sync_claude(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.days[0].elapsed_ms, 60000);
    assert!(a.diagnostics.iter().any(|d| d.contains("oversized")));
}
#[test]
fn bad_records_are_diagnosed_without_secrets_and_do_not_block_valid_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let log = root.join("p").join(format!("{SID}.jsonl"));
    let mut bytes = b"{SECRET}\n\xff\n".to_vec();
    bytes.extend_from_slice(format!("{}\n", "[".repeat(150) + &"]".repeat(150)).as_bytes());
    bytes.extend_from_slice(b"{\"type\":\"unknown\",\"SECRET\":true}\n");
    bytes.extend_from_slice(message("a", "invalid", "user").as_bytes());
    bytes.extend_from_slice(b"{\"type\":\"user\"}\n");
    bytes.extend_from_slice(message("b", "2026-11-20T10:00:00Z", "user").as_bytes());
    bytes.extend_from_slice(message("c", "2026-11-20T10:01:00Z", "assistant").as_bytes());
    bytes.extend_from_slice(b"{\"type\":");
    fs::write(&log, &bytes).unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_claude(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.days[0].elapsed_ms, 60000);
    assert!(!a.diagnostics.join(" ").contains("SECRET"));
    assert!(a.diagnostics.join(" ").contains("UTF-8"));
    assert!(!s.source_excerpt(&a.turns[0].id).unwrap().contains("SECRET"));
    assert_eq!(s.sync_claude(&root).unwrap().bytes_read, 0);
    let c = rusqlite::Connection::open(tmp.path().join("cache.db")).unwrap();
    let events: String = c
        .query_row("SELECT group_concat(event) FROM events", [], |r| r.get(0))
        .unwrap();
    assert!(!events.contains("SECRET"));
}
#[test]
fn projects_worktrees_parallel_subagents_and_copies_use_shared_union() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    for p in ["p", "copy", "worktree"] {
        fs::create_dir_all(root.join(p)).unwrap();
    }
    let body = message("a", "2026-11-20T10:00:00Z", "user")
        + &message("b", "2026-11-20T10:02:00Z", "assistant");
    for p in ["p", "copy"] {
        fs::write(root.join(p).join(format!("{SID}.jsonl")), &body).unwrap();
    }
    fs::write(
        root.join("worktree/550e8400-e29b-41d4-a716-446655440001.jsonl"),
        body.replace("work\\\\demo", "work\\\\demo-wt")
            .replace(SID, "550e8400-e29b-41d4-a716-446655440001"),
    )
    .unwrap();
    let child = root.join("p").join(SID).join("subagents");
    fs::create_dir_all(&child).unwrap();
    fs::write(child.join("agent-a1.jsonl"), &body).unwrap();
    fs::write(root.join("p/bad-id.jsonl"), "SECRET").unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    let p = s.sync_claude(&root).unwrap();
    assert!(!p.warnings.is_empty());
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 3);
    assert_eq!(a.projects.len(), 2);
    assert_eq!(a.days[0].elapsed_ms, 120000);
    assert_eq!(a.days[0].total_ms, 360000);
    let sub = a
        .turns
        .iter()
        .find(|t| t.session_id.contains("agent-a1"))
        .unwrap();
    assert!(s
        .source_excerpt(&sub.id)
        .unwrap()
        .contains("parent_session_id"));
}
#[test]
fn rejects_invalid_metadata_paths_and_timestamps() {
    let path = format!("p/{SID}.jsonl");
    let path = Path::new(&path);
    let base: serde_json::Value =
        serde_json::from_str(&message("a", "2026-11-20T10:00:00Z", "user")).unwrap();
    for (key, value) in [
        ("cwd", json!("../secret")),
        ("cwd", json!("C:\\work\\..\\secret")),
        ("uuid", json!("../secret")),
        ("timestamp", json!("bad")),
        ("message", json!(null)),
        ("sessionId", json!("../bad")),
    ] {
        let mut v = base.clone();
        v[key] = value;
        assert!(parse_line(&v.to_string(), path).is_err());
    }
    assert!(parse_line("", path).is_err());
    assert!(parse_line(r#"{"type":"progress"}"#, path)
        .unwrap()
        .is_none());
}
#[test]
fn rejects_oversized_files_without_reading_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    fs::File::create(root.join("p").join(format!("{SID}.jsonl")))
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    let result = s.sync_claude(&root).unwrap();
    assert_eq!(result.bytes_read, 0);
    assert!(result.warnings.join(" ").contains("256 MiB"));
}
#[test]
fn updates_replacements_and_completed_tail_without_duplicate_activity() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let log = root.join("p").join(format!("{SID}.jsonl"));
    let a = message("a", "2026-11-20T10:00:00Z", "user");
    let b = message("b", "2026-11-20T10:01:00Z", "assistant");
    fs::write(&log, a.clone() + b.trim_end()).unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_claude(&root).unwrap();
    assert!(s.snapshot("UTC").unwrap().days.is_empty());
    fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    s.sync_claude(&root).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().days[0].elapsed_ms, 60000);
    fs::write(&log, a + &b.replace("10:01:00", "10:02:00")).unwrap();
    s.sync_claude(&root).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().days[0].elapsed_ms, 120000);
    fs::write(&log, "").unwrap();
    s.sync_claude(&root).unwrap();
    assert!(s.snapshot("UTC").unwrap().turns.is_empty());
}
#[cfg(unix)]
#[test]
fn does_not_follow_symlinked_projects_sessions_or_subagents() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let log = outside.join(format!("{SID}.jsonl"));
    fs::write(&log, message("a", "2026-11-20T10:00:00Z", "user")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
    std::os::unix::fs::symlink(&log, root.join("p").join(format!("{SID}.jsonl"))).unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    assert_eq!(s.sync_claude(&root).unwrap().files, 0);
    assert!(s.sync_claude(&root.join("linked")).is_err());
}
#[test]
fn empty_sessions_and_mixed_codex_claude_sources_keep_existing_union_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    fs::write(
        root.join("p/550e8400-e29b-41d4-a716-446655440001.jsonl"),
        "",
    )
    .unwrap();
    fs::write(
        root.join("p").join(format!("{SID}.jsonl")),
        message("a", "2026-11-20T10:00:00Z", "user")
            + &message("b", "2026-11-20T10:01:00Z", "assistant"),
    )
    .unwrap();
    let codex = tmp.path().join("codex");
    fs::create_dir(&codex).unwrap();
    fs::write(codex.join("codex.jsonl"),concat!(
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"C:/work/demo\"}}\n",
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"r\"}}\n",
        "{\"timestamp\":\"2026-11-20T10:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"r\"}}\n")).unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_all_sources(&codex, None, Some(&root), |_, _| {})
        .unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 2);
    assert_eq!(a.projects.len(), 1);
    assert_eq!(a.days[0].elapsed_ms, 60000);
    assert_eq!(a.days[0].total_ms, 120000);
    assert_eq!(
        a.turns
            .iter()
            .find(|t| t.source == "claude")
            .unwrap()
            .details
            .as_ref()
            .unwrap()
            .git_branch
            .as_deref(),
        Some("main")
    );
    s.sync_all_sources(
        &tmp.path().join("missing-codex"),
        None,
        Some(&root),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 2);
}
#[test]
fn concurrent_appends_are_eventually_imported_once_without_changing_source() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir_all(root.join("p")).unwrap();
    let log = root.join("p").join(format!("{SID}.jsonl"));
    fs::write(&log, "").unwrap();
    let log_copy = log.clone();
    let writer = std::thread::spawn(move || {
        let mut f = fs::OpenOptions::new().append(true).open(log_copy).unwrap();
        for i in 0..1000 {
            let time = chrono::DateTime::from_timestamp_millis(1795168800000 + i * 10)
                .unwrap()
                .to_rfc3339();
            let raw = message(
                &format!("m{i}"),
                &time,
                if i == 999 { "assistant" } else { "user" },
            );
            let (a, b) = raw.split_at(raw.len() / 2);
            f.write_all(a.as_bytes()).unwrap();
            std::thread::yield_now();
            f.write_all(b.as_bytes()).unwrap();
        }
    });
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_claude(&root).unwrap();
    writer.join().unwrap();
    let original = fs::read(&log).unwrap();
    s.sync_claude(&root).unwrap();
    assert_eq!(s.sync_claude(&root).unwrap().bytes_read, 0);
    assert_eq!(s.snapshot("UTC").unwrap().days[0].elapsed_ms, 9990);
    assert_eq!(fs::read(&log).unwrap(), original);
}
#[cfg(windows)]
#[test]
fn rejects_windows_junction_escape() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    fs::create_dir(&root).unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(
        outside.join(format!("{SID}.jsonl")),
        message("a", "2026-11-20T10:00:00Z", "user"),
    )
    .unwrap();
    let link = root.join("junction");
    let result = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(result.status.success(), "cannot create test junction");
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    let result = s.sync_claude(&root).unwrap();
    assert_eq!(result.files, 0);
    assert!(!result.warnings.is_empty());
    assert!(s.sync_claude(&link).is_err());
}
#[test]
fn discovers_nested_subagent_transcripts_without_requiring_parent_log() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("projects");
    let agents = root.join("p").join(SID).join("subagents/team");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("agent-a2.jsonl"),
        message("a", "2026-11-20T10:00:00Z", "user")
            + &message("b", "2026-11-20T10:01:00Z", "assistant"),
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("cache.db")).unwrap();
    s.sync_claude(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert_eq!(
        a.turns[0]
            .details
            .as_ref()
            .unwrap()
            .parent_session_id
            .as_deref(),
        Some(format!("claude:{SID}").as_str())
    );
    assert_eq!(a.days[0].elapsed_ms, 60000);
}
