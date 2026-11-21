use codecal_lib::store::Store;
use std::fs;
#[test]
fn rename_reuses_cursor_without_reading_the_log_again() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.jsonl");
    fs::write(&a,"{\"type\":\"session_meta\",\"timestamp\":\"2026-11-20T10:00:00Z\",\"payload\":{\"id\":\"s\",\"cwd\":\"/demo\"}}\n").unwrap();
    let mut store = Store::open(&tmp.path().join("db.sqlite")).unwrap();
    store.sync(tmp.path()).unwrap();
    fs::rename(a, tmp.path().join("b.jsonl")).unwrap();
    assert_eq!(store.sync(tmp.path()).unwrap().bytes_read, 0);
}
