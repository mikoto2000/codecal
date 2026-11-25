use codecal_lib::{
    human_runtime::{HumanPreferences, HumanRuntime},
    store::Store,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn source(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    let c = rusqlite::Connection::open(root.join("storage.db")).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); INSERT INTO activity_records VALUES('synthetic',1000,'{\"durationEstimate\":2,\"foreground\":{\"title\":\"PRIVATE_RUNTIME_MARKER\"},\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"ENTERTAINMENT\"}}}');").unwrap();
}
fn synced(runtime: &HumanRuntime, revision: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.status().revision < revision {
        assert!(runtime.status().error.is_none(), "sync error");
        assert!(Instant::now() < deadline, "sync timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn disabled_runtime_never_opens_missing_source_and_refuses_periods() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let staging = tmp.path().join("staging");
    let runtime = HumanRuntime::new(cache, staging.clone(), |_| {});
    assert_eq!(
        runtime.period(0, 1000).unwrap_err(),
        "human_activity_disabled"
    );
    assert!(!runtime.status().busy);
    assert!(!staging.exists());
}

#[test]
fn human_only_syncs_without_agent_events_and_off_retains_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("staging"), |_| {});
    runtime
        .configure(
            HumanPreferences {
                enabled: true,
                root: root.clone(),
                auto_refresh: true,
            },
            || Ok(()),
        )
        .unwrap();
    synced(&runtime, 1);
    let wire = serde_json::to_string(&runtime.period(0, 10000).unwrap()).unwrap();
    assert!(wire.contains("entertainment"));
    assert!(!wire.contains("PRIVATE_RUNTIME_MARKER"));
    assert!(!wire.contains("synthetic"));
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                root,
                auto_refresh: true,
            },
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        runtime.period(0, 10000).unwrap_err(),
        "human_activity_disabled"
    );
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    let c = rusqlite::Connection::open(cache).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM turns", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
