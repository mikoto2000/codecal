//! Minimized Rei observation reader. Original payloads never cross SQLite's boundary.
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MIN_TIME: i64 = -62_135_596_800_000;
const MAX_TIME: i64 = 253_402_300_799_999;

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
pub struct HumanError(&'static str);
impl fmt::Display for HumanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for HumanError {}

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
    conn: Connection,
    _file: tempfile::NamedTempFile,
    source_id: String,
    source_path: String,
    file_identity: String,
    version: i64,
    diagnostics: Vec<HumanDiagnostic>,
}
impl HumanStage {
    pub fn data_version(&self) -> i64 {
        self.version
    }
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
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
    pub cursor: i64,
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
        let path = path
            .canonicalize()
            .map_err(|_| HumanError("source_unavailable"))?;
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
        let file = tempfile::NamedTempFile::new().map_err(|_| HumanError("staging_failed"))?;
        let conn = Connection::open(file.path()).map_err(|_| HumanError("staging_failed"))?;
        conn.execute_batch("PRAGMA cache_size=-2048; CREATE TABLE records(native_id TEXT PRIMARY KEY,captured_at INTEGER,duration_ms INTEGER,kind TEXT,content_hash TEXT);").map_err(|_| HumanError("staging_failed"))?;
        let staging = conn
            .unchecked_transaction()
            .map_err(|_| HumanError("staging_failed"))?;
        let snapshot = self
            .conn
            .unchecked_transaction()
            .map_err(|_| HumanError("source_read_failed"))?;
        let mut counts = BTreeMap::new();
        let mut cursor = 0;
        loop {
            check()?;
            let page = self.read_page(cursor)?;
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
                let kind = match record.kind {
                    HumanKind::Entertainment => "entertainment",
                    HumanKind::NonEntertainment => "non_entertainment",
                    HumanKind::Unknown => "unknown",
                };
                staging
                    .execute(
                        "INSERT INTO records VALUES(?,?,?,?,?)",
                        params![
                            record.native_id,
                            record.captured_at,
                            record.duration_ms,
                            kind,
                            crate::store::hash(content)
                        ],
                    )
                    .map_err(|_| HumanError("staging_failed"))?;
            }
        }
        check()?;
        snapshot
            .commit()
            .map_err(|_| HumanError("source_read_failed"))?;
        self.verify_identity()?;
        staging.commit().map_err(|_| HumanError("staging_failed"))?;
        let source_path = self.path.to_string_lossy().to_string();
        #[cfg(windows)]
        let source_path = source_path.to_lowercase();
        Ok(HumanStage {
            conn,
            _file: file,
            source_id: self.source_id.clone(),
            source_path,
            file_identity: self.file_identity.clone(),
            version,
            diagnostics: diagnostics(counts),
        })
    }
    pub fn read_page(&self, after: i64) -> Result<ObservationPage, HumanError> {
        self.verify_identity()?;
        let read = || -> rusqlite::Result<ObservationPage> {
            let mut stmt = self.conn.prepare("SELECT rowid,
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
                ELSE 'ok' END AS status FROM activity_records WHERE rowid>? ORDER BY rowid LIMIT 256)")?;
            let mut rows = stmt.query([after])?;
            let mut page = ObservationPage {
                cursor: after,
                records: vec![],
                diagnostics: vec![],
            };
            let mut counts = BTreeMap::new();
            while let Some(row) = rows.next()? {
                page.cursor = row.get(0)?;
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
        let result = read().map_err(|_| HumanError("source_read_failed"))?;
        self.verify_identity()?;
        Ok(result)
    }
}

impl crate::store::Store {
    pub fn apply_human_stage(
        &mut self,
        stage: &HumanStage,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), HumanError> {
        if cancelled() {
            return Err(HumanError("sync_cancelled"));
        }
        let mut write = || -> Result<(), HumanError> {
            let tx = self
                .conn
                .transaction()
                .map_err(|_| HumanError("cache_write_failed"))?;
            tx.execute("INSERT INTO human_activity_sources VALUES(?,?,?,?,?,'synced',?) ON CONFLICT(source_id) DO UPDATE SET reader_version=excluded.reader_version,last_sync=excluded.last_sync,sync_status=excluded.sync_status,diagnostics=excluded.diagnostics", params![stage.source_id, stage.source_path, stage.file_identity, 1, chrono::Utc::now().timestamp_millis(), serde_json::to_string(&stage.diagnostics).map_err(|_| HumanError("cache_write_failed"))?]).map_err(|_| HumanError("cache_write_failed"))?;
            {
                let mut query = stage
                    .conn
                    .prepare(
                        "SELECT native_id,captured_at,duration_ms,kind,content_hash FROM records",
                    )
                    .map_err(|_| HumanError("staging_failed"))?;
                let mut rows = query.query([]).map_err(|_| HumanError("staging_failed"))?;
                let mut insert = tx.prepare("INSERT INTO human_activity_records VALUES(?,?,?,?,?,?,NULL) ON CONFLICT(source_id,native_id) DO UPDATE SET captured_at=excluded.captured_at,duration_ms=excluded.duration_ms,kind=excluded.kind,content_hash=excluded.content_hash WHERE content_hash<>excluded.content_hash").map_err(|_| HumanError("cache_write_failed"))?;
                while let Some(row) = rows.next().map_err(|_| HumanError("staging_failed"))? {
                    if cancelled() {
                        return Err(HumanError("sync_cancelled"));
                    }
                    let id: String = row.get(0).map_err(|_| HumanError("staging_failed"))?;
                    let start: i64 = row.get(1).map_err(|_| HumanError("staging_failed"))?;
                    let duration: i64 = row.get(2).map_err(|_| HumanError("staging_failed"))?;
                    let kind: String = row.get(3).map_err(|_| HumanError("staging_failed"))?;
                    let hash: String = row.get(4).map_err(|_| HumanError("staging_failed"))?;
                    insert
                        .execute(params![stage.source_id, id, start, duration, kind, hash])
                        .map_err(|_| HumanError("cache_write_failed"))?;
                }
            }
            tx.execute_batch("CREATE TEMP TABLE human_resolved(native_id TEXT PRIMARY KEY,effective_end INTEGER);").map_err(|_| HumanError("cache_write_failed"))?;
            tx.execute("INSERT INTO human_resolved WITH ranked AS (
                SELECT native_id,captured_at,duration_ms,row_number() OVER(PARTITION BY captured_at ORDER BY native_id DESC) AS rank FROM human_activity_records WHERE source_id=?),
                representatives AS (SELECT native_id,captured_at,duration_ms,lead(captured_at) OVER(ORDER BY captured_at) AS next_start FROM ranked WHERE rank=1)
                SELECT native_id,min(captured_at+duration_ms,coalesce(next_start,captured_at+duration_ms)) FROM representatives", [&stage.source_id]).map_err(|_| HumanError("cache_write_failed"))?;
            tx.execute("UPDATE human_activity_records SET effective_end=(SELECT effective_end FROM human_resolved r WHERE r.native_id=human_activity_records.native_id) WHERE source_id=? AND effective_end IS NOT (SELECT effective_end FROM human_resolved r WHERE r.native_id=human_activity_records.native_id)", [&stage.source_id]).map_err(|_| HumanError("cache_write_failed"))?;
            tx.execute_batch("DROP TABLE human_resolved;")
                .map_err(|_| HumanError("cache_write_failed"))?;
            let conflicts: i64 = tx.query_row("SELECT count(*) FROM (SELECT captured_at FROM human_activity_records WHERE source_id=? GROUP BY captured_at HAVING count(DISTINCT kind)>1)", [&stage.source_id], |r| r.get(0)).map_err(|_| HumanError("cache_write_failed"))?;
            if conflicts > 0 {
                let mut counts = stage
                    .diagnostics
                    .iter()
                    .map(|d| (d.code, d.count))
                    .collect::<BTreeMap<_, _>>();
                counts.insert(DiagnosticCode::SimultaneousConflict, conflicts as u64);
                tx.execute(
                    "UPDATE human_activity_sources SET diagnostics=? WHERE source_id=?",
                    params![
                        serde_json::to_string(&diagnostics(counts))
                            .map_err(|_| HumanError("cache_write_failed"))?,
                        stage.source_id
                    ],
                )
                .map_err(|_| HumanError("cache_write_failed"))?;
            }
            if cancelled() {
                return Err(HumanError("sync_cancelled"));
            }
            tx.commit().map_err(|_| HumanError("cache_write_failed"))
        };
        write()
    }
    pub fn human_activity(
        &self,
        sources: &[String],
        from: i64,
        to: i64,
    ) -> Result<HumanActivityResponse, HumanError> {
        if from >= to
            || from < MIN_TIME
            || to > MAX_TIME
            || to.checked_sub(from).is_none_or(|v| v > 62 * 86_400_000)
        {
            return Err(HumanError("invalid_period"));
        }
        let read = || -> rusqlite::Result<HumanActivityResponse> {
            let mut records = vec![];
            let mut seen = HashSet::new();
            let mut counts = BTreeMap::new();
            for source in sources {
                let raw: String = self.conn.query_row(
                    "SELECT diagnostics FROM human_activity_sources WHERE source_id=?",
                    [source],
                    |r| r.get(0),
                )?;
                let ds: Vec<HumanDiagnostic> = serde_json::from_str(&raw).unwrap_or_default();
                for d in ds {
                    *counts.entry(d.code).or_insert(0) += d.count;
                }
                let mut query = self.conn.prepare("SELECT native_id,content_hash,captured_at,effective_end,kind FROM human_activity_records WHERE source_id=? AND captured_at<? AND (effective_end>? OR (effective_end=captured_at AND captured_at>=?)) ORDER BY captured_at,native_id")?;
                let mut rows = query.query(params![source, to, from, from])?;
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    let hash: String = row.get(1)?;
                    let start: i64 = row.get(2)?;
                    let end: i64 = row.get(3)?;
                    let label: String = row.get(4)?;
                    if seen.insert((id, hash, start, end, label.clone())) {
                        records.push(HumanActivity {
                            start,
                            end,
                            kind: HumanKind::from_sql(&label),
                        });
                    }
                }
            }
            records.sort_by_key(|r| (r.start, r.end, r.kind));
            Ok(HumanActivityResponse {
                records,
                diagnostics: diagnostics(counts),
            })
        };
        read().map_err(|_| HumanError("cache_read_failed"))
    }
}
