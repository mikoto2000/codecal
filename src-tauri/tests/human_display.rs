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
    writer.execute_batch("BEGIN;").unwrap();
    for (i, (start, seconds, kind)) in records.iter().enumerate() {
        let payload = serde_json::json!({"durationEstimate":seconds,"detection":{"diagnostics":{"entertainmentDisposition":kind}},"foreground":{"title":"PRIVATE_DISPLAY_MARKER"}});
        writer
            .execute(
                "INSERT INTO activity_records VALUES(?,?,?)",
                params![format!("synthetic-{i}"), start, payload.to_string()],
            )
            .unwrap();
    }
    writer.execute_batch("COMMIT;").unwrap();
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

#[test]
fn dense_contiguous_observations_are_grouped_before_limiting_output() {
    let records = (0..50_001)
        .map(|i| (i + 1000, 1, "ENTERTAINMENT"))
        .collect::<Vec<_>>();
    let (_tmp, store, id) = cache(&records);
    assert_eq!(
        store
            .human_activity_display(&id, 0, 100000, "UTC", false)
            .unwrap_err()
            .to_string(),
        "period_too_dense"
    );
    let grouped = store
        .human_activity_display(&id, 0, 100000, "UTC", true)
        .unwrap();
    assert_eq!(grouped.records.len(), 1);
    assert_eq!(
        (grouped.records[0].start, grouped.records[0].end),
        (1000, 52000)
    );
}

fn ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}
#[test]
fn grouping_splits_real_dst_days_and_skipped_dates_without_cross_day_merges() {
    for (zone, from, to, lengths) in [
        (
            "America/New_York",
            "2026-03-07T05:00:00Z",
            "2026-03-09T04:00:00Z",
            vec![24, 23],
        ),
        (
            "America/New_York",
            "2026-10-31T04:00:00Z",
            "2026-11-02T05:00:00Z",
            vec![24, 25],
        ),
        (
            "Pacific/Apia",
            "2011-12-29T10:00:00Z",
            "2011-12-31T10:00:00Z",
            vec![24, 24],
        ),
    ] {
        let (from, to) = (ms(from), ms(to));
        let (_tmp, store, id) = cache(&[(from, (to - from) / 1000, "ENTERTAINMENT")]);
        let response = store
            .human_activity_display(&id, from, to, zone, true)
            .unwrap();
        assert_eq!(
            response
                .records
                .iter()
                .map(|r| (r.end - r.start) / 3_600_000)
                .collect::<Vec<_>>(),
            lengths
        );
        assert_eq!(response.records.first().unwrap().start, from);
        assert_eq!(response.records.last().unwrap().end, to);
    }
}

#[test]
fn long_estimates_are_clipped_before_daily_display_splitting() {
    let (_tmp, store, id) = cache(&[(1000, 31_536_000_000, "UNCERTAIN")]);
    let (from, to) = (ms("2026-11-20T00:00:00Z"), ms("2026-11-27T00:00:00Z"));
    let response = store
        .human_activity_display(&id, from, to, "UTC", true)
        .unwrap();
    assert_eq!(response.records.len(), 7);
    assert!(response
        .records
        .iter()
        .all(|r| r.end - r.start == 86_400_000));
    assert_eq!(
        store
            .human_activity_display(&id, from, to, "bad/private-zone", true)
            .unwrap_err()
            .to_string(),
        "invalid_timezone"
    );
}
