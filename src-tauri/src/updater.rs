//! The built-in updater. It reads the signed release catalog (crates/island-catalog) at most once a
//! day, and whenever asked.
//!
//! - Island Shell itself: it downloads the new installer, keeps it only if its size and sha256 match
//!   what the signed catalog promised, runs it with /UPDATE and quits. The installer asks a running
//!   island to quit first and opens the new one when it's done (src-tauri/installer), and the island's
//!   own exit puts the taskbar back, so the taskbar is never left hidden. This one always waits for
//!   the user's click.
//! - Modules: newer versions of installed ones (that this base can run) are downloaded, checked the
//!   same way, and swapped in by modules.rs with the old version kept until the new one is in place.
//!   With daily checks on, the ones set to update by themselves do so in the background.
//! - New modules: `install_module` puts one from the catalog in place (the Modules sheet's Install).
//!
//! The public key is `src-tauri/catalog.pub`, built in. Without one, the updater stays off.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Storage::Streams::{DataReader, IBuffer};
use windows::Web::Http::Filters::{HttpBaseProtocolFilter, HttpCacheReadBehavior};
use windows::Web::Http::{HttpClient, HttpCompletionOption};
use windows_future::AsyncOperationProgressHandler;

use crate::util::{self, log_line};

const KEY: &str = include_str!("../catalog.pub");
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const UA: &str = concat!("IslandShell/", env!("CARGO_PKG_VERSION"));
const DAY_MS: u64 = 24 * 3600 * 1000;
/// The biggest installer, and module package, the updater will download.
const MAX_INSTALLER: u64 = 200 << 20;
const MAX_MODULE: u64 = island_host::package::MAX_UNPACKED;

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub current: String,
    /// "off" (no key in this build), "idle", "checking", "available", "downloading", "installing" or "error".
    pub state: String,
    /// The newest version the catalog offers, once checked.
    pub latest: String,
    pub notes: String,
    pub size: u64,
    /// 0..1 while downloading.
    pub progress: f64,
    /// When the catalog was last read (ms since 1970), 0 if never.
    pub checked: u64,
    pub error: String,
    /// Check once a day on its own (and let modules update themselves).
    pub auto: bool,
    /// Newer versions of installed modules, from the last check.
    pub modules: Vec<ModuleUpdate>,
    /// The module being updated or installed right now ("" when none).
    pub module_busy: String,
}

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct ModuleUpdate {
    pub id: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default = "yes")]
    auto: bool,
    #[serde(default)]
    checked: u64,
}

fn yes() -> bool {
    true
}

struct State {
    status: Status,
    base: Option<island_catalog::Base>,
    /// The last catalog read, for module updates and installs.
    catalog: Option<island_catalog::Catalog>,
}

/// A module update or install is under way (one at a time).
static MODULES_BUSY: AtomicBool = AtomicBool::new(false);

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    let saved = load(&util::data_dir());
    let state = if island_catalog::public_key(KEY).is_ok() { "idle" } else { "off" };
    Mutex::new(State {
        status: Status { current: CURRENT.into(), state: state.into(), checked: saved.checked, auto: saved.auto, ..Default::default() },
        base: None,
        catalog: None,
    })
});

