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

#[test]
fn local_migration_omits_human_cache_without_changing_the_original() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("roaming");
    let local = tmp.path().join("local");
    fs::create_dir_all(&old).unwrap();
    let original = old.join("codecal.db");
    let store = Store::open(&original).unwrap();
    store.set_setting("AI-marker", "preserved").unwrap();
    drop(store);
    let writer = rusqlite::Connection::open(&original).unwrap();
    writer.execute_batch("INSERT INTO human_activity_sources VALUES('s','SYNTHETIC_HUMAN_SOURCE','identity',1,0,'synced','[]');
        INSERT INTO human_activity_records VALUES('s','SYNTHETIC_HUMAN_NATIVE',1000,1000,'unknown','hash',2000);").unwrap();
    let migrated = prepare_database(&local, &old).unwrap();
    let copied = rusqlite::Connection::open(&migrated).unwrap();
    for table in ["human_activity_records", "human_activity_sources"] {
        assert_eq!(
            copied
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            writer
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert_eq!(
        Store::open(&migrated)
            .unwrap()
            .get_setting("AI-marker")
            .unwrap()
            .as_deref(),
        Some("preserved")
    );
    drop(copied);
    let bytes = fs::read(migrated).unwrap();
    assert!(!bytes
        .windows(b"SYNTHETIC_HUMAN".len())
        .any(|w| w == b"SYNTHETIC_HUMAN"));
}
