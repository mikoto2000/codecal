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

fn linked(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points, not just symlinks.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.is_symlink()
    }
}

pub(crate) fn validate_file(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().take(5) {
        if linked(&std::fs::symlink_metadata(ancestor).map_err(|_| "unreadable transcript path")?) {
            return Err("linked transcript path rejected".into());
        }
    }
    if !path.is_file() {
        return Err("not a transcript file".into());
    }
    Ok(())
}

pub(crate) fn discover(root: &Path) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    let meta = std::fs::symlink_metadata(root)
        .map_err(|_| "Claude Code ログディレクトリが見つかりません")?;
    if !meta.is_dir() || linked(&meta) {
        return Err("Claude Code projects must be an unlinked directory".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "unreadable Claude Code projects directory")?;
    let rejected_links = std::cell::Cell::new(0usize);
    let mut files = vec![];
    let mut warnings = vec![];
    for entry in walkdir::WalkDir::new(&root)
        .max_depth(4)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .filter_entry(|e| {
            std::fs::symlink_metadata(e.path()).is_ok_and(|m| {
                if linked(&m) {
                    rejected_links.set(rejected_links.get() + 1);
                    false
                } else {
                    true
                }
            })
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                if warnings.len() < 20 {
                    warnings.push("Claude Code: unreadable transcript directory".into());
                }
                continue;
            }
        };
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|s| s != "jsonl") {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| "transcript outside root")?;
        let depth = relative.components().count();
        let main = depth == 2
            && relative
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(uuid);
        let child = depth == 4
            && relative
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|s| s == "subagents");
        if !(main || child) || session_key(relative).is_err() {
            if warnings.len() < 20 {
                warnings.push("Claude Code: invalid transcript path skipped".into());
            }
            continue;
        }
        files.push(entry.into_path());
    }
    if rejected_links.get() > 0 {
        warnings.push(format!(
            "Claude Code: {} linked paths rejected",
            rejected_links.get()
        ));
    }
    Ok((files, warnings))
}

/// Claude transcripts have message timestamps rather than explicit run ends.
/// Convert adjacent activity (at most five minutes apart) into the existing
/// lifecycle model. A text answer is an endpoint; user thinking time is excluded.
/// The common aggregate still computes both union and summed durations.
pub(crate) fn activity_events(events: Vec<(String, String, Event)>) -> Vec<(String, Event)> {
    let mut previous: std::collections::BTreeMap<String, Event> = std::collections::BTreeMap::new();
    let mut result = vec![];
    for (_, sid, e) in events {
        if e.source.as_deref() != Some("claude") {
            result.push((sid, e));
            continue;
        }
        if let Some(p) = previous.remove(&sid) {
            if e.timestamp >= p.timestamp && e.timestamp - p.timestamp <= 300_000 {
                let mut end = p;
                end.timestamp = e.timestamp;
                end.kind = "completed".into();
                result.push((sid.clone(), end));
            }
        }
        if e.text.as_deref() == Some("アシスタント応答") {
            continue;
        }
        let mut start = e.clone();
        start.kind = "start".into();
        result.push((sid.clone(), start));
        previous.insert(sid, e);
    }
    result
}
