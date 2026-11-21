use codecal_lib::{storage::prepare_database, store::Store};
use std::fs;
#[test]
fn uses_local_data_and_migrates_sqlite_including_wal_without_deleting_original() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("roaming");
    let local = tmp.path().join("local");
    fs::create_dir_all(&old).unwrap();
    let store = Store::open(&old.join("codecal.db")).unwrap();
    store.set_setting("marker", "preserved").unwrap();
    let path = prepare_database(&local, &old).unwrap();
    assert_eq!(path, local.join("codecal.db"));
    assert!(old.join("codecal.db").exists());
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .get_setting("marker")
            .unwrap()
            .as_deref(),
        Some("preserved")
    );
}
#[test]
fn never_overwrites_an_existing_local_database() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("roaming");
    let local = tmp.path().join("local");
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&local).unwrap();
    Store::open(&old.join("codecal.db"))
        .unwrap()
        .set_setting("marker", "old")
        .unwrap();
    Store::open(&local.join("codecal.db"))
        .unwrap()
        .set_setting("marker", "new")
        .unwrap();
    let path = prepare_database(&local, &old).unwrap();
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .get_setting("marker")
            .unwrap()
            .as_deref(),
        Some("new")
    );
}
#[test]
fn fresh_profile_and_same_directory_need_no_migration() {
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("local");
    let path = prepare_database(&local, &local).unwrap();
    assert_eq!(path, local.join("codecal.db"));
    assert!(!path.exists());
    assert!(local.is_dir());
}
