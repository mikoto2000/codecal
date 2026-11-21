use codecal_lib::store::Store;
use rusqlite::{params, Connection};
use serde_json::json;
use std::fs;

fn event(id: &str, run: &str, kind: &str, time: serde_json::Value) -> String {
    json!({"id":id,"projectId":"p","sessionId":"s","turnId":null,"runId":run,
        "timestamp":time,"type":kind,"version":1,"payload":{}})
    .to_string()
}
fn database(root: &std::path::Path) -> Connection {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("projects.json"),
        r#"{"projects":[{"id":"p","name":"Demo","path":"C:\\work\\demo"}]}"#,
    )
    .unwrap();
    let c = Connection::open(root.join("storage.db")).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE agent_events(project_id TEXT,sequence INTEGER,id TEXT,event_type TEXT,record TEXT,PRIMARY KEY(project_id,sequence)); CREATE TABLE turns(project_id TEXT,run_id TEXT,record TEXT);").unwrap();
    c
}
#[test]
fn empty_rei_directory_warns_and_recovers_when_a_source_appears() {
    let tmp = tempfile::tempdir().unwrap();
    let codex = tmp.path().join("codex");
    let rei = tmp.path().join("rei");
    fs::create_dir(&codex).unwrap();
    fs::create_dir(&rei).unwrap();
    let mut store = Store::open(&tmp.path().join("data.db")).unwrap();
    let result = store.sync_sources(&codex, Some(&rei), |_, _| {}).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("storage.db または projects"));
    assert_eq!(store.snapshot("UTC").unwrap().diagnostics_total, 1);
    assert_eq!(fs::read_dir(&rei).unwrap().count(), 0);
    fs::create_dir(rei.join("projects")).unwrap();
    assert!(store
        .sync_sources(&codex, Some(&rei), |_, _| {})
        .unwrap()
        .warnings
        .is_empty());
    assert_eq!(store.snapshot("UTC").unwrap().diagnostics_total, 0);
    fs::remove_dir(rei.join("projects")).unwrap();
    fs::write(rei.join("storage.db"), "not a SQLite database").unwrap();
    let result = store.sync_sources(&codex, Some(&rei), |_, _| {}).unwrap();
    assert!(result.warnings.iter().any(|s| s.contains("storage.db")));
    assert_eq!(
        fs::read(rei.join("storage.db")).unwrap(),
        b"not a SQLite database"
    );
}
fn insert(c: &Connection, seq: i64, id: &str, run: &str, kind: &str, time: serde_json::Value) {
    c.execute(
        "INSERT INTO agent_events VALUES('p',?,?,?,?)",
        params![seq, id, kind, event(id, run, kind, time)],
    )
    .unwrap();
}

#[test]
fn reads_wal_incrementally_deduplicates_legacy_and_keeps_codex() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(
        &c,
        1,
        "start",
        "r",
        "AGENT_RUN_STARTED",
        serde_json::from_str("1795168800.123456789").unwrap(),
    );
    c.execute(
        "INSERT INTO turns VALUES('p','r',?)",
        [json!({"request":"文章をまとめて"}).to_string()],
    )
    .unwrap();
    let mut store = Store::open(&tmp.path().join("codecal.db")).unwrap();
    store.sync_rei(&root).unwrap();
    let first = store.snapshot("UTC").unwrap();
    assert_eq!(first.turns.len(), 1);
    assert_eq!(first.turns[0].status, "unknown");
    assert_eq!(first.turns[0].prompt, "文章をまとめて");
    insert(
        &c,
        2,
        "end",
        "r",
        "AGENT_RUN_COMPLETED",
        serde_json::from_str("1795172400.123456789").unwrap(),
    );
    store.sync_rei(&root).unwrap();
    let a = store.snapshot("UTC").unwrap();
    assert_eq!(a.days[0].total_ms, 3600000);
    assert_eq!(a.turns[0].source, "rei");
    assert!(store
        .source_excerpt(&a.turns[0].id)
        .unwrap()
        .contains("AGENT_RUN_STARTED"));
    assert_eq!(store.sync_rei(&root).unwrap().bytes_read, 0);
    fs::create_dir_all(root.join("projects/p/events")).unwrap();
    fs::write(
        root.join("projects/p/events/events.jsonl"),
        format!(
            "{}\n{}\n",
            event(
                "start",
                "r",
                "AGENT_RUN_STARTED",
                serde_json::from_str("1795168800.123456789").unwrap()
            ),
            event(
                "end",
                "r",
                "AGENT_RUN_COMPLETED",
                serde_json::from_str("1795172400.123456789").unwrap()
            )
        ),
    )
    .unwrap();
    store.sync_rei(&root).unwrap();
    assert_eq!(store.snapshot("UTC").unwrap().turns.len(), 1);
    // The legacy copy remains a usable source if the database is removed.
    drop(c);
    fs::remove_file(root.join("storage.db")).unwrap();
    assert!(store
        .source_excerpt(&a.turns[0].id)
        .unwrap()
        .contains("AGENT_RUN_STARTED"));
    let logs = tmp.path().join("codex");
    fs::create_dir(&logs).unwrap();
    fs::write(logs.join("a.jsonl"),concat!(
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"C:/work/demo\"}}\n",
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"r\"}}\n",
        "{\"timestamp\":\"2026-11-20T11:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"r\"}}\n")).unwrap();
    store.sync(&logs).unwrap();
    let a = store.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 2);
    assert_eq!(a.projects.len(), 1);
    assert_eq!(a.days[0].total_ms, 7200000);
    assert_eq!(a.days[0].elapsed_ms, 3600123);
    drop(store);
    assert_eq!(
        Store::open(&tmp.path().join("codecal.db"))
            .unwrap()
            .snapshot("UTC")
            .unwrap()
            .turns
            .len(),
        2
    );
}

