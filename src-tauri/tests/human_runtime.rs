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
    assert_eq!(
        runtime.period_bands(0, 1000, "UTC").unwrap_err(),
        "human_activity_disabled"
    );
    assert_eq!(
        runtime.summary(0, 1000, "UTC", &[]).unwrap_err(),
        "human_activity_disabled"
    );
    assert!(!runtime.status().busy);
    assert!(runtime.status().generation > 0);
    assert!(!staging.exists());
}

#[test]
fn unidentifiable_existing_source_never_displays_the_old_physical_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("staging"), |_| {});
    let preferences = HumanPreferences {
        enabled: true,
        root: root.clone(),
        auto_refresh: false,
    };
    runtime.configure(preferences.clone(), || Ok(())).unwrap();
    synced(&runtime, 1);
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                ..preferences.clone()
            },
            || Ok(()),
        )
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(3)));
    std::fs::rename(root.join("storage.db"), root.join("old-source.db")).unwrap();
    std::fs::create_dir(root.join("storage.db")).unwrap();
    runtime.configure(preferences, || Ok(())).unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(3)));
    assert!(
        runtime
            .period(0, 10000)
            .map(|v| v.records.is_empty())
            .unwrap_or(true),
        "old physical source was displayed"
    );
    assert!(
        runtime
            .summary(0, 10000, "UTC", &[])
            .map(|v| v.days.is_empty())
            .unwrap_or(true),
        "old physical source was summarized"
    );
    let cached = rusqlite::Connection::open(cache).unwrap();
    assert_eq!(
        cached
            .query_row("SELECT count(*) FROM human_activity_records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
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

#[test]
fn failed_settings_save_does_not_change_enabled_source_or_visible_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), |_| {});
    runtime
        .configure(
            HumanPreferences {
                enabled: true,
                root,
                auto_refresh: false,
            },
            || Ok(()),
        )
        .unwrap();
    synced(&runtime, 1);
    let before = serde_json::to_value(runtime.period(0, 10000).unwrap()).unwrap();
    assert_eq!(
        runtime
            .configure(HumanPreferences::default(), || Err(
                "synthetic_save_failure".into()
            ))
            .unwrap_err(),
        "synthetic_save_failure"
    );
    assert_eq!(
        serde_json::to_value(runtime.period(0, 10000).unwrap()).unwrap(),
        before
    );
    assert_eq!(runtime.status().revision, 1);
}

#[test]
fn replaced_source_is_not_confused_with_retained_old_cache_before_resync() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let preferences = HumanPreferences {
        enabled: true,
        root: root.clone(),
        auto_refresh: false,
    };
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), |_| {});
    runtime.configure(preferences.clone(), || Ok(())).unwrap();
    synced(&runtime, 1);
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                ..preferences.clone()
            },
            || Ok(()),
        )
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    std::fs::rename(root.join("storage.db"), root.join("old.db")).unwrap();
    // A new physical file exists at the same path but is not a supported source.
    std::fs::write(
        root.join("storage.db"),
        b"synthetic unsupported replacement",
    )
    .unwrap();
    runtime.configure(preferences, || Ok(())).unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    assert!(runtime.status().error.is_some());
    assert!(runtime.period(0, 10000).unwrap().records.is_empty());
}

#[test]
fn selected_cache_is_available_before_reopening_or_rescanning_source() {
    use std::sync::{mpsc, Arc, Barrier};
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let preferences = HumanPreferences {
        enabled: true,
        root,
        auto_refresh: false,
    };
    {
        let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("staging"), |_| {});
        runtime.configure(preferences.clone(), || Ok(())).unwrap();
        synced(&runtime, 1);
    }
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = barrier.clone();
    let (sender, receiver) = mpsc::channel();
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), move |status| {
        if status.busy {
            sender.send(()).unwrap();
            worker_barrier.wait();
        }
    });
    runtime.configure(preferences, || Ok(())).unwrap();
    receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let response = runtime.period(0, 10000);
    let bins = runtime.period_bands(0, 10000, "UTC");
    barrier.wait();
    assert_eq!(response.unwrap().records.len(), 1);
    assert_eq!(bins.unwrap().bins.unwrap().len(), 1);
}

