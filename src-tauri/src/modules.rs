//! The base side of modules (docs/design/modules.md): island-host's supervisor, wired to the island.
//! Module code lives in `%LOCALAPPDATA%\league.seaweed.islandshell\modules\<id>\` (or, in developer
//! mode, wherever its folder is), its data in `%APPDATA%\…\modules\<id>\`, and `installed.json` says
//! which are installed and on. What backends send shows up as Tauri events:
//!
//! | Event | Payload |
//! | --- | --- |
//! | `mod:<id>:<name>` | whatever the module emitted |
//! | `mod:notify` | `{ module, title, body, icon, urgent, action, sound }` |
//! | `mod:changed` | the module's `ModuleInfo` (state, pid, error, …) |
//! | `island:activities` | every live activity, highest priority first |
//! | `calendar:changed` | nothing: module events changed |

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use island_host::installed::{Entry, Installed};
use island_host::proto::msg::{CalendarPut, Notify, Theme};
use island_host::proto::RpcError;
use island_host::{ActivityOf, ModuleInfo, Paths, Sink, Supervisor, Timing};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::util::{self, log_line};

static SUP: OnceLock<Supervisor> = OnceLock::new();
/// What http://mod.localhost/ serves: module UI files, hash-checked (island-host's ui.rs).
static UI: std::sync::LazyLock<island_host::ui::UiFiles> = std::sync::LazyLock::new(island_host::ui::UiFiles::new);
/// Installed modules that couldn't be loaded, with why (shown in the Modules sheet).
static BROKEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Where installed module code lives, and installed.json.
pub fn code_root() -> PathBuf {
    util::local_dir().join("modules")
}

fn installed_path() -> PathBuf {
    code_root().join("installed.json")
}

pub fn data_root() -> PathBuf {
    util::data_dir().join("modules")
}

pub fn cache_root() -> PathBuf {
    util::local_dir().join("modules-cache")
}

/// An installed module's folder: its own for catalog installs, its source for developer mode.
pub fn folder_of(id: &str, e: &Entry) -> PathBuf {
    if e.dev && !e.source.is_empty() {
        PathBuf::from(&e.source)
    } else {
        code_root().join(id)
    }
}

struct IslandSink {
    app: AppHandle,
    dir: PathBuf,
}

impl Sink for IslandSink {
    fn emit(&self, module: &str, name: &str, data: Value) {
        let _ = self.app.emit(&format!("mod:{module}:{name}"), data);
    }
    fn notify(&self, module: &str, n: Notify) {
        let mut v = serde_json::to_value(n).unwrap_or(Value::Null);
        v["module"] = module.into();
        let _ = self.app.emit("mod:notify", v);
    }
    fn activities(&self, list: Vec<ActivityOf>) {
        let _ = self.app.emit("island:activities", list);
    }
    fn calendar(&self, module: &str, put: CalendarPut) {
        crate::calendar::put_module_events(&self.dir, module, &put.source, &put.events);
        let _ = self.app.emit("calendar:changed", ());
    }
    fn log(&self, module: &str, level: &str, msg: &str) {
        log_line(&format!("[mod:{module}] {level}: {msg}"));
    }
    fn changed(&self, info: &ModuleInfo) {
        let _ = self.app.emit("mod:changed", info);
    }
    fn topic(&self, _module: &str, topic: &str, data: &Value) {
        let _ = self.app.emit(&format!("topic:{topic}"), data);
    }
}

pub fn sup() -> Option<&'static Supervisor> {
    SUP.get()
}