#[test]
fn explicit_runs_handle_missing_sessions_clock_rollback_and_all_terminal_states() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    for (seq, id, run, kind, time) in [
        (1, "a", "one", "AGENT_RUN_STARTED", 10),
        (2, "b", "two", "AGENT_RUN_STARTED", 11),
        (3, "c", "one", "AGENT_RUN_FAILED", 20),
        (4, "d", "two", "AGENT_RUN_CANCELLED", 21),
        (5, "e", "back", "AGENT_RUN_STARTED", 30),
        (6, "f", "back", "AGENT_RUN_COMPLETED", 29),
        (7, "g", "orphan", "AGENT_RUN_COMPLETED", 40),
    ] {
        insert(&c, seq, id, run, kind, json!(time));
    }
    c.execute(
        "UPDATE agent_events SET record=json_set(record,'$.sessionId',NULL) WHERE sequence<=2",
        [],
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 3);
    assert_eq!(a.turns[0].status, "failed");
    assert_eq!(a.turns[1].status, "interrupted");
    assert_eq!(a.turns[2].status, "unknown");
    assert_eq!(a.days[0].total_ms, 20000);
    // Restoring a DB with a different event at the saved sequence triggers a rescan.
    c.execute("DELETE FROM agent_events", []).unwrap();
    insert(&c, 1, "restored", "new", "AGENT_RUN_STARTED", json!(50));
    s.sync_rei(&root).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 4);
}

#[test]
fn legacy_waits_for_newline_skips_bad_rows_and_reads_original_excerpt() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let path = root.join("projects/p/events/events.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let start = event("a", "r", "AGENT_RUN_STARTED", json!("2026-11-20T10:00:00Z"));
    let end = event(
        "b",
        "r",
        "AGENT_RUN_COMPLETED",
        json!("2026-11-20T11:00:00Z"),
    );
    fs::write(&path, format!("bad\n{start}\n{end}")).unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns[0].status, "unknown");
    assert!(!a.diagnostics.is_empty());
    use std::io::Write;
    writeln!(fs::OpenOptions::new().append(true).open(&path).unwrap()).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns[0].status, "completed");
    assert!(s
        .source_excerpt(&a.turns[0].id)
        .unwrap()
        .contains("AGENT_RUN_STARTED"));
    fs::remove_file(&path).unwrap();
    s.sync_rei(&root).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 1);
}

#[test]
fn late_prompts_and_bad_records_do_not_require_new_lifecycle_events() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(
        &c,
        1,
        "start",
        "r",
        "AGENT_RUN_STARTED",
        json!("2026-11-20T10:00:00Z"),
    );
    c.execute(
        "INSERT INTO agent_events VALUES('p',2,'bad','AGENT_RUN_STARTED','broken secret text')",
        [],
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert!(a.diagnostics.iter().any(|s| s.contains("sequence 2")));
    assert!(!a.diagnostics.iter().any(|s| s.contains("secret")));
    c.execute(
        "INSERT INTO turns VALUES('p','r',?)",
        [json!({"request":"後から保存された依頼"}).to_string()],
    )
    .unwrap();
    s.sync_rei(&root).unwrap();
    assert_eq!(
        s.snapshot("UTC").unwrap().turns[0].prompt,
        "後から保存された依頼"
    );
}

