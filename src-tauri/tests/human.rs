use codecal_lib::rei_human::{DiagnosticCode, HumanKind, HumanReader};
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
fn source_projects_only_normalized_classification_and_estimated_time() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER NOT NULL,payload TEXT NOT NULL);").unwrap();
    for (i, disposition) in [
        Some("ENTERTAINMENT"),
        Some("NON_ENTERTAINMENT"),
        Some("UNCERTAIN"),
        None,
        Some("SECRET_UNKNOWN_DISPOSITION"),
    ]
    .into_iter()
    .enumerate()
    {
        let payload = serde_json::json!({"durationEstimate":60,"foreground":{"title":"SECRET_WINDOW_TITLE","application":"SECRET_APPLICATION"},"inference":{"summary":"SECRET_INFERENCE"},"detection":{"diagnostics":{"entertainmentDisposition":disposition}}});
        writer
            .execute(
                "INSERT INTO activity_records VALUES(?,?,?)",
                rusqlite::params![format!("id-{i}"), 1000 + i as i64, payload.to_string()],
            )
            .unwrap();
    }
    let reader = HumanReader::open(&path).unwrap();
    let page = reader.read_page(0).unwrap();
    assert_eq!(
        page.records.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [
            HumanKind::Entertainment,
            HumanKind::NonEntertainment,
            HumanKind::Unknown,
            HumanKind::Unknown,
            HumanKind::Unknown
        ]
    );
    assert_eq!(page.records[0].captured_at, 1000);
    assert_eq!(page.records[0].duration_ms, 60000);
    assert!(page.diagnostics.is_empty());
}

#[test]
fn invalid_json_is_counted_without_blocking_valid_observations() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('bad',1000,'SECRET_INVALID_JSON{');
        INSERT INTO activity_records VALUES('type',1001,'{\"durationEstimate\":\"SECRET_DURATION\"}');
        INSERT INTO activity_records VALUES('overflow',1002,'{\"durationEstimate\":9223372036854775808}');
        INSERT INTO activity_records VALUES('good',2000,'{\"durationEstimate\":1}');").unwrap();
    writer
        .execute(
            "INSERT INTO activity_records VALUES('huge',1003,?)",
            ["SECRET_HUGE".to_owned() + &"x".repeat(1024 * 1024)],
        )
        .unwrap();
    let page = HumanReader::open(&path).unwrap().read_page(0).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].native_id, "good");
    assert_eq!(page.diagnostics[0].code, DiagnosticCode::InvalidJson);
    assert_eq!(page.diagnostics[0].count, 1);
    assert_eq!(
        page.diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::PayloadTooLarge)
            .unwrap()
            .count,
        1
    );
    assert_eq!(
        page.diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::InvalidDurationType)
            .unwrap()
            .count,
        2
    );
    assert!(!serde_json::to_string(&page.diagnostics)
        .unwrap()
        .contains("SECRET_"));
}

#[test]
fn invalid_time_estimates_are_skipped_and_zero_and_long_estimates_survive() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('zero',1000,'{\"durationEstimate\":0}');
        INSERT INTO activity_records VALUES('negative',1000,'{\"durationEstimate\":-1}');
        INSERT INTO activity_records VALUES('overflow',1000,'{\"durationEstimate\":9223372036854775807}');
        INSERT INTO activity_records VALUES('bad-date',9223372036854775807,'{\"durationEstimate\":1}');
        INSERT INTO activity_records VALUES('bad-type','SECRET_DATE','{\"durationEstimate\":1}');
        INSERT INTO activity_records VALUES('long',1000,'{\"durationEstimate\":172800}');").unwrap();
    let page = HumanReader::open(&path).unwrap().read_page(0).unwrap();
    assert_eq!(
        page.records
            .iter()
            .map(|r| r.native_id.as_str())
            .collect::<Vec<_>>(),
        ["zero", "long"]
    );
    assert_eq!(page.records[0].duration_ms, 0);
    assert_eq!(page.records[1].duration_ms, 172800000);
    for (code, count) in [
        (DiagnosticCode::InvalidDuration, 2),
        (DiagnosticCode::InvalidTimestamp, 2),
        (DiagnosticCode::LongEstimate, 1),
    ] {
        assert_eq!(
            page.diagnostics
                .iter()
                .find(|d| d.code == code)
                .unwrap()
                .count,
            count
        );
    }
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
