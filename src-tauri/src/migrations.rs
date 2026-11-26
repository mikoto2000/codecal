//! Transactional, additive migrations for the known CodeCal database layouts.
use crate::store::err;
use rusqlite::{Connection, MAIN_DB};
use std::path::Path;

const VERSION: i64 = 4;
pub(crate) const HUMAN_START_INDEX_SQL: &str = "CREATE INDEX human_activity_start ON human_activity_records(source_id,captured_at,native_id,duration_ms,effective_end,kind);";

fn covering_period_index(conn: &Connection) -> Result<bool, String> {
    let mut query = conn
        .prepare("PRAGMA index_info(human_activity_start)")
        .map_err(err)?;
    let names = query
        .query_map([], |row| row.get::<_, String>(2))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let unused_end: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='human_activity_end')",[],|r|r.get(0)).map_err(err)?;
    Ok(!unused_end
        && names
            == [
                "source_id",
                "captured_at",
                "native_id",
                "duration_ms",
                "effective_end",
                "kind",
            ])
}
fn ensure_period_index(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("DROP INDEX IF EXISTS human_activity_end;")
        .map_err(err)?;
    if !covering_period_index(conn)? {
        conn.execute_batch("DROP INDEX IF EXISTS human_activity_start;")
            .map_err(err)?;
        conn.execute_batch(HUMAN_START_INDEX_SQL).map_err(err)?;
    }
    Ok(())
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    // All identifiers passed here are constants from this module.
    let mut q = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(err)?;
    let rows = q.query_map([], |r| r.get(1)).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}
fn validate_base(conn: &Connection) -> Result<(), String> {
    for (table, expected) in [
        ("schema_migrations", "version"),
        ("projects", "id,cwd,name,color,last_seen"),
        ("sessions", "id,project_id,start,last_event"),
        ("turns", "id,session_id,turn_id,project_id,start,end,status,prompt,tokens,source,native_session_id,native_turn_id"),
        ("activity_intervals", "id,project_id,turn_id,start,end,status"),
        ("events", "id,session_id,ts,event"),
        ("event_sources", "event_id,path,offset,line"),
        ("sync_state", "path,offset,line,head,head_len,identity,session_id,updated"),
        ("diagnostics", "path,message"),
        ("settings", "key,value"),
        ("rei_event_sources", "event_id,path,project,native_id,offset"),
        ("rei_sync", "path,project,sequence,anchor,identity"),
        ("rei_file_sync", "path,offset,head,head_len,identity"),
    ] {
        if columns(conn, table)?.join(",") != expected {
            return Err(format!("DBの自動移行に対応していないテーブル構造です: {table}（データは保持されています）"));
        }
    }
    // A historical v3 used this optional cache. Keep it and all its records.
    let cache = columns(conn, "claude_files")?;
    if !cache.is_empty() && cache.join(",") != "path,fingerprint" {
        return Err("DBの自動移行に対応していないテーブル構造です: claude_files".into());
    }
    Ok(())
}
fn validate(conn: &Connection) -> Result<(), String> {
    validate_base(conn)?;
    for (table, expected) in [
        (
            "human_activity_sources",
            "source_id,source_path,file_identity,reader_version,last_sync,sync_status,diagnostics",
        ),
        (
            "human_activity_records",
            "source_id,native_id,captured_at,duration_ms,kind,content_hash,effective_end",
        ),
    ] {
        if columns(conn, table)?.join(",") != expected {
            return Err("人間アクティビティキャッシュの構造に対応していません".into());
        }
    }
    Ok(())
}
fn backup(conn: &Connection, path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if tables == 0 {
        return Ok(());
    }
    let mut name = path
        .file_name()
        .ok_or("DBのファイル名が不正です")?
        .to_os_string();
    name.push(".before-v4.bak");
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    conn.backup(MAIN_DB, temporary.path(), None)
        .map_err(|e| format!("DB移行前のバックアップに失敗しました: {e}"))?;
    crate::storage::exclude_human_cache(temporary.path())?;
    for sequence in 0_u64.. {
        let mut candidate = name.clone();
        if sequence > 0 {
            candidate.push(format!(".{sequence}"));
        }
        let target = path.with_file_name(candidate);
        if target.exists() {
            if !target.is_file() {
                return Err("DB移行前のバックアップ先がファイルではありません".into());
            }
            continue;
        }
        match temporary.persist_noclobber(target) {
            Ok(_) => return Ok(()),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temporary = e.file,
            Err(e) => {
                return Err(format!(
                    "DB移行前のバックアップを保存できません: {}",
                    e.error
                ))
            }
        }
    }
    Err("DB移行前のバックアップ名を確保できません".into())
}

