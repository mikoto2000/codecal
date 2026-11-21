use codecal_lib::store::Store;
use std::fs;
#[test]
fn request_before_a_new_start_does_not_overwrite_completed_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let lines = [
        r#"{"timestamp":"2026-11-20T10:00:00Z","type":"session_meta","payload":{"id":"s","cwd":"/demo"}}"#,
        r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"a"}}"#,
        r#"{"timestamp":"2026-11-20T11:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"a"}}"#,
        r#"{"timestamp":"2026-11-20T12:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"second request"}}"#,
        r#"{"timestamp":"2026-11-20T12:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"b"}}"#,
    ];
    fs::write(tmp.path().join("a.jsonl"), lines.join("\n") + "\n").unwrap();
    let mut store = Store::open(&tmp.path().join("data.sqlite")).unwrap();
    store.sync(tmp.path()).unwrap();
    let s = store.snapshot("UTC").unwrap();
    assert_eq!(s.turns[0].prompt, "");
    assert_eq!(s.turns[1].prompt, "second request");
}
#[test]
fn refuses_future_database_schema_instead_of_silently_downgrading() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(Store::open(&path).is_err());
}
