use codecal_lib::{human_runtime::HumanRuntime, store::Store};

#[test]
fn disabled_runtime_never_opens_missing_source_and_refuses_periods() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache.db");
    drop(Store::open(&cache).unwrap());
    let staging = tmp.path().join("staging");
    let runtime = HumanRuntime::new(cache, staging.clone(), |_| {});
    assert_eq!(runtime.period(0, 1000).unwrap_err(), "human_activity_disabled");
    assert!(!runtime.status().busy);
    assert!(!staging.exists());
}