pub(crate) fn migrate(
    conn: &mut Connection,
    path: &Path,
    on_migration: impl Fn(),
) -> Result<(), String> {
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(err)?;
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(err)?;
    if version > VERSION {
        return Err(format!("このデータベースは新しい CodeCal で作成されています（DB v{version} / 対応 v{VERSION}）。対応する新しい CodeCal を使用してください"));
    }
    if version == VERSION {
        validate(conn)?;
    } else {
        // v3 was a validated baseline, not an uninitialized database.
        if version == 3 {
            validate_base(conn)?;
        }
        on_migration();
        backup(conn, path)?;
    }
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
        .map_err(err)?;
    if version == VERSION {
        if !covering_period_index(conn)? {
            on_migration();
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(err)?;
            let locked: i64 = tx
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(err)?;
            if locked != VERSION {
                return Err("別のCodeCalがDBの構造を更新しました".into());
            }
            validate(&tx)?;
            ensure_period_index(&tx)?;
            tx.commit().map_err(err)?;
        }
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(err)?;
    // Another executable may have migrated while we waited for the write lock.
    let locked_version: i64 = tx
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(err)?;
    if locked_version > VERSION {
        return Err("別のCodeCalがDBを新しいバージョンへ更新しました".into());
    }
    if locked_version == VERSION {
        validate(&tx)?;
        ensure_period_index(&tx)?;
        return tx.commit().map_err(err);
    }
    // v0 -> v1: initialize the base tables. IF NOT EXISTS also repairs a legacy
    // marker written before all tables had been initialized.
    tx.execute_batch("
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

  ").map_err(err)?;
    // v1 -> v2: older executables sometimes rewrote user_version despite these
    // columns already existing. Inspect the schema rather than adding twice.
    for (column, definition) in [
        ("source", "source TEXT NOT NULL DEFAULT 'codex'"),
        ("native_session_id", "native_session_id TEXT"),
        ("native_turn_id", "native_turn_id TEXT"),
    ] {
        if !columns(&tx, "turns")?.iter().any(|c| c == column) {
            tx.execute_batch(&format!("ALTER TABLE turns ADD COLUMN {definition}"))
                .map_err(err)?;
        }
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS rei_event_sources(event_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,path TEXT NOT NULL,project TEXT NOT NULL,native_id TEXT NOT NULL,offset INTEGER,PRIMARY KEY(event_id,path));
        CREATE TABLE IF NOT EXISTS rei_sync(path TEXT NOT NULL,project TEXT NOT NULL,sequence INTEGER NOT NULL,anchor TEXT NOT NULL,identity TEXT NOT NULL,PRIMARY KEY(path,project));
        CREATE TABLE IF NOT EXISTS rei_file_sync(path TEXT PRIMARY KEY,offset INTEGER NOT NULL,head TEXT NOT NULL,head_len INTEGER NOT NULL,identity TEXT NOT NULL);").map_err(err)?;
    // v2 -> v3: establish a validated compatibility baseline. The historical
    // claude_files cache is optional and is deliberately left untouched.
    tx.execute_batch("CREATE TABLE IF NOT EXISTS human_activity_sources(
        source_id BLOB PRIMARY KEY,source_path TEXT NOT NULL,file_identity TEXT NOT NULL,
        reader_version INTEGER NOT NULL,last_sync INTEGER NOT NULL,
        sync_status TEXT NOT NULL,diagnostics TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS human_activity_records(
        source_id BLOB NOT NULL REFERENCES human_activity_sources(source_id) ON DELETE CASCADE,
        native_id TEXT NOT NULL,captured_at INTEGER NOT NULL,duration_ms INTEGER NOT NULL CHECK(duration_ms>=0),
        kind TEXT NOT NULL CHECK(kind IN ('entertainment','non_entertainment','unknown')),
        content_hash BLOB NOT NULL,effective_end INTEGER,
        PRIMARY KEY(source_id,native_id)) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS human_activity_source_path ON human_activity_sources(source_path,last_sync);").map_err(err)?;
    validate(&tx)?;
    ensure_period_index(&tx)?;
    tx.execute_batch(
        "INSERT OR IGNORE INTO schema_migrations VALUES(1),(2),(3),(4); PRAGMA user_version=4;",
    )
    .map_err(err)?;
    tx.commit().map_err(err)
}
