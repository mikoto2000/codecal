//! Local transcript adapter. Based on NexA-LLC/claude-cli-sdk-rs at
//! 679be0382b880a46e9888a1f5876a263155523c1 (MIT); see THIRD_PARTY_NOTICES.
use crate::parser::{Event, SessionDetails};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub fn default_root() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|p| !p.is_empty())
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".claude")
        })
        .join("projects")
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// Only project/session.jsonl and project/session/subagents/agent-id.jsonl.
/// No names obtained from a transcript are ever used to open files.
pub fn session_key(path: &Path) -> Result<(String, String, Option<String>), String> {
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("invalid transcript path".into());
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid session id")?;
    if path.extension().is_none_or(|s| s != "jsonl") {
        return Err("invalid transcript suffix".into());
    }
    if uuid(stem) {
        return Ok((format!("claude:{stem}"), stem.into(), None));
    }
    let parent = path.parent().ok_or("invalid subagent path")?;
    let sid = parent
        .parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .ok_or("invalid parent session")?;
    let agent = stem.strip_prefix("agent-").ok_or("invalid session id")?;
    if parent.file_name().is_none_or(|s| s != "subagents")
        || !uuid(sid)
        || agent.is_empty()
        || agent.len() > 128
        || !agent
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("invalid subagent id".into());
    }
    Ok((
        format!("claude:{sid}:{stem}"),
        sid.into(),
        Some(format!("claude:{sid}")),
    ))
}

pub fn parse_line(raw: &str, path: &Path) -> Result<Option<Event>, String> {
    let v: Value = serde_json::from_str(raw).map_err(|_| "invalid JSON")?;
    let typ = v["type"].as_str().ok_or("missing event type")?;
    if !matches!(typ, "user" | "assistant") {
        return Ok(None);
    }
    if v["isMeta"] == true || v["isCompactSummary"] == true {
        return Ok(None);
    }
    let (session, native, parent) = session_key(path)?;
    if v["sessionId"].as_str().is_some_and(|s| s != native) {
        return Err("session id mismatch".into());
    }
    let id = v["uuid"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
        .ok_or("missing or invalid message id")?;
    let timestamp = v["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .ok_or("missing or invalid timestamp")?
        .timestamp_millis();
    let cwd = v["cwd"]
        .as_str()
        .filter(|s| valid_cwd(s))
        .ok_or("missing or invalid cwd")?;
    let content = &v["message"]["content"];
    if !v["message"].is_object() || !(content.is_string() || content.is_array()) {
        return Err("missing message content".into());
    }
    let has = |kind: &str| {
        content
            .as_array()
            .is_some_and(|a| a.iter().any(|b| b["type"] == kind))
    };
    let label = if has("tool_use") {
        "ツール呼び出し"
    } else if has("tool_result") {
        "ツール実行結果"
    } else if typ == "user" {
        "ユーザー入力"
    } else {
        "アシスタント応答"
    };
    Ok(Some(Event {
        details: Some(SessionDetails {
            parent_session_id: parent,
            git_branch: v["gitBranch"]
                .as_str()
                .map(|s| s.chars().filter(|c| !c.is_control()).take(256).collect()),
        }),
        source: Some("claude".into()),
        project_name: None,
        native_session_id: Some(native),
        native_turn_id: Some(id.into()),
        timestamp,
        kind: "activity".into(),
        session_id: Some(session),
        turn_id: Some(id.into()),
        cwd: Some(cwd.into()),
        text: Some(label.into()),
        tokens: None,
    }))
}

fn valid_cwd(s: &str) -> bool {
    let p = s.replace('\\', "/");
    !p.is_empty()
        && p.len() <= 32768
        && !p.chars().any(char::is_control)
        && (p.starts_with('/')
            || (p.as_bytes().get(1) == Some(&b':') && p.as_bytes().get(2) == Some(&b'/')))
        && !p.split('/').any(|s| s == ".." || s == ".")
}
