use crate::human_runtime::{HumanClearResult, HumanPreferences, HumanRuntime, HumanSyncStatus};
use crate::store::{Progress, Snapshot, Store};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub log_root: String,
    #[serde(default = "default_claude_enabled")]
    pub claude_enabled: bool,
    #[serde(default = "default_claude_root")]
    pub claude_root: String,
    #[serde(default = "default_rei_enabled")]
    pub rei_enabled: bool,
    #[serde(default)]
    pub human_activity_enabled: bool,
    #[serde(default = "default_rei_root")]
    pub rei_root: String,
    pub timezone: String,
    pub week_start: u8,
    pub theme: String,
    #[serde(deserialize_with = "read_thresholds")]
    pub thresholds: [f64; 3],
    pub auto_refresh: bool,
}
fn default_claude_root() -> String {
    crate::claude::default_root().display().to_string()
}
fn default_claude_enabled() -> bool {
    crate::claude::default_root().is_dir()
}
fn default_rei_root() -> String {
    crate::rei::default_root().display().to_string()
}
fn default_rei_enabled() -> bool {
    crate::rei::default_root().is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_and_invalid_settings_are_backed_up_and_recovered_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for raw in ["{broken".to_string(), serde_json::json!({"log_root":"/logs","timezone":"bad/zone","week_start":1,"theme":"system","thresholds":[1,3,6],"auto_refresh":true}).to_string()] {
            store.set_setting("config", &raw).unwrap();
            store.set_setting("unrelated", "keep").unwrap();
            let (c, first, notices) = bootstrap_config(&store, "Asia/Tokyo".into()).unwrap();
            assert!(!first);
            assert_eq!(c.timezone, "Asia/Tokyo");
            assert_eq!(notices.len(), 1);
            let start = notices[0].find('「').unwrap() + '「'.len_utf8();
            let end = notices[0].find('」').unwrap();
            let backup = &notices[0][start..end];
            assert_eq!(store.get_setting(backup).unwrap().as_deref(), Some(raw.as_str()));
            assert_eq!(store.get_setting("unrelated").unwrap().as_deref(), Some("keep"));
            let (_, _, next_notices) = bootstrap_config(&store, "UTC".into()).unwrap();
            assert!(next_notices.is_empty());
            assert_eq!(config(&store).unwrap().timezone, "Asia/Tokyo");
        }
    }
    #[test]
    fn first_boot_reports_unknown_timezone_and_keeps_saved_choice() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let (c, first, notices) = bootstrap_config(&store, "invalid/zone".into()).unwrap();
        assert!(first);
        assert_eq!(c.timezone, "UTC");
        assert!(notices[0].contains("UTC"));
        let (c, first, notices) = bootstrap_config(&store, "Asia/Tokyo".into()).unwrap();
        assert!(!first);
        assert_eq!(c.timezone, "UTC");
        assert!(notices.is_empty());
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let (c, _, notices) = bootstrap_config(&store, "Asia/Tokyo".into()).unwrap();
        assert_eq!(c.timezone, "Asia/Tokyo");
        assert!(notices.is_empty());
    }
    #[test]
    fn settings_validation_identifies_the_invalid_field() {
        let mut c = Config {
            timezone: "invalid/zone".into(),
            ..Config::default()
        };
        assert!(validate_config(&c).unwrap_err().contains("タイムゾーン"));
        c.timezone = "Asia/Tokyo".into();
        c.thresholds = [3., 1., 6.];
        assert!(validate_config(&c).unwrap_err().contains("しきい値"));
        c.thresholds = [1., 3., 6.];
        assert!(validate_config(&c).is_ok());
    }
    #[test]
    fn old_configuration_gets_rei_defaults_and_keeps_existing_preferences() {
        let c: Config = serde_json::from_str(r#"{"log_root":"/logs","timezone":"Asia/Tokyo","week_start":0,"theme":"dark","thresholds":[1,3,6],"auto_refresh":false}"#).unwrap();
        assert_eq!(c.claude_root, default_claude_root());
        assert_eq!(c.claude_enabled, default_claude_enabled());
        assert_eq!(c.rei_root, default_rei_root());
        assert_eq!(c.rei_enabled, default_rei_enabled());
        assert!(!c.human_activity_enabled);
        assert_eq!(c.timezone, "Asia/Tokyo");
        assert_eq!(c.theme, "dark");
        assert!(!c.auto_refresh);
        let c: Config = serde_json::from_str(r##"{"log_root":"/logs","timezone":"UTC","week_start":1,"theme":"system","thresholds":[1,3,6,12],"auto_refresh":false}"##).unwrap();
        assert_eq!(c.thresholds, [1., 3., 6.]);
    }
    #[test]
    fn missing_log_roots_warn_without_rejecting_future_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Config {
            log_root: tmp.path().display().to_string(),
            claude_enabled: false,
            rei_enabled: false,
            rei_root: tmp.path().join("missing-rei").display().to_string(),
            ..Config::default()
        };
        assert!(log_root_warnings(&c).is_empty());
        c.rei_enabled = true;
        assert_eq!(log_root_warnings(&c).len(), 1);
        c.log_root = tmp.path().join("missing-codex").display().to_string();
        let warnings = log_root_warnings(&c);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("Codex"));
        assert!(warnings[1].contains("れい"));
        std::fs::create_dir(&c.log_root).unwrap();
        std::fs::create_dir(&c.rei_root).unwrap();
        assert!(log_root_warnings(&c)[0].contains("storage.db または projects"));
        std::fs::create_dir(std::path::Path::new(&c.rei_root).join("projects")).unwrap();
        assert!(log_root_warnings(&c).is_empty());
        let file = tmp.path().join("not-a-directory");
        std::fs::write(&file, "").unwrap();
        c.log_root = file.display().to_string();
        assert_eq!(log_root_warnings(&c).len(), 1);
    }
    #[test]
    fn human_and_ai_settings_persist_independently_in_all_four_combinations() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for ai in [false, true] {
            for human in [false, true] {
                let value = Config {
                    rei_enabled: ai,
                    human_activity_enabled: human,
                    ..Config::default()
                };
                store
                    .set_setting("config", &serde_json::to_string(&value).unwrap())
                    .unwrap();
                let loaded = config(&store).unwrap();
                assert_eq!(loaded.rei_enabled, ai);
                assert_eq!(loaded.human_activity_enabled, human);
                assert_eq!(human_preferences(&loaded).enabled, human);
            }
        }
    }
    #[test]
    fn human_only_watches_fixed_database_files_and_both_off_removes_rei_targets() {
        let tmp = tempfile::tempdir().unwrap();
        for file in [
            "storage.db",
            "storage.db-wal",
            "projects.json",
            "PRIVATE_SCREENSHOT.png",
        ] {
            std::fs::write(tmp.path().join(file), b"synthetic").unwrap();
        }
        std::fs::create_dir(tmp.path().join("projects")).unwrap();
        let mut c = Config {
            log_root: tmp.path().join("missing-codex").display().to_string(),
            claude_enabled: false,
            rei_enabled: false,
            human_activity_enabled: true,
            rei_root: tmp.path().display().to_string(),
            ..Config::default()
        };
        let targets = watch_targets(&c);
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().all(|target| !target.recursive));
        assert!(targets.iter().all(|target| ["storage.db", "storage.db-wal"]
            .iter()
            .any(|name| target.path.file_name().unwrap() == *name)));
        c.human_activity_enabled = false;
        assert!(watch_targets(&c).is_empty());
        c.rei_enabled = true;
        assert_eq!(watch_targets(&c).len(), 4);
    }
}
fn read_thresholds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[f64; 3], D::Error> {
    let values = Vec::<f64>::deserialize(deserializer)?;
    if values.len() != 3 && values.len() != 4 {
        return Err(serde::de::Error::custom("three thresholds required"));
    }
    // Keep accepting legacy four-value settings without resetting preferences.
    Ok([values[0], values[1], values[2]])
}
impl Default for Config {
    fn default() -> Self {
        Self {
            claude_enabled: default_claude_enabled(),
            claude_root: default_claude_root(),
            rei_enabled: default_rei_enabled(),
            human_activity_enabled: false,
            rei_root: default_rei_root(),
            log_root: std::env::var("CODEX_HOME").unwrap_or_else(|_| {
                let home = std::env::var("USERPROFILE")
                    .or_else(|_| std::env::var("HOME"))
                    .unwrap_or_default();
                PathBuf::from(home).join(".codex").display().to_string()
            }),
            timezone: "UTC".into(),
            week_start: 1,
            theme: "system".into(),
            thresholds: [1., 3., 6.],
            auto_refresh: true,
        }
    }
}
pub struct AppState {
    pub store: Mutex<Store>,
    pub db_path: String,
    pub human: HumanRuntime,
}
fn human_preferences(config: &Config) -> HumanPreferences {
    HumanPreferences {
        enabled: config.human_activity_enabled,
        root: PathBuf::from(&config.rei_root),
        auto_refresh: config.auto_refresh,
    }
}
#[derive(PartialEq, Eq)]
struct WatchTarget {
    path: PathBuf,
    recursive: bool,
    identity: Option<String>,
}
fn watch_targets(config: &Config) -> Vec<WatchTarget> {
    let mut paths = vec![(PathBuf::from(&config.log_root), true)];
    if config.claude_enabled {
        paths.push((PathBuf::from(&config.claude_root), true));
    }
    if config.rei_enabled || config.human_activity_enabled {
        let root = PathBuf::from(&config.rei_root);
        paths.push((root.join("storage.db"), false));
        paths.push((root.join("storage.db-wal"), false));
        if config.rei_enabled {
            paths.push((root.join("projects.json"), false));
            paths.push((root.join("projects"), true));
        }
    }
    let mut targets = Vec::new();
    for (path, recursive) in paths {
        if !path.exists()
            || targets
                .iter()
                .any(|target: &WatchTarget| target.path == path && target.recursive == recursive)
        {
            continue;
        }
        let identity = (!recursive)
            .then(|| crate::rei_human::source_identity(&path).ok())
            .flatten();
        targets.push(WatchTarget {
            path,
            recursive,
            identity,
        });
    }
    targets
}
struct StartupContext {
    local: PathBuf,
    previous: PathBuf,
    initialization: Mutex<()>,
}
#[derive(Serialize)]
pub struct Bootstrap {
    config: Config,
    db_path: String,
    first_run: bool,
    notices: Vec<String>,
}
fn config(store: &Store) -> Result<Config, String> {
    let c = store
        .get_setting("config")?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(Config::default()))?;
    validate_config(&c)?;
    Ok(c)
}
#[tauri::command]
pub async fn bootstrap(app: tauri::AppHandle, timezone: String) -> Result<Bootstrap, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let startup = app.state::<StartupContext>();
        // StrictMode and retries can invoke bootstrap more than once. Only one
        // worker may prepare the database, and a successful Store is reused.
        let initialization = startup.initialization.lock().map_err(|e| e.to_string())?;
        if app.try_state::<AppState>().is_none() {
            let path = crate::storage::prepare_database(&startup.local, &startup.previous)?;
            let store = Store::open_with_progress(&path, || {
                let _ = app.emit("startup-progress", "migrating");
            })?;
            let (c, first_run, notices) = bootstrap_config(&store, timezone.clone())?;
            let handle = app.clone();
            let human = HumanRuntime::new(
                path.clone(),
                path.parent()
                    .ok_or("invalid_cache_path")?
                    .join("human-staging"),
                move |status| {
                    let _ = handle.emit("human-status", status);
                },
            );
            human.configure(human_preferences(&c), || Ok(()))?;
            app.manage(AppState {
                store: Mutex::new(store),
                db_path: path.display().to_string(),
                human,
            });
            return Ok(Bootstrap {
                config: c,
                db_path: path.display().to_string(),
                first_run,
                notices,
            });
        }
        drop(initialization);
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|e| e.to_string())?;
        let (c, first_run, notices) = bootstrap_config(&store, timezone)?;
        Ok(Bootstrap {
            config: c,
            db_path: state.db_path.clone(),
            first_run,
            notices,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn bootstrap_config(
    store: &Store,
    timezone: String,
) -> Result<(Config, bool, Vec<String>), String> {
    let first_run = store.get_setting("config")?.is_none();
    let mut notices = vec![];
    let loaded = if first_run {
        Ok(Config::default())
    } else {
        config(store)
    };
    let recovered = loaded.is_err();
    let mut c = loaded.unwrap_or_default();
    if first_run || recovered {
        if timezone.parse::<chrono_tz::Tz>().is_ok() {
            c.timezone = timezone;
        } else {
            notices.push(format!("端末のタイムゾーン「{timezone}」を認識できなかったため UTC を使用しています。設定で確認してください。"));
        }
        let serialized = serde_json::to_string(&c).map_err(|e| e.to_string())?;
        if recovered {
            let backup = store.reset_setting_with_backup("config", &serialized)?;
            notices.push(format!("保存済みの設定を読み込めなかったため、既定の設定で復旧しました。元の設定はDBの settings テーブルの「{backup}」に退避しています。取り込み先とタイムゾーンを設定で確認してください。"));
        } else {
            store.set_setting("config", &serialized)?;
        }
    }
    Ok((c, first_run, notices))
}
fn log_root_warnings(value: &Config) -> Vec<String> {
    let mut warnings = vec![];
    for (source, root, enabled) in [
        ("Codex", &value.log_root, true),
        ("れい", &value.rei_root, value.rei_enabled),
        ("Claude Code", &value.claude_root, value.claude_enabled),
    ] {
        if enabled && !std::path::Path::new(root).is_dir() {
            warnings.push(format!(
                "{source} のログディレクトリが見つかりません: {root}"
            ));
        }
    }
    let rei_root = std::path::Path::new(&value.rei_root);
    if value.rei_enabled && rei_root.is_dir() {
        if let Err(message) = crate::rei::validate_root(rei_root) {
            warnings.push(format!("{message}: {}", rei_root.display()));
        }
    }
    if value.human_activity_enabled && !rei_root.join("storage.db").is_file() {
        warnings.push("人間のアクティビティ用 storage.db が見つかりません。前回の正常なキャッシュは保持します。".into());
    }
    warnings
}
#[tauri::command]
pub async fn save_config(app: tauri::AppHandle, value: Config) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_config(&value)?;
        let warnings = log_root_warnings(&value);
        let state = app.state::<AppState>();
        state.human.configure(human_preferences(&value), || {
            state.store.lock().map_err(|e| e.to_string())?.set_setting(
                "config",
                &serde_json::to_string(&value).map_err(|e| e.to_string())?,
            )
        })?;
        Ok(warnings)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn validate_config(value: &Config) -> Result<(), String> {
    if value.timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err("タイムゾーンを確認してください（例: Asia/Tokyo、UTC）".into());
    }
    if ![0, 1].contains(&value.week_start) {
        return Err("週の開始曜日を確認してください".into());
    }
    if !["system", "light", "dark"].contains(&value.theme.as_str()) {
        return Err("テーマを確認してください".into());
    }
    if value.thresholds.iter().any(|v| !v.is_finite() || *v <= 0.)
        || value.thresholds.windows(2).any(|w| w[0] >= w[1])
    {
        return Err("ヒートマップのしきい値は正の数を昇順で指定してください".into());
    }
    if value.log_root.trim().is_empty()
        || ((value.rei_enabled || value.human_activity_enabled) && value.rei_root.trim().is_empty())
        || (value.claude_enabled && value.claude_root.trim().is_empty())
    {
        return Err("ログディレクトリを指定してください".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn snapshot(app: tauri::AppHandle, timezone: String) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|_| "invalid timezone")?;
        app.try_state::<AppState>()
            .ok_or("CodeCal がデータベースを準備しています")?
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .snapshot_for_display()
    })
    .await
    .map_err(|e| e.to_string())?
}
fn sync_app(app: &tauri::AppHandle) -> Result<Progress, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or("CodeCal がデータベースを準備しています")?;
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let c = config(&store)?;
    let rei_root = PathBuf::from(c.rei_root);
    let claude_root = PathBuf::from(c.claude_root);
    let result = store.sync_all_sources(
        &PathBuf::from(c.log_root),
        c.rei_enabled.then_some(rei_root.as_path()),
        c.claude_enabled.then_some(claude_root.as_path()),
        |done, total| {
            let _ = app.emit(
                "sync-progress",
                serde_json::json!({"done":done,"total":total}),
            );
        },
    );
    if result.is_ok() {
        let _ = app.emit("data-changed", ());
    }
    result
}
#[tauri::command]
pub async fn sync_logs(app: tauri::AppHandle) -> Result<Progress, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(state) = app.try_state::<AppState>() {
            state.human.request(true);
        }
        sync_app(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn human_status(state: State<'_, AppState>) -> HumanSyncStatus {
    state.human.status()
}
#[tauri::command]
pub fn sync_human(state: State<'_, AppState>) {
    state.human.request(true);
}
#[tauri::command]
pub async fn human_activity(
    app: tauri::AppHandle,
    from: i64,
    to: i64,
) -> Result<crate::rei_human::HumanActivityResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.try_state::<AppState>()
            .ok_or("human_not_ready")?
            .human
            .period(from, to)
    })
    .await
    .map_err(|_| "human_request_failed")?
}
#[tauri::command]
pub async fn clear_human_activity(
    app: tauri::AppHandle,
    all: bool,
) -> Result<HumanClearResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.try_state::<AppState>().ok_or("human_not_ready")?;
        let result = state.human.clear(all, |store| {
            let mut c = config(store)?;
            c.human_activity_enabled = false;
            serde_json::to_string(&c).map_err(|_| "invalid_config".into())
        })?;
        let _ = app.emit("human-status", state.human.status());
        Ok(result)
    })
    .await
    .map_err(|_| "human_request_failed")?
}
#[tauri::command]
pub async fn update_project(
    state: State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<(), String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .update_project(&id, &name, &color)
}
#[tauri::command]
pub async fn source_excerpt(app: tauri::AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.try_state::<AppState>()
            .ok_or("CodeCal がデータベースを準備しています")?
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .source_excerpt(&id)
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (local, previous) = if let Some(dir) = std::env::var_os("CODECAL_DATA_DIR") {
                let dir = std::path::absolute(PathBuf::from(dir))?;
                (dir.clone(), dir)
            } else {
                (app.path().app_local_data_dir()?, app.path().app_data_dir()?)
            };
            let state_file = local.join("window-state.json");
            app.handle().plugin(
                tauri_plugin_window_state::Builder::default()
                    .with_filename(state_file.to_string_lossy().into_owned())
                    .with_state_flags(
                        tauri_plugin_window_state::StateFlags::SIZE
                            | tauri_plugin_window_state::StateFlags::POSITION
                            | tauri_plugin_window_state::StateFlags::MAXIMIZED
                            | tauri_plugin_window_state::StateFlags::VISIBLE,
                    )
                    .build(),
            )?;

            app.manage(StartupContext {
                local,
                previous,
                initialization: Mutex::new(()),
            });
            // Register window-state before creating the main window so its
            // ready callback restores geometry and installs event tracking.
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .ok_or("main window configuration missing")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), window_config)?.build()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                use notify::{RecursiveMode, Watcher};
                let (sender, receiver) = std::sync::mpsc::channel();
                let event_handle = handle.clone();
                let watcher_result =
                    notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                        if result.as_ref().is_ok_and(|event| {
                            event.paths.iter().any(|path| {
                                path.file_name().is_some_and(|name| {
                                    name == "storage.db" || name == "storage.db-wal"
                                })
                            })
                        }) {
                            if let Some(state) = event_handle.try_state::<AppState>() {
                                state.human.request(false);
                            }
                        }
                        let _ = sender.send(result);
                    });
                let watcher_warning = watcher_result.as_ref().err().map(|e| {
                    format!("ファイル監視を初期化できません。30秒ごとの定期走査で補完します: {e}")
                });
                let mut watcher = watcher_result.ok();
                let mut watching: Vec<WatchTarget> = Vec::new();
                let mut dirty: Option<Instant> = None;
                let mut first_dirty: Option<Instant> = None;
                let mut last_scan = Instant::now();
                loop {
                    let received = receiver.recv_timeout(Duration::from_millis(300));
                    if matches!(
                        received,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
                    ) {
                        // A failed watcher drops its sender; avoid polling a disconnected channel.
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    if let Ok(event) = received {
                        match event {
                            Ok(event)
                                if event.paths.iter().any(|p| {
                                    p.extension().is_some_and(|ext| ext == "jsonl")
                                        || p.file_name().is_some_and(|name| {
                                            matches!(
                                                name.to_str(),
                                                Some(
                                                    "storage.db"
                                                        | "storage.db-wal"
                                                        | "projects.json"
                                                )
                                            )
                                        })
                                        || p.is_dir()
                                }) =>
                            {
                                dirty = Some(Instant::now());
                                first_dirty.get_or_insert(Instant::now());
                            }
                            Ok(_) => {}
                            Err(_) => {
                                let _ = handle.emit(
                                    "watch-warning",
                                    "ファイル監視エラー。定期走査で補完します",
                                );
                            }
                        }
                    }
                    let c = {
                        let Some(state) = handle.try_state::<AppState>() else {
                            continue;
                        };
                        let Ok(store) = state.store.try_lock() else {
                            continue;
                        };
                        config(&store).unwrap_or_default()
                    };
                    let roots = watch_targets(&c);
                    if roots != watching {
                        let mut watch_warnings = vec![];
                        if let Some(message) = &watcher_warning {
                            watch_warnings.push(message.clone());
                        }
                        if let Some(w) = watcher.as_mut() {
                            for root in &watching {
                                let _ = w.unwatch(&root.path);
                            }
                            for root in &roots {
                                if w.watch(
                                    &root.path,
                                    if root.recursive {
                                        RecursiveMode::Recursive
                                    } else {
                                        RecursiveMode::NonRecursive
                                    },
                                )
                                .is_err()
                                {
                                    watch_warnings
                                        .push("監視を開始できません。定期走査で補完します".into());
                                }
                            }
                        }
                        watching = roots;
                        let _ = handle.emit("watch-warning", watch_warnings.join("\n"));
                    }
                    if c.auto_refresh
                        && (dirty.is_some_and(|t| t.elapsed() >= Duration::from_millis(600))
                            || first_dirty.is_some_and(|t| t.elapsed() >= Duration::from_secs(5))
                            || last_scan.elapsed() >= Duration::from_secs(30))
                    {
                        match sync_app(&handle) {
                            Ok(p) => {
                                let _ = handle.emit("sync-warning", p.warnings);
                            }
                            Err(e) => {
                                let _ = handle.emit("sync-error", e);
                            }
                        }
                        dirty = None;
                        first_dirty = None;
                        last_scan = Instant::now();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_config,
            snapshot,
            sync_logs,
            human_status,
            sync_human,
            human_activity,
            clear_human_activity,
            update_project,
            source_excerpt
        ])
        .run(tauri::generate_context!())
        .expect("CodeCal could not start");
}
