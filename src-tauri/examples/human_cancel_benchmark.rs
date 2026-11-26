//! Owned synthetic payload/cancellation/deadline verification. No path arguments.
use codecal_lib::{rei_human::HumanReader, store::Store};
use rusqlite::{params, Connection};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() != 1 {
        return Err("no arguments accepted".into());
    }
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("synthetic.db");
    let cache = tmp.path().join("cache.db");
    let mut writer = Connection::open(&source)?;
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); INSERT INTO activity_records VALUES('normal',1000,'{\"durationEstimate\":1}');")?;
    let reader = HumanReader::open(&source)?;
    let mut store = Store::open(&cache)?;
    let stage = reader.stage(|| false)?;
    store.apply_human_stage(&stage, || false)?;
    drop(stage);
    let baseline =
        serde_json::to_vec(&store.human_activity(&[reader.source_id().into()], 0, 10000)?)?;
    let payload=serde_json::json!({"durationEstimate":1,"foreground":{"title":"SYNTHETIC_PAYLOAD_MARKER".to_string()+&"x".repeat(1_047_900)}}).to_string();
    assert!(payload.len() <= 1_048_576);
    {
        let tx = writer.transaction()?;
        let mut insert = tx.prepare("INSERT INTO activity_records VALUES(?,?,?)")?;
        for i in 0..1000 {
            insert.execute(params![format!("large-{i:04}"), 2000 + i * 1000, payload])?;
        }
        drop(insert);
        tx.commit()?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let notifier = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.store(true, Ordering::SeqCst);
    });
    let start = Instant::now();
    let result = reader.stage(|| stop.load(Ordering::SeqCst));
    let cancel_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        result.err().ok_or("expected cancellation")?.to_string(),
        "sync_cancelled"
    );
    notifier.join().map_err(|_| "notifier panic")?;
    assert_eq!(
        serde_json::to_vec(&store.human_activity(&[reader.source_id().into()], 0, 10000)?)?,
        baseline
    );
    // Deliberately slow the cancellation callback to exercise the real 30s deadline
    // without manufacturing tens of gigabytes. No production timeout override.
    let start = Instant::now();
    let result = reader.stage(|| {
        std::thread::sleep(Duration::from_millis(20));
        false
    });
    let deadline_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!((30_000.0..31_000.0).contains(&deadline_ms));
    assert_eq!(
        result.err().ok_or("expected deadline")?.to_string(),
        "sync_cancelled"
    );
    assert_eq!(
        serde_json::to_vec(&store.human_activity(&[reader.source_id().into()], 0, 10000)?)?,
        baseline
    );
    let start = Instant::now();
    let retry = reader.stage(|| false)?;
    store.apply_human_stage(&retry, || false)?;
    let retry_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        Connection::open(&cache)?.query_row(
            "SELECT count(*) FROM human_activity_records",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1001
    );
    println!(
        "{}",
        serde_json::json!({"rows":1001,"large_payload_bytes":payload.len(),"cancel_requested_after_ms":100,"cancel_total_ms":cancel_ms,"deadline_ms":deadline_ms,"deadline_fixture":"20ms slow cancellation callback; production 30s deadline unchanged","retry_ms":retry_ms,"normal_cache_preserved":true,"retry_rows":1001})
    );
    Ok(())
}