/// Starts the supervisor and loads every installed module.
pub fn init(app: &AppHandle, dir: &Path) {
    let paths = Paths { data_root: data_root(), cache_root: cache_root(), legacy_dir: util::data_dir() };
    let sink = Arc::new(IslandSink { app: app.clone(), dir: dir.to_path_buf() });
    let sup = Supervisor::new(paths, sink, env!("CARGO_PKG_VERSION"), Timing::default());
    // An install a crash cut short is finished or undone before anything loads.
    for id in island_host::install::recover(&code_root()) {
        log_line(&format!("modules: put {id} back after an interrupted update"));
    }
    let installed = Installed::load(&installed_path());
    for (id, e) in &installed.modules {
        let folder = folder_of(id, e);
        match sup.load(&folder, e.dev, e.enabled) {
            Ok(_) => log_line(&format!("modules: loaded {id} {}{}", e.version, if e.dev { " (developer mode)" } else { "" })),
            Err(err) => {
                log_line(&format!("modules: couldn't load {id}: {err}"));
                BROKEN.lock().unwrap().push((id.clone(), err));
            }
        }
    }
    let _ = SUP.set(sup);
    sync_contributions(dir);
    spawn_process_watch();
}

/// Asks every backend to stop (the job object ends whatever is left when the island exits).
pub fn shutdown() {
    if let Some(s) = sup() {
        s.shutdown();
    }
}

/// A topic the base publishes.
pub fn publish(topic: &str, data: Value) {
    if let Some(s) = sup() {
        s.publish(topic, data);
    }
}

/// Tells the calendar which module sources are live, and the Windows key which Win + letter
/// shortcuts modules have (`tab:<id>` opens the tab, `cube:<id>` the Command Center sheet).
fn sync_contributions(dir: &Path) {
    let Some(s) = sup() else { return };
    let on: Vec<ModuleInfo> = s.list().into_iter().filter(|m| m.enabled).collect();
    let list: Vec<(String, String, String, String)> =
        on.iter().flat_map(|m| m.manifest.contributes.calendar.iter().map(|c| (m.id.clone(), c.source.clone(), c.name.clone(), c.color.clone())).collect::<Vec<_>>()).collect();
    crate::calendar::set_module_sources(dir, &list);
    let keys: Vec<(char, String)> = on
        .iter()
        .flat_map(|m| m.manifest.contributes.hotkeys.iter())
        .filter_map(|h| {
            let c = h.key.chars().next()?;
            let target = h.opens.strip_prefix("tab:").map(String::from).or_else(|| h.opens.starts_with("cube:").then(|| h.opens.clone()))?;
            Some((c, target))
        })
        .collect();
    crate::winkey::set_module_keys(keys);
}

/// Every 2 s, while some `while-process` module is waiting, tells the supervisor what's running.
fn spawn_process_watch() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(2));
        let Some(s) = sup() else { continue };
        if s.watched_processes().is_empty() {
            continue;
        }
        s.set_processes(process_names());
    });
}

fn process_names() -> Vec<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push(String::from_utf16_lossy(&e.szExeFile[..len]));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn save_installed(f: impl FnOnce(&mut Installed)) -> Result<(), String> {
    let path = installed_path();
    let mut set = Installed::load(&path);
    f(&mut set);
    set.save(&path).map_err(|e| format!("Couldn't save installed.json: {e}"))
}

/// The `mod` protocol: `http://mod.localhost/<id>/<path>` for module UI (handoff §4.10). Module
/// scripts are imported cross-origin, so responses allow the island's origin.
pub fn serve_ui(path: &str) -> tauri::http::Response<Vec<u8>> {
    use island_host::ui::Served;
    let served = match sup() {
        Some(s) => UI.serve(&s.list(), &cache_root(), path),
        None => Served::NotFound,
    };
    let b = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "http://tauri.localhost").header("Cache-Control", "no-cache");
    let r = match served {
        Served::Ok { body, mime } => b.status(200).header("Content-Type", mime).body(body),
        Served::NotFound => b.status(404).body(Vec::new()),
        Served::Forbidden(why) => {
            log_line(&format!("modules: refused {path}: {why}"));
            b.status(403).body(Vec::new())
        }
    };
    r.unwrap_or_default()
}

// ---------- commands ----------

#[derive(Serialize)]
pub struct Listing {
    pub modules: Vec<ModuleInfo>,
    /// Installed but couldn't be loaded: `[id, why]`.
    pub broken: Vec<(String, String)>,
}