#[test]
fn normalized_staging_is_owned_identifiable_and_removed_after_use() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("source");
    source(&root);
    let staging = tmp.path().join("owned-staging");
    std::fs::create_dir(&staging).unwrap();
    let reader = codecal_lib::rei_human::HumanReader::open(&root.join("storage.db")).unwrap();
    let stage = reader.stage_in(&staging, || false).unwrap();
    let files = std::fs::read_dir(&staging)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let file = &files[0];
    assert!(file.file_name().to_str().unwrap().starts_with("human-"));
    assert!(file.file_name().to_str().unwrap().ends_with(".db"));
    let bytes = std::fs::read(file.path()).unwrap();
    assert!(!bytes
        .windows(b"PRIVATE_RUNTIME_MARKER".len())
        .any(|w| w == b"PRIVATE_RUNTIME_MARKER"));
    drop(stage);
    assert_eq!(std::fs::read_dir(staging).unwrap().count(), 0);
}

#[test]
fn clearing_current_source_while_off_preserves_other_sources_ai_and_originals() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    let store = Store::open(&cache).unwrap();
    store
        .set_setting(
            "config",
            r#"{"human_activity_enabled":true,"keep":"synthetic preference"}"#,
        )
        .unwrap();
    drop(store);
    let root = tmp.path().join("one");
    source(&root);
    let other = tmp.path().join("two");
    source(&other);
    let original = std::fs::read(root.join("storage.db")).unwrap();
    let staging = tmp.path().join("staging");
    let runtime = HumanRuntime::new(cache.clone(), staging.clone(), |_| {});
    for location in [&other, &root] {
        runtime
            .configure(
                HumanPreferences {
                    enabled: true,
                    root: location.clone(),
                    auto_refresh: false,
                },
                || Ok(()),
            )
            .unwrap();
        synced(&runtime, 1);
    }
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                root: root.clone(),
                auto_refresh: false,
            },
            || Ok(()),
        )
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    let conn = rusqlite::Connection::open(&cache).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES('keep-project','/synthetic','AI keep','#123456',0);",
    )
    .unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let result = runtime
        .clear(false, |s| {
            let mut config: serde_json::Value =
                serde_json::from_str(&s.get_setting("config").unwrap().unwrap()).unwrap();
            config["human_activity_enabled"] = false.into();
            Ok(config.to_string())
        })
        .unwrap();
    assert!(result.staging_cleared);
    assert!(result.compacted);
    assert_eq!(
        runtime.period(0, 10000).unwrap_err(),
        "human_activity_disabled"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT name FROM projects WHERE id='keep-project'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "AI keep"
    );
    let config: serde_json::Value = serde_json::from_str(
        &Store::open(&cache)
            .unwrap()
            .get_setting("config")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(config["human_activity_enabled"], false);
    assert_eq!(config["keep"], "synthetic preference");
    assert_eq!(std::fs::read(root.join("storage.db")).unwrap(), original);
    runtime.clear(true, |_| Ok(config.to_string())).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn staging_cleanup_never_follows_a_replaced_directory_link() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let outside = tmp.path().join("other-owner");
    std::fs::create_dir(&outside).unwrap();
    let file = outside.join(format!("human-{}-synthetic.db", "0".repeat(64)));
    std::fs::write(&file, b"synthetic other owner's file").unwrap();
    let staging = tmp.path().join("staging");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &staging).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&staging)
        .arg(&outside)
        .output()
        .unwrap()
        .status
        .success());
    let runtime = HumanRuntime::new(cache, staging, |_| {});
    let result = runtime
        .clear(true, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
        .unwrap();
    assert!(
        file.exists(),
        "cleanup crossed the owned directory boundary"
    );
    assert!(!result.staging_cleared);
}

#[test]
fn a_dangling_staging_link_is_not_reported_as_successfully_cleaned() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let target = tmp.path().join("removed-target");
    std::fs::create_dir(&target).unwrap();
    let staging = tmp.path().join("staging");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &staging).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&staging)
        .arg(&target)
        .output()
        .unwrap()
        .status
        .success());
    std::fs::remove_dir(&target).unwrap();
    let runtime = HumanRuntime::new(cache, staging, |_| {});
    assert!(
        !runtime
            .clear(true, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
            .unwrap()
            .staging_cleared
    );
}

