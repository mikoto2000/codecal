use codecal_lib::parser::parse_line;
#[test]
fn understands_explicit_turn_boundaries_and_does_not_guess() {
    let e = parse_line(r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#).unwrap().unwrap();
    assert_eq!(e.kind, "start");
    assert_eq!(e.turn_id.as_deref(), Some("t1"));
    assert!(parse_line("{bad").is_err());
    assert!(parse_line(r#"{"type":"future_event","payload":{}}"#)
        .unwrap()
        .is_none());
}
#[test]
fn recognizes_metadata_context_prompt_tokens_and_interruptions() {
    for (raw, kind) in [
        (
            r#"{"timestamp":"2026-11-20T10:00:00Z","type":"session_meta","payload":{"id":"s1","cwd":"C:\\work\\demo"}}"#,
            "meta",
        ),
        (
            r#"{"timestamp":"2026-11-20T10:00:00Z","type":"turn_context","payload":{"turn_id":"t1","cwd":"/work/demo"}}"#,
            "context",
        ),
        (
            r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"hello"}}"#,
            "prompt",
        ),
        (
            r#"{"timestamp":"2026-11-20T10:00:00Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"t1"}}"#,
            "interrupted",
        ),
        (
            r#"{"timestamp":"2026-11-20T10:00:00Z","type":"token_usage_record","payload":{"turn_id":"t1","turn_token_usage":{"total_tokens":123}}}"#,
            "tokens",
        ),
    ] {
        assert_eq!(parse_line(raw).unwrap().unwrap().kind, kind);
    }
}

#[test]
fn reads_current_desktop_user_items() {
 let e=parse_line(r#"{"timestamp":"2026-11-20T10:00:01Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t1","item":{"type":"UserMessage","content":[{"type":"Text","text":"依頼"}]}}}"#).unwrap().unwrap();
 assert_eq!(e.text.as_deref(),Some("依頼"));
}
