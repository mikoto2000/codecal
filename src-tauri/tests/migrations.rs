use codecal_lib::store::Store;
use rusqlite::Connection;
use std::path::Path;
fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}
#[test]
fn opens_known_legacy_v3_without_losing_history_settings_or_claude_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DROP TABLE human_activity_records;
        DROP TABLE human_activity_sources;
        DELETE FROM schema_migrations WHERE version>3;
        CREATE TABLE claude_files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL);
        INSERT INTO claude_files VALUES('/synthetic/session.jsonl','cached');
        INSERT INTO projects VALUES('p','/synthetic','Custom name','#123456',1000);
        INSERT INTO sessions VALUES('s','p',0,1000);
        INSERT INTO turns VALUES('s:t','s','t','p',0,1000,'completed','synthetic',NULL,'codex',NULL,NULL);
        INSERT INTO settings VALUES('keep','synthetic setting');
        INSERT OR IGNORE INTO schema_migrations VALUES(3);
        PRAGMA user_version=3;").unwrap();
    drop(c);
    let s = Store::open(&path).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 1);
    assert_eq!(a.projects[0].name, "Custom name");
    assert_eq!(
        s.get_setting("keep").unwrap().as_deref(),
        Some("synthetic setting")
    );
    drop(s);
    let c = Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("SELECT fingerprint FROM claude_files", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "cached"
    );
    assert_eq!(version(&path), 4);
}
#[test]
fn upgrades_v2_automatically_with_a_consistent_backup_and_idempotent_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.db");
    let s = Store::open(&path).unwrap();
    s.set_setting("keep", "before upgrade").unwrap();
    drop(s);
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DELETE FROM schema_migrations WHERE version>2; PRAGMA user_version=2;")
        .unwrap();
    drop(c);
    let s = Store::open(&path).unwrap();
    assert_eq!(
        s.get_setting("keep").unwrap().as_deref(),
        Some("before upgrade")
    );
    drop(s);
    assert_eq!(version(&path), 4);
    let backup = tmp.path().join("old.db.before-v4.bak");
    assert!(backup.is_file());
    assert_eq!(version(&backup), 2);
    let before = std::fs::read(&backup).unwrap();
    drop(Store::open(&path).unwrap());
    assert_eq!(std::fs::read(&backup).unwrap(), before);
}
#[test]
fn retries_keep_older_backups_and_capture_the_latest_pre_migration_state() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("retry.db");
    drop(Store::open(&path).unwrap());
    let original = tmp.path().join("retry.db.before-v4.bak");
    std::fs::write(&original, b"original backup").unwrap();
    for sequence in 1..=2 {
        let c = Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA user_version=2;").unwrap();
        c.execute(
            "INSERT OR REPLACE INTO settings VALUES('keep',?)",
            [format!("state-{sequence}")],
        )
        .unwrap();
        drop(c);
        drop(Store::open(&path).unwrap());
        let backup_path = tmp
            .path()
            .join(format!("retry.db.before-v4.bak.{sequence}"));
        assert_eq!(version(&backup_path), 2);
        let backup = Connection::open(backup_path).unwrap();
        let value: String = backup
            .query_row("SELECT value FROM settings WHERE key='keep'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(value, format!("state-{sequence}"));
        assert_eq!(std::fs::read(&original).unwrap(), b"original backup");
    }
}
#[test]
fn old_version_marker_with_already_added_columns_does_not_fail_duplicate_column() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DELETE FROM schema_migrations WHERE version>1; PRAGMA user_version=1;")
        .unwrap();
    drop(c);
    drop(Store::open(&path).unwrap());
    assert_eq!(version(&path), 4);
}
#[test]
fn refuses_a_directory_at_backup_path_without_migrating() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DELETE FROM schema_migrations WHERE version>2; PRAGMA user_version=2;")
        .unwrap();
    drop(c);
    std::fs::create_dir(tmp.path().join("old.db.before-v4.bak")).unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(version(&path), 2);
}
#[test]
fn migration_failure_rolls_back_schema_and_keeps_original_version_and_data() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("old.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE turns(id TEXT PRIMARY KEY,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,project_id TEXT NOT NULL,start INTEGER NOT NULL,end INTEGER,status TEXT NOT NULL,prompt TEXT NOT NULL,tokens INTEGER,UNIQUE(session_id,turn_id));
        INSERT INTO turns VALUES('s:t','s','t','p',0,1000,'completed','keep',NULL);
        CREATE TABLE rei_sync(incompatible TEXT); PRAGMA user_version=1;").unwrap();
    drop(c);
    assert!(Store::open(&path).is_err());
    assert_eq!(version(&path), 1);
    let c = Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("SELECT prompt FROM turns", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    let added: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('turns') WHERE name='source'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(added, 0);
}
#[test]
fn future_and_incompatible_v3_are_rejected_without_resetting_schema() {
    let tmp = tempfile::tempdir().unwrap();
    for v in [3, 99] {
        let path = tmp.path().join(format!("unknown-{v}.db"));
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('keep');")
            .unwrap();
        c.pragma_update(None, "user_version", v).unwrap();
        drop(c);
        assert!(Store::open(&path).is_err());
        assert_eq!(version(&path), v);
        let c = Connection::open(&path).unwrap();
        assert_eq!(
            c.query_row("SELECT value FROM sentinel", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
#[test]
fn backs_up_committed_wal_records_and_preserves_zero_version_legacy_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("wal.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT INTO settings VALUES('keep','wal value'); PRAGMA user_version=0;").unwrap();
    assert!(tmp.path().join("wal.db-wal").is_file());
    let s = Store::open(&path).unwrap();
    assert_eq!(s.get_setting("keep").unwrap().as_deref(), Some("wal value"));
    drop(s);
    let backup = Connection::open(tmp.path().join("wal.db.before-v4.bak")).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT value FROM settings WHERE key='keep'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "wal value"
    );
    assert_eq!(version(&path), 4);
    drop(c);
}
#[test]
fn preserves_explicit_legacy_claude_turn_boundaries_after_migration() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    for (id, turn, kind, time) in [
        ("a", "one", "start", 0),
        ("b", "one", "completed", 60000),
        ("c", "two", "start", 120000),
        ("d", "two", "completed", 180000),
    ] {
        let event = serde_json::json!({"source":"claude","timestamp":time,"kind":kind,"session_id":"claude:legacy","turn_id":turn,"cwd":"/synthetic","text":"legacy task","tokens":null});
        c.execute(
            "INSERT INTO events VALUES(?,?,?,?)",
            rusqlite::params![id, "claude:legacy", time, event.to_string()],
        )
        .unwrap();
    }
    c.execute("INSERT INTO settings VALUES('_activity_dirty','1')", [])
        .unwrap();
    drop(c);
    let s = Store::open(&path).unwrap();
    let a = s.snapshot("UTC").unwrap();
    assert_eq!(a.turns.len(), 2);
    assert_eq!(a.turns[0].end, Some(60000));
    assert_eq!(a.days[0].elapsed_ms, 120000);
}
#[test]
fn reports_migration_before_backup_and_schema_changes_but_not_for_current_schema() {
    use std::cell::Cell;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("progress.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DELETE FROM schema_migrations WHERE version>2; PRAGMA user_version=2;")
        .unwrap();
    drop(c);
    let calls = Cell::new(0);
    drop(
        Store::open_with_progress(&path, || {
            calls.set(calls.get() + 1);
            assert_eq!(version(&path), 2);
            assert!(!tmp.path().join("progress.db.before-v4.bak").exists());
        })
        .unwrap(),
    );
    assert_eq!(calls.get(), 1);
    drop(Store::open_with_progress(&path, || calls.set(calls.get() + 1)).unwrap());
    assert_eq!(calls.get(), 1);
}
