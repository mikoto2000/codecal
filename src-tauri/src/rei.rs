//! Read-only adapters for Rei's typed event database and legacy JSONL files.
use crate::{
    parser::Event,
    store::{err, hash, mark_activity_dirty, Progress, Store},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 1024 * 1024;
const READER_VERSION: u8 = 2;
const LIFECYCLE: &str = "'AGENT_RUN_STARTED','AGENT_RUN_COMPLETED','AGENT_RUN_FAILED','AGENT_RUN_CANCELLED','agent.run.started','agent.run.completed','agent.run.failed','agent.run.cancelled'";

pub fn default_root() -> PathBuf {
    if let Some(path) = std::env::var_os("REI_DATA_DIR").filter(|s| !s.is_empty()) {
        return path.into();
    }
    let home = std::env::var_os("USERPROFILE")
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Rei")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Rei")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("rei")
    }
}

fn readonly(path: &Path) -> Result<Connection, String> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(err)?;
    c.busy_timeout(std::time::Duration::from_millis(500))
        .map_err(err)?;
    c.pragma_update(None, "query_only", true).map_err(err)?;
    Ok(c)
}
pub(crate) fn validate_root(root: &Path) -> Result<(), String> {
    if !root.is_dir() {
        return Err("れいのデータディレクトリが見つかりません".into());
    }
    if !root.join("storage.db").is_file() && !root.join("projects").is_dir() {
        return Err(
            "れいの取り込み対象（storage.db または projects ディレクトリ）が見つかりません".into(),
        );
    }
    Ok(())
}
fn table(c: &Connection, name: &str) -> Result<bool, String> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        [name],
        |r| r.get(0),
    )
    .map_err(err)
}
fn identity(path: &Path) -> Result<String, String> {
    Ok(format!(
        "{:?}",
        path.metadata().map_err(err)?.created().ok()
    ))
}
fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
fn timestamp(v: &Value) -> Result<i64, String> {
    if let Some(s) = v.as_str() {
        return chrono::DateTime::parse_from_rfc3339(s)
            .map(|t| t.timestamp_millis())
            .map_err(|_| "invalid timestamp".into());
    }
    // Jackson's Instant is decimal Unix seconds. Avoid rounding across a millisecond boundary.
    let s = v.to_string();
    let negative = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let seconds = whole.parse::<i64>().map_err(|_| "invalid timestamp")?;
    let millis = format!("{frac:0<3}")[..3]
        .parse::<i64>()
        .map_err(|_| "invalid timestamp")?;
    let result = seconds
        .checked_mul(1000)
        .and_then(|n| n.checked_add(millis))
        .ok_or("invalid timestamp")?;
    let result = if negative {
        result
            .checked_neg()
            .and_then(|n| {
                n.checked_sub(i64::from(
                    frac.len() > 3 && frac[3..].chars().any(|c| c != '0'),
                ))
            })
            .ok_or("invalid timestamp")?
    } else {
        result
    };
    chrono::DateTime::from_timestamp_millis(result).ok_or("invalid timestamp")?;
    Ok(result)
}
type Projects = BTreeMap<String, (String, String)>;
fn projects(root: &Path) -> Result<Projects, String> {
    let path = root.join("projects.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    if path.metadata().map_err(err)?.len() > 16 * LIMIT {
        return Err("projects.json is too large".into());
    }
    let v: Value = serde_json::from_reader(File::open(path).map_err(err)?)
        .map_err(|_| "invalid projects.json")?;
    let mut result = BTreeMap::new();
    for p in v["projects"].as_array().ok_or("invalid projects.json")? {
        if let Some(id) = text(p, "id") {
            result.insert(
                id.clone(),
                (
                    text(p, "path").unwrap_or_else(|| format!("rei-project:{id}")),
                    text(p, "name").unwrap_or(id),
                ),
            );
        }
    }
    Ok(result)
}
struct Record {
    id: String,
    project: String,
    event: Event,
}
fn parse(raw: &str, project: &str, projects: &Projects) -> Result<Option<Record>, String> {
    let v: Value = serde_json::from_str(raw).map_err(|_| "invalid JSON")?;
    let kind = match v["type"].as_str().ok_or("missing event type")? {
        "AGENT_RUN_STARTED" | "agent.run.started" => "start",
        "AGENT_RUN_COMPLETED" | "agent.run.completed" => "completed",
        "AGENT_RUN_FAILED" | "agent.run.failed" => "failed",
        "AGENT_RUN_CANCELLED" | "agent.run.cancelled" => "interrupted",
        _ => return Ok(None),
    };
    if v.get("version")
        .and_then(Value::as_i64)
        .is_some_and(|n| n != 1)
    {
        return Err("unsupported event version".into());
    }
    if text(&v, "projectId").is_some_and(|p| p != project) {
        return Err("project mismatch".into());
    }
    let run = text(&v, "runId")
        .or_else(|| text(&v["payload"], "runId"))
        .ok_or("missing run id")?;
    let (cwd, name) = projects
        .get(project)
        .cloned()
        .unwrap_or_else(|| (format!("rei-project:{project}"), format!("れい {project}")));
    Ok(Some(Record {
        id: text(&v, "id").ok_or("missing event id")?,
        project: project.into(),
        event: Event {
            details: None,
            source: Some("rei".into()),
            project_name: Some(name),
            native_session_id: text(&v, "sessionId"),
            native_turn_id: text(&v, "turnId"),
            timestamp: timestamp(&v["timestamp"])?,
            kind: kind.into(),
            // Lifecycle correlation must survive missing/changing session metadata.
            // Keep the original session separately for display; project + run is authoritative.
            session_id: Some(format!("rei:{}", hash(format!("{project}\0{run}")))),
            turn_id: Some(run),
            cwd: Some(cwd),
            text: None,
            tokens: None,
        },
    }))
}
fn save(
    tx: &Transaction<'_>,
    record: &Record,
    path: &str,
    offset: Option<u64>,
) -> Result<(), String> {
    let id = format!("rei:{}", hash(format!("{}\0{}", record.project, record.id)));
    let mut event = record.event.clone();
    let previous: Option<String> = tx
        .query_row("SELECT event FROM events WHERE id=?", [&id], |r| r.get(0))
        .optional()
        .map_err(err)?;
    if let Some(previous) = previous {
        let previous: Event = serde_json::from_str(&previous).map_err(err)?;
        event.text = event.text.or(previous.text);
        event.native_session_id = event.native_session_id.or(previous.native_session_id);
        event.native_turn_id = event.native_turn_id.or(previous.native_turn_id);
    }
    // Retain optional enrichment, while refreshing project and execution metadata.
    let json = serde_json::to_string(&event).map_err(err)?;
    let changed = tx.execute("INSERT INTO events VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET event=excluded.event,session_id=excluded.session_id,ts=excluded.ts WHERE events.event<>excluded.event OR events.session_id<>excluded.session_id OR events.ts<>excluded.ts",params![id,event.session_id,event.timestamp,json]).map_err(err)?;
    if changed > 0 {
        mark_activity_dirty(tx)?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO rei_event_sources VALUES(?,?,?,?,?)",
        params![
            id,
            path,
            record.project,
            record.id,
            offset.map(|n| n as i64)
        ],
    )
    .map_err(err)?;
    Ok(())
}
fn prompt(c: &Connection, project: &str, run: &str) -> Result<Option<String>, String> {
    let raw:Option<String>=c.query_row("SELECT record FROM turns WHERE project_id=? AND run_id=? AND length(CAST(record AS BLOB))<=1048576 LIMIT 1",params![project,run],|r|r.get(0)).optional().map_err(err)?;
    Ok(raw
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| text(&v, "request"))
        .map(|s| s.chars().take(16384).collect()))
}

