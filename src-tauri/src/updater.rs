//! The built-in updater. It reads the signed release catalog (crates/island-catalog) at most once a
//! day, and whenever asked. To update, it downloads the new installer, keeps it only if its size and
//! sha256 match what the signed catalog promised, runs it with /UPDATE and quits. The installer asks a
//! running island to quit first and opens the new one when it's done (src-tauri/installer), and the
//! island's own exit puts the taskbar back, so the taskbar is never left hidden.
//!
//! The public key is `src-tauri/catalog.pub`, built in. Without one, the updater stays off.

use std::path::{Path, PathBuf};
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
/// The biggest installer the updater will download.
const MAX_INSTALLER: u64 = 200 << 20;

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
    /// Check once a day on its own.
    pub auto: bool,
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
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    let saved = load(&util::data_dir());
    let state = if island_catalog::public_key(KEY).is_ok() { "idle" } else { "off" };
    Mutex::new(State {
        status: Status { current: CURRENT.into(), state: state.into(), checked: saved.checked, auto: saved.auto, ..Default::default() },
        base: None,
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
            let base = catalog.base.filter(|b| island_catalog::newer(&b.version, CURRENT));
            STATE.lock().unwrap().base = base.clone();
            let st = set(app, |s| {
                s.checked = util::now_ms();
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
            log_line(&format!("updater: checked, {}", if base.is_some() { format!("{} is available", st.latest) } else { "up to date".into() }));
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

/// Downloads and checks the new installer, starts it, and quits.
pub fn install(app: &AppHandle) -> Result<(), String> {
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
