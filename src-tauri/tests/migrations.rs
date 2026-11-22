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
    c.execute_batch("CREATE TABLE claude_files(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL);
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
    assert_eq!(version(&path), 3);
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
    assert_eq!(version(&path), 3);
    let backup = tmp.path().join("old.db.before-v3.bak");
    assert!(backup.is_file());
    assert_eq!(version(&backup), 2);
    let before = std::fs::read(&backup).unwrap();
    drop(Store::open(&path).unwrap());
    assert_eq!(std::fs::read(&backup).unwrap(), before);
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
    assert_eq!(version(&path), 3);
}
