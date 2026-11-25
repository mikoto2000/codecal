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
fn dense_period_fails_explicitly_instead_of_truncating_or_allocating_unbounded_records() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cache.db");
    let store = Store::open(&path).unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("INSERT INTO human_activity_sources VALUES('s','synthetic','identity',1,0,'synced','[]');
        WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50001)
        INSERT INTO human_activity_records SELECT 's',printf('id-%d',x),x,1,'unknown',printf('hash-%d',x),x+1 FROM n;").unwrap();
    let response = store.human_activity(&["s".into()], 0, 100000);
    assert!(
        response.is_err(),
        "a dense period must return an explicit error"
    );
    assert_eq!(response.unwrap_err().to_string(), "period_too_dense");
}

#[test]
fn copied_sources_are_deduplicated_after_their_different_effective_ends_are_resolved() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.db");
    let b = tmp.path().join("b.db");
    let c = tmp.path().join("c.db");
    let writer = Connection::open(&a).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('same',1000,'{\"durationEstimate\":60}');",
        )
        .unwrap();
    drop(writer);
    // These are generated fixtures, never a user's original Rei database.
    std::fs::copy(&a, &b).unwrap();
    let writer = Connection::open(&a).unwrap();
    writer
        .execute_batch(
            "INSERT INTO activity_records VALUES('next-a',3000,'{\"durationEstimate\":0}');",
        )
        .unwrap();
    drop(writer);
    let writer = Connection::open(&b).unwrap();
    writer
        .execute_batch(
            "INSERT INTO activity_records VALUES('next-b',5000,'{\"durationEstimate\":0}');",
        )
        .unwrap();
    drop(writer);
    std::fs::copy(&b, &c).unwrap();
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    let mut sources = vec![];
    for path in [&a, &b, &c] {
        let reader = HumanReader::open(path).unwrap();
        store
            .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
            .unwrap();
        sources.push(reader.source_id().to_owned());
    }
    let response = store.human_activity(&sources, 0, 10000).unwrap();
    assert_eq!(response.records.len(), 4);
    assert_eq!(
        response
            .records
            .iter()
            .filter(|r| r.start == 1000)
            .map(|r| r.end)
            .collect::<Vec<_>>(),
        [3000, 5000]
    );
    assert_eq!(
        store
            .human_activity(&sources[..1], 0, 10000)
            .unwrap()
            .records
            .len(),
        2
    );
}

#[test]
fn updates_are_idempotent_and_invalid_updates_deletions_and_cancellation_keep_last_good_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let cache = tmp.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('a',1000,'{\"durationEstimate\":1}');").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let sources = [reader.source_id().to_owned()];
    let mut store = Store::open(&cache).unwrap();
    let initial = reader.stage(|| false).unwrap();
    store.apply_human_stage(&initial, || false).unwrap();
    store.apply_human_stage(&initial, || false).unwrap();
    assert_eq!(
        store
            .human_activity(&sources, 0, 10000)
            .unwrap()
            .records
            .len(),
        1
    );
    writer
        .execute_batch(
            "UPDATE activity_records SET payload='{\"durationEstimate\":2}' WHERE id='a';",
        )
        .unwrap();
    let changed = reader.stage(|| false).unwrap();
    let calls = std::cell::Cell::new(0);
    let error = store
        .apply_human_stage(&changed, || {
            calls.set(calls.get() + 1);
            calls.get() > 2
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "sync_cancelled");
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records[0].end,
        2000
    );
    store.apply_human_stage(&changed, || false).unwrap();
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records[0].end,
        3000
    );
    assert_eq!(
        reader.stage(|| true).err().unwrap().to_string(),
        "sync_cancelled"
    );
    writer
        .execute_batch("UPDATE activity_records SET payload='SECRET_INVALID_UPDATE';")
        .unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records[0].end,
        3000
    );
    writer
        .execute_batch("DELETE FROM activity_records;")
        .unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records[0].end,
        3000
    );
}