#[test]
fn failing_source_does_not_block_the_other_provider_or_legacy() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let file = root.join("projects/p/events/events.jsonl");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(root.join("storage.db"), "not a database").unwrap();
    fs::write(
        &file,
        event("a", "r", "AGENT_RUN_STARTED", json!(10)) + "\n",
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    let result = s
        .sync_sources(&tmp.path().join("missing-codex"), Some(&root), |_, _| {})
        .unwrap();
    assert!(result.warnings.iter().any(|w| w.starts_with("Codex:")));
    assert!(result.warnings.iter().any(|w| w.contains("storage.db")));
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert!(a.diagnostics.len() >= 2);
    let codex = tmp.path().join("codex");
    fs::create_dir(&codex).unwrap();
    fs::write(codex.join("s.jsonl"),concat!(
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"/work\"}}\n",
        "{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"r\"}}\n")).unwrap();
    let result = s
        .sync_sources(&codex, Some(&tmp.path().join("missing-rei")), |_, _| {})
        .unwrap();
    assert!(result.warnings.iter().any(|w| w.starts_with("れい:")));
    assert_eq!(s.snapshot("UTC").unwrap().turns.len(), 2);
}

#[test]
fn sync_warnings_describe_only_the_current_attempt() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    let codex = tmp.path().join("codex");
    let rei = tmp.path().join("rei");
    let result = s.sync_sources(&codex, Some(&rei), |_, _| {}).unwrap();
    assert_eq!(result.files, 0);
    assert_eq!(result.warnings.len(), 2);
    fs::create_dir(&codex).unwrap();
    let result = s.sync_sources(&codex, None, |_, _| {}).unwrap();
    assert!(result.warnings.is_empty());
    assert!(!s.snapshot("UTC").unwrap().diagnostics.is_empty());
}

#[test]
fn migrates_v1_without_losing_existing_projects_turns_or_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("old.db");
    let c = Connection::open(&db).unwrap();
    c.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,cwd TEXT NOT NULL UNIQUE,name TEXT NOT NULL,color TEXT NOT NULL,last_seen INTEGER NOT NULL);
        INSERT INTO projects VALUES('p','/work','My name','#123456',0);
        CREATE TABLE sessions(id TEXT PRIMARY KEY,project_id TEXT,start INTEGER,last_event INTEGER);
        INSERT INTO sessions VALUES('s','p',0,1000);
        CREATE TABLE turns(id TEXT PRIMARY KEY,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,project_id TEXT NOT NULL,start INTEGER NOT NULL,end INTEGER,status TEXT NOT NULL,prompt TEXT NOT NULL,tokens INTEGER,UNIQUE(session_id,turn_id));
        INSERT INTO turns VALUES('s:r','s','r','p',0,1000,'completed','original',1);
        CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        INSERT INTO settings VALUES('config','{\"log_root\":\"/logs\"}');
        PRAGMA user_version=1;").unwrap();
    drop(c);
    let s = Store::open(&db).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns[0].source, "codex");
    assert_eq!(a.turns[0].prompt, "original");
    assert_eq!(a.projects[0].name, "My name");
    assert_eq!(a.projects[0].color, "#123456");
    assert!(s
        .get_setting("config")
        .unwrap()
        .unwrap()
        .contains("log_root"));
    drop(s);
    assert_eq!(
        Connection::open(db)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn submillisecond_boundaries_are_truncated_without_floating_point_rounding() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(
        &c,
        1,
        "a",
        "r",
        "AGENT_RUN_STARTED",
        serde_json::from_str("1795168800.999999999").unwrap(),
    );
    insert(
        &c,
        2,
        "b",
        "r",
        "AGENT_RUN_COMPLETED",
        serde_json::from_str("1795168801.000000001").unwrap(),
    );
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.days[0].total_ms, 1);
    assert_eq!(a.turns[0].native_session_id.as_deref(), Some("s"));
    assert!(a.turns[0].native_turn_id.is_none());
}

