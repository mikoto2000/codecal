//! Minimized Rei observation reader. Original payloads never cross SQLite's boundary.
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MIN_TIME: i64 = -62_135_596_800_000;
const MAX_TIME: i64 = 253_402_300_799_999;
pub(crate) fn validate_period(from: i64, to: i64) -> Result<(), HumanError> {
    if from >= to
        || from < MIN_TIME
        || to > MAX_TIME
        || to.checked_sub(from).is_none_or(|v| v > 62 * 86_400_000)
    {
        return Err(HumanError("invalid_period"));
    }
    Ok(())
}

pub(crate) fn logical_source_path(path: &Path) -> Result<String, HumanError> {
    let path = std::path::absolute(path).map_err(|_| HumanError("source_unavailable"))?;
    let value = path.to_string_lossy().to_string();
    #[cfg(windows)]
    let value = value.to_lowercase();
    Ok(value)
}

/// Identify the current physical file without opening or querying SQLite.
pub fn source_identity(path: &Path) -> Result<String, HumanError> {
    let file = File::open(path).map_err(|_| HumanError("source_unavailable"))?;
    Ok(crate::store::hash(format!(
        "{}\0{}",
        logical_source_path(path)?,
        identity(&file)?
    )))
}

pub(crate) fn staging_prefix(path: &Path) -> Result<String, HumanError> {
    Ok(format!(
        "human-{}-",
        crate::store::hash(logical_source_path(path)?)
    ))
}

pub(crate) fn validate_staging_directory(path: &Path) -> Result<(), HumanError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| HumanError("staging_failed"))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(HumanError("staging_failed"));
        }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(HumanError("staging_failed"));
    }
    Ok(())
}

fn validate_source_schema(conn: &Connection) -> Result<(), HumanError> {
    let ordinary: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_list WHERE schema='main' AND name='activity_records' AND type='table' AND wr=0)",[],|r|r.get(0)).map_err(|_|HumanError("unsupported_source"))?;
    let columns: i64 = conn.query_row("SELECT count(*) FROM pragma_table_xinfo('activity_records') WHERE hidden=0 AND ((name='id' AND upper(type)='TEXT' AND pk=1) OR (name='captured_at' AND upper(type)='INTEGER') OR (name='payload' AND upper(type)='TEXT'))",[],|r|r.get(0)).map_err(|_|HumanError("unsupported_source"))?;
    let shadows: i64 = conn.query_row("SELECT count(*) FROM pragma_table_xinfo('activity_records') WHERE lower(name) IN ('rowid','_rowid_','oid')",[],|r|r.get(0)).map_err(|_|HumanError("unsupported_source"))?;
    if !ordinary || columns != 3 || shadows != 0 {
        return Err(HumanError("unsupported_source"));
    }
    Ok(())
}