#[test]
fn source_errors_are_fixed_codes_and_ai_only_sources_are_unsupported() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ai-only.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE agent_events(id TEXT);")
        .unwrap();
    assert_eq!(
        HumanReader::open(&path).err().unwrap().to_string(),
        "unsupported_source"
    );
    let malformed = tmp.path().join("SECRET_SOURCE_NAME.db");
    std::fs::write(&malformed, b"SECRET_NOT_A_DATABASE").unwrap();
    let error = HumanReader::open(&malformed).err().unwrap().to_string();
    assert!(!error.contains("SECRET"));
    assert!(!error.contains(tmp.path().to_str().unwrap()));
}

#[test]
fn privacy_outputs_and_logs_do_not_contain_the_synthetic_secret() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "human_privacy_child", "--nocapture"])
        .output()
        .unwrap();
    assert!(output.status.success(), "isolated privacy scenario failed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "isolated privacy scenario did not execute"
    );
    for bytes in [&output.stdout, &output.stderr] {
        for marker in [
            b"PRIVATE_HUMAN_MARKER".as_slice(),
            b"PRIVATE_ERROR_MARKER".as_slice(),
        ] {
            assert!(
                !bytes.windows(marker.len()).any(|w| w == marker),
                "private source value leaked to process output"
            );
        }
    }
}

#[test]
fn human_privacy_child() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let cache = tmp.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
    let payload = serde_json::json!({"durationEstimate":2,"duplicate":true,"foreground":{"title":"PRIVATE_HUMAN_MARKER","application":"PRIVATE_HUMAN_MARKER","process":"PRIVATE_HUMAN_MARKER","command":"PRIVATE_HUMAN_MARKER","path":"PRIVATE_HUMAN_MARKER","url":"PRIVATE_HUMAN_MARKER"},"observations":["PRIVATE_HUMAN_MARKER"],"screenshotReferences":["PRIVATE_HUMAN_MARKER"],"ocr":"PRIVATE_HUMAN_MARKER","inference":{"summary":"PRIVATE_HUMAN_MARKER","activities":[{"projectCandidate":"PRIVATE_HUMAN_MARKER"}]},"userInput":"PRIVATE_HUMAN_MARKER","detection":{"reason":"PRIVATE_HUMAN_MARKER","diagnostics":{"entertainmentDisposition":"PRIVATE_HUMAN_MARKER","entertainmentReason":"PRIVATE_HUMAN_MARKER"}}});
    writer
        .execute(
            "INSERT INTO activity_records VALUES('observation',1000,?)",
            [payload.to_string()],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO activity_records VALUES('bad',5000,?)",
            ["PRIVATE_HUMAN_MARKER{"],
        )
        .unwrap();
    let db_before = std::fs::read(&path).unwrap();
    let wal_path = path.with_file_name("source.db-wal");
    let wal_before = std::fs::read(&wal_path).unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let mut store = Store::open(&cache).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let sources = [reader.source_id().to_owned()];
    let response = store.human_activity(&sources, 0, 10000).unwrap();
    let wire = serde_json::to_string(&response).unwrap();
    assert!(!wire.contains("PRIVATE_HUMAN_MARKER"));
    let record = serde_json::to_value(&response.records[0]).unwrap();
    assert_eq!(
        record
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["end", "kind", "start"]
    );
    println!("{wire}");
    assert_eq!(std::fs::read(&path).unwrap(), db_before);
    assert_eq!(std::fs::read(&wal_path).unwrap(), wal_before);
    assert!(store.snapshot("UTC").unwrap().turns.is_empty());
    assert!(!serde_json::to_string(&store.snapshot("UTC").unwrap())
        .unwrap()
        .contains("PRIVATE_HUMAN_MARKER"));
    let conn = Connection::open(&cache).unwrap();
    let hash: String = conn
        .query_row(
            "SELECT hex(content_hash) FROM human_activity_records",
            [],
            |r| r.get(0),
        )
        .unwrap();
    writer.execute("UPDATE activity_records SET payload=? WHERE id='observation'",["{\"durationEstimate\":2,\"foreground\":{\"title\":\"PRIVATE_HUMAN_MARKER_CHANGED\"},\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"PRIVATE_HUMAN_MARKER_CHANGED\"}}}"]).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT hex(content_hash) FROM human_activity_records",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        hash
    );
    conn.execute_batch("CREATE TRIGGER reject_human BEFORE INSERT ON human_activity_records BEGIN SELECT RAISE(ABORT,'PRIVATE_ERROR_MARKER'); END;").unwrap();
    // Exercise a real normalized delta; unchanged rows no longer issue writes.
    writer
        .execute(
            "UPDATE activity_records SET payload='{\"durationEstimate\":3}' WHERE id='observation'",
            [],
        )
        .unwrap();
    let error = store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap_err();
    assert_eq!(error.to_string(), "cache_write_failed");
    eprintln!("{error}");
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records,
        response.records
    );
    for suffix in ["", "-wal", "-shm"] {
        let file = cache.with_file_name(format!("cache.db{suffix}"));
        if file.exists() {
            let bytes = std::fs::read(file).unwrap();
            assert!(!bytes
                .windows(b"PRIVATE_HUMAN_MARKER".len())
                .any(|w| w == b"PRIVATE_HUMAN_MARKER"));
        }
    }
}

