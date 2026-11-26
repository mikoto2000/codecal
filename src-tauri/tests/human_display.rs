use codecal_lib::{
    rei_human::{HumanKind, HumanReader},
    store::Store,
};
use rusqlite::{params, Connection};

fn cache(records: &[(i64, i64, &str)]) -> (tempfile::TempDir, Store, String) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("synthetic.db");
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);",
        )
        .unwrap();
    for (i, (start, seconds, kind)) in records.iter().enumerate() {
        let payload = serde_json::json!({"durationEstimate":seconds,"detection":{"diagnostics":{"entertainmentDisposition":kind}},"foreground":{"title":"PRIVATE_DISPLAY_MARKER"}});
        writer
            .execute(
                "INSERT INTO activity_records VALUES(?,?,?)",
                params![format!("synthetic-{i}"), start, payload.to_string()],
            )
            .unwrap();
    }
    let reader = HumanReader::open(&path).unwrap();
    let mut store = Store::open(&tmp.path().join("cache.db")).unwrap();
    store
        .apply_human_stage(&reader.stage(|| false).unwrap(), || false)
        .unwrap();
    let id = reader.source_id().to_owned();
    (tmp, store, id)
}

#[test]
fn grouping_joins_only_touching_same_kind_and_keeps_gaps_and_points() {
    let (_tmp, store, id) = cache(&[
        (1000, 1, "ENTERTAINMENT"),
        (2000, 1, "ENTERTAINMENT"),
        (4000, 1, "ENTERTAINMENT"),
        (5000, 1, "NON_ENTERTAINMENT"),
        (6000, 0, "NON_ENTERTAINMENT"),
    ]);
    let grouped = store
        .human_activity_display(&id, 0, 10000, "UTC", true)
        .unwrap();
    assert_eq!(
        grouped
            .records
            .iter()
            .map(|r| (r.start, r.end, r.kind))
            .collect::<Vec<_>>(),
        vec![
            (1000, 3000, HumanKind::Entertainment),
            (4000, 5000, HumanKind::Entertainment),
            (5000, 6000, HumanKind::NonEntertainment),
            (6000, 6000, HumanKind::NonEntertainment)
        ]
    );
    let wire = serde_json::to_string(&grouped).unwrap();
    assert!(!wire.contains("PRIVATE_DISPLAY_MARKER"));
    assert!(!wire.contains("synthetic-"));
    let clipped = store
        .human_activity_display(&id, 1500, 4500, "UTC", true)
        .unwrap();
    assert_eq!(
        clipped
            .records
            .iter()
            .map(|r| (r.start, r.end))
            .collect::<Vec<_>>(),
        vec![(1500, 3000), (4000, 4500)]
    );
    let raw = store
        .human_activity_display(&id, 1500, 4500, "UTC", false)
        .unwrap();
    assert_eq!(raw.records.first().unwrap().start, 1000);
    assert_eq!(raw.records.last().unwrap().end, 5000);
}
