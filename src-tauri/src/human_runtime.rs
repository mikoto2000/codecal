//! Human-only synchronization control. No activity contents enter this module.
use crate::{
    rei_human::{HumanActivityResponse, HumanReader},
    store::Store,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const AUTO_SYNC_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Default, Serialize)]
pub struct HumanSyncStatus {
    pub generation: u64,
    pub busy: bool,
    pub revision: u64,
    pub error: Option<String>,
    pub last_success: Option<i64>,
}
#[derive(Serialize)]
pub struct HumanClearResult {
    pub compacted: bool,
    pub staging_cleared: bool,
}

fn purge_staging(directory: &Path, prefix: Option<&str>) -> bool {
    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
        Ok(_) => {}
    }
    if crate::rei_human::validate_staging_directory(directory).is_err() {
        return false;
    }
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    let mut complete = true;
    for entry in entries {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let owned = name.starts_with("human-")
            && name.len() > 72
            && name.as_bytes()[6..70].iter().all(u8::is_ascii_hexdigit)
            && name.as_bytes()[70] == b'-'
            && [".db", ".db-journal", ".db-wal", ".db-shm"]
                .iter()
                .any(|suffix| name.ends_with(suffix));
        if !owned || prefix.is_some_and(|prefix| !name.starts_with(prefix)) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_file())
            && std::fs::remove_file(entry.path()).is_err()
        {
            complete = false;
        }
    }
    complete
}

