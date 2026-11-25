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
    let page = reader.read_page(None).unwrap();
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
    let page = HumanReader::open(&path).unwrap().read_page(None).unwrap();
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
    let page = HumanReader::open(&path).unwrap().read_page(None).unwrap();
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
fn source_identity_separates_replacements_and_data_version_tracks_wal_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let first_id = reader.source_id().to_owned();
    let version = reader.data_version().unwrap();
    writer
        .execute_batch(
            "INSERT INTO activity_records VALUES('observed',1000,'{\"durationEstimate\":1}');",
        )
        .unwrap();
    assert_ne!(reader.data_version().unwrap(), version);
    assert_eq!(reader.read_page(None).unwrap().records.len(), 1);
    reader.verify_identity().unwrap();
    drop(reader);
    drop(writer);
    // Windows SQLite's standard VFS keeps an open DB from being renamed.
    // Close it as feature OFF does; replacement must be detected on reopening.
    std::fs::rename(&path, tmp.path().join("old.db")).unwrap();
    let replacement = Connection::open(&path).unwrap();
    replacement
        .execute_batch(
            "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);",
        )
        .unwrap();
    assert_ne!(HumanReader::open(&path).unwrap().source_id(), first_id);
}

#[test]
fn staged_sync_clips_at_next_observation_and_preserves_points_and_gaps() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('a',1000,'{\"durationEstimate\":60,\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"ENTERTAINMENT\"}}}');
        INSERT INTO activity_records VALUES('b',3000,'{\"durationEstimate\":0}');
        INSERT INTO activity_records VALUES('c',5000,'{\"durationEstimate\":1}');").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let stage = reader.stage(|| false).unwrap();
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store.apply_human_stage(&stage, || false).unwrap();
    let response = store
        .human_activity(&[reader.source_id().to_owned()], 0, 10000)
        .unwrap();
    assert_eq!(
        response
            .records
            .iter()
            .map(|r| (r.start, r.end, r.kind))
            .collect::<Vec<_>>(),
        [
            (1000, 3000, HumanKind::Entertainment),
            (3000, 3000, HumanKind::Unknown),
            (5000, 6000, HumanKind::Unknown)
        ]
    );
    assert!(response.diagnostics.is_empty());
    let wire = serde_json::to_string(&response).unwrap();
    assert!(!wire.contains("source_id"));
    assert!(!wire.contains("native_id"));
}

#[test]
fn simultaneous_observations_choose_last_id_and_report_classification_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('a',1000,'{\"durationEstimate\":60,\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"ENTERTAINMENT\"}}}');
        INSERT INTO activity_records VALUES('z',1000,'{\"durationEstimate\":5,\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"NON_ENTERTAINMENT\"}}}');
        INSERT INTO activity_records VALUES('next',3000,'{\"durationEstimate\":0}');").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let response = store
        .human_activity(&[reader.source_id().to_owned()], 0, 10000)
        .unwrap();
    assert_eq!(response.records.len(), 2);
    assert_eq!(response.records[0].kind, HumanKind::NonEntertainment);
    assert_eq!(response.records[0].end, 3000);
    assert_eq!(
        response
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::SimultaneousConflict)
            .map(|d| d.count),
        Some(1)
    );
}

#[test]
fn paging_includes_zero_negative_and_maximum_sqlite_rowids() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records(rowid,id,captured_at,payload) VALUES(-9223372036854775808,'min',1000,'{\"durationEstimate\":1}'),(0,'zero',2000,'{\"durationEstimate\":1}'),(9223372036854775807,'max',3000,'{\"durationEstimate\":1}');").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    assert_eq!(reader.read_page(None).unwrap().records.len(), 3);
    let stage = reader.stage(|| false).unwrap();
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store.apply_human_stage(&stage, || false).unwrap();
    assert_eq!(
        store
            .human_activity(&[reader.source_id().to_owned()], 0, 10000)
            .unwrap()
            .records
            .len(),
        3
    );
}

#[test]
fn a_source_without_a_cache_returns_an_empty_period() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("cache.db")).unwrap();
    let response = store
        .human_activity(&["not-yet-synced".into()], 0, 1000)
        .unwrap();
    assert!(response.records.is_empty());
    assert!(response.diagnostics.is_empty());
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
