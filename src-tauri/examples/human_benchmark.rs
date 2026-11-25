//! Synthetic-only core benchmark. Never accepts or opens a user's Rei path.
use codecal_lib::{rei_human::HumanReader, store::Store};
use rusqlite::{params, Connection};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let sizes = if args.is_empty() {
        vec![10_000, 100_000, 1_000_000]
    } else {
        args.iter()
            .map(|s| s.parse::<usize>())
            .collect::<Result<Vec<_>, _>>()?
    };
    for count in sizes {
        if !(1..=1_000_000).contains(&count) {
            return Err("rows must be within 1..1000000".into());
        }
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("synthetic-source.db");
        let cache = tmp.path().join("synthetic-cache.db");
        let mut writer = Connection::open(&source)?;
        writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); CREATE TABLE agent_events(sequence INTEGER);")?;
        let payload = r#"{"durationEstimate":60,"foreground":{"title":"SYNTHETIC_PRIVATE_CONTENT"},"detection":{"diagnostics":{"entertainmentDisposition":"NON_ENTERTAINMENT"}}}"#;
        let base = 1_700_000_000_000_i64;
        {
            let tx = writer.transaction()?;
            let mut insert = tx.prepare("INSERT INTO activity_records VALUES(?,?,?)")?;
            for i in 0..count {
                insert.execute(params![
                    format!("{i:012}"),
                    base + i as i64 * 60_000,
                    payload
                ])?;
            }
            drop(insert);
            tx.commit()?;
        }
        let reader = HumanReader::open(&source)?;
        let mut store = Store::open(&cache)?;
        let mut metrics = serde_json::json!({"rows":count});
        let stage_time = Instant::now();
        let first = reader.stage(|| false)?;
        metrics["initial_stage_ms"] = (stage_time.elapsed().as_secs_f64() * 1000.0).into();
        let apply_time = Instant::now();
        store.apply_human_stage(&first, || false)?;
        metrics["initial_apply_ms"] = (apply_time.elapsed().as_secs_f64() * 1000.0).into();
        let start = Instant::now();
        assert_eq!(reader.data_version()?, first.data_version());
        metrics["unchanged_check_ms"] = (start.elapsed().as_secs_f64() * 1000.0).into();
        drop(first);
        let sources = [reader.source_id().to_owned()];
        for (name, days) in [("day", 1), ("week", 7), ("month_grid", 42)] {
            let start = Instant::now();
            let result = store.human_activity(&sources, base, base + days * 86_400_000);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            metrics[name] = match result {
                Ok(response) => {
                    serde_json::json!({"ms":elapsed,"records":response.records.len(),"response_bytes":serde_json::to_vec(&response)?.len()})
                }
                Err(error) => serde_json::json!({"ms":elapsed,"error":error.to_string()}),
            };
        }
        for (name, from) in [
            (
                "last_day",
                base + count.saturating_sub(1440) as i64 * 60_000,
            ),
            (
                "empty_future_day",
                base + count as i64 * 60_000 + 86_400_000,
            ),
        ] {
            let start = Instant::now();
            let response = store.human_activity(&sources, from, from + 86_400_000)?;
            metrics[name] = serde_json::json!({"ms":start.elapsed().as_secs_f64()*1000.0,"records":response.records.len()});
        }
        for scenario in ["one_added", "one_updated", "ai_only"] {
            match scenario {
                "one_added" => {
                    writer.execute(
                        "INSERT INTO activity_records VALUES(?,?,?)",
                        params![
                            format!("{count:012}"),
                            base + count as i64 * 60_000,
                            payload
                        ],
                    )?;
                }
                "one_updated" => {
                    writer.execute("UPDATE activity_records SET payload='{\"durationEstimate\":1}' WHERE id='000000000000'",[])?;
                }
                _ => {
                    writer.execute("INSERT INTO agent_events VALUES(1)", [])?;
                }
            }
            let start = Instant::now();
            let stage = reader.stage(|| false)?;
            metrics[format!("{scenario}_stage_ms")] =
                (start.elapsed().as_secs_f64() * 1000.0).into();
            let apply_start = Instant::now();
            store.apply_human_stage(&stage, || false)?;
            metrics[format!("{scenario}_apply_ms")] =
                (apply_start.elapsed().as_secs_f64() * 1000.0).into();
            metrics[format!("{scenario}_sync_ms")] =
                (start.elapsed().as_secs_f64() * 1000.0).into();
        }
        metrics["cache_main_bytes"] = std::fs::metadata(&cache)?.len().into();
        let wal = cache.with_file_name("synthetic-cache.db-wal");
        metrics["cache_wal_bytes"] = std::fs::metadata(wal).map(|m| m.len()).unwrap_or(0).into();
        println!("{metrics}");
    }
    Ok(())
}