#[test]
fn optional_history_schema_failure_does_not_drop_activity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    c.execute_batch("DROP TABLE turns; CREATE TABLE turns(legacy_record TEXT);")
        .unwrap();
    insert(&c, 1, "a", "r", "AGENT_RUN_STARTED", json!(10));
    insert(&c, 2, "b", "r", "AGENT_RUN_COMPLETED", json!(20));
    let mut s = Store::open(&tmp.path().join("codecal.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert_eq!(a.days[0].total_ms, 10000);
    assert!(a.diagnostics.iter().any(|s| s.contains("依頼文")));
}

#[test]
fn skipped_sqlite_records_remain_diagnosable_on_incremental_sync() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(&c, 1, "a", "r", "AGENT_RUN_STARTED", json!(10));
    c.execute(
        "INSERT INTO agent_events VALUES('p',2,'bad','AGENT_RUN_COMPLETED','broken')",
        [],
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("codecal.db")).unwrap();
    s.sync_rei(&root).unwrap();
    s.sync_rei(&root).unwrap();
    assert!(s
        .snapshot("UTC")
        .unwrap()
        .diagnostics
        .iter()
        .any(|s| s.contains("sequence 2")));
}

#[test]
fn legacy_metadata_refresh_keeps_cached_prompt_and_updates_project_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(&c, 1, "a", "r", "AGENT_RUN_STARTED", json!(10));
    c.execute(
        "INSERT INTO turns VALUES('p','r',?)",
        [json!({"request":"cached request"}).to_string()],
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("codecal.db")).unwrap();
    s.sync_rei(&root).unwrap();
    let path = root.join("projects/p/events/events.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, event("a", "r", "AGENT_RUN_STARTED", json!(10)) + "\n").unwrap();
    drop(c);
    fs::remove_file(root.join("storage.db")).unwrap();
    fs::write(
        root.join("projects.json"),
        json!({"projects":[{"id":"p","name":"Moved","path":"/new/path"}]}).to_string(),
    )
    .unwrap();
    s.sync_rei(&root).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns[0].prompt, "cached request");
    assert_eq!(a.projects.len(), 1);
    assert_eq!(a.projects[0].cwd, "/new/path");
}

#[test]
fn unchanged_sync_does_not_rewrite_all_derived_history_and_failed_rebuild_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let c = database(&root);
    insert(&c, 1, "a", "r", "AGENT_RUN_STARTED", json!(10));
    let db = tmp.path().join("codecal.db");
    let mut s = Store::open(&db).unwrap();
    s.sync_rei(&root).unwrap();
    let local = Connection::open(&db).unwrap();
    local.execute_batch("CREATE TRIGGER reject_rebuild BEFORE DELETE ON turns BEGIN SELECT RAISE(ABORT,'unexpected rebuild'); END;").unwrap();
    assert_eq!(s.sync_rei(&root).unwrap().bytes_read, 0);
    // Force failure after committing new source events, before replacing derived tables.
    insert(&c, 2, "b", "r", "AGENT_RUN_COMPLETED", json!(20));
    assert!(s.sync_rei(&root).is_err());
    drop(s);
    local.execute_batch("DROP TRIGGER reject_rebuild").unwrap();
    drop(local);
    let s = Store::open(&db).unwrap();
    assert_eq!(s.snapshot("UTC").unwrap().turns[0].status, "completed");
}

#[test]
fn legacy_same_size_rewrite_after_unchanged_prefix_is_not_missed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let path = root.join("projects/p/events/events.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let prefix = "{\"type\":\"ignored\"}\n".repeat(300);
    fs::write(
        &path,
        prefix.clone() + &event("a", "first", "AGENT_RUN_STARTED", json!(10)) + "\n",
    )
    .unwrap();
    let mut s = Store::open(&tmp.path().join("data.db")).unwrap();
    s.sync_rei(&root).unwrap();
    fs::write(
        &path,
        prefix + &event("b", "other", "AGENT_RUN_STARTED", json!(10)) + "\n",
    )
    .unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100))
        .unwrap();
    s.sync_rei(&root).unwrap();
    assert!(s
        .snapshot("UTC")
        .unwrap()
        .turns
        .iter()
        .any(|t| t.turn_id == "other"));
    assert_eq!(s.sync_rei(&root).unwrap().bytes_read, 0);
}
