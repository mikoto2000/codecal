use codecal_lib::store::Store;
use std::fs;
#[test]
fn detects_same_size_rewrite_even_when_first_page_is_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("a.jsonl");
    let prefix="{\"timestamp\":\"2026-11-20T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cwd\":\"/sample\"}}\n".to_owned()+&"{\"type\":\"ignored\"}\n".repeat(300);
    let tail = r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"old"}}
"#;
    fs::write(&path, prefix.clone() + tail).unwrap();
    let mut store = Store::open(&tmp.path().join("data.sqlite")).unwrap();
    store.sync(tmp.path()).unwrap();
    fs::write(&path, prefix + &tail.replace("old", "new")).unwrap();
    store.sync(tmp.path()).unwrap();
    assert_eq!(store.snapshot("UTC").unwrap().turns[0].turn_id, "new");
}
