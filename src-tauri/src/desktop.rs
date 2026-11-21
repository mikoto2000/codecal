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
    #[serde(default = "default_rei_enabled")]
    pub rei_enabled: bool,
    #[serde(default = "default_rei_root")]
    pub rei_root: String,
    pub timezone: String,
    pub week_start: u8,
    pub theme: String,
    #[serde(deserialize_with = "read_thresholds")]
    pub thresholds: [f64; 3],
    pub auto_refresh: bool,
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
    fn old_configuration_gets_rei_defaults_and_keeps_existing_preferences() {
        let c: Config = serde_json::from_str(r#"{"log_root":"/logs","timezone":"Asia/Tokyo","week_start":0,"theme":"dark","thresholds":[1,3,6],"auto_refresh":false}"#).unwrap();
        assert_eq!(c.rei_root, default_rei_root());
        assert_eq!(c.rei_enabled, default_rei_enabled());
        assert_eq!(c.timezone, "Asia/Tokyo");
        assert_eq!(c.theme, "dark");
        assert!(!c.auto_refresh);
    }
}
fn read_thresholds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[f64; 3], D::Error> {
    let values = Vec::<f64>::deserialize(deserializer)?;
    if values.len() != 3 && values.len() != 4 {
        return Err(serde::de::Error::custom("three thresholds required"));
    }
    Ok([values[0], values[1], values[2]])
}
impl Default for Config {
    fn default() -> Self {
        Self {
            rei_enabled: default_rei_enabled(),
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
}
#[derive(Serialize)]
pub struct Bootstrap {
    config: Config,
    db_path: String,
    first_run: bool,
}
fn config(store: &Store) -> Result<Config, String> {
    store
        .get_setting("config")?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .unwrap_or_else(|| Ok(Config::default()))
}
#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>, timezone: String) -> Result<Bootstrap, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let first_run = store.get_setting("config")?.is_none();
    let mut c = config(&store)?;
    if first_run && timezone.parse::<chrono_tz::Tz>().is_ok() {
        c.timezone = timezone;
        store.set_setting(
            "config",
            &serde_json::to_string(&c).map_err(|e| e.to_string())?,
        )?;
    }
    Ok(Bootstrap {
        config: c,
        db_path: state.db_path.clone(),
        first_run,
    })
}
#[tauri::command]
pub async fn save_config(state: State<'_, AppState>, value: Config) -> Result<(), String> {
    if value.timezone.parse::<chrono_tz::Tz>().is_err()
        || ![0, 1].contains(&value.week_start)
        || !["system", "light", "dark"].contains(&value.theme.as_str())
        || value.thresholds.iter().any(|v| !v.is_finite() || *v <= 0.)
        || value.thresholds.windows(2).any(|w| w[0] >= w[1])
    {
        return Err("設定値を確認してください（時差名・しきい値の昇順）".into());
    }
    if value.log_root.trim().is_empty() || (value.rei_enabled && value.rei_root.trim().is_empty()) {
        return Err("ログディレクトリを指定してください".into());
    }
    state.store.lock().map_err(|e| e.to_string())?.set_setting(
        "config",
        &serde_json::to_string(&value).map_err(|e| e.to_string())?,
    )
}
#[tauri::command]
pub async fn snapshot(app: tauri::AppHandle, timezone: String) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .snapshot(&timezone)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn sync_app(app: &tauri::AppHandle) -> Result<Progress, String> {
    let state = app.state::<AppState>();
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let c = config(&store)?;
    let rei_root = PathBuf::from(c.rei_root);
    let result = store.sync_sources(
        &PathBuf::from(c.log_root),
        c.rei_enabled.then_some(rei_root.as_path()),
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
    tauri::async_runtime::spawn_blocking(move || sync_app(&app))
        .await
        .map_err(|e| e.to_string())?
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
        app.state::<AppState>()
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
            let path = if let Some(dir) = std::env::var_os("CODECAL_DATA_DIR") {
                let dir = PathBuf::from(dir);
                crate::storage::prepare_database(&dir, &dir)
            } else {
                crate::storage::prepare_database(
                    &app.path().app_local_data_dir()?,
                    &app.path().app_data_dir()?,
                )
            }
            .map_err(std::io::Error::other)?;
            let store = Store::open(&path).map_err(std::io::Error::other)?;
            let state_file = std::fs::canonicalize(&path)?.with_file_name("window-state.json");
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

            app.manage(AppState {
                store: Mutex::new(store),
                db_path: path.display().to_string(),
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
                let mut watcher = notify::recommended_watcher(move |result| {
                    let _ = sender.send(result);
                })
                .ok();
                let mut watching: Vec<String> = Vec::new();
                let mut dirty: Option<Instant> = None;
                let mut first_dirty: Option<Instant> = None;
                let mut last_scan = Instant::now();
                loop {
                    if let Ok(event) = receiver.recv_timeout(Duration::from_millis(300)) {
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
                                let _ = handle
                                    .emit("sync-error", "ファイル監視エラー。定期走査で補完します");
                            }
                        }
                    }
                    let c = {
                        let state = handle.state::<AppState>();
                        let Ok(store) = state.store.try_lock() else {
                            continue;
                        };
                        config(&store).unwrap_or_default()
                    };
                    let mut roots = vec![c.log_root];
                    if c.rei_enabled && !roots.contains(&c.rei_root) {
                        roots.push(c.rei_root);
                    }
                    if roots != watching {
                        if let Some(w) = watcher.as_mut() {
                            for root in &watching {
                                let _ = w.unwatch(std::path::Path::new(root));
                            }
                            for root in &roots {
                                if let Err(e) =
                                    w.watch(std::path::Path::new(root), RecursiveMode::Recursive)
                                {
                                    let _ = handle
                                        .emit("sync-error", format!("監視を開始できません: {e}"));
                                }
                            }
                        }
                        watching = roots;
                    }
                    if c.auto_refresh
                        && (dirty.is_some_and(|t| t.elapsed() >= Duration::from_millis(600))
                            || first_dirty.is_some_and(|t| t.elapsed() >= Duration::from_secs(5))
                            || last_scan.elapsed() >= Duration::from_secs(30))
                    {
                        if let Err(e) = sync_app(&handle) {
                            let _ = handle.emit("sync-error", e);
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
            update_project,
            source_excerpt
        ])
        .run(tauri::generate_context!())
        .expect("CodeCal could not start");
}
