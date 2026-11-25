use rusqlite::{Connection, OpenFlags, MAIN_DB};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Sanitize only a newly created CodeCal copy, never a Rei/source database.
pub(crate) fn exclude_human_cache(copy: &Path) -> Result<(), String> {
    let sanitize = || -> rusqlite::Result<()> {
        let mut conn = Connection::open(copy)?;
        let tables = ["human_activity_records", "human_activity_sources"];
        let present = tables
            .iter()
            .map(|table| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
                    [table],
                    |r| r.get::<_, bool>(0),
                )
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !present.iter().any(|v| *v) {
            return Ok(());
        }
        conn.pragma_update(None, "secure_delete", true)?;
        let tx = conn.transaction()?;
        for (table, exists) in tables.iter().zip(present) {
            if exists {
                tx.execute(&format!("DELETE FROM {table}"), [])?;
            }
        }
        tx.commit()?;
        // Rebuild this private copy so freed pages do not carry cached records.
        conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    };
    sanitize().map_err(|_| "CodeCalのコピーから人間キャッシュを除外できません".into())
}
/// Local machine data takes priority. Keep any former roaming database intact.
pub fn prepare_database(local: &Path, previous: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(local).map_err(|e| e.to_string())?;
    let target = local.join("codecal.db");
    let source = previous.join("codecal.db");
    if target.exists() || local == previous || !source.is_file() {
        return Ok(target);
    }
    let source_conn = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("既存DBを開けません: {e}"))?;
    let temporary = tempfile::NamedTempFile::new_in(local).map_err(|e| e.to_string())?;
    // SQLite backup takes a consistent snapshot, including committed WAL pages.
    source_conn
        .backup(MAIN_DB, temporary.path(), None)
        .map_err(|e| format!("DBの移行に失敗しました: {e}"))?;
    exclude_human_cache(temporary.path())?;
    // No-clobber publication also protects an independently created local DB.
    match temporary.persist_noclobber(&target) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("移行先DBを保存できません: {}", e.error)),
    }
    Ok(target)
}
