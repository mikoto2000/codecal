use crate::store::{Store, Snapshot, Progress};
use serde::{Serialize,Deserialize};
use std::{path::PathBuf, sync::Mutex, time::{Duration, Instant}};
use tauri::{Emitter,Manager,State};
#[derive(Clone, Serialize,Deserialize)]
pub struct Config {pub log_root:String,pub timezone:String,pub week_start:u8,pub theme:String,pub thresholds:[f64;4],pub auto_refresh:bool}
impl Default for Config {fn default()->Self{Self{log_root:std::env::var("CODEX_HOME").unwrap_or_else(|_|{let home=std::env::var("USERPROFILE").or_else(|_|std::env::var("HOME")).unwrap_or_default();PathBuf::from(home).join(".codex").display().to_string()}),timezone:"UTC".into(),week_start:1,theme:"system".into(),thresholds:[1.,3.,6.,9.],auto_refresh:true}}}
pub struct AppState {pub store:Mutex<Store>,pub db_path:String}
#[derive(Serialize)] pub struct Bootstrap {config:Config,db_path:String,first_run:bool}
fn config(store:&Store)->Result<Config,String>{store.get_setting("config")?.map(|s|serde_json::from_str(&s).map_err(|e|e.to_string())).unwrap_or_else(||Ok(Config::default()))}
#[tauri::command]
pub async fn bootstrap(state:State<'_,AppState>,timezone:String)->Result<Bootstrap,String>{
 let store=state.store.lock().map_err(|e|e.to_string())?;let first_run=store.get_setting("config")?.is_none();let mut c=config(&store)?;
 if first_run && timezone.parse::<chrono_tz::Tz>().is_ok(){c.timezone=timezone;store.set_setting("config",&serde_json::to_string(&c).map_err(|e|e.to_string())?)?;}
 Ok(Bootstrap{config:c,db_path:state.db_path.clone(),first_run})
}
#[tauri::command]
pub async fn save_config(state:State<'_,AppState>,value:Config)->Result<(),String>{
 if value.timezone.parse::<chrono_tz::Tz>().is_err() || ![0,1].contains(&value.week_start) || !["system","light","dark"].contains(&value.theme.as_str()) || value.thresholds.iter().any(|v|!v.is_finite() || *v<=0.) || value.thresholds.windows(2).any(|w|w[0]>=w[1]) {return Err("設定値を確認してください（時差名・しきい値の昇順）".into())}
 if !PathBuf::from(&value.log_root).is_dir(){return Err("ログディレクトリが存在しません".into())}
 state.store.lock().map_err(|e|e.to_string())?.set_setting("config",&serde_json::to_string(&value).map_err(|e|e.to_string())?)
}
#[tauri::command]
pub async fn snapshot(app:tauri::AppHandle,timezone:String)->Result<Snapshot,String>{
 tauri::async_runtime::spawn_blocking(move||app.state::<AppState>().store.lock().map_err(|e|e.to_string())?.snapshot(&timezone)).await.map_err(|e|e.to_string())?
}
fn sync_app(app:&tauri::AppHandle)->Result<Progress,String>{
 let state=app.state::<AppState>();let mut store=state.store.lock().map_err(|e|e.to_string())?;let c=config(&store)?;
 let result=store.sync_progress(&PathBuf::from(c.log_root),|done,total|{let _=app.emit("sync-progress",serde_json::json!({"done":done,"total":total}));});
 if result.is_ok(){let _=app.emit("data-changed",());} result
}
#[tauri::command]
pub async fn sync_logs(app:tauri::AppHandle)->Result<Progress,String>{tauri::async_runtime::spawn_blocking(move||sync_app(&app)).await.map_err(|e|e.to_string())?}
#[tauri::command]
pub async fn update_project(state:State<'_,AppState>,id:String,name:String,color:String)->Result<(),String>{state.store.lock().map_err(|e|e.to_string())?.update_project(&id,&name,&color)}
#[tauri::command]
pub async fn source_excerpt(app:tauri::AppHandle,id:String)->Result<String,String>{tauri::async_runtime::spawn_blocking(move||app.state::<AppState>().store.lock().map_err(|e|e.to_string())?.source_excerpt(&id)).await.map_err(|e|e.to_string())?}
pub fn run(){
 tauri::Builder::default().setup(|app|{
  let dir=app.path().app_data_dir()?;std::fs::create_dir_all(&dir)?;let path=dir.join("codecal.db");let store=Store::open(&path).map_err(std::io::Error::other)?;
  app.manage(AppState{store:Mutex::new(store),db_path:path.display().to_string()});
  let handle=app.handle().clone();
  std::thread::spawn(move||{
   use notify::{Watcher,RecursiveMode};
   let (sender,receiver)=std::sync::mpsc::channel();let mut watcher=notify::recommended_watcher(move|result|{let _=sender.send(result);}).ok();let mut watching=String::new();let mut dirty:Option<Instant>=None;let mut first_dirty:Option<Instant>=None;let mut last_scan=Instant::now();
   loop {
    if let Ok(event)=receiver.recv_timeout(Duration::from_millis(300)) {match event {Ok(_)=>{dirty=Some(Instant::now());first_dirty.get_or_insert(Instant::now());},Err(_)=>{let _=handle.emit("sync-error","ファイル監視エラー。定期走査で補完します");}}}
    let c={let state=handle.state::<AppState>();let Ok(store)=state.store.try_lock() else {continue};config(&store).unwrap_or_default()};
    if c.log_root!=watching {if let Some(w)=watcher.as_mut(){if !watching.is_empty(){let _=w.unwatch(std::path::Path::new(&watching));}if let Err(e)=w.watch(std::path::Path::new(&c.log_root),RecursiveMode::Recursive){let _=handle.emit("sync-error",format!("監視を開始できません: {e}"));}}watching=c.log_root;}
    if c.auto_refresh && (dirty.is_some_and(|t|t.elapsed()>=Duration::from_millis(600)) || first_dirty.is_some_and(|t|t.elapsed()>=Duration::from_secs(5)) || last_scan.elapsed()>=Duration::from_secs(30)) {
     if let Err(e)=sync_app(&handle){let _=handle.emit("sync-error",e);}dirty=None;first_dirty=None;last_scan=Instant::now();
    }
   }
  });Ok(())
 }).invoke_handler(tauri::generate_handler![bootstrap,save_config,snapshot,sync_logs,update_project,source_excerpt]).run(tauri::generate_context!()).expect("CodeCal could not start");
}