#[test]
fn paged_snapshot_is_consistent_and_concurrent_wal_updates_remain_detectable() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<599)
        INSERT INTO activity_records SELECT printf('id-%d',x),x*1000,'{\"durationEstimate\":1}' FROM n;").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let calls = std::cell::Cell::new(0);
    let stage = reader.stage(|| {
        calls.set(calls.get()+1);
        if calls.get()==4 {
            writer.execute_batch("UPDATE activity_records SET payload='{\"durationEstimate\":1,\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"ENTERTAINMENT\"}}}' WHERE id='id-0';
                INSERT INTO activity_records VALUES('late',700000,'{\"durationEstimate\":1}');").unwrap();
        }
        false
    }).unwrap();
    assert_ne!(stage.data_version(), reader.data_version().unwrap());
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    let sources = [reader.source_id().to_owned()];
    store.apply_human_stage(&stage, || false).unwrap();
    let old = store.human_activity(&sources, 0, 1_000_000).unwrap();
    assert_eq!(old.records.len(), 600);
    assert_eq!(old.records[0].kind, HumanKind::Unknown);
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let new = store.human_activity(&sources, 0, 1_000_000).unwrap();
    assert_eq!(new.records.len(), 601);
    assert_eq!(new.records[0].kind, HumanKind::Entertainment);
    writer
        .execute_batch("DROP TABLE activity_records;")
        .unwrap();
    assert!(reader.stage(|| false).is_err());
    assert_eq!(
        store
            .human_activity(&sources, 0, 1_000_000)
            .unwrap()
            .records,
        new.records
    );
}

#[test]
fn periods_include_prior_overlap_but_exclude_the_end_and_do_not_change_effective_intervals() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        INSERT INTO activity_records VALUES('a',1000,'{\"durationEstimate\":60}');
        INSERT INTO activity_records VALUES('b',3000,'{\"durationEstimate\":0}');
        INSERT INTO activity_records VALUES('c',5000,'{\"durationEstimate\":1}');",
        )
        .unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let sources = [reader.source_id().to_owned()];
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let before = store.human_activity(&sources, 2000, 3000).unwrap();
    assert_eq!(before.records.len(), 1);
    assert_eq!(
        (before.records[0].start, before.records[0].end),
        (1000, 3000)
    );
    let points = store.human_activity(&sources, 3000, 5000).unwrap();
    assert_eq!(points.records.len(), 1);
    assert_eq!(
        (points.records[0].start, points.records[0].end),
        (3000, 3000)
    );
    assert_eq!(
        store.human_activity(&sources, 0, 10000).unwrap().records[0],
        before.records[0]
    );
    assert_eq!(
        store
            .human_activity(&sources, 0, 63 * 86_400_000)
            .unwrap_err()
            .to_string(),
        "invalid_period"
    );
}

