use codecal_lib::{
    human_runtime::{HumanPreferences, HumanRuntime},
    store::Store,
};
use rusqlite::Connection;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn observation(path: &Path, seconds: i64) {
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
    c.execute("INSERT INTO activity_records VALUES('synthetic-id',1000,?)",[format!(r#"{{"durationEstimate":{seconds},"foreground":{{"title":"PRIVATE_SOURCE_FIX_MARKER"}}}}"#)]).unwrap();
}
fn prefs(root: &Path, enabled: bool) -> HumanPreferences {
    HumanPreferences {
        enabled,
        root: root.into(),
        auto_refresh: true,
    }
}
fn synced(r: &HumanRuntime, revision: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while r.status().revision < revision {
        assert!(
            r.status().error.is_none(),
            "unexpected fixed sync error: {:?}",
            r.status().error
        );
        assert!(Instant::now() < deadline, "sync timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(r.wait_idle(Duration::from_secs(5)));
}

#[test]
fn legacy_memory_observations_work_with_an_ai_only_storage_database() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    std::fs::create_dir(&root).unwrap();
    observation(&root.join("memory.db"), 2);
    Connection::open(root.join("storage.db"))
        .unwrap()
        .execute_batch("CREATE TABLE agent_events(sequence INTEGER);")
        .unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let runtime = HumanRuntime::new(cache, tmp.path().join("stage"), |_| {});
    runtime.configure(prefs(&root, true), || Ok(())).unwrap();
    synced(&runtime, 1);
    let records = runtime.period(0, 10000).unwrap();
    assert_eq!(records.records.len(), 1);
    assert_eq!(records.records[0].end, 3000);
    assert!(!serde_json::to_string(&records)
        .unwrap()
        .contains("PRIVATE_SOURCE_FIX_MARKER"));
    assert_eq!(runtime.summary(0, 10000, "UTC", &[]).unwrap().days.len(), 1);
}

#[test]
fn storage_observations_take_priority_and_switch_reset_the_retained_reader_and_version() {
    let tmp = tempfile::tempdir().unwrap();
    observation(&tmp.path().join("memory.db"), 2);
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("stage"), |_| {});
    runtime
        .configure(prefs(tmp.path(), true), || Ok(()))
        .unwrap();
    synced(&runtime, 1);
    observation(&tmp.path().join("storage.db"), 7);
    runtime.request(false);
    synced(&runtime, 2);
    let records = runtime.period(0, 10000).unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].end, 8000);
    let c = Connection::open(&cache).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    runtime
        .configure(prefs(tmp.path(), false), || Ok(()))
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(5)));
    observation(&tmp.path().join("other.db"), 9);
    std::fs::rename(
        tmp.path().join("memory.db"),
        tmp.path().join("offline-memory.db"),
    )
    .unwrap();
    std::fs::rename(
        tmp.path().join("storage.db"),
        tmp.path().join("offline-storage.db"),
    )
    .unwrap();
    runtime
        .configure(prefs(tmp.path(), true), || Ok(()))
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(5)));
    assert_eq!(runtime.period(0, 10000).unwrap().records[0].end, 8000);
}

#[test]
fn an_invalid_primary_database_does_not_fall_back_to_another_physical_source() {
    let tmp = tempfile::tempdir().unwrap();
    observation(&tmp.path().join("memory.db"), 2);
    Connection::open(tmp.path().join("storage.db")).unwrap().execute_batch("CREATE TABLE activity_records(id INTEGER PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let runtime = HumanRuntime::new(cache, tmp.path().join("stage"), |_| {});
    runtime
        .configure(prefs(tmp.path(), true), || Ok(()))
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(5)));
    assert_eq!(
        runtime.status().error.as_deref(),
        Some("unsupported_source")
    );
    assert!(runtime.period(0, 10000).unwrap().records.is_empty());
}

#[test]
fn clearing_the_current_root_removes_both_database_generations_but_preserves_other_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("rei");
    let other = tmp.path().join("other");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&other).unwrap();
    observation(&root.join("memory.db"), 2);
    observation(&other.join("storage.db"), 4);
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("stage"), |_| {});
    runtime.configure(prefs(&root, true), || Ok(())).unwrap();
    synced(&runtime, 1);
    observation(&root.join("storage.db"), 7);
    runtime.request(true);
    synced(&runtime, 2);
    runtime.configure(prefs(&other, true), || Ok(())).unwrap();
    synced(&runtime, 1);
    runtime.configure(prefs(&root, false), || Ok(())).unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(5)));
    let result = runtime
        .clear(false, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
        .unwrap();
    assert!(result.compacted && result.staging_cleared);
    let c = Connection::open(&cache).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(root.join("storage.db").is_file() && root.join("memory.db").is_file());
}