#[cfg(windows)]
fn identity(file: &File) -> Result<String, HumanError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };
    let mut info = std::mem::MaybeUninit::<FILE_ID_INFO>::zeroed();
    // The buffer has the API's exact layout/size and remains valid for the call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(HumanError("source_unavailable"));
    }
    // A successful call initializes the complete FILE_ID_INFO structure.
    let info = unsafe { info.assume_init() };
    Ok(format!(
        "{}:{:02x?}",
        info.VolumeSerialNumber, info.FileId.Identifier
    ))
}
#[cfg(unix)]
fn identity(file: &File) -> Result<String, HumanError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|_| HumanError("source_unavailable"))?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HumanKind {
    Entertainment,
    NonEntertainment,
    Unknown,
}
impl HumanKind {
    fn from_sql(value: &str) -> Self {
        match value {
            "entertainment" => Self::Entertainment,
            "non_entertainment" => Self::NonEntertainment,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HumanError(pub(crate) &'static str);
impl fmt::Display for HumanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for HumanError {}
fn cache_read_error(_: rusqlite::Error) -> HumanError {
    HumanError("cache_read_failed")
}
fn cache_write_error(_: rusqlite::Error) -> HumanError {
    HumanError("cache_write_failed")
}
struct CachedObservation {
    id: String,
    start: i64,
    hash: Option<Vec<u8>>,
}
fn next_cached(rows: &mut rusqlite::Rows<'_>) -> Result<Option<CachedObservation>, HumanError> {
    rows.next()
        .map_err(cache_write_error)?
        .map(|row| {
            Ok(CachedObservation {
                id: row.get(0).map_err(cache_write_error)?,
                start: row.get(1).map_err(cache_write_error)?,
                hash: row.get(2).map_err(cache_write_error)?,
            })
        })
        .transpose()
}
fn source_key(conn: &Connection, label: &str) -> Result<rusqlite::types::Value, HumanError> {
    use rusqlite::types::Value;
    let key = if label.len() == 64 && label.bytes().all(|b| b.is_ascii_hexdigit()) {
        fn digit(b: u8) -> u8 {
            if b.is_ascii_digit() {
                b - b'0'
            } else {
                (b | 32) - b'a' + 10
            }
        }
        Value::Blob(
            label
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| digit(b[0]) * 16 + digit(b[1]))
                .collect(),
        )
    } else {
        Value::Text(label.to_owned())
    };
    Ok(conn.query_row("SELECT source_id FROM human_activity_sources WHERE source_id=?1 OR source_id=?2 ORDER BY typeof(source_id) LIMIT 1",params![&key,label],|r|r.get::<_,Value>(0)).optional().map_err(cache_read_error)?.unwrap_or(key))
}

struct PendingInterval {
    id: String,
    start: i64,
    candidate_end: i64,
    old_end: Option<i64>,
    kind: String,
}

fn queue_end(
    insert: &mut rusqlite::Statement<'_>,
    record: &PendingInterval,
    end: Option<i64>,
) -> Result<(), HumanError> {
    if record.old_end != end {
        insert
            .execute(params![record.id, end])
            .map_err(cache_write_error)?;
    }
    Ok(())
}

fn resolve_all(
    tx: &Connection,
    source: &rusqlite::types::Value,
    cancelled: &impl Fn() -> bool,
) -> Result<u64, HumanError> {
    tx.execute_batch(
        "CREATE TEMP TABLE human_ends(native_id TEXT PRIMARY KEY,effective_end INTEGER);",
    )
    .map_err(cache_write_error)?;
    let mut conflicts = 0;
    {
        let mut query = tx.prepare("SELECT native_id,captured_at,duration_ms,effective_end,kind FROM human_activity_records INDEXED BY human_activity_start WHERE source_id=? ORDER BY captured_at,native_id").map_err(cache_write_error)?;
        let mut rows = query.query([source]).map_err(cache_write_error)?;
        let mut insert = tx
            .prepare("INSERT INTO human_ends VALUES(?,?)")
            .map_err(cache_write_error)?;
        let mut pending: Option<PendingInterval> = None;
        let mut conflict = false;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(cache_write_error)? {
            count += 1;
            if count % 256 == 0 && cancelled() {
                return Err(HumanError("sync_cancelled"));
            }
            let start: i64 = row.get(1).map_err(cache_write_error)?;
            let duration: i64 = row.get(2).map_err(cache_write_error)?;
            let current = PendingInterval {
                id: row.get(0).map_err(cache_write_error)?,
                start,
                candidate_end: start
                    .checked_add(duration)
                    .ok_or(HumanError("cache_write_failed"))?,
                old_end: row.get(3).map_err(cache_write_error)?,
                kind: row.get(4).map_err(cache_write_error)?,
            };
            if let Some(previous) = pending.take() {
                if previous.start == current.start {
                    conflict |= previous.kind != current.kind;
                    queue_end(&mut insert, &previous, None)?;
                } else {
                    queue_end(
                        &mut insert,
                        &previous,
                        Some(previous.candidate_end.min(current.start)),
                    )?;
                    conflicts += u64::from(conflict);
                    conflict = false;
                }
            }
            pending = Some(current);
        }
        if let Some(last) = pending {
            queue_end(&mut insert, &last, Some(last.candidate_end))?;
            conflicts += u64::from(conflict);
        }
    }
    tx.execute("UPDATE human_activity_records SET effective_end=(SELECT effective_end FROM human_ends e WHERE e.native_id=human_activity_records.native_id) WHERE source_id=? AND native_id IN (SELECT native_id FROM human_ends)",[source]).map_err(cache_write_error)?;
    tx.execute_batch("DROP TABLE human_ends;")
        .map_err(cache_write_error)?;
    Ok(conflicts)
}

fn conflict_at(
    conn: &Connection,
    source: &rusqlite::types::Value,
    time: i64,
) -> Result<u64, HumanError> {
    let count: i64 = conn.query_row("SELECT count(DISTINCT kind) FROM human_activity_records WHERE source_id=? AND captured_at=?",params![source,time],|r|r.get(0)).map_err(cache_write_error)?;
    Ok(u64::from(count > 1))
}

fn resolve_times(
    tx: &Connection,
    source: &rusqlite::types::Value,
    times: &HashSet<i64>,
) -> Result<(), HumanError> {
    let mut affected = times.clone();
    for time in times {
        let previous: Option<i64> = tx.query_row("SELECT max(captured_at) FROM human_activity_records WHERE source_id=? AND captured_at<?",params![source,time],|r|r.get(0)).map_err(cache_write_error)?;
        if let Some(time) = previous {
            affected.insert(time);
        }
    }
    let mut update = tx.prepare("UPDATE human_activity_records SET effective_end=CASE WHEN native_id=(SELECT max(r.native_id) FROM human_activity_records r WHERE r.source_id=?1 AND r.captured_at=?2) THEN min(captured_at+duration_ms,coalesce((SELECT min(r.captured_at) FROM human_activity_records r WHERE r.source_id=?1 AND r.captured_at>?2),captured_at+duration_ms)) ELSE NULL END WHERE source_id=?1 AND captured_at=?2").map_err(cache_write_error)?;
    for time in affected {
        update
            .execute(params![source, time])
            .map_err(cache_write_error)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    InvalidJson,
    PayloadTooLarge,
    InvalidPayloadType,
    InvalidDurationType,
    InvalidDuration,
    InvalidTimestamp,
    InvalidId,
    LongEstimate,
    SimultaneousConflict,
}
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HumanDiagnostic {
    pub code: DiagnosticCode,
    pub count: u64,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HumanActivity {
    pub start: i64,
    pub end: i64,
    pub kind: HumanKind,
}
#[derive(Debug, Serialize)]
pub struct HumanActivityResponse {
    pub records: Vec<HumanActivity>,
    pub diagnostics: Vec<HumanDiagnostic>,
}

// The normalized staging database is private and discarded after cache commit.
pub struct HumanStage {
    _file: tempfile::NamedTempFile,
    source_id: String,
    source_path: String,
    file_identity: String,
    version: i64,
    diagnostics: Vec<HumanDiagnostic>,
    resolved_conflicts: Option<u64>,
}
impl HumanStage {
    pub fn data_version(&self) -> i64 {
        self.version
    }
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
}
struct StagedObservation {
    record: Observation,
    hash: [u8; 32],
}
fn candidate_end(row: &StagedObservation) -> Result<i64, HumanError> {
    row.record
        .captured_at
        .checked_add(row.record.duration_ms)
        .ok_or(HumanError("staging_failed"))
}
struct MinimizedAttachment<'a>(&'a Connection);
impl<'a> MinimizedAttachment<'a> {
    fn new(conn: &'a Connection, stage: &HumanStage) -> Result<Self, HumanError> {
        let path = stage
            ._file
            .path()
            .to_str()
            .ok_or(HumanError("staging_failed"))?
            .replace('\\', "/");
        let mut uri = String::from("file:");
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/:-._~".contains(&byte) {
                uri.push(char::from(byte));
            } else {
                use std::fmt::Write;
                write!(&mut uri, "%{byte:02X}").map_err(|_| HumanError("staging_failed"))?;
            }
        }
        uri.push_str("?mode=ro");
        conn.execute("ATTACH DATABASE ? AS human_stage", [uri])
            .map_err(|_| HumanError("staging_failed"))?;
        let guard = Self(conn);
        if !conn
            .is_readonly("human_stage")
            .map_err(|_| HumanError("staging_failed"))?
        {
            return Err(HumanError("staging_failed"));
        }
        Ok(guard)
    }
}
impl Drop for MinimizedAttachment<'_> {
    fn drop(&mut self) {
        let _ = self.0.execute_batch("DETACH DATABASE human_stage;");
    }
}

fn append_staged(
    insert: &mut rusqlite::Statement<'_>,
    row: &StagedObservation,
    end: Option<i64>,
) -> Result<(), HumanError> {
    let kind = match row.record.kind {
        HumanKind::Entertainment => "entertainment",
        HumanKind::NonEntertainment => "non_entertainment",
        HumanKind::Unknown => "unknown",
    };
    insert
        .execute(params![
            row.record.native_id,
            row.record.captured_at,
            row.record.duration_ms,
            kind,
            row.hash.as_slice(),
            end
        ])
        .map(|_| ())
        .map_err(|_| HumanError("staging_failed"))
}
fn diagnostics(counts: BTreeMap<DiagnosticCode, u64>) -> Vec<HumanDiagnostic> {
    counts
        .into_iter()
        .map(|(code, count)| HumanDiagnostic { code, count })
        .collect()
}

// Synchronization-only model: deliberately not Serialize or Debug.
pub struct Observation {
    pub native_id: String,
    pub captured_at: i64,
    pub duration_ms: i64,
    pub kind: HumanKind,
}
pub struct ObservationPage {
    pub cursor: Option<i64>,
    pub records: Vec<Observation>,
    pub diagnostics: Vec<HumanDiagnostic>,
}
pub struct HumanReader {
    conn: Connection,
    path: PathBuf,
    file_identity: String,
    source_id: String,
    _file: File,
}
impl HumanReader {
    pub fn open(path: &Path) -> Result<Self, HumanError> {
        // Preserve the selected logical path: resolving a junction/symlink once
        // would hide later changes to its target from identity checks.
        let path = std::path::absolute(path).map_err(|_| HumanError("source_unavailable"))?;
        let file = File::open(&path).map_err(|_| HumanError("source_unavailable"))?;
        let file_identity = identity(&file)?;
        let source_path = path.to_string_lossy().to_string();
        #[cfg(windows)]
        let source_path = source_path.to_lowercase();
        let source_id = crate::store::hash(format!("{source_path}\0{file_identity}"));
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| HumanError("source_unavailable"))?;
        conn.busy_timeout(Duration::from_millis(500))
            .map_err(|_| HumanError("source_unavailable"))?;
        conn.pragma_update(None, "query_only", true)
            .map_err(|_| HumanError("source_unavailable"))?;
        conn.pragma_update(None, "trusted_schema", false)
            .map_err(|_| HumanError("source_unavailable"))?;
        validate_source_schema(&conn)?;
        conn.prepare("SELECT rowid,id,captured_at,payload FROM activity_records LIMIT 0")
            .map_err(|_| HumanError("unsupported_source"))?;
        let reader = Self {
            conn,
            path,
            file_identity,
            source_id,
            _file: file,
        };
        reader.verify_identity()?;
        Ok(reader)
    }
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn data_version(&self) -> Result<i64, HumanError> {
        self.verify_identity()?;
        self.conn
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .map_err(|_| HumanError("source_read_failed"))
    }
    pub fn verify_identity(&self) -> Result<(), HumanError> {
        let file = File::open(&self.path).map_err(|_| HumanError("source_unavailable"))?;
        if identity(&file)? != self.file_identity {
            return Err(HumanError("source_replaced"));
        }
        Ok(())
    }
    pub fn stage(&self, cancelled: impl Fn() -> bool) -> Result<HumanStage, HumanError> {
        self.stage_to(None, cancelled)
    }
    pub fn stage_in(
        &self,
        directory: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<HumanStage, HumanError> {
        self.stage_to(Some(directory), cancelled)
    }
    fn stage_to(
        &self,
        directory: Option<&Path>,
        cancelled: impl Fn() -> bool,
    ) -> Result<HumanStage, HumanError> {
        if let Some(directory) = directory {
            validate_staging_directory(directory)?;
        }
        let began = Instant::now();
        let check = || {
            if cancelled() || began.elapsed() > Duration::from_secs(30) {
                Err(HumanError("sync_cancelled"))
            } else {
                Ok(())
            }
        };
        check()?;
        let version = self.data_version()?;
        let file = match directory {
            Some(directory) => tempfile::Builder::new()
                .prefix(&staging_prefix(&self.path)?)
                .suffix(".db")
                .tempfile_in(directory),
            None => tempfile::NamedTempFile::new(),
        }
        .map_err(|_| HumanError("staging_failed"))?;
        let conn = Connection::open(file.path()).map_err(|_| HumanError("staging_failed"))?;
        conn.execute_batch("PRAGMA cache_size=-2048; CREATE TABLE records(native_id TEXT,captured_at INTEGER,duration_ms INTEGER,kind TEXT,content_hash BLOB,effective_end INTEGER);").map_err(|_| HumanError("staging_failed"))?;
        let staging = conn
            .unchecked_transaction()
            .map_err(|_| HumanError("staging_failed"))?;
        let snapshot = self
            .conn
            .unchecked_transaction()
            .map_err(|_| HumanError("source_read_failed"))?;
        validate_source_schema(&snapshot)?;
        let mut counts = BTreeMap::new();
        let mut cursor = None;
        let mut pending: Option<StagedObservation> = None;
        let mut last_start = None;
        let mut ordered = true;
        let mut conflicts = 0;
        let mut group_conflict = false;
        let mut insert = staging
            .prepare("INSERT INTO records VALUES(?,?,?,?,?,?)")
            .map_err(|_| HumanError("staging_failed"))?;
        loop {
            check()?;
            let page = self.read_page_checked(cursor, &check)?;
            if page.cursor == cursor {
                break;
            }
            cursor = page.cursor;
            for diag in page.diagnostics {
                *counts.entry(diag.code).or_insert(0) += diag.count;
            }
            for record in page.records {
                check()?;
                let content = serde_json::to_vec(&(
                    &record.native_id,
                    record.captured_at,
                    record.duration_ms,
                    record.kind,
                ))
                .map_err(|_| HumanError("staging_failed"))?;
                let current = StagedObservation {
                    record,
                    hash: Sha256::digest(content).into(),
                };
                ordered &= last_start.is_none_or(|last| current.record.captured_at >= last);
                last_start = Some(current.record.captured_at);
                match pending.take() {
                    None => pending = Some(current),
                    Some(previous) if previous.record.captured_at == current.record.captured_at => {
                        group_conflict |= previous.record.kind != current.record.kind;
                        if current.record.native_id > previous.record.native_id {
                            append_staged(&mut insert, &previous, None)?;
                            pending = Some(current);
                        } else {
                            append_staged(&mut insert, &current, None)?;
                            pending = Some(previous);
                        }
                    }
                    Some(previous) => {
                        let end = if current.record.captured_at > previous.record.captured_at {
                            candidate_end(&previous)?.min(current.record.captured_at)
                        } else {
                            candidate_end(&previous)?
                        };
                        append_staged(&mut insert, &previous, Some(end))?;
                        conflicts += u64::from(group_conflict);
                        group_conflict = false;
                        pending = Some(current);
                    }
                }
            }
        }
        if let Some(last) = pending {
            append_staged(&mut insert, &last, Some(candidate_end(&last)?))?;
            conflicts += u64::from(group_conflict);
        }
        drop(insert);
        check()?;
        snapshot
            .commit()
            .map_err(|_| HumanError("source_read_failed"))?;
        self.verify_identity()?;
        staging.commit().map_err(|_| HumanError("staging_failed"))?;
        // Append during the source snapshot, then sort only minimized fields.
        // A covering index avoids random heap lookups for UUID-ordered merging.
        check()?;
        conn.execute_batch("CREATE INDEX records_native_order ON records(native_id,captured_at,duration_ms,kind,content_hash,effective_end);").map_err(|_| HumanError("staging_failed"))?;
        check()?;
        self.verify_identity()?;
        let source_path = self.path.to_string_lossy().to_string();
        #[cfg(windows)]
        let source_path = source_path.to_lowercase();
        Ok(HumanStage {
            _file: file,
            source_id: self.source_id.clone(),
            source_path,
            file_identity: self.file_identity.clone(),
            version,
            diagnostics: diagnostics(counts),
            resolved_conflicts: ordered.then_some(conflicts),
        })
    }
    pub fn read_page(&self, after: Option<i64>) -> Result<ObservationPage, HumanError> {
        self.read_page_checked(after, || Ok(()))
    }
    fn read_page_checked(
        &self,
        after: Option<i64>,
        check: impl Fn() -> Result<(), HumanError>,
    ) -> Result<ObservationPage, HumanError> {
        check()?;
        self.verify_identity()?;
        let lower = match after {
            Some(i64::MAX) => {
                return Ok(ObservationPage {
                    cursor: after,
                    records: vec![],
                    diagnostics: vec![],
                })
            }
            Some(value) => value + 1,
            None => i64::MIN,
        };
        let mut interrupted = None;
        let mut read = || -> rusqlite::Result<ObservationPage> {
            let mut stmt = self.conn.prepare_cached("SELECT rowid,
                CASE WHEN typeof(id)='text' AND octet_length(id) BETWEEN 1 AND 256 THEN id END,
                CASE WHEN typeof(captured_at)='integer' THEN captured_at END,
                CASE WHEN status='ok' THEN json_extract(payload,'$.durationEstimate') END,
                CASE WHEN status='ok' THEN CASE json_extract(payload,'$.detection.diagnostics.entertainmentDisposition')
                WHEN 'ENTERTAINMENT' THEN 'entertainment' WHEN 'NON_ENTERTAINMENT' THEN 'non_entertainment'
                ELSE 'unknown' END ELSE 'unknown' END,status FROM (
                SELECT rowid,id,captured_at,payload,CASE
                WHEN typeof(payload)<>'text' THEN 'invalid_payload_type'
                WHEN octet_length(payload)>1048576 THEN 'payload_too_large'
                WHEN NOT json_valid(payload) THEN 'invalid_json'
                WHEN json_type(payload,'$.durationEstimate') IS NOT 'integer' THEN 'invalid_duration_type'
                WHEN typeof(json_extract(payload,'$.durationEstimate'))<>'integer' THEN 'invalid_duration_type'
                ELSE 'ok' END AS status FROM activity_records WHERE rowid>=? ORDER BY rowid LIMIT 256)")?;
            let mut rows = stmt.query([lower])?;
            let mut page = ObservationPage {
                cursor: after,
                records: vec![],
                diagnostics: vec![],
            };
            let mut counts = BTreeMap::new();
            loop {
                if let Err(error) = check() {
                    interrupted = Some(error);
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let Some(row) = rows.next()? else { break };
                page.cursor = Some(row.get(0)?);
                let status: String = row.get(5)?;
                let code = match status.as_str() {
                    "ok" => None,
                    "invalid_json" => Some(DiagnosticCode::InvalidJson),
                    "payload_too_large" => Some(DiagnosticCode::PayloadTooLarge),
                    "invalid_payload_type" => Some(DiagnosticCode::InvalidPayloadType),
                    _ => Some(DiagnosticCode::InvalidDurationType),
                };
                if let Some(code) = code {
                    *counts.entry(code).or_insert(0) += 1;
                    continue;
                }
                let Some(native_id) = row.get::<_, Option<String>>(1).ok().flatten() else {
                    *counts.entry(DiagnosticCode::InvalidId).or_insert(0) += 1;
                    continue;
                };
                let Some(captured_at) = row.get::<_, Option<i64>>(2)? else {
                    *counts.entry(DiagnosticCode::InvalidTimestamp).or_insert(0) += 1;
                    continue;
                };
                if !(MIN_TIME..=MAX_TIME).contains(&captured_at) {
                    *counts.entry(DiagnosticCode::InvalidTimestamp).or_insert(0) += 1;
                    continue;
                }
                let seconds: i64 = row.get(3)?;
                let Some(duration_ms) = seconds.checked_mul(1000).filter(|v| *v >= 0) else {
                    *counts.entry(DiagnosticCode::InvalidDuration).or_insert(0) += 1;
                    continue;
                };
                if captured_at
                    .checked_add(duration_ms)
                    .is_none_or(|end| end > MAX_TIME)
                {
                    *counts.entry(DiagnosticCode::InvalidTimestamp).or_insert(0) += 1;
                    continue;
                }
                if duration_ms > 86_400_000 {
                    *counts.entry(DiagnosticCode::LongEstimate).or_insert(0) += 1;
                }
                page.records.push(Observation {
                    native_id,
                    captured_at,
                    duration_ms,
                    kind: HumanKind::from_sql(&row.get::<_, String>(4)?),
                });
            }
            page.diagnostics = diagnostics(counts);
            Ok(page)
        };
        let result = read();
        if let Some(error) = interrupted {
            return Err(error);
        }
        let result = result.map_err(|_| HumanError("source_read_failed"))?;
        self.verify_identity()?;
        Ok(result)
    }
}

impl crate::store::Store {
    pub fn human_summary(
        &self,
        source: &str,
        from: i64,
        to: i64,
        timezone: &str,
        visible: &[HumanKind],
    ) -> Result<crate::human_summary::HumanSummaryResponse, HumanError> {
        let mut summary = crate::human_summary::Accumulator::new(from, to, timezone, visible)?;
        let diagnostics =
            self.visit_human_source(source, from, to, false, |_id, _hash, record| {
                summary.add(record)
            })?;
        Ok(crate::human_summary::HumanSummaryResponse {
            days: summary.finish(),
            diagnostics,
        })
    }
    /// Display grouping is intentionally restricted to one selected source.
    pub fn human_activity_display(
        &self,
        source: &str,
        from: i64,
        to: i64,
        timezone: &str,
        grouped: bool,
    ) -> Result<HumanActivityResponse, HumanError> {
        if !grouped {
            return self.human_activity(&[source.to_owned()], from, to);
        }
        let tz: chrono_tz::Tz = timezone
            .parse()
            .map_err(|_| HumanError("invalid_timezone"))?;
        validate_period(from, to)?;
        let mut out: Vec<HumanActivity> = vec![];
        let mut last_date = None;
        let diagnostics =
            self.visit_human_source(source, from, to, false, |_id, _hash, record| {
                let mut start = record.start.max(from);
                let end = record.end.min(to);
                loop {
                    let local = chrono::DateTime::from_timestamp_millis(start)
                        .ok_or(HumanError("cache_read_failed"))?
                        .with_timezone(&tz);
                    let date = local.date_naive();
                    let boundary = crate::activity::next_day_boundary(&local)
                        .map_err(|_| HumanError("cache_read_failed"))?;
                    let stop = end.min(boundary);
                    if let Some(previous) = out.last_mut().filter(|previous| {
                        last_date == Some(date)
                            && previous.kind == record.kind
                            && previous.end == start
                            && previous.start < previous.end
                            && start < stop
                    }) {
                        previous.end = stop;
                    } else {
                        if out.len() == 50_000 {
                            return Err(HumanError("period_too_dense"));
                        }
                        out.push(HumanActivity {
                            start,
                            end: stop,
                            kind: record.kind,
                        });
                    }
                    last_date = Some(date);
                    if stop >= end {
                        break;
                    }
                    start = stop;
                }
                Ok(())
            })?;
        Ok(HumanActivityResponse {
            records: out,
            diagnostics,
        })
    }
    fn try_empty_human_import(
        &mut self,
        stage: &HumanStage,
        key: &rusqlite::types::Value,
        cancelled: &impl Fn() -> bool,
    ) -> Result<bool, HumanError> {
        let Some(conflicts) = stage.resolved_conflicts else {
            return Ok(false);
        };
        if self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM human_activity_records LIMIT 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(cache_read_error)?
        {
            return Ok(false);
        }
        let _attachment = MinimizedAttachment::new(&self.conn, stage)?;
        let tx = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(cache_write_error)?;
        // Recheck under the write lock before bypassing the retained-history merge.
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM human_activity_records LIMIT 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(cache_read_error)?
        {
            return Ok(false);
        }
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        tx.execute_batch(
            "DROP INDEX IF EXISTS human_activity_start; DROP INDEX IF EXISTS human_activity_end;",
        )
        .map_err(cache_write_error)?;
        let mut counts = stage
            .diagnostics
            .iter()
            .map(|d| (d.code, d.count))
            .collect::<BTreeMap<_, _>>();
        if conflicts > 0 {
            counts.insert(DiagnosticCode::SimultaneousConflict, conflicts);
        }
        let encoded = serde_json::to_string(&diagnostics(counts))
            .map_err(|_| HumanError("cache_write_failed"))?;
        tx.execute("INSERT INTO human_activity_sources VALUES(?,?,?,?,?,'synced',?) ON CONFLICT(source_id) DO UPDATE SET reader_version=excluded.reader_version,last_sync=excluded.last_sync,sync_status=excluded.sync_status,diagnostics=excluded.diagnostics",params![key,stage.source_path,stage.file_identity,1,chrono::Utc::now().timestamp_millis(),encoded]).map_err(cache_write_error)?;
        tx.execute("INSERT INTO human_activity_records SELECT ?,native_id,captured_at,duration_ms,kind,content_hash,effective_end FROM human_stage.records ORDER BY native_id",[key]).map_err(cache_write_error)?;
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        tx.execute_batch(crate::migrations::HUMAN_START_INDEX_SQL)
            .map_err(cache_write_error)?;
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        tx.commit().map_err(cache_write_error)?;
        Ok(true)
    }
    fn prepare_human_changes(
        &self,
        stage: &HumanStage,
        key: &rusqlite::types::Value,
        cancelled: &impl Fn() -> bool,
    ) -> Result<usize, HumanError> {
        self.conn.execute_batch("PRAGMA temp_store=FILE; DROP TABLE IF EXISTS temp.human_changes;
            CREATE TEMP TABLE human_changes(native_id TEXT PRIMARY KEY,old_start INTEGER,captured_at INTEGER,duration_ms INTEGER,kind TEXT,content_hash BLOB) WITHOUT ROWID;").map_err(cache_write_error)?;
        // Merge two ordered streams of allowed fields. Neither the original
        // database nor a staging database is attached to the application cache.
        let staged =
            Connection::open_with_flags(stage._file.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| HumanError("staging_failed"))?;
        let mut query = staged.prepare("SELECT native_id,captured_at,duration_ms,kind,content_hash FROM records ORDER BY native_id").map_err(|_|HumanError("staging_failed"))?;
        let mut rows = query.query([]).map_err(|_| HumanError("staging_failed"))?;
        let changes = self
            .conn
            .unchecked_transaction()
            .map_err(cache_write_error)?;
        let mut cached_query = changes.prepare("SELECT native_id,captured_at,CASE WHEN typeof(content_hash)='blob' AND length(content_hash)=32 THEN content_hash WHEN typeof(content_hash)='text' AND octet_length(content_hash)=64 THEN unhex(content_hash) END FROM human_activity_records WHERE source_id=? ORDER BY native_id").map_err(cache_write_error)?;
        let mut cached_rows = cached_query.query([key]).map_err(cache_write_error)?;
        let mut old = next_cached(&mut cached_rows)?;
        let mut insert = changes
            .prepare("INSERT INTO human_changes VALUES(?,?,?,?,?,?)")
            .map_err(cache_write_error)?;
        let mut changed = 0;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(|_| HumanError("staging_failed"))? {
            count += 1;
            if count % 256 == 0 && cancelled() {
                return Err(HumanError("sync_cancelled"));
            }
            let id: String = row.get(0).map_err(|_| HumanError("staging_failed"))?;
            while old.as_ref().is_some_and(|r| r.id.as_str() < id.as_str()) {
                old = next_cached(&mut cached_rows)?;
            }
            let prior = old.as_ref().filter(|r| r.id == id);
            let hash: Vec<u8> = row.get(4).map_err(|_| HumanError("staging_failed"))?;
            if prior.is_some_and(|r| r.hash.as_deref() == Some(hash.as_slice())) {
                continue;
            }
            let start: i64 = row.get(1).map_err(|_| HumanError("staging_failed"))?;
            let duration: i64 = row.get(2).map_err(|_| HumanError("staging_failed"))?;
            let kind: String = row.get(3).map_err(|_| HumanError("staging_failed"))?;
            insert
                .execute(params![
                    id,
                    prior.map(|r| r.start),
                    start,
                    duration,
                    kind,
                    hash
                ])
                .map_err(cache_write_error)?;
            changed += 1;
        }
        drop(insert);
        drop(cached_rows);
        drop(cached_query);
        changes.commit().map_err(cache_write_error)?;
        Ok(changed)
    }
    pub fn apply_human_stage(
        &mut self,
        stage: &HumanStage,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), HumanError> {
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        // Bound the worker's SQLite page cache while avoiding UUID-keyed
        // bulk imports repeatedly reading pages evicted by SQLite's 2 MiB default.
        self.conn
            .execute_batch("PRAGMA cache_size=-32768;")
            .map_err(cache_write_error)?;
        let key = source_key(&self.conn, &stage.source_id)?;
        if self.try_empty_human_import(stage, &key, &cancelled)? {
            return Ok(());
        }
        let changed = self.prepare_human_changes(stage, &key, &cancelled)?;
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        let mut write = || -> Result<(), HumanError> {
            let tx = self
                .conn
                .transaction()
                .map_err(|_| HumanError("cache_write_failed"))?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT diagnostics FROM human_activity_sources WHERE source_id=?",
                    [&key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(cache_write_error)?;
            let prior_diags: Vec<HumanDiagnostic> = previous
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| HumanError("cache_write_failed"))?
                .unwrap_or_default();
            let mut conflicts = prior_diags
                .iter()
                .find(|d| d.code == DiagnosticCode::SimultaneousConflict)
                .map_or(0, |d| d.count);
            let mut times = HashSet::new();
            if changed <= 4096 {
                let mut query = tx
                    .prepare("SELECT old_start,captured_at FROM human_changes")
                    .map_err(cache_write_error)?;
                let mut rows = query.query([]).map_err(cache_write_error)?;
                while let Some(row) = rows.next().map_err(cache_write_error)? {
                    if let Some(time) = row.get::<_, Option<i64>>(0).map_err(cache_write_error)? {
                        times.insert(time);
                    }
                    times.insert(row.get::<_, i64>(1).map_err(cache_write_error)?);
                }
                for time in &times {
                    conflicts = conflicts.saturating_sub(conflict_at(&tx, &key, *time)?);
                }
            }
            tx.execute("INSERT INTO human_activity_sources VALUES(?,?,?,?,?,'synced','[]') ON CONFLICT(source_id) DO UPDATE SET reader_version=excluded.reader_version,last_sync=excluded.last_sync,sync_status=excluded.sync_status",params![&key,stage.source_path,stage.file_identity,1,chrono::Utc::now().timestamp_millis()]).map_err(cache_write_error)?;
            let build_indexes = changed > 4096
                && !tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM human_activity_records LIMIT 1)",
                        [],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(cache_write_error)?;
            if build_indexes {
                tx.execute_batch(
                    "DROP INDEX human_activity_start; DROP INDEX IF EXISTS human_activity_end;",
                )
                .map_err(cache_write_error)?;
            }
            tx.execute("INSERT INTO human_activity_records SELECT ?,native_id,captured_at,duration_ms,kind,content_hash,captured_at+duration_ms FROM human_changes WHERE true ON CONFLICT(source_id,native_id) DO UPDATE SET captured_at=excluded.captured_at,duration_ms=excluded.duration_ms,kind=excluded.kind,content_hash=excluded.content_hash,effective_end=excluded.effective_end WHERE content_hash<>excluded.content_hash",[&key]).map_err(cache_write_error)?;
            if build_indexes {
                tx.execute_batch(crate::migrations::HUMAN_START_INDEX_SQL)
                    .map_err(cache_write_error)?;
            }
            if changed > 4096 {
                conflicts = resolve_all(&tx, &key, &cancelled)?;
            } else if changed > 0 {
                resolve_times(&tx, &key, &times)?;
                for time in &times {
                    conflicts += conflict_at(&tx, &key, *time)?;
                }
            }
            let mut counts = stage
                .diagnostics
                .iter()
                .map(|d| (d.code, d.count))
                .collect::<BTreeMap<_, _>>();
            if conflicts > 0 {
                counts.insert(DiagnosticCode::SimultaneousConflict, conflicts);
            }
            tx.execute(
                "UPDATE human_activity_sources SET diagnostics=? WHERE source_id=?",
                params![
                    serde_json::to_string(&diagnostics(counts))
                        .map_err(|_| HumanError("cache_write_failed"))?,
                    &key
                ],
            )
            .map_err(cache_write_error)?;
            if cancelled() {
                return Err(HumanError("sync_cancelled"));
            }
            tx.commit().map_err(|_| HumanError("cache_write_failed"))
        };
        let result = write();
        // This is disposable normalized data, not part of committed cache state.
        let _ = self
            .conn
            .execute_batch("DROP TABLE IF EXISTS temp.human_changes;");
        result
    }
    pub fn human_activity(
        &self,
        sources: &[String],
        from: i64,
        to: i64,
    ) -> Result<HumanActivityResponse, HumanError> {
        validate_period(from, to)?;
        let mut records = vec![];
        let mut seen = HashSet::new();
        let mut counts = BTreeMap::new();
        for source in sources {
            let ds = self.visit_human_source(source, from, to, true, |id, hash, record| {
                if seen.insert((id, hash, record.start, record.end, record.kind)) {
                    if records.len() == 50_000 {
                        return Err(HumanError("period_too_dense"));
                    }
                    records.push(record);
                }
                Ok(())
            })?;
            for d in ds {
                *counts.entry(d.code).or_insert(0) += d.count;
            }
        }
        records.sort_by_key(|r| (r.start, r.end, r.kind));
        Ok(HumanActivityResponse {
            records,
            diagnostics: diagnostics(counts),
        })
    }
    fn visit_human_source(
        &self,
        source: &str,
        from: i64,
        to: i64,
        identity: bool,
        mut visit: impl FnMut(String, String, HumanActivity) -> Result<(), HumanError>,
    ) -> Result<Vec<HumanDiagnostic>, HumanError> {
        let key = source_key(&self.conn, source)?;
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT diagnostics FROM human_activity_sources WHERE source_id=?",
                [&key],
                |r| r.get(0),
            )
            .optional()
            .map_err(cache_read_error)?;
        let Some(raw) = raw else {
            return Ok(vec![]);
        };
        let diagnostics =
            serde_json::from_str(&raw).map_err(|_| HumanError("cache_read_failed"))?;
        // Representatives are disjoint per source. Seek the preceding one and
        // this period, then stream minimized cache rows to a bounded consumer.
        let identity_columns = if identity {
            "native_id,CASE WHEN typeof(content_hash)='blob' THEN hex(content_hash) ELSE upper(content_hash) END"
        } else {
            "'',''"
        };
        let sql=format!("SELECT {identity_columns},captured_at,effective_end,kind FROM human_activity_records WHERE source_id=?1 AND captured_at>=coalesce((SELECT max(p.captured_at) FROM human_activity_records p INDEXED BY human_activity_start WHERE p.source_id=?1 AND p.captured_at<?3 AND p.effective_end IS NOT NULL),?3) AND captured_at<?2 AND (effective_end>?3 OR (effective_end=captured_at AND captured_at>=?4)) ORDER BY captured_at,native_id");
        let mut query = self.conn.prepare(&sql).map_err(cache_read_error)?;
        let mut rows = query
            .query(params![&key, to, from, from])
            .map_err(cache_read_error)?;
        while let Some(row) = rows.next().map_err(cache_read_error)? {
            let id = row.get(0).map_err(cache_read_error)?;
            let hash = row.get(1).map_err(cache_read_error)?;
            let start = row.get(2).map_err(cache_read_error)?;
            let end = row.get(3).map_err(cache_read_error)?;
            let kind = HumanKind::from_sql(&row.get::<_, String>(4).map_err(cache_read_error)?);
            visit(id, hash, HumanActivity { start, end, kind })?;
        }
        Ok(diagnostics)
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    #[test]
    fn only_minimized_staging_is_attached_read_only_and_detached_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("source.db");
        let source = Connection::open(&path).unwrap();
        source.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT); INSERT INTO activity_records VALUES('fixture',1000,'{\"durationEstimate\":1}');").unwrap();
        let stage = HumanReader::open(&path).unwrap().stage(|| false).unwrap();
        let cache = Connection::open_in_memory().unwrap();
        {
            let _attachment = MinimizedAttachment::new(&cache, &stage).unwrap();
            assert!(cache.is_readonly("human_stage").unwrap());
            assert!(cache
                .execute("DELETE FROM human_stage.records", [])
                .is_err());
            assert_eq!(
                cache
                    .query_row("SELECT count(*) FROM human_stage.records", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert!(cache
                .prepare("SELECT payload FROM human_stage.records")
                .is_err());
        }
        assert!(cache.prepare("SELECT * FROM human_stage.records").is_err());
    }

    #[test]
    fn a_page_can_be_cancelled_before_loading_all_large_payload_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("synthetic.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);").unwrap();
        let payload =
            serde_json::json!({"durationEstimate":1,"foreground":{"title":"x".repeat(1_000_000)}})
                .to_string();
        for i in 0..8 {
            writer
                .execute(
                    "INSERT INTO activity_records VALUES(?,?,?)",
                    params![format!("synthetic-{i}"), 1000 + i, payload],
                )
                .unwrap();
        }
        let reader = HumanReader::open(&path).unwrap();
        let checks = std::cell::Cell::new(0);
        let error = reader
            .read_page_checked(None, || {
                checks.set(checks.get() + 1);
                if checks.get() >= 3 {
                    Err(HumanError("sync_cancelled"))
                } else {
                    Ok(())
                }
            })
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "sync_cancelled");
        assert_eq!(checks.get(), 3);
    }

    #[test]
    fn private_staging_file_never_contains_original_payload_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("synthetic.db");
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("CREATE TABLE activity_records(id TEXT PRIMARY KEY,captured_at INTEGER,payload TEXT);
            INSERT INTO activity_records VALUES('allowed-id',1000,'{\"durationEstimate\":1,\"foreground\":{\"title\":\"PRIVATE_STAGING_MARKER\"},\"detection\":{\"diagnostics\":{\"entertainmentDisposition\":\"PRIVATE_STAGING_MARKER\"}}}');").unwrap();
        let stage = HumanReader::open(&path).unwrap().stage(|| false).unwrap();
        let bytes = std::fs::read(stage._file.path()).unwrap();
        assert!(!bytes
            .windows(b"PRIVATE_STAGING_MARKER".len())
            .any(|w| w == b"PRIVATE_STAGING_MARKER"));
        assert!(!bytes
            .windows(b"foreground".len())
            .any(|w| w == b"foreground"));
        assert_eq!(
            Connection::open(stage._file.path())
                .unwrap()
                .query_row("SELECT kind FROM records", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "unknown"
        );
    }
}