#[derive(Clone, Default)]
pub struct HumanPreferences {
    pub enabled: bool,
    pub root: PathBuf,
    pub auto_refresh: bool,
}
struct Control {
    preferences: HumanPreferences,
    status: HumanSyncStatus,
    pending: bool,
    force: bool,
    settled_epoch: u64,
}
struct Shared {
    control: Mutex<Control>,
    wake: Condvar,
    operation: Mutex<()>,
    gate: Mutex<()>,
    epoch: AtomicU64,
    stopping: AtomicBool,
    cache: PathBuf,
    staging: PathBuf,
    notify: Box<dyn Fn(HumanSyncStatus) + Send + Sync>,
    auto_interval: Duration,
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
fn cache(path: &Path, read_only: bool) -> Result<Store, String> {
    let conn = Connection::open_with_flags(
        path,
        if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI
        },
    )
    .map_err(|_| "cache_open_failed")?;
    conn.busy_timeout(Duration::from_millis(500))
        .map_err(|_| "cache_open_failed")?;
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(|_| "cache_open_failed")?;
    if read_only {
        conn.pragma_update(None, "query_only", true)
            .map_err(|_| "cache_open_failed")?;
    }
    Ok(Store { conn })
}
pub struct HumanRuntime {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl HumanRuntime {
    /// Synthetic benchmark only. Desktop always uses `new` with the hourly interval.
    #[cfg(feature = "human-benchmark")]
    pub fn new_for_benchmark(
        cache: PathBuf,
        staging: PathBuf,
        notify: impl Fn(HumanSyncStatus) + Send + Sync + 'static,
    ) -> Self {
        Self::with_auto_interval(cache, staging, notify, Duration::from_secs(1))
    }
    pub fn new(
        cache: PathBuf,
        staging: PathBuf,
        notify: impl Fn(HumanSyncStatus) + Send + Sync + 'static,
    ) -> Self {
        Self::with_auto_interval(cache, staging, notify, AUTO_SYNC_INTERVAL)
    }
    fn with_auto_interval(
        cache: PathBuf,
        staging: PathBuf,
        notify: impl Fn(HumanSyncStatus) + Send + Sync + 'static,
        auto_interval: Duration,
    ) -> Self {
        let shared = Arc::new(Shared {
            control: Mutex::new(Control {
                preferences: HumanPreferences::default(),
                status: HumanSyncStatus {
                    generation: 1,
                    ..HumanSyncStatus::default()
                },
                pending: false,
                force: false,
                settled_epoch: 1,
            }),
            wake: Condvar::new(),
            operation: Mutex::new(()),
            gate: Mutex::new(()),
            epoch: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
            cache,
            staging,
            notify: Box::new(notify),
            auto_interval,
        });
        let state = shared.clone();
        let worker = std::thread::spawn(move || run(state));
        Self {
            shared,
            worker: Some(worker),
        }
    }
    pub fn status(&self) -> HumanSyncStatus {
        lock(&self.shared.control).status.clone()
    }
    pub fn configure(
        &self,
        preferences: HumanPreferences,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let _operation = lock(&self.shared.operation);
        let _gate = lock(&self.shared.gate);
        persist()?;
        let mut control = lock(&self.shared.control);
        let changed = control.preferences.enabled != preferences.enabled
            || control.preferences.root != preferences.root;
        if changed {
            let generation = self.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            control.status = HumanSyncStatus {
                generation,
                ..HumanSyncStatus::default()
            };
            control.pending = preferences.enabled;
            control.force = preferences.enabled;
        }
        if !preferences.auto_refresh && !control.force {
            control.pending = false;
        }
        control.preferences = preferences;
        self.shared.wake.notify_all();
        Ok(())
    }
    pub fn request(&self, force: bool) {
        let mut control = lock(&self.shared.control);
        if !control.preferences.enabled || (!force && !control.preferences.auto_refresh) {
            return;
        }
        control.pending = true;
        control.force |= force;
        self.shared.wake.notify_all();
    }
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let began = Instant::now();
        let mut control = lock(&self.shared.control);
        while control.status.busy
            || control.pending
            || control.settled_epoch != self.shared.epoch.load(Ordering::SeqCst)
        {
            let Some(left) = timeout.checked_sub(began.elapsed()) else {
                return false;
            };
            control = self
                .shared
                .wake
                .wait_timeout(control, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
    /// The adapter derives disabled settings from the saved CodeCal config.
    /// Its update and the selected cache deletion commit in one transaction.
    pub fn clear(
        &self,
        all: bool,
        disabled_config: impl FnOnce(&Store) -> Result<String, String>,
    ) -> Result<HumanClearResult, String> {
        let _operation = lock(&self.shared.operation);
        let gate = lock(&self.shared.gate);
        let root = lock(&self.shared.control).preferences.root.clone();
        let paths = crate::rei_human::SOURCE_FILES.map(|name| root.join(name));
        let logical = paths
            .iter()
            .map(|path| crate::rei_human::logical_source_path(path).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let prefixes = paths
            .iter()
            .map(|path| crate::rei_human::staging_prefix(path).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut store = cache(&self.shared.cache, false)?;
        let config = disabled_config(&store)?;
        #[derive(Deserialize)]
        struct Disabled {
            human_activity_enabled: bool,
        }
        if serde_json::from_str::<Disabled>(&config)
            .map_err(|_| "invalid_config")?
            .human_activity_enabled
        {
            return Err("invalid_config".into());
        }
        store
            .conn
            .pragma_update(None, "secure_delete", true)
            .map_err(|_| "cache_clear_failed")?;
        let tx = store.conn.transaction().map_err(|_| "cache_clear_failed")?;
        tx.execute(
            "DELETE FROM human_activity_sources WHERE ?1 OR source_path IN (?2,?3)",
            rusqlite::params![all, logical[0], logical[1]],
        )
        .map_err(|_| "cache_clear_failed")?;
        tx.execute("INSERT INTO settings VALUES('config',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&config]).map_err(|_| "cache_clear_failed")?;
        tx.commit().map_err(|_| "cache_clear_failed")?;
        {
            let mut control = lock(&self.shared.control);
            let generation = self.shared.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            control.preferences.enabled = false;
            control.pending = false;
            control.force = false;
            control.status = HumanSyncStatus {
                generation,
                ..HumanSyncStatus::default()
            };
            self.shared.wake.notify_all();
        }
        // A stale job can be waiting for this gate; release it before waiting.
        drop(gate);
        let idle = self.wait_idle(Duration::from_secs(35));
        let staging_cleared = idle
            && if all {
                purge_staging(&self.shared.staging, None)
            } else {
                let mut complete = true;
                for prefix in &prefixes {
                    complete &= purge_staging(&self.shared.staging, Some(prefix));
                }
                complete
            };
        let compacted = store.conn.execute_batch("VACUUM;").is_ok()
            && store
                .conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                    r.get::<_, i64>(0)
                })
                .is_ok_and(|busy| busy == 0);
        Ok(HumanClearResult {
            compacted,
            staging_cleared,
        })
    }
    pub fn period(&self, from: i64, to: i64) -> Result<HumanActivityResponse, String> {
        self.period_display(from, to, "UTC", false)
    }
    pub fn period_display(
        &self,
        from: i64,
        to: i64,
        timezone: &str,
        grouped: bool,
    ) -> Result<HumanActivityResponse, String> {
        let _gate = lock(&self.shared.gate);
        let control = lock(&self.shared.control);
        if !control.preferences.enabled {
            return Err("human_activity_disabled".into());
        }
        if grouped {
            timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| "invalid_timezone")?;
        }
        let root = control.preferences.root.clone();
        drop(control);
        let store = cache(&self.shared.cache, true)?;
        let current = selected_source(&store, &root)?;
        match current {
            Some(source) => store.human_activity_display(&source, from, to, timezone, grouped),
            None => store.human_activity(&[], from, to),
        }
        .map_err(|e| e.to_string())
    }
    pub fn period_bands(
        &self,
        from: i64,
        to: i64,
        timezone: &str,
    ) -> Result<HumanActivityResponse, String> {
        let _gate = lock(&self.shared.gate);
        let control = lock(&self.shared.control);
        if !control.preferences.enabled {
            return Err("human_activity_disabled".into());
        }
        crate::rei_human::validate_period(from, to).map_err(|e| e.to_string())?;
        timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "invalid_timezone")?;
        let root = control.preferences.root.clone();
        drop(control);
        let store = cache(&self.shared.cache, true)?;
        match selected_source(&store, &root)? {
            Some(source) => store
                .human_activity_bands(&source, from, to, timezone)
                .map_err(|e| e.to_string()),
            None => Ok(HumanActivityResponse {
                records: vec![],
                bins: Some(vec![]),
                diagnostics: vec![],
            }),
        }
    }
    pub fn summary(
        &self,
        from: i64,
        to: i64,
        timezone: &str,
        visible: &[crate::rei_human::HumanKind],
    ) -> Result<crate::human_summary::HumanSummaryResponse, String> {
        let _gate = lock(&self.shared.gate);
        let control = lock(&self.shared.control);
        if !control.preferences.enabled {
            return Err("human_activity_disabled".into());
        }
        crate::rei_human::validate_period(from, to).map_err(|e| e.to_string())?;
        timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "invalid_timezone")?;
        let root = control.preferences.root.clone();
        drop(control);
        let store = cache(&self.shared.cache, true)?;
        match selected_source(&store, &root)? {
            Some(source) => store
                .human_summary(&source, from, to, timezone, visible)
                .map_err(|e| e.to_string()),
            None => Ok(crate::human_summary::HumanSummaryResponse {
                days: vec![],
                diagnostics: vec![],
            }),
        }
    }
}
fn selected_source(store: &Store, root: &Path) -> Result<Option<String>, String> {
    let path = crate::rei_human::source_path(root);
    match crate::rei_human::source_identity(&path) {
        Ok(id) => Ok(Some(id)),
        Err(_) => {
            // Only a missing path can use its retained logical-source cache.
            // An existing object with unknown identity may be a replacement.
            if !matches!(path.try_exists(), Ok(false)) {
                return Err("source_unavailable".into());
            }
            let logical =
                crate::rei_human::logical_source_path(&path).map_err(|e| e.to_string())?;
            let both_missing = crate::rei_human::SOURCE_FILES
                .iter()
                .all(|name| matches!(root.join(name).try_exists(), Ok(false)));
            let other = if both_missing {
                crate::rei_human::SOURCE_FILES
                    .iter()
                    .map(|name| root.join(name))
                    .find(|candidate| candidate != &path)
                    .unwrap_or_else(|| path.clone())
            } else {
                path.clone()
            };
            let other = crate::rei_human::logical_source_path(&other).map_err(|e| e.to_string())?;
            store.conn.query_row("SELECT CASE WHEN typeof(source_id)='blob' THEN lower(hex(source_id)) ELSE source_id END FROM human_activity_sources WHERE source_path IN (?1,?2) ORDER BY last_sync DESC,source_id DESC LIMIT 1", [&logical,&other], |r| r.get::<_,String>(0)).optional().map_err(|_| "cache_read_failed".into())
        }
    }
}
impl Drop for HumanRuntime {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn sync_ready(control: &Control, elapsed: Duration, interval: Duration) -> bool {
    control.preferences.enabled
        && control.pending
        && (control.force || (control.preferences.auto_refresh && elapsed >= interval))
}
fn run(shared: Arc<Shared>) {
    let mut reader: Option<HumanReader> = None;
    let mut version = None;
    let mut epoch = 1;
    let mut last_finished = Instant::now();
    loop {
        let mut control = lock(&shared.control);
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let current = shared.epoch.load(Ordering::SeqCst);
        if epoch != current {
            reader = None;
            version = None;
            epoch = current;
            control.settled_epoch = epoch;
            shared.wake.notify_all();
        }
        if control.preferences.enabled
            && control.preferences.auto_refresh
            && last_finished.elapsed() >= shared.auto_interval
        {
            control.pending = true;
        }
        let ready = sync_ready(&control, last_finished.elapsed(), shared.auto_interval);
        if !ready {
            drop(
                shared
                    .wake
                    .wait_timeout(control, Duration::from_millis(300))
                    .unwrap_or_else(|e| e.into_inner()),
            );
            continue;
        }
        let preferences = control.preferences.clone();
        let force = control.force;
        control.pending = false;
        control.force = false;
        control.status.busy = true;
        let status = control.status.clone();
        drop(control);
        (shared.notify)(status);
        let began = Instant::now();
        let cancelled = || {
            shared.stopping.load(Ordering::SeqCst)
                || shared.epoch.load(Ordering::SeqCst) != epoch
                || began.elapsed() > Duration::from_secs(30)
        };
        let mut job = || -> Result<Option<i64>, String> {
            if cancelled() {
                return Err("sync_cancelled".into());
            }
            let path = crate::rei_human::source_path(&preferences.root);
            if reader
                .as_ref()
                .is_some_and(|r| !r.matches_path(&path) || r.verify_identity().is_err())
            {
                reader = None;
                version = None;
            }
            if reader.is_none() {
                reader = Some(HumanReader::open(&path).map_err(|e| e.to_string())?);
            }
            let source = reader.as_ref().ok_or("source_unavailable")?;
            if !force && version == Some(source.data_version().map_err(|e| e.to_string())?) {
                return Ok(None);
            }
            std::fs::create_dir_all(&shared.staging).map_err(|_| "staging_failed")?;
            let stage = source
                .stage_in(&shared.staging, cancelled)
                .map_err(|e| e.to_string())?;
            if cancelled() {
                return Err("sync_cancelled".into());
            }
            let _gate = lock(&shared.gate);
            if cancelled() {
                return Err("sync_cancelled".into());
            }
            cache(&shared.cache, false)?
                .apply_human_stage(&stage, cancelled)
                .map_err(|e| e.to_string())?;
            Ok(Some(stage.data_version()))
        };
        let result = job();
        let mut control = lock(&shared.control);
        if shared.epoch.load(Ordering::SeqCst) == epoch {
            match result {
                Ok(Some(v)) => {
                    version = Some(v);
                    control.status.revision += 1;
                    control.status.last_success = Some(chrono::Utc::now().timestamp_millis());
                    control.status.error = None;
                }
                Ok(None) => {
                    control.status.error = None;
                }
                Err(error) => {
                    reader = None;
                    version = None;
                    control.status.error = Some(error);
                }
            }
        }
        control.status.busy = false;
        let status = control.status.clone();
        last_finished = Instant::now();
        shared.wake.notify_all();
        drop(control);
        (shared.notify)(status);
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    fn source(path: &Path, seconds: i64) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT)",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO activity_records VALUES('synthetic',1000,?)",
            [serde_json::json!({"durationEstimate":seconds}).to_string()],
        )
        .unwrap();
    }
    fn wait_for(runtime: &HumanRuntime, condition: impl Fn(&HumanSyncStatus) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition(&runtime.status()) {
            assert!(Instant::now() < deadline, "worker did not settle");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn production_hour_boundary_and_manual_override() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = HumanRuntime::new(
            tmp.path().join("cache.db"),
            tmp.path().join("stage"),
            |_| {},
        );
        assert_eq!(runtime.shared.auto_interval, Duration::from_secs(3600));
        let mut c = lock(&runtime.shared.control);
        c.preferences.enabled = true;
        c.preferences.auto_refresh = true;
        c.pending = true;
        assert!(!sync_ready(
            &c,
            Duration::from_millis(3_599_999),
            runtime.shared.auto_interval
        ));
        assert!(sync_ready(
            &c,
            Duration::from_secs(3600),
            runtime.shared.auto_interval
        ));
        c.preferences.auto_refresh = false;
        assert!(!sync_ready(
            &c,
            Duration::from_secs(7200),
            runtime.shared.auto_interval
        ));
        c.force = true;
        assert!(sync_ready(&c, Duration::ZERO, runtime.shared.auto_interval));
        c.preferences.enabled = false;
        assert!(!sync_ready(
            &c,
            Duration::from_secs(7200),
            runtime.shared.auto_interval
        ));
        c.pending = false;
        c.force = false;
    }
    #[test]
    fn notifications_during_sync_wait_for_next_hour_but_manual_can_override() {
        use std::sync::{mpsc, Arc, Barrier};
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache.db");
        drop(Store::open(&cache).unwrap());
        let root = tmp.path().join("source");
        std::fs::create_dir(&root).unwrap();
        source(&root.join("storage.db"), 1);
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        let once = AtomicBool::new(true);
        let (send, receive) = mpsc::channel();
        let runtime = HumanRuntime::new(cache, tmp.path().join("stage"), move |s| {
            if s.busy && once.swap(false, Ordering::SeqCst) {
                send.send(()).unwrap();
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
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..100 {
            runtime.request(false);
        }
        barrier.wait();
        wait_for(&runtime, |s| !s.busy && s.revision >= 1);
        {
            let c = lock(&runtime.shared.control);
            assert!(c.pending);
            assert!(!sync_ready(
                &c,
                Duration::ZERO,
                runtime.shared.auto_interval
            ));
        }
        runtime.request(true);
        wait_for(&runtime, |s| !s.busy && s.revision >= 2);
    }
    #[test]
    fn periodic_retry_without_notifications_recovers_missing_source() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache.db");
        drop(Store::open(&cache).unwrap());
        let root = tmp.path().join("source");
        std::fs::create_dir(&root).unwrap();
        let runtime = HumanRuntime::with_auto_interval(
            cache,
            tmp.path().join("stage"),
            |_| {},
            Duration::from_millis(150),
        );
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
        wait_for(&runtime, |s| !s.busy && s.error.is_some());
        source(&root.join("storage.db"), 2);
        wait_for(&runtime, |s| !s.busy && s.revision >= 1);
        assert!(runtime.status().error.is_none());
        assert_eq!(runtime.period(0, 10000).unwrap().records[0].end, 3000);
    }
    #[test]
    fn scheduled_check_reopens_switched_file_and_auto_off_discards_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache.db");
        drop(Store::open(&cache).unwrap());
        let root = tmp.path().join("source");
        std::fs::create_dir(&root).unwrap();
        source(&root.join("memory.db"), 1);
        let runtime = HumanRuntime::with_auto_interval(
            cache,
            tmp.path().join("stage"),
            |_| {},
            Duration::from_millis(500),
        );
        let preferences = HumanPreferences {
            enabled: true,
            root: root.clone(),
            auto_refresh: true,
        };
        runtime.configure(preferences.clone(), || Ok(())).unwrap();
        wait_for(&runtime, |s| !s.busy && s.revision >= 1);
        for _ in 0..100 {
            runtime.request(false);
        }
        runtime
            .configure(
                HumanPreferences {
                    auto_refresh: false,
                    ..preferences.clone()
                },
                || Ok(()),
            )
            .unwrap();
        assert!(!lock(&runtime.shared.control).pending);
        assert!(runtime.wait_idle(Duration::from_secs(5)));
        assert_eq!(runtime.status().revision, 1);
        source(&root.join("storage.db"), 3);
        runtime.configure(preferences, || Ok(())).unwrap();
        wait_for(&runtime, |s| !s.busy && s.revision >= 2);
        assert_eq!(runtime.period(0, 10000).unwrap().records[0].end, 4000);
    }
}
