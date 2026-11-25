//! Minimized Rei observation reader. Original payloads never cross SQLite's boundary.
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
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
