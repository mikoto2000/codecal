use codecal_lib::store::Store;
use std::fs;
#[test]
fn codex_home_import_is_scoped_to_session_directories() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("sessions")).unwrap();
    fs::write(tmp.path().join("private-unrelated.jsonl"), "bad\n").unwrap();
    let mut store = Store::open(&tmp.path().join("db.sqlite")).unwrap();
    let result = store.sync(tmp.path()).unwrap();
    assert_eq!(result.files, 0);
    assert!(store.snapshot("UTC").unwrap().diagnostics.is_empty());
}
