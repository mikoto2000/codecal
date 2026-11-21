use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: i64,
    pub kind: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
    pub text: Option<String>,
    pub tokens: Option<i64>,
}
fn text_content(v: &Value) -> Option<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|i| i.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|s| !s.is_empty())
}
pub fn parse_line(line: &str) -> Result<Option<Event>, String> {
    let v: Value = serde_json::from_str(line).map_err(|_| "invalid JSON".to_string())?;
    let p = &v["payload"];
    let typ = v["type"].as_str().ok_or("missing event type")?;
    let sub = p["type"].as_str().unwrap_or("");
    let kind = match (typ, sub) {
        ("session_meta", _) => "meta",
        ("turn_context", _) => "context",
        ("event_msg", "task_started" | "turn_started") => "start",
        ("event_msg", "task_complete" | "turn_complete") => "completed",
        ("event_msg", "turn_aborted") => "interrupted",
        ("event_msg", "task_failed" | "turn_failed") => "failed",
        ("event_msg", "user_message") => "prompt",
        ("response_item", "message") if p["role"] == "user" => "prompt",
        ("event_msg", "item_completed")
            if p["item"]["type"] == "user_message"
                || p["item"]["type"] == "userMessage"
                || p["item"]["type"] == "UserMessage" =>
        {
            "prompt"
        }
        ("token_usage_record", _) => "tokens",
        _ => return Ok(None),
    };
    let timestamp = v["timestamp"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .ok_or("missing or invalid timestamp")?
        .timestamp_millis();
    let string = |key: &str| p[key].as_str().map(str::to_owned);
    let turn_id = string("turn_id").or_else(|| {
        p["internal_chat_message_metadata_passthrough"]["turn_id"]
            .as_str()
            .map(str::to_owned)
    });
    let text = if kind == "prompt" {
        string("message")
            .or_else(|| text_content(&p["content"]))
            .or_else(|| text_content(&p["item"]["content"]))
    } else {
        None
    };
    Ok(Some(Event {
        timestamp,
        kind: kind.into(),
        session_id: if kind == "meta" {
            string("id").or_else(|| string("session_id"))
        } else {
            None
        },
        turn_id,
        cwd: string("cwd"),
        text: text.map(|s| s.chars().take(16384).collect()),
        tokens: p["turn_token_usage"]["total_tokens"].as_i64(),
    }))
}
