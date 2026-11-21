use crate::{
    activity::{aggregate, Day, Interval},
    parser::{parse_line, Event},
};
use rayon::prelude::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn hash(s: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(s.as_ref()))
}
#[derive(Serialize)]
pub struct Project {
    pub id: String,
    pub cwd: String,
    pub name: String,
    pub color: String,
}
#[derive(Serialize)]
pub struct Turn {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub project_id: String,
    pub start: i64,
    pub end: Option<i64>,
    pub status: String,
    pub prompt: String,
    pub tokens: Option<i64>,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub projects: Vec<Project>,
    pub turns: Vec<Turn>,
    pub days: Vec<Day>,
    pub diagnostics: Vec<String>,
}
#[derive(Serialize)]
pub struct Progress {
    pub files: usize,
    pub bytes_read: u64,
}
pub struct Store {
    conn: Connection,
}
struct Delta {
    path: String,
    offset: u64,
    line: u64,
    head: String,
    head_len: u64,
    identity: String,
    reset: bool,
    session: String,
    events: Vec<(Event, u64, u64)>,
    diagnostics: Vec<String>,
    bytes: u64,
}
#[derive(Clone)]
struct Cursor {
    offset: u64,
    line: u64,
    head: String,
    head_len: u64,
    identity: String,
    session: String,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(err)?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
  CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);
  CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,cwd TEXT NOT NULL UNIQUE,name TEXT NOT NULL,color TEXT NOT NULL,last_seen INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY,project_id TEXT REFERENCES projects(id),start INTEGER,last_event INTEGER);
  CREATE TABLE IF NOT EXISTS turns(id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id),turn_id TEXT NOT NULL,project_id TEXT NOT NULL REFERENCES projects(id),start INTEGER NOT NULL,end INTEGER,status TEXT NOT NULL,prompt TEXT NOT NULL,tokens INTEGER,UNIQUE(session_id,turn_id));
  CREATE TABLE IF NOT EXISTS activity_intervals(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),turn_id TEXT NOT NULL UNIQUE REFERENCES turns(id) ON DELETE CASCADE,start INTEGER NOT NULL,end INTEGER,status TEXT NOT NULL);
  CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY,session_id TEXT NOT NULL,ts INTEGER NOT NULL,event TEXT NOT NULL);
  CREATE INDEX IF NOT EXISTS events_session ON events(session_id,ts);
  CREATE TABLE IF NOT EXISTS event_sources(event_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,path TEXT NOT NULL,offset INTEGER NOT NULL,line INTEGER NOT NULL,PRIMARY KEY(event_id,path));
  CREATE TABLE IF NOT EXISTS sync_state(path TEXT PRIMARY KEY,offset INTEGER NOT NULL,line INTEGER NOT NULL,head TEXT NOT NULL,head_len INTEGER NOT NULL,identity TEXT NOT NULL,session_id TEXT NOT NULL,updated INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS diagnostics(path TEXT PRIMARY KEY,message TEXT NOT NULL);
  CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
  INSERT OR IGNORE INTO schema_migrations VALUES(1);
  PRAGMA user_version=1;").map_err(err)?;
        Ok(Self { conn })
    }
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn.execute("INSERT INTO settings VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(err)?;
        Ok(())
    }
    pub fn update_project(&self, id: &str, name: &str, color: &str) -> Result<(), String> {
        if name.trim().is_empty()
            || name.chars().count() > 120
            || color.len() != 7
            || !color.starts_with('#')
            || !color[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err("表示名または色が不正です".into());
        }
        self.conn
            .execute(
                "UPDATE projects SET name=?,color=? WHERE id=?",
                params![name, color, id],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn sync(&mut self, root: &Path) -> Result<Progress, String> {
        self.sync_progress(root, |_, _| {})
    }
    pub fn sync_progress(
        &mut self,
        root: &Path,
        progress: impl Fn(usize, usize),
    ) -> Result<Progress, String> {
        if !root.is_dir() {
            return Err("ログディレクトリが見つかりません".into());
        }
        let mut files: Vec<PathBuf> = vec![];
        for e in walkdir::WalkDir::new(root).follow_links(false) {
            match e {
                Ok(e)
                    if e.file_type().is_file()
                        && e.path().extension().is_some_and(|x| x == "jsonl") =>
                {
                    files.push(e.into_path())
                }
                Err(e) => {
                    self.conn
                        .execute(
                            "INSERT OR REPLACE INTO diagnostics VALUES(?,?)",
                            params![
                                e.path()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_default(),
                                "ディレクトリを読み取れません"
                            ],
                        )
                        .map_err(err)?;
                }
                _ => {}
            }
        }
        files.sort();
        let mut bytes_read = 0;
        let mut changed = false;
        let total = files.len();
        let mut cursors: BTreeMap<String, Cursor> = BTreeMap::new();
        {
            let mut q = self
                .conn
                .prepare(
                    "SELECT path,offset,line,head,head_len,identity,session_id FROM sync_state",
                )
                .map_err(err)?;
            for r in q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        Cursor {
                            offset: r.get::<_, i64>(1)? as u64,
                            line: r.get::<_, i64>(2)? as u64,
                            head: r.get(3)?,
                            head_len: r.get::<_, i64>(4)? as u64,
                            identity: r.get(5)?,
                            session: r.get(6)?,
                        },
                    ))
                })
                .map_err(err)?
            {
                let (p, c) = r.map_err(err)?;
                cursors.insert(p, c);
            }
        }
        // Four files at a time bounds cross-session memory while parsing concurrently.
        for (batch_index, batch) in files.chunks(4).enumerate() {
            let deltas: Vec<_> = batch
                .par_iter()
                .map(|p| read_delta(p, cursors.get(&p.display().to_string())))
                .collect();
            for (p, result) in batch.iter().zip(deltas) {
                let d = match result {
                    Ok(d) => d,
                    Err(e) => {
                        self.conn
                            .execute(
                                "INSERT OR REPLACE INTO diagnostics VALUES(?,?)",
                                params![p.display().to_string(), e],
                            )
                            .map_err(err)?;
                        continue;
                    }
                };
                bytes_read += d.bytes;
                if d.bytes == 0 && !d.reset {
                    continue;
                }
                changed = true;
                let tx = self.conn.transaction().map_err(err)?;
                if d.reset {
                    tx.execute("DELETE FROM event_sources WHERE path=?", [&d.path])
                        .map_err(err)?;
                }
                for (event, offset, line) in &d.events {
                    let json = serde_json::to_string(event).map_err(err)?;
                    let id = hash(format!("{}:{}", d.session, json));
                    tx.execute(
                        "INSERT OR IGNORE INTO events VALUES(?,?,?,?)",
                        params![id, d.session, event.timestamp, json],
                    )
                    .map_err(err)?;
                    tx.execute(
                        "INSERT OR REPLACE INTO event_sources VALUES(?,?,?,?)",
                        params![id, d.path, *offset as i64, *line as i64],
                    )
                    .map_err(err)?;
                }
                tx.execute(
                    "INSERT OR REPLACE INTO sync_state VALUES(?,?,?,?,?,?,?,?)",
                    params![
                        d.path,
                        d.offset as i64,
                        d.line as i64,
                        d.head,
                        d.head_len as i64,
                        d.identity,
                        d.session,
                        chrono::Utc::now().timestamp_millis(),
                    ],
                )
                .map_err(err)?;
                // Diagnostics contain only classification/offset, never raw log content.
                if !d.diagnostics.is_empty() {
                    tx.execute(
                        "INSERT OR REPLACE INTO diagnostics VALUES(?,?)",
                        params![d.path, d.diagnostics.join("\n")],
                    )
                    .map_err(err)?;
                } else if d.reset {
                    tx.execute("DELETE FROM diagnostics WHERE path=?", [&d.path])
                        .map_err(err)?;
                }
                tx.execute("DELETE FROM events WHERE NOT EXISTS(SELECT 1 FROM event_sources WHERE event_id=events.id)",[]).map_err(err)?;
                tx.commit().map_err(err)?;
            }
            progress(((batch_index + 1) * 4).min(total), total);
        }
        if changed {
            self.rebuild()?;
        }
        Ok(Progress {
            files: total,
            bytes_read,
        })
    }
    fn rebuild(&mut self) -> Result<(), String> {
        let events: Vec<(String, String, Event)> = {
            let mut q = self
                .conn
                .prepare("SELECT id,session_id,event FROM events ORDER BY ts,rowid")
                .map_err(err)?;
            let rows = q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(err)?;
            let mut result = vec![];
            for row in rows {
                let (id, s, json) = row.map_err(err)?;
                result.push((id, s, serde_json::from_str(&json).map_err(err)?));
            }
            result
        };
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM activity_intervals", [])
            .map_err(err)?;
        tx.execute("DELETE FROM turns", []).map_err(err)?;
        tx.execute("DELETE FROM sessions", []).map_err(err)?;
        let mut cwd: BTreeMap<String, String> = BTreeMap::new();
        let mut active: BTreeMap<String, String> = BTreeMap::new();
        let mut turns: BTreeMap<String, Turn> = BTreeMap::new();
        let mut pending: BTreeMap<String, String> = BTreeMap::new();
        for (_, sid, e) in events {
            let dir = cwd.entry(sid.clone()).or_insert_with(|| "(unknown)".into());
            if let Some(d) = &e.cwd {
                *dir = normalize_path(d);
            }
            let pid = hash(dir.as_bytes());
            let name = dir.rsplit('/').find(|s| !s.is_empty()).unwrap_or("Unknown");
            const COLORS: [&str; 8] = [
                "#14b8a6", "#818cf8", "#f59e0b", "#f472b6", "#38bdf8", "#a3e635", "#fb923c",
                "#c084fc",
            ];
            let color = COLORS[usize::from_str_radix(&pid[..2], 16).unwrap_or(0) % COLORS.len()];
            tx.execute("INSERT INTO projects VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET last_seen=MAX(last_seen,excluded.last_seen)",params![pid,dir.as_str(),name,color,e.timestamp]).map_err(err)?;
            tx.execute("INSERT INTO sessions VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET start=MIN(start,excluded.start),last_event=MAX(last_event,excluded.last_event)",params![sid,pid,e.timestamp,e.timestamp]).map_err(err)?;
            let tid = e.turn_id.clone().or_else(|| active.get(&sid).cloned());
            if e.kind == "start" {
                let tid = e
                    .turn_id
                    .unwrap_or_else(|| format!("legacy-{}", e.timestamp));
                let id = format!("{sid}:{tid}");
                active.insert(sid.clone(), tid.clone());
                turns.entry(id.clone()).or_insert(Turn {
                    id,
                    session_id: sid.clone(),
                    turn_id: tid,
                    project_id: pid,
                    start: e.timestamp,
                    end: None,
                    status: "unknown".into(),
                    prompt: pending.remove(&sid).unwrap_or_default(),
                    tokens: None,
                });
            } else if let Some(tid) = tid {
                if let Some(t) = turns.get_mut(&format!("{sid}:{tid}")) {
                    match e.kind.as_str() {
                        "context" => t.project_id = pid,
                        "prompt" => {
                            if let Some(text) = e.text {
                                t.prompt = text;
                            }
                        }
                        "tokens" => t.tokens = e.tokens,
                        "completed" | "failed" | "interrupted" => {
                            if e.timestamp >= t.start {
                                t.end = Some(e.timestamp);
                                t.status = e.kind;
                            }
                        }
                        _ => {}
                    }
                } else if e.kind == "prompt" {
                    if let Some(text) = e.text {
                        pending.insert(sid, text);
                    }
                }
            } else if e.kind == "prompt" {
                if let Some(text) = e.text {
                    pending.insert(sid, text);
                }
            }
        }
        for t in turns.values() {
            tx.execute(
                "INSERT INTO turns VALUES(?,?,?,?,?,?,?,?,?)",
                params![
                    t.id,
                    t.session_id,
                    t.turn_id,
                    t.project_id,
                    t.start,
                    t.end,
                    t.status,
                    t.prompt,
                    t.tokens
                ],
            )
            .map_err(err)?;
            tx.execute(
                "INSERT INTO activity_intervals VALUES(?,?,?,?,?,?)",
                params![t.id, t.project_id, t.id, t.start, t.end, t.status],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(())
    }
    pub fn snapshot(&self, timezone: &str) -> Result<Snapshot, String> {
        let mut q=self.conn.prepare("SELECT id,cwd,name,color FROM projects WHERE EXISTS(SELECT 1 FROM turns WHERE project_id=projects.id) ORDER BY name").map_err(err)?;
        let projects = q
            .query_map([], |r| {
                Ok(Project {
                    id: r.get(0)?,
                    cwd: r.get(1)?,
                    name: r.get(2)?,
                    color: r.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let mut q=self.conn.prepare("SELECT id,session_id,turn_id,project_id,start,end,status,prompt,tokens FROM turns ORDER BY start").map_err(err)?;
        let turns = q
            .query_map([], |r| {
                Ok(Turn {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    turn_id: r.get(2)?,
                    project_id: r.get(3)?,
                    start: r.get(4)?,
                    end: r.get(5)?,
                    status: r.get(6)?,
                    prompt: r.get(7)?,
                    tokens: r.get(8)?,
                })
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let intervals = turns
            .iter()
            .map(|t| Interval {
                project_id: t.project_id.clone(),
                start: t.start,
                end: t.end,
            })
            .collect::<Vec<_>>();
        let days = aggregate(&intervals, timezone)?;
        let mut q = self
            .conn
            .prepare("SELECT path,message FROM diagnostics ORDER BY path LIMIT 200")
            .map_err(err)?;
        let diagnostics = q
            .query_map([], |r| {
                Ok(format!(
                    "{}: {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                ))
            })
            .map_err(err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        Ok(Snapshot {
            projects,
            turns,
            days,
            diagnostics,
        })
    }
    pub fn source_excerpt(&self, turn_id: &str) -> Result<String, String> {
        let (path,offset):(String,i64)=self.conn.query_row("SELECT es.path,es.offset FROM event_sources es JOIN events e ON e.id=es.event_id JOIN turns t ON t.session_id=e.session_id WHERE t.id=? AND e.ts>=t.start ORDER BY e.ts LIMIT 1",[turn_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(err)?;
        let mut f = File::open(&path).map_err(err)?;
        f.seek(SeekFrom::Start(offset as u64)).map_err(err)?;
        let mut bytes = vec![];
        f.take(65536).read_to_end(&mut bytes).map_err(err)?;
        Ok(format!(
            "{path}\nbyte {offset}\n{}",
            String::from_utf8_lossy(&bytes)
        ))
    }
}
pub fn normalize_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_end_matches('/');
    if p.as_bytes().get(1) == Some(&b':') {
        p.to_lowercase()
    } else {
        p.to_string()
    }
}
fn read_delta(path: &Path, previous: Option<&Cursor>) -> Result<Delta, String> {
    let file = File::open(path).map_err(err)?;
    let meta = file.metadata().map_err(err)?;
    let len = meta.len();
    let identity = format!("{:?}", meta.created().ok());
    let mut reader = BufReader::new(file);
    let old_head_len = previous.map_or(0, |c| c.head_len);
    let mut old_head = vec![0; old_head_len.min(len) as usize];
    reader.read_exact(&mut old_head).map_err(err)?;
    let reset = previous
        .is_none_or(|c| len < c.offset || identity != c.identity || hash(&old_head) != c.head);
    reader.seek(SeekFrom::Start(0)).map_err(err)?;
    let head_len = len.min(4096);
    let mut head = vec![0; head_len as usize];
    reader.read_exact(&mut head).map_err(err)?;
    let mut offset = if reset {
        0
    } else {
        previous.map_or(0, |c| c.offset)
    };
    let start_offset = offset;
    let mut line = if reset {
        0
    } else {
        previous.map_or(0, |c| c.line)
    };
    reader.seek(SeekFrom::Start(offset)).map_err(err)?;
    let mut events = vec![];
    let mut diagnostics = vec![];
    let mut skipped = 0;
    let mut session = if reset {
        String::new()
    } else {
        previous.map_or(String::new(), |c| c.session.clone())
    };
    loop {
        let before = offset;
        let mut buf = Vec::new();
        let mut limited = (&mut reader).take(1048577);
        let n = limited.read_until(b'\n', &mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        if buf.last() != Some(&b'\n') {
            if n > 1048576 {
                let mut discard = vec![];
                loop {
                    discard.clear();
                    let n = (&mut reader)
                        .take(65536)
                        .read_until(b'\n', &mut discard)
                        .map_err(err)?;
                    buf.extend_from_slice(&[]);
                    offset += n as u64;
                    if n == 0 || discard.last() == Some(&b'\n') {
                        break;
                    }
                }
                offset += buf.len() as u64;
                line += 1;
                if diagnostics.len() < 20 {
                    diagnostics.push(format!("line {line}: oversized line skipped"));
                }
                continue;
            }
            break; // Incomplete tail: retain offset until newline arrives.
        }
        offset += n as u64;
        line += 1;
        match std::str::from_utf8(&buf)
            .map_err(|_| "invalid UTF-8".to_string())
            .and_then(parse_line)
        {
            Ok(Some(e)) => {
                if let Some(s) = &e.session_id {
                    session = s.clone();
                }
                events.push((e, before, line));
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                if diagnostics.len() < 20 {
                    diagnostics.push(format!("line {line}, byte {before}: {e}"));
                }
            }
        }
    }
    if skipped > 0 {
        diagnostics.push(format!("{skipped} non-activity / unknown events skipped"));
    }
    if session.is_empty() {
        if events.is_empty() {
            session = format!("unidentified-{}", hash(&head));
        } else {
            return Err("session_meta with session id required".into());
        }
    }
    Ok(Delta {
        path: path.display().to_string(),
        offset,
        line,
        head: hash(&head),
        head_len,
        identity,
        reset,
        session,
        events,
        diagnostics,
        bytes: offset - start_offset,
    })
}
