use codecal_lib::store::Store;
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let root = std::path::Path::new(args.get(1).ok_or("root required")?);
    let db = std::path::Path::new(args.get(2).ok_or("database path required")?);
    let mut store = Store::open(db)?;
    let started = std::time::Instant::now();
    let progress = store.sync(root)?;
    let snapshot = store.snapshot("Asia/Tokyo")?;
    println!(
        "files={} bytes={} projects={} turns={} days={} diagnostic_files={} elapsed_ms={}",
        progress.files,
        progress.bytes_read,
        snapshot.projects.len(),
        snapshot.turns.len(),
        snapshot.days.len(),
        snapshot.diagnostics.len(),
        started.elapsed().as_millis()
    );
    let statuses = snapshot.turns.iter().fold(
        std::collections::BTreeMap::<String, usize>::new(),
        |mut m, t| {
            *m.entry(t.status.clone()).or_default() += 1;
            m
        },
    );
    println!("statuses={statuses:?}");
    let result = store.sync(root)?;
    println!("unchanged_sync_bytes={}", result.bytes_read);
    Ok(())
}