#[test]
fn large_import_and_small_timestamp_updates_obey_the_same_interval_and_conflict_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<4999)
        INSERT INTO activity_records SELECT printf('id-%d',x),x*1000,'{\"durationEstimate\":2}' FROM n;
        INSERT INTO activity_records VALUES('zz-tie',1000,'{\"durationEstimate\":0,\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"ENTERTAINMENT\"}}}');").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let sources = [reader.source_id().to_owned()];
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let first = store.human_activity(&sources, 0, 6_000_000).unwrap();
    assert_eq!(first.records.len(), 5000);
    assert_eq!((first.records[0].start, first.records[0].end), (0, 1000));
    assert_eq!(
        (
            first.records[1].start,
            first.records[1].end,
            first.records[1].kind
        ),
        (1000, 1000, HumanKind::Entertainment)
    );
    assert_eq!(first.records.last().unwrap().end, 5_001_000);
    assert_eq!(
        first
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::SimultaneousConflict)
            .unwrap()
            .count,
        1
    );
    writer
        .execute_batch("UPDATE activity_records SET captured_at=2500 WHERE id='zz-tie';")
        .unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let next = store.human_activity(&sources, 0, 6_000_000).unwrap();
    assert_eq!(next.records.len(), 5001);
    assert_eq!(
        next.records.iter().find(|r| r.start == 1000).unwrap().end,
        2000
    );
    assert_eq!(
        next.records.iter().find(|r| r.start == 2000).unwrap().end,
        2500
    );
    assert_eq!(
        next.records.iter().find(|r| r.start == 2500).unwrap().end,
        2500
    );
    assert!(!next
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::SimultaneousConflict));
}

#[test]
fn changing_a_directory_link_is_detected_even_when_the_old_database_still_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let link = tmp.path().join("selected");
    for root in [&a, &b] {
        std::fs::create_dir(root).unwrap();
        Connection::open(root.join("storage.db")).unwrap().execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
    }
    fn directory_link(target: &std::path::Path, link: &std::path::Path) {
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status
                .success(),
            "cannot create synthetic junction"
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
    }
    directory_link(&a, &link);
    let reader = HumanReader::open(&link.join("storage.db")).unwrap();
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
    directory_link(&b, &link);
    assert_eq!(
        reader.verify_identity().unwrap_err().to_string(),
        "source_replaced"
    );
    assert_ne!(
        HumanReader::open(&link.join("storage.db"))
            .unwrap()
            .source_id(),
        reader.source_id()
    );
}

#[test]
fn failed_large_first_import_restores_indexes_and_an_empty_cache_before_retry() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    let cache = tmp.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
        WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<5000)
        INSERT INTO activity_records SELECT printf('id-%d',x),x*1000,'{\"durationEstimate\":1}' FROM n;").unwrap();
    let reader = HumanReader::open(&path).unwrap();
    let stage = reader.stage(|| false).unwrap();
    let mut store = Store::open(&cache).unwrap();
    let conn = Connection::open(&cache).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_human BEFORE INSERT ON human_activity_records BEGIN SELECT RAISE(ABORT,'SYNTHETIC_FAILURE'); END;").unwrap();
    assert_eq!(
        store
            .apply_human_stage(&stage, || false)
            .unwrap_err()
            .to_string(),
        "cache_write_failed"
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN ('human_activity_start','human_activity_end')",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER reject_human;").unwrap();
    store.apply_human_stage(&stage, || false).unwrap();
    assert_eq!(
        store
            .human_activity(&[reader.source_id().to_owned()], 0, 6_000_000)
            .unwrap()
            .records
            .len(),
        5000
    );
}

#[test]
fn generated_payload_columns_are_rejected_before_reading_an_unsupported_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("source.db");
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,private_body TEXT,payload TEXT GENERATED ALWAYS AS(private_body) VIRTUAL);").unwrap();
    assert!(
        HumanReader::open(&path).is_err(),
        "only Rei's ordinary stored columns are supported"
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
