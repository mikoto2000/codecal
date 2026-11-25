//! Minimized Rei observation reader. Original payloads never cross SQLite's boundary.
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, path::Path, time::Duration};

const MIN_TIME: i64 = -62_135_596_800_000;
const MAX_TIME: i64 = 253_402_300_799_999;

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
}
impl HumanReader {
    pub fn open(path: &Path) -> Result<Self, HumanError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| HumanError("source_unavailable"))?;
        conn.busy_timeout(Duration::from_millis(500))
            .map_err(|_| HumanError("source_unavailable"))?;
        conn.pragma_update(None, "query_only", true)
            .map_err(|_| HumanError("source_unavailable"))?;
        Ok(Self { conn })
    }
    pub fn read_page(&self, after: i64) -> Result<ObservationPage, HumanError> {
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
        read().map_err(|_| HumanError("source_read_failed"))
    }
}
