//! Human-only synchronization control. No activity contents enter this module.
use crate::rei_human::HumanActivityResponse;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Default, Serialize)]
pub struct HumanSyncStatus {
    pub busy: bool,
    pub revision: u64,
    pub error: Option<String>,
    pub last_success: Option<i64>,
}

pub struct HumanRuntime;
impl HumanRuntime {
    pub fn new(
        _cache: PathBuf,
        _staging: PathBuf,
        _notify: impl Fn(HumanSyncStatus) + Send + Sync + 'static,
    ) -> Self {
        Self
    }
    pub fn status(&self) -> HumanSyncStatus {
        HumanSyncStatus::default()
    }
    pub fn period(&self, _from: i64, _to: i64) -> Result<HumanActivityResponse, String> {
        Err("human_activity_disabled".into())
    }
}
