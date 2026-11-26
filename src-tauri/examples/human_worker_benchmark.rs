//! Synthetic-only worker/writer benchmark; accepts a row count, never a path.
use codecal_lib::{
    human_runtime::{HumanPreferences, HumanRuntime},
    store::Store,
};
use rusqlite::{params, Connection};
#[path = "support/human_fixture.rs"]
mod human_fixture;
use human_fixture::{native_id, normal_payload};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn percentile(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * 95).div_ceil(100).saturating_sub(1)]
}
fn writer_samples(conn: &Connection, stop: Option<&AtomicBool>) -> (Vec<f64>, u64) {
    let wal = format!("{}-wal", conn.path().expect("owned fixture path"));
    let mut peak_wal = 0;
    let mut samples = Vec::new();
    while samples.len() < 200 || stop.is_some_and(|s| !s.load(Ordering::SeqCst)) {
        let start = Instant::now();
        conn.execute("INSERT INTO agent_events VALUES(1)", [])
            .expect("synthetic writer");
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        peak_wal = peak_wal.max(std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0));
        std::thread::sleep(Duration::from_millis(10));
    }
    (samples, peak_wal)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1000000".into())
        .parse::<usize>()?;
    if !(1..=1_000_000).contains(&count) {
        return Err("rows must be within 1..1000000".into());
    }
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().join("source");
    std::fs::create_dir(&root)?;
    let source = root.join("storage.db");
    let mut writer = Connection::open(&source)?;
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); CREATE TABLE agent_events(sequence INTEGER);")?;
    let base = 1_700_000_000_000_i64;
    let payload = normal_payload(60);
    {
        let tx = writer.transaction()?;
        let mut insert = tx.prepare("INSERT INTO activity_records VALUES(?,?,?)")?;
        for i in 0..count {
            insert.execute(params![
                native_id(i, true),
                base + i as i64 * 60000,
                payload
            ])?;
        }
        drop(insert);
        tx.commit()?;
    }
    // Discard the fixture-generation WAL before either measured workload.
    // This is the owned writer, never a production reader connection.
    writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let (mut baseline, baseline_wal_peak) = writer_samples(&writer, None);
    let baseline_p95 = percentile(&mut baseline);
    let cache = tmp.path().join("cache.db");
    // AppState retains this connection in the actual desktop application.
    // Keep it alive here too; otherwise every worker job adds a synthetic
    // last-connection close/checkpoint/delete that production does not perform.
    let _application_store = Store::open(&cache)?;
    let events = Arc::new(Mutex::new(
        Vec::<(Instant, bool, u64, Option<String>)>::new(),
    ));
    let listener = events.clone();
    let runtime = HumanRuntime::new_for_benchmark(cache, tmp.path().join("stage"), move |s| {
        listener
            .lock()
            .unwrap()
            .push((Instant::now(), s.busy, s.revision, s.error))
    });
    let start = Instant::now();
    runtime.configure(
        HumanPreferences {
            enabled: true,
            root: root.clone(),
            auto_refresh: true,
        },
        || Ok(()),
    )?;
    if !runtime.wait_idle(Duration::from_secs(40)) {
        return Err("initial worker timeout".into());
    }
    let initial_ms = start.elapsed().as_secs_f64() * 1000.0;
    if runtime.status().error.is_some() {
        return Err("initial worker failed".into());
    }
    let revision = runtime.status().revision;
    let mut unchanged = Vec::new();
    let mut queue = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let before = events.lock().unwrap().len();
        runtime.request(false);
        if !runtime.wait_idle(Duration::from_secs(5)) {
            return Err("unchanged worker timeout".into());
        }
        let notify_deadline = Instant::now() + Duration::from_secs(1);
        while !events.lock().unwrap()[before..].iter().any(|v| !v.1) {
            if Instant::now() > notify_deadline {
                return Err("missing idle notification".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let records = events.lock().unwrap();
        let new = &records[before..];
        let busy = new.iter().find(|v| v.1).ok_or("missing busy event")?.0;
        let idle = new
            .iter()
            .rev()
            .find(|v| !v.1)
            .ok_or("missing idle event")?
            .0;
        unchanged.push(idle.duration_since(busy).as_secs_f64() * 1000.0);
        queue.push(busy.duration_since(start).as_secs_f64() * 1000.0);
    }
    assert_eq!(runtime.status().revision, revision);
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = stop.clone();
    let writer_source = source.clone();
    let concurrent = std::thread::spawn(move || {
        let conn = Connection::open(writer_source).unwrap();
        conn.busy_timeout(Duration::from_secs(1)).unwrap();
        writer_samples(&conn, Some(&writer_stop))
    });
    let start = Instant::now();
    runtime.request(true);
    if !runtime.wait_idle(Duration::from_secs(40)) {
        return Err("forced worker timeout".into());
    }
    let forced_ms = start.elapsed().as_secs_f64() * 1000.0;
    stop.store(true, Ordering::SeqCst);
    let (mut concurrent, concurrent_wal_peak) =
        concurrent.join().map_err(|_| "synthetic writer panic")?;
    if runtime.status().error.is_some() {
        return Err("forced worker failed".into());
    }
    // Writes committed after the snapshot must be acknowledged by a later check.
    let revision_before_followup = runtime.status().revision;
    runtime.request(false);
    if !runtime.wait_idle(Duration::from_secs(40)) {
        return Err("followup timeout".into());
    }
    assert!(runtime.status().error.is_none());
    assert!(runtime.status().revision > revision_before_followup);
    let summary_start = Instant::now();
    let summary = runtime.summary(base, base + 42 * 86400000, "UTC", &[])?;
    let summary_ms = summary_start.elapsed().as_secs_f64() * 1000.0;
    let wal = source.with_file_name("storage.db-wal");
    let wal_bytes = std::fs::metadata(&wal)?.len();
    // Only the owned fixture writer checkpoints. Product readers never do so.
    let checkpoint: (i64, i64, i64) =
        writer.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    assert_eq!(checkpoint.0, 0);
    assert_eq!(checkpoint.1, checkpoint.2);
    writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let reclaimed_wal_bytes = std::fs::metadata(&wal)?.len();
    let concurrent_p95 = percentile(&mut concurrent);
    let writer_budget_pass =
        concurrent_p95 <= baseline_p95 * 2.0 && concurrent_p95 - baseline_p95 <= 100.0;
    println!(
        "{}",
        serde_json::json!({"auto_interval_ms":1000,"production_auto_interval_ms":3600000,"rows":count,"initial_worker_ms":initial_ms,"forced_worker_ms":forced_ms,"unchanged_active_ms":unchanged,"unchanged_queue_ms":queue,"summary_ms":summary_ms,"summary_bytes":serde_json::to_vec(&summary)?.len(),"writer_baseline_samples":baseline.len(),"writer_baseline_p95_ms":baseline_p95,"writer_concurrent_samples":concurrent.len(),"writer_concurrent_p95_ms":concurrent_p95,"payload_bytes":payload.len(),"id_kind":"uuid","baseline_wal_peak_bytes":baseline_wal_peak,"concurrent_wal_peak_bytes":concurrent_wal_peak,"followup_caught_writes":runtime.status().revision>revision_before_followup,"writer_baseline_commit_ms":baseline,"writer_concurrent_commit_ms":concurrent,"writer_budget_pass":writer_budget_pass,"fixture_writer_truncated_wal_bytes":reclaimed_wal_bytes,"source_wal_bytes_after_read":wal_bytes,"writer_passive_checkpoint_after_read":checkpoint,"worker_revision":runtime.status().revision})
    );
    if !writer_budget_pass {
        return Err("synthetic writer latency budget exceeded".into());
    }
    Ok(())
}
