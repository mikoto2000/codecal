//! Synthetic-only core benchmark. Never accepts or opens a user's Rei path.
use codecal_lib::{
    rei_human::{HumanKind, HumanReader},
    store::Store,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
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
    let rounds = std::env::var("CODECAL_BENCH_ROUNDS")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()?;
    if !(1..=20).contains(&rounds) {
        return Err("rounds must be within 1..20".into());
    }
    let use_uuid = match std::env::var("CODECAL_BENCH_IDS")
        .as_deref()
        .unwrap_or("sequential")
    {
        "sequential" => false,
        "uuid" => true,
        _ => return Err("unsupported fixture ID distribution".into()),
    };
    let cadence = std::env::var("CODECAL_BENCH_CADENCE_MS")
        .unwrap_or_else(|_| "60000".into())
        .parse::<i64>()?;
    if !(1..=60_000).contains(&cadence) {
        return Err("cadence must be within 1..60000ms".into());
    }
    for count in sizes {
        if !(1..=1_000_000).contains(&count) {
            return Err("rows must be within 1..1000000".into());
        }
        for round in 0..rounds {
            let tmp = tempfile::tempdir()?;
            let source = tmp.path().join("synthetic-source.db");
            let cache = tmp.path().join("synthetic-cache.db");
            let mut writer = Connection::open(&source)?;
            writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); CREATE TABLE agent_events(sequence INTEGER);")?;
            let payload = normal_payload(60);
            let base = 1_700_000_000_000_i64;
            {
                let tx = writer.transaction()?;
                let mut insert = tx.prepare("INSERT INTO activity_records VALUES(?,?,?)")?;
                for i in 0..count {
                    insert.execute(params![
                        native_id(i, use_uuid),
                        base + i as i64 * cadence,
                        payload
                    ])?;
                }
                drop(insert);
                tx.commit()?;
            }
            let reader = HumanReader::open(&source)?;
            let mut store = Store::open(&cache)?;
            let mut metrics = serde_json::json!({"rows":count,"round":round+1,"id_kind":if use_uuid {"uuid"} else {"sequential"},"cadence_ms":cadence,"payload_bytes":payload.len(),"process_id":std::process::id(),"sqlite_version":rusqlite::version()});
            let stage_time = Instant::now();
            let first = match reader.stage(|| false) {
                Ok(stage) => stage,
                Err(error) => {
                    metrics["initial_stage_ms"] =
                        (stage_time.elapsed().as_secs_f64() * 1000.0).into();
                    metrics["initial_error"] = error.to_string().into();
                    println!("{metrics}");
                    continue;
                }
            };
            metrics["initial_stage_ms"] = (stage_time.elapsed().as_secs_f64() * 1000.0).into();
            let apply_time = Instant::now();
            store.apply_human_stage(&first, || false)?;
            metrics["initial_apply_ms"] = (apply_time.elapsed().as_secs_f64() * 1000.0).into();
            metrics["initial_sync_ms"] = (stage_time.elapsed().as_secs_f64() * 1000.0).into();
            assert_eq!(
                Connection::open_with_flags(&cache, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?
                    .query_row("SELECT count(*) FROM human_activity_records", [], |r| r
                        .get::<_, i64>(0))?,
                count as i64
            );
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
            let summary_time = Instant::now();
            let summary = store.human_summary(
                &sources[0],
                base,
                base + 42 * 86_400_000,
                "UTC",
                &[
                    HumanKind::Entertainment,
                    HumanKind::NonEntertainment,
                    HumanKind::Unknown,
                ],
            )?;
            metrics["month_summary"] = serde_json::json!({"ms":summary_time.elapsed().as_secs_f64()*1000.0,"days":summary.days.len(),"response_bytes":serde_json::to_vec(&summary)?.len()});
            for (name, from) in [
                (
                    "last_day",
                    base + count.saturating_sub((86_400_000 / cadence) as usize) as i64 * cadence,
                ),
                (
                    "empty_future_day",
                    base + count as i64 * cadence + 86_400_000,
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
                                native_id(count, use_uuid),
                                base + count as i64 * cadence,
                                payload
                            ],
                        )?;
                    }
                    "one_updated" => {
                        writer.execute(
                            "UPDATE activity_records SET payload=? WHERE id=?",
                            params![normal_payload(1), native_id(0, use_uuid)],
                        )?;
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
            let projection = Instant::now();
            let mut cursor = None;
            let mut projected = 0;
            loop {
                let page = reader.read_page(cursor)?;
                if page.cursor == cursor {
                    break;
                }
                cursor = page.cursor;
                projected += page.records.len();
            }
            assert_eq!(projected, count + 1);
            metrics["projection_only_ms"] = (projection.elapsed().as_secs_f64() * 1000.0).into();
            let identity = Instant::now();
            for _ in 0..1000 {
                reader.verify_identity()?;
            }
            metrics["identity_1000_ms"] = (identity.elapsed().as_secs_f64() * 1000.0).into();
            metrics["cache_main_bytes"] = std::fs::metadata(&cache)?.len().into();
            let wal = cache.with_file_name("synthetic-cache.db-wal");
            metrics["cache_wal_bytes"] =
                std::fs::metadata(wal).map(|m| m.len()).unwrap_or(0).into();
            println!("{metrics}");
        }
    }
    Ok(())
}

fn native_id(value: usize, uuid: bool) -> String {
    if !uuid {
        return format!("{value:012}");
    }
    let digest = Sha256::digest((value as u64).to_le_bytes());
    let mut bytes: [u8; 16] = digest[..16].try_into().expect("sixteen digest bytes");
    bytes[6] = (bytes[6] & 15) | 0x40;
    bytes[8] = (bytes[8] & 63) | 0x80;
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.into_iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").expect("string write");
    }
    out
}
fn normal_payload(duration: i64) -> String {
    let make = |padding: &str| {
        serde_json::json!({"durationEstimate":duration,"foreground":{"title":format!("SYNTHETIC_PRIVATE_CONTENT{padding}")},"detection":{"diagnostics":{"entertainmentDisposition":"NON_ENTERTAINMENT"}}}).to_string()
    };
    let initial = make("");
    make(&"x".repeat(200 - initial.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn realistic_fixture_has_unique_uuid_shaped_ids_and_two_hundred_byte_payloads() {
        let ids = (0..10_000)
            .map(|i| native_id(i, true))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 10_000);
        for id in ids {
            assert_eq!(id.len(), 36);
            assert_eq!(&id[14..15], "4");
            assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        }
        let payload = normal_payload(60);
        assert_eq!(payload.len(), 200);
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["durationEstimate"], 60);
        assert!(payload.contains("SYNTHETIC_PRIVATE_CONTENT"));
    }
}