#[tauri::command]
pub fn mod_list() -> Listing {
    Listing { modules: sup().map(|s| s.list()).unwrap_or_default(), broken: BROKEN.lock().unwrap().clone() }
}

/// A UI call to a module's backend. Errors are `{ code, message }` (handoff §4.6).
#[tauri::command]
pub async fn mod_call(id: String, method: String, params: Option<Value>) -> Result<Value, RpcError> {
    let s = sup().ok_or_else(|| RpcError::new(island_host::proto::code::STOPPED, "Modules aren't running"))?;
    // UI calls follow a click in the island, which has the foreground: let the module bring a window
    // forward if that's what was asked (Radmin's, a game launcher's).
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
    }
    tauri::async_runtime::spawn_blocking(move || s.call(&id, &method, params.unwrap_or(Value::Null))).await.map_err(|e| RpcError::failed(e.to_string()))?
}

#[tauri::command]
pub fn mod_set_enabled(id: String, on: bool, shared: tauri::State<'_, Arc<crate::Shared>>) -> Result<(), String> {
    let s = sup().ok_or("Modules aren't running")?;
    s.set_enabled(&id, on)?;
    save_installed(|set| {
        if let Some(e) = set.modules.get_mut(&id) {
            e.enabled = on;
        }
    })?;
    sync_contributions(&shared.dir);
    Ok(())
}

#[tauri::command]
pub fn mod_restart(id: String) -> Result<(), String> {
    sup().ok_or("Modules aren't running")?.restart(&id)
}

#[tauri::command]
pub fn mod_logs(id: String) -> Vec<String> {
    sup().map(|s| s.logs(&id)).unwrap_or_default()
}

/// A topic's last value, for the page (it hears later ones as `topic:<name>`).
#[tauri::command]
pub fn topic_last(topic: String) -> Value {
    sup().map(|s| s.topic(&topic)).unwrap_or(Value::Null)
}

/// The page's theme, for modules (`theme` topic and the next `island.init`).
#[tauri::command]
pub fn mod_theme(dark: bool, accent: String) {
    if let Some(s) = sup() {
        let theme = Theme { dark, accent: accent.clone() };
        s.set_env(&locale(), theme);
        s.publish(island_host::proto::topic::THEME, json!({ "dark": dark, "accent": accent }));
    }
}

/// Developer mode: loads an unpacked module folder (unsigned; the Modules sheet badges it).
/// With no path, asks for a folder.
#[tauri::command]
pub async fn mod_load_folder(path: Option<String>, app: AppHandle, shared: tauri::State<'_, Arc<crate::Shared>>) -> Result<Option<ModuleInfo>, String> {
    let dir = shared.dir.clone();
    let folder = match path {
        Some(p) => PathBuf::from(p),
        None => match tauri::async_runtime::spawn_blocking(pick_folder).await.map_err(|e| e.to_string())? {
            Some(p) => p,
            None => return Ok(None),
        },
    };
    let s = sup().ok_or("Modules aren't running")?;
    let bytes = std::fs::read(folder.join("module.json")).map_err(|_| "That folder has no module.json".to_string())?;
    let manifest = island_host::proto::Manifest::parse(&bytes)?;
    if s.info(&manifest.id).is_some() {
        return Err(format!("{} is already installed; remove it first", manifest.name));
    }
    let id = s.load(&folder, true, true)?;
    save_installed(|set| {
        set.modules.insert(id.clone(), Entry { version: manifest.version.clone(), enabled: true, auto_update: false, dev: true, installed: now_rfc3339(), source: folder.to_string_lossy().into_owned() });
    })?;
    BROKEN.lock().unwrap().retain(|(b, _)| *b != id);
    sync_contributions(&dir);
    let _ = app.emit("mod:installed", &id);
    log_line(&format!("modules: loaded {id} from {} (developer mode)", folder.display()));
    Ok(s.info(&id))
}

