use rusqlite::{Connection, OpenFlags, MAIN_DB};
use std::{
    fs,
    path::{Path, PathBuf},
};
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
    // No-clobber publication also protects an independently created local DB.
    match temporary.persist_noclobber(&target) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("移行先DBを保存できません: {}", e.error)),
    }
    Ok(target)
}