#[test]
fn a_job_started_before_clear_cannot_resurrect_cache() {
    use std::sync::{mpsc, Arc, Barrier};
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    let store = Store::open(&cache).unwrap();
    store
        .set_setting("config", r#"{"human_activity_enabled":true}"#)
        .unwrap();
    drop(store);
    let root = tmp.path().join("source");
    source(&root);
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = barrier.clone();
    let (sender, receiver) = mpsc::channel();
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("staging"), move |status| {
        if status.busy {
            sender.send(()).unwrap();
            worker_barrier.wait();
        }
    });
    runtime
        .configure(
            HumanPreferences {
                enabled: true,
                root,
                auto_refresh: true,
            },
            || Ok(()),
        )
        .unwrap();
    receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::scope(|scope| {
        let clear = scope
            .spawn(|| runtime.clear(true, |_| Ok(r#"{"human_activity_enabled":false}"#.into())));
        let deadline = Instant::now() + Duration::from_secs(2);
        let observer = Store::open(&cache).unwrap();
        while observer
            .get_setting("config")
            .unwrap()
            .unwrap()
            .contains("true")
        {
            if Instant::now() >= deadline {
                barrier.wait();
                panic!("clear did not persist OFF");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        barrier.wait();
        assert!(clear.join().unwrap().unwrap().staging_cleared);
    });
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    let conn = rusqlite::Connection::open(cache).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM human_activity_records", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(runtime.status().revision, 0);
    assert_eq!(
        runtime.period(0, 10000).unwrap_err(),
        "human_activity_disabled"
    );
}

#[test]
fn clear_failure_rolls_back_config_cache_and_runtime_without_exposing_sqlite_values() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    let store = Store::open(&cache).unwrap();
    store
        .set_setting("config", r#"{"human_activity_enabled":true}"#)
        .unwrap();
    drop(store);
    let root = tmp.path().join("source");
    source(&root);
    let runtime = HumanRuntime::new(cache.clone(), tmp.path().join("staging"), |_| {});
    runtime
        .configure(
            HumanPreferences {
                enabled: true,
                root,
                auto_refresh: false,
            },
            || Ok(()),
        )
        .unwrap();
    synced(&runtime, 1);
    let conn = rusqlite::Connection::open(&cache).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_clear BEFORE DELETE ON human_activity_records BEGIN SELECT RAISE(ABORT,'PRIVATE_CACHE_DELETE_MARKER'); END;").unwrap();
    let error = runtime
        .clear(true, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
        .err()
        .unwrap();
    assert_eq!(error, "cache_clear_failed");
    assert_eq!(runtime.period(0, 10000).unwrap().records.len(), 1);
    assert!(Store::open(&cache)
        .unwrap()
        .get_setting("config")
        .unwrap()
        .unwrap()
        .contains("true"));
    assert_eq!(runtime.status().revision, 1);
}

#[test]
fn orphan_staging_cleanup_respects_selected_source_and_keeps_unrelated_files() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let staging = tmp.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let first = tmp.path().join("first");
    source(&first);
    let second = tmp.path().join("second");
    source(&second);
    let legacy = rusqlite::Connection::open(first.join("memory.db")).unwrap();
    legacy.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); INSERT INTO activity_records VALUES('synthetic-memory',1000,'{\"durationEstimate\":1}');").unwrap();
    drop(legacy);
    let mut files = vec![];
    for path in [
        first.join("storage.db"),
        first.join("memory.db"),
        second.join("storage.db"),
    ] {
        let reader = codecal_lib::rei_human::HumanReader::open(&path).unwrap();
        let stage = reader.stage_in(&staging, || false).unwrap();
        let file = std::fs::read_dir(&staging)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| !files.contains(&entry.path()))
            .unwrap()
            .path();
        let bytes = std::fs::read(&file).unwrap();
        drop(stage);
        std::fs::write(&file, bytes).unwrap();
        files.push(file);
    }
    let unrelated = staging.join("unrelated.txt");
    std::fs::write(&unrelated, b"keep").unwrap();
    let runtime = HumanRuntime::new(cache, staging, |_| {});
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                root: first,
                auto_refresh: false,
            },
            || Ok(()),
        )
        .unwrap();
    assert!(
        runtime
            .clear(false, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
            .unwrap()
            .staging_cleared
    );
    assert!(!files[0].exists());
    assert!(!files[1].exists());
    assert!(files[2].exists());
    assert!(unrelated.exists());
    assert!(
        runtime
            .clear(true, |_| Ok(r#"{"human_activity_enabled":false}"#.into()))
            .unwrap()
            .staging_cleared
    );
    assert!(!files[2].exists());
    assert!(unrelated.exists());
}

#[test]
fn hourly_auto_requests_wait_while_manual_sync_observes_wal_and_ai_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let writer = rusqlite::Connection::open(root.join("storage.db")).unwrap();
    writer
        .execute_batch("CREATE TABLE agent_events(sequence INTEGER);")
        .unwrap();
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), |_| {});
    runtime
        .configure(
            HumanPreferences {
                enabled: true,
                root,
                auto_refresh: true,
            },
            || Ok(()),
        )
        .unwrap();
    synced(&runtime, 1);
    runtime.request(false);
    assert!(!runtime.wait_idle(Duration::from_millis(1200)));
    assert_eq!(
        runtime.status().revision,
        1,
        "unchanged data_version should not scan/apply"
    );
    writer
        .execute_batch(
            "UPDATE activity_records SET payload=json_set(payload,'$.durationEstimate',3);",
        )
        .unwrap();
    for _ in 0..100 {
        runtime.request(false);
    }
    assert!(!runtime.wait_idle(Duration::from_millis(1200)));
    assert_eq!(runtime.status().revision, 1);
    runtime.request(true);
    synced(&runtime, 2);
    assert!(runtime.wait_idle(Duration::from_secs(3)));
    assert_eq!(runtime.status().revision, 2);
    assert_eq!(runtime.period(0, 10000).unwrap().records[0].end, 4000);
    writer
        .execute_batch("INSERT INTO agent_events VALUES(1);")
        .unwrap();
    runtime.request(false);
    assert!(!runtime.wait_idle(Duration::from_millis(50)));
    runtime.request(true);
    synced(&runtime, 3);
    assert_eq!(runtime.period(0, 10000).unwrap().records[0].end, 4000);
    runtime.request(true);
    synced(&runtime, 4);
    assert_eq!(runtime.period(0, 10000).unwrap().records.len(), 1);
}

#[test]
fn unavailable_source_keeps_cache_and_manual_retry_does_not_wait_for_hourly_schedule() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let root = tmp.path().join("source");
    source(&root);
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), |_| {});
    let preferences = HumanPreferences {
        enabled: true,
        root: root.clone(),
        auto_refresh: true,
    };
    runtime.configure(preferences.clone(), || Ok(())).unwrap();
    synced(&runtime, 1);
    runtime
        .configure(
            HumanPreferences {
                enabled: false,
                ..preferences.clone()
            },
            || Ok(()),
        )
        .unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    std::fs::rename(
        root.join("storage.db"),
        root.join("temporarily-unavailable.db"),
    )
    .unwrap();
    runtime.configure(preferences, || Ok(())).unwrap();
    assert!(runtime.wait_idle(Duration::from_secs(2)));
    assert_eq!(
        runtime.status().error.as_deref(),
        Some("source_unavailable")
    );
    assert_eq!(runtime.period(0, 10000).unwrap().records.len(), 1);
    std::fs::rename(
        root.join("temporarily-unavailable.db"),
        root.join("storage.db"),
    )
    .unwrap();
    runtime.request(true);
    let deadline = Instant::now() + Duration::from_secs(3);
    while runtime.status().revision < 1 {
        assert!(Instant::now() < deadline, "manual retry did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(runtime.status().error.is_none());
    assert_eq!(runtime.period(0, 10000).unwrap().records.len(), 1);
}

#[test]
fn display_grouping_never_joins_the_previous_source_after_switching_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let first = tmp.path().join("first");
    source(&first);
    let second = tmp.path().join("second");
    source(&second);
    rusqlite::Connection::open(second.join("storage.db"))
        .unwrap()
        .execute_batch("UPDATE activity_records SET captured_at=3000;")
        .unwrap();
    let runtime = HumanRuntime::new(cache, tmp.path().join("staging"), |_| {});
    for (root, expected) in [
        (&first, (1000, 3000)),
        (&second, (3000, 5000)),
        (&first, (1000, 3000)),
    ] {
        runtime
            .configure(
                HumanPreferences {
                    enabled: true,
                    root: root.clone(),
                    auto_refresh: false,
                },
                || Ok(()),
            )
            .unwrap();
        synced(&runtime, 1);
        let response = runtime.period_display(0, 10000, "UTC", true).unwrap();
        assert_eq!(response.records.len(), 1);
        assert_eq!(
            (response.records[0].start, response.records[0].end),
            expected
        );
        let daily = runtime
            .summary(
                0,
                3000,
                "UTC",
                &[codecal_lib::rei_human::HumanKind::Entertainment],
            )
            .unwrap();
        assert_eq!(
            daily.days.iter().map(|d| d.total_ms).sum::<i64>(),
            if expected.0 == 1000 { 2000 } else { 0 }
        );
    }
}