impl Store {
    /// A failure of one Rei source does not prevent importing the other sources.
    pub fn sync_rei(&mut self, root: &Path) -> Result<Progress, String> {
        validate_root(root)?;
        let mut warnings = vec![];
        let map = match projects(root) {
            Ok(p) => {
                self.rei_diagnostic(&root.join("projects.json"), None)?;
                p
            }
            Err(e) => {
                warnings.push(format!("れい: projects.json: {e}"));
                self.rei_diagnostic(&root.join("projects.json"), Some(&e))?;
                BTreeMap::new()
            }
        };
        let mut progress = Progress {
            files: 0,
            bytes_read: 0,
            warnings,
        };
        let db = root.join("storage.db");
        if db.is_file() {
            progress.files += 1;
            match self.rei_database(&db, &map) {
                Ok(bytes) => {
                    progress.bytes_read += bytes;
                    self.rei_diagnostic(&db, None)?;
                }
                Err(e) => {
                    self.rei_diagnostic(&db, Some(&e))?;
                    progress.warnings.push(format!("れい: storage.db: {e}"));
                }
            }
        }
        let dirs = root.join("projects");
        if dirs.is_dir() {
            let entries = match std::fs::read_dir(&dirs) {
                Ok(entries) => Some(entries),
                Err(_) => {
                    progress
                        .warnings
                        .push("れい: 旧ログディレクトリを読み取れません".into());
                    self.rei_diagnostic(&dirs, Some("れいの旧ログディレクトリを読み取れません"))?;
                    None
                }
            };
            for entry in entries.into_iter().flatten() {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        progress
                            .warnings
                            .push("れい: 旧ログ項目を読み取れません".into());
                        self.rei_diagnostic(&dirs, Some("れいの旧ログ項目を読み取れません"))?;
                        continue;
                    }
                };
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let file = entry.path().join("events/events.jsonl");
                if !file.is_file() {
                    continue;
                }
                progress.files += 1;
                match self.rei_jsonl(&file, &entry.file_name().to_string_lossy(), &map) {
                    Ok(bytes) => progress.bytes_read += bytes,
                    Err(e) => {
                        self.rei_diagnostic(&file, Some(&e))?;
                        progress
                            .warnings
                            .push(format!("れい: {}: {e}", file.display()));
                    }
                }
            }
        }
        // Also recovers derived data if a previous sync stopped after committing a page.
        self.rebuild_if_dirty()?;
        Ok(progress)
    }
    pub(crate) fn rei_diagnostic(&self, path: &Path, message: Option<&str>) -> Result<(), String> {
        let path = path.display().to_string();
        if let Some(message) = message {
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO diagnostics VALUES(?,?)",
                    params![path, message],
                )
                .map_err(err)?;
        } else {
            self.conn
                .execute("DELETE FROM diagnostics WHERE path=?", [path])
                .map_err(err)?;
        }
        Ok(())
    }
    fn rei_database(&mut self, path: &Path, map: &Projects) -> Result<u64, String> {
        let c = readonly(path)?;
        c.execute_batch("BEGIN").map_err(err)?;
        if !table(&c, "agent_events")? {
            return Err("れいの agent_events テーブルがありません".into());
        }
        let has_turns = table(&c, "turns")?;
        let project_query = if table(&c, "event_sequences")? {
            "SELECT project_id FROM event_sequences"
        } else {
            "SELECT DISTINCT project_id FROM agent_events"
        };
        let projects: Vec<String> = c
            .prepare(project_query)
            .map_err(err)?
            .query_map([], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let key = path.display().to_string();
        let identity = format!(
            "{READER_VERSION}|{}|{}",
            identity(path)?,
            hash(serde_json::to_vec(map).map_err(err)?)
        );
        let mut bytes = 0;
        for project in projects {
            let high:Option<(i64,String)>=c.query_row("SELECT sequence,id FROM agent_events WHERE project_id=? ORDER BY sequence DESC LIMIT 1",[&project],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?;
            let Some((high, anchor)) = high else {
                continue;
            };
            let previous: Option<(i64, String, String)> = self
                .conn
                .query_row(
                    "SELECT sequence,anchor,identity FROM rei_sync WHERE path=? AND project=?",
                    params![key, project],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(err)?;
            let mut after = 0;
            if let Some((seq, old_anchor, old_identity)) = previous {
                let current: Option<String> = c
                    .query_row(
                        "SELECT id FROM agent_events WHERE project_id=? AND sequence=?",
                        params![project, seq],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(err)?;
                if old_identity == identity
                    && seq <= high
                    && current.as_deref() == Some(&old_anchor)
                {
                    after = seq;
                }
            }
            let diagnostic_path = PathBuf::from(format!("{key} [project {project}]"));
            let mut warnings = if after == 0 {
                Vec::new()
            } else {
                self.conn
                    .query_row(
                        "SELECT message FROM diagnostics WHERE path=?",
                        [diagnostic_path.display().to_string()],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(err)?
                    .map(|s| s.lines().map(str::to_owned).collect())
                    .unwrap_or_default()
            };
            loop {
                let sql=format!("SELECT sequence,id,CASE WHEN length(CAST(record AS BLOB))<=1048576 THEN record END FROM agent_events WHERE project_id=? AND sequence>? AND sequence<=? AND event_type IN ({LIFECYCLE}) ORDER BY sequence LIMIT 256");
                let rows: Vec<(i64, String, Option<String>)> = c
                    .prepare(&sql)
                    .map_err(err)?
                    .query_map(params![project, after, high], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })
                    .map_err(err)?
                    .collect::<Result<_, _>>()
                    .map_err(err)?;
                if rows.is_empty() {
                    break;
                }
                let mut records = Vec::new();
                for (sequence, id, raw) in &rows {
                    let Some(raw) = raw else {
                        if warnings.len() < 20 {
                            warnings.push(format!("sequence {sequence}: oversized event skipped"));
                        }
                        continue;
                    };
                    bytes += raw.len() as u64;
                    match parse(raw, &project, map) {
                        Ok(Some(record)) if record.id == *id => {
                            records.push(record);
                        }
                        Ok(None) => {}
                        _ => {
                            if warnings.len() < 20 {
                                warnings.push(format!(
                                    "sequence {sequence}: invalid activity event skipped"
                                ));
                            }
                        }
                    }
                }
                let (last, last_id, _) = rows.last().unwrap();
                let tx = self.conn.transaction().map_err(err)?;
                for record in records {
                    save(&tx, &record, &key, None)?;
                }
                tx.execute(
                    "INSERT OR REPLACE INTO rei_sync VALUES(?,?,?,?,?)",
                    params![key, project, last, last_id, identity],
                )
                .map_err(err)?;
                if !warnings.is_empty() {
                    tx.execute(
                        "INSERT OR REPLACE INTO diagnostics VALUES(?,?)",
                        params![diagnostic_path.display().to_string(), warnings.join("\n")],
                    )
                    .map_err(err)?;
                }
                tx.commit().map_err(err)?;
                after = *last;
            }
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO rei_sync VALUES(?,?,?,?,?)",
                    params![key, project, high, anchor, identity],
                )
                .map_err(err)?;
            let warning_message = warnings.join("\n");
            self.rei_diagnostic(
                &diagnostic_path,
                (!warning_message.is_empty()).then_some(warning_message.as_str()),
            )?;
        }
        let prompt_path = PathBuf::from(format!("{key} [依頼文]"));
        if has_turns && self.rei_prompts(&c, &key).is_err() {
            self.rei_diagnostic(
                &prompt_path,
                Some("れいの依頼文を読み取れません。実行イベントの取り込みは継続します"),
            )?;
        } else {
            self.rei_diagnostic(&prompt_path, None)?;
        }
        Ok(bytes)
    }
    fn rei_prompts(&mut self, source: &Connection, path: &str) -> Result<(), String> {
        let mut after = String::new();
        loop {
            let rows: Vec<(String,String,String)> = self.conn.prepare("SELECT e.id,s.project,e.event FROM events e JOIN rei_event_sources s ON s.event_id=e.id WHERE s.path=? AND e.id>? AND json_extract(e.event,'$.kind')='start' AND json_extract(e.event,'$.text') IS NULL ORDER BY e.id LIMIT 256").map_err(err)?
                .query_map(params![path,after], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(err)?.collect::<Result<_,_>>().map_err(err)?;
            if rows.is_empty() {
                break;
            }
            let mut updates = Vec::new();
            for (id, project, json) in &rows {
                let mut e: Event = serde_json::from_str(json).map_err(err)?;
                if let Some(request) =
                    prompt(source, project, e.turn_id.as_deref().unwrap_or_default())?
                {
                    e.text = Some(request);
                    updates.push((id, serde_json::to_string(&e).map_err(err)?));
                }
            }
            if !updates.is_empty() {
                let tx = self.conn.transaction().map_err(err)?;
                for (id, json) in updates {
                    tx.execute("UPDATE events SET event=? WHERE id=?", params![json, id])
                        .map_err(err)?;
                }
                mark_activity_dirty(&tx)?;
                tx.commit().map_err(err)?;
            }
            after = rows.last().unwrap().0.clone();
        }
        Ok(())
    }
    fn rei_jsonl(&mut self, path: &Path, project: &str, map: &Projects) -> Result<u64, String> {
        let key = path.display().to_string();
        let stable_identity = format!(
            "{READER_VERSION}|{}|{}",
            identity(path)?,
            hash(serde_json::to_vec(map).map_err(err)?)
        );
        let file = File::open(path).map_err(err)?;
        let metadata = file.metadata().map_err(err)?;
        let len = metadata.len();
        let identity = format!("{stable_identity}|{:?}|{len}", metadata.modified().ok());
        let mut reader = BufReader::new(file);
        let previous: Option<(u64, String, u64, String)> = self
            .conn
            .query_row(
                "SELECT offset,head,head_len,identity FROM rei_file_sync WHERE path=?",
                [&key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u64,
                        r.get(1)?,
                        r.get::<_, i64>(2)? as u64,
                        r.get(3)?,
                    ))
                },
            )
            .optional()
            .map_err(err)?;
        let mut offset = 0;
        if let Some((old, head, head_len, old_identity)) = previous {
            let mut buf = vec![0; head_len.min(len) as usize];
            reader.read_exact(&mut buf).map_err(err)?;
            if old <= len
                && old_identity.starts_with(&format!("{stable_identity}|"))
                && (len != old || old_identity == identity)
                && hash(&buf) == head
            {
                offset = old;
            }
        }
        reader.seek(SeekFrom::Start(0)).map_err(err)?;
        let head_len = len.min(4096);
        let mut head = vec![0; head_len as usize];
        reader.read_exact(&mut head).map_err(err)?;
        let head = hash(&head);
        reader.seek(SeekFrom::Start(offset)).map_err(err)?;
        let initial = offset;
        let mut warnings = Vec::new();
        loop {
            let tx = self.conn.transaction().map_err(err)?;
            let mut consumed = 0;
            for _ in 0..256 {
                let before = offset;
                let mut buf = Vec::new();
                let n = reader
                    .by_ref()
                    .take(LIMIT + 1)
                    .read_until(b'\n', &mut buf)
                    .map_err(err)?;
                if n == 0 {
                    break;
                }
                let mut size = n as u64;
                let mut complete = buf.last() == Some(&b'\n');
                if !complete && size > LIMIT {
                    loop {
                        let mut discard = Vec::new();
                        let n = reader
                            .by_ref()
                            .take(65536)
                            .read_until(b'\n', &mut discard)
                            .map_err(err)?;
                        size += n as u64;
                        if n == 0 {
                            break;
                        }
                        if discard.last() == Some(&b'\n') {
                            complete = true;
                            break;
                        }
                    }
                }
                if !complete {
                    break;
                }
                offset += size;
                consumed += 1;
                let parsed = if size > LIMIT {
                    Err("oversized line".into())
                } else {
                    std::str::from_utf8(&buf)
                        .map_err(|_| "invalid UTF-8".into())
                        .and_then(|s| parse(s, project, map))
                };
                match parsed {
                    Ok(Some(record)) => save(&tx, &record, &key, Some(before))?,
                    Ok(None) => {}
                    Err(e) => {
                        if warnings.len() < 20 {
                            warnings.push(format!("byte {before}: {e}"));
                        }
                    }
                }
            }
            tx.execute(
                "INSERT OR REPLACE INTO rei_file_sync VALUES(?,?,?,?,?)",
                params![key, offset as i64, head, head_len as i64, identity],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
            if consumed < 256 {
                break;
            }
        }
        if !warnings.is_empty() {
            self.rei_diagnostic(path, Some(&warnings.join("\n")))?;
        }
        Ok(offset - initial)
    }
    pub(crate) fn rei_excerpt(&self, turn: &str) -> Result<Option<String>, String> {
        let sources: Vec<(String,String,String,Option<i64>)> = self.conn.prepare("SELECT s.path,s.project,s.native_id,s.offset FROM rei_event_sources s JOIN events e ON e.id=s.event_id JOIN turns t ON t.session_id=e.session_id WHERE t.id=? AND json_extract(e.event,'$.turn_id')=t.turn_id AND json_extract(e.event,'$.kind')='start' ORDER BY s.offset IS NOT NULL").map_err(err)?
            .query_map([turn],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(err)?.collect::<Result<_,_>>().map_err(err)?;
        if sources.is_empty() {
            return Ok(None);
        }
        for (path, project, id, offset) in sources {
            let result = (|| -> Result<String, String> {
                if let Some(offset) = offset {
                    let mut f = File::open(&path).map_err(err)?;
                    f.seek(SeekFrom::Start(offset as u64)).map_err(err)?;
                    let mut bytes = Vec::new();
                    BufReader::new(f)
                        .take(LIMIT + 1)
                        .read_until(b'\n', &mut bytes)
                        .map_err(err)?;
                    let first = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
                    let record: Value = serde_json::from_slice(first)
                        .map_err(|_| "original event is unavailable")?;
                    if record["id"].as_str() != Some(&id) {
                        return Err("original event was replaced".into());
                    }
                    bytes.truncate(65536);
                    return Ok(format!(
                        "{path}\nbyte {offset}\n{}",
                        String::from_utf8_lossy(&bytes)
                    ));
                }
                let c = readonly(Path::new(&path))?;
                let raw:String=c.query_row("SELECT substr(CAST(record AS BLOB),1,65536) FROM agent_events WHERE project_id=? AND id=?",params![project,id],|r|{let bytes:Vec<u8>=r.get(0)?; Ok(String::from_utf8_lossy(&bytes).into_owned())}).map_err(err)?;
                Ok(format!("{path}\nproject {project} / event {id}\n{raw}"))
            })();
            if let Ok(excerpt) = result {
                return Ok(Some(excerpt));
            }
        }
        Err("元の実行イベントを読み取れません。取り込み済みの履歴は保持しています".into())
    }
}