/// Removes a module: stops it, deletes its code (not a developer-mode folder) and, if asked, its
/// keys and data.
#[tauri::command]
pub fn mod_remove(id: String, forget: bool, app: AppHandle, shared: tauri::State<'_, Arc<crate::Shared>>) -> Result<(), String> {
    let s = sup().ok_or("Modules aren't running")?;
    let entry = Installed::load(&installed_path()).modules.get(&id).cloned();
    s.unload(&id);
    if let Some(e) = &entry {
        if !e.dev {
            let _ = std::fs::remove_dir_all(code_root().join(&id));
        }
    }
    if forget {
        let _ = std::fs::remove_dir_all(data_root().join(&id));
        let _ = std::fs::remove_dir_all(cache_root().join(&id));
        crate::calendar::forget_module(&shared.dir, &id);
    }
    save_installed(|set| {
        set.modules.remove(&id);
    })?;
    BROKEN.lock().unwrap().retain(|(b, _)| *b != id);
    sync_contributions(&shared.dir);
    let _ = app.emit("mod:removed", &id);
    log_line(&format!("modules: removed {id}{}", if forget { " and its data" } else { "" }));
    Ok(())
}

/// What's installed, by id (for the updater).
pub fn installed_entries() -> std::collections::BTreeMap<String, Entry> {
    Installed::load(&installed_path()).modules
}

/// Puts a module package from the catalog in place and (re)loads it: a new module comes on, an
/// update keeps whether it was on and its own auto-update setting. If anything fails, the version
/// that was there runs again.
pub fn install_package(app: &AppHandle, zip: &[u8], want: &island_catalog::Module) -> Result<(), String> {
    let pkg = island_host::package::read(zip).map_err(|e| format!("The {} package was refused: {e}", want.name))?;
    if pkg.manifest.id != want.id || pkg.manifest.version != want.version {
        return Err(format!("The {} package doesn't hold what the catalog says", want.name));
    }
    let s = sup().ok_or("Modules aren't running")?;
    let old = Installed::load(&installed_path()).modules.get(&want.id).cloned();
    if old.as_ref().is_some_and(|e| e.dev) {
        return Err(format!("{} is loaded from a folder in developer mode; remove it first", want.name));
    }
    let enabled = old.as_ref().map_or(true, |e| e.enabled);
    let auto_update = old.as_ref().map_or(true, |e| e.auto_update);
    let was_loaded = s.info(&want.id).is_some();
    // Stopped first: its exe can't be replaced while it runs.
    s.unload(&want.id);
    let folder = match island_host::install::install(&code_root(), &pkg) {
        Ok(f) => f,
        Err(e) => {
            if was_loaded {
                let _ = s.load(&code_root().join(&want.id), false, enabled);
            }
            return Err(e);
        }
    };
    save_installed(|set| {
        set.modules.insert(want.id.clone(), Entry { version: want.version.clone(), enabled, auto_update, dev: false, installed: now_rfc3339(), source: "catalog".into() });
    })?;
    s.load(&folder, false, enabled)?;
    BROKEN.lock().unwrap().retain(|(b, _)| *b != want.id);
    sync_contributions(&util::data_dir());
    let _ = app.emit("mod:installed", serde_json::json!({ "id": want.id, "version": want.version, "updated": old.is_some() }));
    log_line(&format!("modules: {} {} {}", if old.is_some() { "updated" } else { "installed" }, want.id, want.version));
    Ok(())
}

fn locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if n > 1 {
        String::from_utf16_lossy(&buf[..n as usize - 1])
    } else {
        "en-US".into()
    }
}

fn now_rfc3339() -> String {
    let ms = util::now_ms() as i64;
    let days = ms.div_euclid(crate::ics::DAY);
    let (y, m, d) = crate::ics::ymd_from_days(days);
    let s = ms.rem_euclid(crate::ics::DAY) / 1000;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// The Windows folder picker.
fn pick_folder() -> Option<PathBuf> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
    util::com_sta();
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dialog.GetOptions().ok()?;
        dialog.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let _ = dialog.SetTitle(&windows::core::HSTRING::from("Load a module folder"));
        dialog.Show(None).ok()?;
        let item = dialog.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        Some(PathBuf::from(util::take_pwstr(p)))
    }
}
