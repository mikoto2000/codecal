use codecal_lib::store::Store;
use std::fs;
#[test]
fn failed_setting_recovery_keeps_original_without_a_partial_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.db");
    let store = Store::open(&path).unwrap();
    store.set_setting("config", "{broken").unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_reset BEFORE UPDATE ON settings WHEN OLD.key='config' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
    assert!(store.reset_setting_with_backup("config", "{}").is_err());
    assert_eq!(
        store.get_setting("config").unwrap().as_deref(),
        Some("{broken")
    );
    let backups: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key GLOB '_config_backup_*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(backups, 0);
}
#[test]
fn display_snapshot_omits_unused_daily_totals_and_truncated_prompts_are_diagnosed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let long_prompt = "依".repeat(16385);
    fs::write(
        root.join("a.jsonl"),
        fixture().replace("合成サンプルの作業", &long_prompt),
    )
    .unwrap();
    let mut store = Store::open(&tmp.path().join("test.db")).unwrap();
    store.sync(&root).unwrap();
    let calendar = store.snapshot_for_display().unwrap();
    assert_eq!(calendar.turns[0].prompt.chars().count(), 16384);
    assert!(calendar
        .diagnostics
        .iter()
        .any(|d| d.contains("16,384文字")));
    assert!(!serde_json::to_value(&calendar)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("days"));
    assert_eq!(store.snapshot("UTC").unwrap().days[0].total_ms, 3600000);
    assert!(!calendar.diagnostics.join("\n").contains(&long_prompt));
}
fn fixture() -> String {
    [
 r#"{"timestamp":"2026-11-20T10:00:00Z","type":"session_meta","payload":{"id":"s1","cwd":"C:\\work\\demo"}}"#,
 r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#,
 r#"{"timestamp":"2026-11-20T10:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"合成サンプルの作業"}}"#,
 r#"{"timestamp":"2026-11-20T11:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1"}}"#,
 ].join("\n")+"\n"
}
#[test]
fn snapshot_reports_total_when_diagnostics_are_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir_all(&root).unwrap();
    for i in 0..201 {
        fs::write(root.join(format!("{i:03}.jsonl")), "not-json\n").unwrap();
    }
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync(&root).unwrap();
    let snapshot = s.snapshot("UTC").unwrap();
    assert_eq!(snapshot.diagnostics_total, 201);
    assert_eq!(snapshot.diagnostics.len(), 200);
    assert!(snapshot.diagnostics[0].contains("000.jsonl"));
    assert!(snapshot.diagnostics[199].contains("199.jsonl"));
    for i in 0..201 {
        fs::write(root.join(format!("{i:03}.jsonl")), fixture()).unwrap();
    }
    s.sync(&root).unwrap();
    let snapshot = s.snapshot("UTC").unwrap();
    assert_eq!(snapshot.diagnostics_total, 0);
    assert!(snapshot.diagnostics.is_empty());
}
#[test]
fn persists_turns_and_avoids_duplicates_across_archive_and_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir_all(root.join("sessions")).unwrap();
    fs::create_dir_all(root.join("archived_sessions")).unwrap();
    let p = root.join("sessions/a.jsonl");
    fs::write(&p, fixture()).unwrap();
    let db = tmp.path().join("data.db");
    let mut s = Store::open(&db).unwrap();
    s.sync(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert_eq!(a.turns[0].status, "completed");
    assert_eq!(a.days[0].total_ms, 3600000);
    assert_eq!(a.turns[0].prompt, "合成サンプルの作業");
    assert_eq!(s.sync(&root).unwrap().bytes_read, 0);
    fs::copy(&p, root.join("archived_sessions/a.jsonl")).unwrap();
    s.sync(&root).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 1);
    drop(s);
    assert_eq!(
        Store::open(&db)
            .unwrap()
            .snapshot("UTC")
            .unwrap()
            .turns
            .len(),
        1
    );
}
#[test]
fn incremental_append_waits_for_complete_line_and_handles_truncation() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("a.jsonl");
    let initial = fixture();
    fs::write(&p, &initial).unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync(tmp.path()).unwrap();
    let start = r#"{"timestamp":"2026-11-20T12:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}"#;
    let mut f = fs::OpenOptions::new().append(true).open(&p).unwrap();
    write!(f, "{}", start).unwrap();
    s.sync(tmp.path()).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 1);
    writeln!(f).unwrap();
    s.sync(tmp.path()).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 2);
    assert!(a
        .turns
        .iter()
        .any(|t| t.status == "unknown" && t.end.is_none()));
    assert_eq!(a.days[0].total_ms, 3600000);
    fs::write(&p, initial.lines().next().unwrap().to_owned() + "\n").unwrap();
    s.sync(tmp.path()).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 0);
}
#[test]
fn malformed_utf8_and_unknown_lines_do_not_stop_import() {
    let tmp = tempfile::tempdir().unwrap();
    let mut input = fixture().into_bytes();
    input.extend_from_slice(b"{bad}\n\xff\n{\"type\":\"future_event\"}\n");
    fs::write(tmp.path().join("a.jsonl"), input).unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync(tmp.path()).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert!(!a.diagnostics.is_empty());
}
#[test]
fn tracks_context_changes_and_closes_explicit_turn_only() {
    let tmp = tempfile::tempdir().unwrap();
    let text = fixture()
        + r#"{"timestamp":"2026-11-20T12:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}
{"timestamp":"2026-11-20T12:00:00Z","type":"turn_context","payload":{"turn_id":"t2","cwd":"/work/second"}}
{"timestamp":"2026-11-20T12:05:00Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"t2"}}
"#;
    fs::write(tmp.path().join("a.jsonl"), text).unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync(tmp.path()).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.projects.len(), 2);
    assert_eq!(
        a.turns
            .iter()
            .find(|t| t.id.ends_with("t2"))
            .unwrap()
            .status,
        "interrupted"
    );
}
