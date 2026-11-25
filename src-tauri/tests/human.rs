use codecal_lib::store::Store;
use rusqlite::Connection;

#[test]
fn new_database_has_separate_human_cache_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("codecal.db");
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    let tables: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('human_activity_sources','human_activity_records')", [], |r| r.get(0)).unwrap();
    assert_eq!(tables, 2);
    let columns = conn
        .prepare("PRAGMA table_info(human_activity_records)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "source_id",
            "native_id",
            "captured_at",
            "duration_ms",
            "kind",
            "content_hash",
            "effective_end"
        ]
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM turns", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn migration_backup_excludes_human_cache_and_keeps_ai_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("codecal.db");
    let store = Store::open(&path).unwrap();
    store.set_setting("keep", "AI setting").unwrap();
    drop(store);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("INSERT INTO human_activity_sources VALUES('source','SYNTHETIC_SOURCE_PATH','identity',1,0,'synced','[]');
        INSERT INTO human_activity_records VALUES('source','SYNTHETIC_NATIVE_ID',1000,1000,'unknown','hash',2000);
        PRAGMA user_version=3;").unwrap();
    drop(conn);
    drop(Store::open(&path).unwrap());
    let backup = Connection::open(tmp.path().join("codecal.db.before-v4.bak")).unwrap();
    assert_eq!(
        backup
            .query_row("SELECT count(*) FROM human_activity_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        backup
            .query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        backup
            .query_row("SELECT value FROM settings WHERE key='keep'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "AI setting"
    );
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM human_activity_records", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
