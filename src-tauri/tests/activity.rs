use codecal_lib::activity::{aggregate, Interval};
fn t(s: &str) -> i64 { chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis() }
#[test]
fn sums_parallel_work_and_unions_wall_time() {
 let a=vec![Interval{project_id:"a".into(),start:t("2026-11-20T10:00:00Z"),end:Some(t("2026-11-20T12:00:00Z"))},Interval{project_id:"b".into(),start:t("2026-11-20T11:00:00Z"),end:Some(t("2026-11-20T13:00:00Z"))}];
 let d=aggregate(&a,"UTC").unwrap(); assert_eq!(d[0].elapsed_ms,10800000); assert_eq!(d[0].total_ms,14400000); assert_eq!(d[0].projects["a"],7200000);
}
#[test]
fn splits_at_local_midnight_and_excludes_unknown_end() {
 let a=vec![Interval{project_id:"a".into(), start:t("2026-11-20T14:00:00Z"),end:Some(t("2026-11-20T17:00:00Z"))},Interval{project_id:"b".into(),start:0,end:None}];
 let d=aggregate(&a,"Asia/Tokyo").unwrap(); assert_eq!(d.len(),2); assert_eq!(d[0].date,"2026-11-20");assert_eq!(d[0].total_ms,3600000);assert_eq!(d[1].total_ms,7200000);
}
#[test]
fn handles_dst_days_and_bad_zones() {
 let a=vec![Interval{project_id:"a".into(),start:t("2026-03-08T05:00:00Z"),end:Some(t("2026-03-09T04:00:00Z"))}];
 let d=aggregate(&a,"America/New_York").unwrap(); assert_eq!(d.len(),1);assert_eq!(d[0].total_ms,23*3600000);assert!(aggregate(&a,"bogus").is_err());
 let a=vec![Interval{project_id:"a".into(),start:t("2026-11-01T04:00:00Z"),end:Some(t("2026-11-02T05:00:00Z"))}];
 assert_eq!(aggregate(&a,"America/New_York").unwrap()[0].total_ms,25*3600000);
}