fn load(dir: &Path) -> Saved {
    std::fs::read(dir.join("updater.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Saved { auto: true, checked: 0 })
}

fn save(dir: &Path, st: &Status) {
    if let Ok(b) = serde_json::to_vec_pretty(&Saved { auto: st.auto, checked: st.checked }) {
        let _ = std::fs::write(dir.join("updater.json"), b);
    }
}

pub fn status() -> Status {
    STATE.lock().unwrap().status.clone()
}

fn set(app: &AppHandle, f: impl FnOnce(&mut Status)) -> Status {
    let st = {
        let mut s = STATE.lock().unwrap();
        f(&mut s.status);
        s.status.clone()
    };
    let _ = app.emit("update:status", &st);
    st
}

pub fn set_auto(app: &AppHandle, on: bool) -> Status {
    let st = set(app, |s| s.auto = on);
    save(&util::data_dir(), &st);
    st
}

/// Where downloaded installers wait to run (cleared on the next start).
fn updates_dir() -> PathBuf {
    util::local_dir().join("updates")
}

/// Checks once a day (the first check a minute after starting), while automatic checks are on.
pub fn spawn(app: AppHandle) {
    let _ = std::fs::remove_dir_all(updates_dir());
    if status().state == "off" {
        log_line("updater: no key in this build, so it stays off");
        return;
    }
    std::thread::spawn(move || {
        util::com_mta();
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let st = status();
            if st.auto && util::now_ms().saturating_sub(st.checked) > DAY_MS && matches!(st.state.as_str(), "idle" | "error") {
                let _ = check(&app);
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

/// Reads the catalog now. The page decides what to tell the user.
pub fn check(app: &AppHandle) -> Result<Status, String> {
    util::com_mta();
    let key = island_catalog::public_key(KEY).map_err(|e| e.to_string())?;
    {
        let s = STATE.lock().unwrap();
        if matches!(s.status.state.as_str(), "checking" | "downloading" | "installing") {
            return Ok(s.status.clone());
        }
    }
    set(app, |s| {
        s.state = "checking".into();
        s.error.clear();
    });
    let result = (|| {
        let data = get(island_catalog::CATALOG_URL, island_catalog::MAX_CATALOG_BYTES as u64, |_| {})?;
        let sig = get(island_catalog::SIGNATURE_URL, 64 << 10, |_| {})?;
        let sig = String::from_utf8(sig).map_err(|_| "The catalog's signature isn't text".to_string())?;
        island_catalog::parse_verified(&data, &sig, &key).map_err(|e| e.to_string())
    })();
    let st = match result {
        Ok(catalog) => {
            let base = catalog.base.clone().filter(|b| island_catalog::newer(&b.version, CURRENT));
            let modules = module_updates(&catalog, &crate::modules::installed_entries());
            {
                let mut s = STATE.lock().unwrap();
                s.base = base.clone();
                s.catalog = Some(catalog);
            }
            let st = set(app, |s| {
                s.checked = util::now_ms();
                s.modules = modules.clone();
                match &base {
                    Some(b) => {
                        s.state = "available".into();
                        s.latest = b.version.clone();
                        s.notes = b.notes.clone();
                        s.size = b.size;
                    }
                    None => {
                        s.state = "idle".into();
                        s.latest = CURRENT.into();
                        s.notes.clear();
                        s.size = 0;
                    }
                }
            });
            save(&util::data_dir(), &st);
            log_line(&format!(
                "updater: checked, {}{}",
                if base.is_some() { format!("{} is available", st.latest) } else { "up to date".into() },
                if modules.is_empty() { String::new() } else { format!("; module updates: {}", modules.iter().map(|m| format!("{} {}", m.id, m.to)).collect::<Vec<_>>().join(", ")) }
            ));
            // Modules set to update by themselves do, in the background, while daily checks are on.
            if st.auto && !modules.is_empty() {
                let app = app.clone();
                std::thread::spawn(move || {
                    util::com_mta();
                    let _ = update_modules(&app, true);
                });
            }
            st
        }
        Err(e) => {
            log_line(&format!("updater: check failed: {e}"));
            set(app, |s| {
                s.state = "error".into();
                s.error = e.clone();
            })
        }
    };
    if st.state == "error" { Err(st.error) } else { Ok(st) }
}

/// Installed modules (not developer-mode ones) the catalog has a newer version of, that this base
/// can run.
fn module_updates(catalog: &island_catalog::Catalog, installed: &std::collections::BTreeMap<String, island_host::installed::Entry>) -> Vec<ModuleUpdate> {
    let api = island_host::install::supported_api();
    catalog
        .modules
        .iter()
        .filter_map(|m| {
            let e = installed.get(&m.id).filter(|e| !e.dev)?;
            (island_catalog::newer(&m.version, &e.version) && api.contains(&m.api)).then(|| ModuleUpdate { id: m.id.clone(), name: m.name.clone(), from: e.version.clone(), to: m.version.clone(), size: m.size })
        })
        .collect()
}

/// Updates installed modules from the last catalog: all of them, or (`only_auto`) the ones set to
/// update by themselves. One at a time; each says how it went (`update:module`). Returns the ids
/// updated.
pub fn update_modules(app: &AppHandle, only_auto: bool) -> Result<Vec<String>, String> {
    util::com_mta();
    let installed = crate::modules::installed_entries();
    let todo: Vec<ModuleUpdate> = status().modules.into_iter().filter(|m| !only_auto || installed.get(&m.id).is_some_and(|e| e.auto_update)).collect();
    let mut done = vec![];
    for u in todo {
        match fetch_and_install(app, &u.id) {
            Ok(()) => {
                done.push(u.id.clone());
                let _ = app.emit("update:module", serde_json::json!({ "id": u.id, "name": u.name, "version": u.to, "ok": true }));
            }
            Err(e) => {
                log_line(&format!("updater: {} {} failed: {e}", u.id, u.to));
                let _ = app.emit("update:module", serde_json::json!({ "id": u.id, "name": u.name, "version": u.to, "ok": false, "error": e }));
            }
        }
    }
    // What's left to update now.
    let left = STATE.lock().unwrap().catalog.as_ref().map(|c| module_updates(c, &crate::modules::installed_entries())).unwrap_or_default();
    set(app, |s| s.modules = left);
    Ok(done)
}

/// Installs a module from the catalog (reading it first if this session hasn't yet).
pub fn install_module(app: &AppHandle, id: &str) -> Result<(), String> {
    if STATE.lock().unwrap().catalog.is_none() {
        check(app)?;
    }
    fetch_and_install(app, id)
}

/// The modules the last catalog offers (empty until it's been read).
pub fn catalog_modules() -> Vec<island_catalog::Module> {
    STATE.lock().unwrap().catalog.as_ref().map(|c| c.modules.clone()).unwrap_or_default()
}

/// Downloads a module's package, keeps it only if it's what the signed catalog promised, and has
/// modules.rs swap it in.
fn fetch_and_install(app: &AppHandle, id: &str) -> Result<(), String> {
    let m = STATE.lock().unwrap().catalog.as_ref().and_then(|c| c.modules.iter().find(|m| m.id == id).cloned()).ok_or_else(|| format!("{id} isn't in the catalog"))?;
    if !island_host::install::supported_api().contains(&m.api) {
        return Err(format!("{} {} needs a newer Island Shell", m.name, m.version));
    }
    if m.size > MAX_MODULE {
        return Err(format!("{} is bigger than a module may be, so it wasn't downloaded", m.name));
    }
    if MODULES_BUSY.swap(true, Ordering::SeqCst) {
        return Err("Another module is being installed; try again in a moment".into());
    }
    set(app, |s| s.module_busy = m.name.clone());
    let result = (|| {
        let zip = get(&m.url, m.size, |_| {})?;
        island_catalog::check_download(&zip, m.size, &m.sha256).map_err(|e| e.to_string())?;
        crate::modules::install_package(app, &zip, &m)
    })();
    MODULES_BUSY.store(false, Ordering::SeqCst);
    set(app, |s| s.module_busy.clear());
    result
}

/// Downloads and checks the new installer, starts it, and quits.
pub fn install(app: &AppHandle) -> Result<(), String> {
    util::com_mta();
    let Some(base) = STATE.lock().unwrap().base.clone() else {
        return Err("There's no update to install. Check for updates first.".into());
    };
    if base.size > MAX_INSTALLER {
        return Err("The update is bigger than expected, so it wasn't downloaded".into());
    }
    {
        let mut s = STATE.lock().unwrap();
        if s.status.state != "available" && s.status.state != "error" {
            return Err("An update is already being installed".into());
        }
        s.status.state = "downloading".into();
        s.status.progress = 0.0;
        s.status.error.clear();
    }
    let _ = app.emit("update:status", status());
    let result = (|| {
        let progress_app = app.clone();
        let total = base.size.max(1) as f64;
        let mut last = 0.0;
        let data = get(&base.url, base.size, move |got| {
            let p = (got as f64 / total).min(1.0);
            if p - last >= 0.02 {
                last = p;
                set(&progress_app, |s| s.progress = p);
            }
        })?;
        island_catalog::check_download(&data, base.size, &base.sha256).map_err(|e| e.to_string())?;
        let dir = updates_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("IslandShellSetup-{}.exe", base.version));
        std::fs::write(&path, &data).map_err(|e| format!("Couldn't save the update: {e}"))?;
        drop(data);
        run_installer(&path)
    })();
    match result {
        Ok(()) => {
            log_line(&format!("updater: installing {}", base.version));
            set(app, |s| {
                s.state = "installing".into();
                s.progress = 1.0;
            });
            // Let the page show "Installing…", then quit the normal way (which puts the taskbar back).
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(600));
                app.exit(0);
            });
            Ok(())
        }
        Err(e) => {
            log_line(&format!("updater: install failed: {e}"));
            set(app, |s| {
                s.state = "error".into();
                s.error = e.clone();
            });
            Err(e)
        }
    }
}

/// /UPDATE keeps the one-click install from adding a desktop shortcut again; the installer still shows
/// its small progress window and opens the island when it's done.
fn run_installer(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(path)
        .arg("/UPDATE")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the installer: {e}"))
}

/// A plain GET of at most `max` bytes, with progress in bytes received.
fn get(url: &str, max: u64, mut progress: impl FnMut(u64) + Send + 'static) -> Result<Vec<u8>, String> {
    let e = |e: windows::core::Error| format!("Couldn't reach GitHub: {}", e.message());
    let filter = HttpBaseProtocolFilter::new().map_err(e)?;
    let _ = filter.CacheControl().and_then(|c| c.SetReadBehavior(HttpCacheReadBehavior::NoCache));
    let client = HttpClient::Create(&filter).map_err(e)?;
    let _ = client.DefaultRequestHeaders().and_then(|h| h.UserAgent()).and_then(|ua| ua.TryParseAdd(&HSTRING::from(UA)));
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(e)?;
    let resp = client.GetWithOptionAsync(&uri, HttpCompletionOption::ResponseHeadersRead).and_then(|op| op.get()).map_err(e)?;
    let code = resp.StatusCode().map_err(e)?.0;
    if code == 404 {
        return Err("No release has update information yet".into());
    }
    if !(200..300).contains(&code) {
        return Err(format!("GitHub answered HTTP {code}"));
    }
    let content = resp.Content().map_err(e)?;
    if let Ok(len) = content.Headers().and_then(|h| h.ContentLength()).and_then(|r| r.Value()) {
        if len > max {
            return Err("The download is bigger than expected, so it was stopped".into());
        }
    }
    let op = content.ReadAsBufferAsync().map_err(e)?;
    let _ = op.SetProgress(&AsyncOperationProgressHandler::<IBuffer, u64>::new(move |_, got| {
        progress(*got);
        Ok(())
    }));
    let buf = op.get().map_err(e)?;
    let n = buf.Length().map_err(e)? as u64;
    if n > max {
        return Err("The download is bigger than expected, so it was thrown away".into());
    }
    let reader = DataReader::FromBuffer(&buf).map_err(e)?;
    let mut out = vec![0u8; n as usize];
    reader.ReadBytes(&mut out).map_err(e)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use island_host::installed::Entry;

    #[test]
    fn module_updates_need_a_newer_runnable_version() {
        let m = |id: &str, version: &str, api: u32| island_catalog::Module { id: id.into(), name: id.into(), version: version.into(), api, size: 10, url: "https://example.com/x.zip".into(), sha256: "0".repeat(64), ..Default::default() };
        let catalog = island_catalog::Catalog { schema: 1, modules: vec![m("destiny", "1.1.0", 1), m("canvas", "1.0.0", 1), m("games", "2.0.0", 99), m("vpn", "1.2.0", 1), m("music", "1.0.0", 1)], ..Default::default() };
        let e = |version: &str, dev: bool| Entry { version: version.into(), enabled: true, auto_update: true, dev, installed: String::new(), source: String::new() };
        let installed = [("destiny", e("1.0.0", false)), ("canvas", e("1.0.0", false)), ("games", e("1.0.0", false)), ("vpn", e("1.0.0", true))].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        // Destiny is newer; Canvas is the same; Games needs a newer base; VPN is a developer folder;
        // Music isn't installed.
        let u = module_updates(&catalog, &installed);
        assert_eq!(u, vec![ModuleUpdate { id: "destiny".into(), name: "destiny".into(), from: "1.0.0".into(), to: "1.1.0".into(), size: 10 }]);
    }
}
