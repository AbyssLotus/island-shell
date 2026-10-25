#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applemusic;
mod apps;
mod audio;
mod bar;
mod bench;
mod calendar;
mod canvas;
mod clipboard;
mod conns;
mod destiny;
mod diskscan;
mod discord;
mod embed;
mod games;
mod ics;
mod gaze;
mod icons;
mod installed;
mod island;
mod lyrics;
mod media;
mod modules;
mod music;
mod places;
mod power;
mod radios;
mod running;
mod sensors;
mod services;
mod stats;
mod sysinfo;
mod taskbar;
mod taskmgr;
mod tray;
mod updater;
mod util;
mod voicemeeter;
mod vpn;
mod wallpaper;
mod winkey;
mod wpe;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewWindow, WindowEvent, Wry};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub struct Shared {
    pub dir: PathBuf,
    pub status: Mutex<Status>,
    pub stats: Mutex<stats::Stats>,
    pub stats_wanted: AtomicU64,
    pub clips: Mutex<clipboard::History>,
    pub apps: Mutex<Vec<apps::AppEntry>>,
    pub geom: Mutex<island::Geom>,
    pub place: Mutex<island::Placement>,
    pub fullscreen_hidden: AtomicBool,
    pub taskbar_hidden: AtomicBool,
    pub tray_taskbar: Mutex<Option<CheckMenuItem<Wry>>>,
    pub prev_fg: AtomicIsize,
    /// The island window's HWND, once it exists.
    pub island_hwnd: AtomicIsize,
    /// True while the panel or a notification is showing, so the island sits above apps.
    pub raised: AtomicBool,
    /// Top-bar style instead of the floating island.
    pub bar_mode: AtomicBool,
    /// The taller bar ("tall" style).
    pub bar_tall: AtomicBool,
    /// Media session the user picked in the Music tab (app id); empty = choose automatically.
    pub media_source: Mutex<String>,
    /// Process image name of the player being shown, for its volume and level meter.
    pub media_exe: Mutex<String>,
    pub media_playing: AtomicBool,
    /// A notification that must be seen is showing: the island is above everything, fullscreen apps too.
    pub urgent: AtomicBool,
    /// The page is showing more than the resting pill (panel, preview, notification).
    pub active: AtomicBool,
    /// When the island last went back to rest (ms since start).
    pub idle_since: AtomicU64,
    /// WebView2 is in its low-memory mode.
    pub mem_low: AtomicBool,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub media: Option<media::MediaInfo>,
    pub audio: Option<audio::AudioInfo>,
    pub wifi: radios::RadioInfo,
    pub bluetooth: radios::RadioInfo,
    pub net: radios::NetInfo,
    pub taskbar_hidden: bool,
    pub battery: bar::Battery,
    pub desktops: bar::Desktops,
    pub cpu: f64,
    /// The CPU chip's temperature, when Windows can read it (see sensors.rs).
    pub cpu_temp: Option<f64>,
}

#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default = "default_true")]
    hide_taskbar: bool,
    /// Collect tray icons by acting as a second notification-area host (see tray.rs).
    #[serde(default = "default_true")]
    tray_host: bool,
    /// "island", "bar" or "tall" (a taller bar).
    #[serde(default = "default_style")]
    style: String,
    /// See `Shared::media_source`.
    #[serde(default)]
    media_source: String,
    /// Eye-tracking features (only does anything with a Tobii tracker).
    #[serde(default = "default_true")]
    gaze: bool,
    /// Pause the music after stepping away from the tracker; resume on return.
    #[serde(default)]
    gaze_pause: bool,
}

fn default_true() -> bool {
    true
}

fn default_style() -> String {
    "island".into()
}

fn is_bar_style(style: &str) -> bool {
    style == "bar" || style == "tall"
}

fn current_style(shared: &Shared) -> &'static str {
    match (shared.bar_mode.load(Ordering::Relaxed), shared.bar_tall.load(Ordering::Relaxed)) {
        (false, _) => "island",
        (true, false) => "bar",
        (true, true) => "tall",
    }
}

impl Settings {
    fn load(shared_dir: &std::path::Path) -> Self {
        std::fs::read(shared_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Settings { hide_taskbar: true, tray_host: true, style: default_style(), media_source: String::new(), gaze: true, gaze_pause: false })
    }
    fn save(&self, dir: &std::path::Path) {
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(dir.join("settings.json"), b);
        }
    }
}

type Shr<'a> = State<'a, Arc<Shared>>;

/// Process start, for cheap monotonic timestamps shared between threads.
static STARTED: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        util::com_mta();
        f()
    })
    .await
    .map_err(|e| e.to_string())
}

fn apply_taskbar(shared: &Shared, hide: bool) {
    if hide {
        taskbar::hide(&shared.dir);
    } else {
        taskbar::restore(&shared.dir);
    }
    shared.taskbar_hidden.store(hide, Ordering::Relaxed);
    shared.status.lock().unwrap().taskbar_hidden = hide;
    let mut settings = Settings::load(&shared.dir);
    settings.hide_taskbar = hide;
    settings.save(&shared.dir);
    if let Some(item) = &*shared.tray_taskbar.lock().unwrap() {
        let _ = item.set_checked(hide);
    }
}

// ---------- commands ----------

#[tauri::command]
fn get_status(shared: Shr) -> Status {
    let mut s = shared.status.lock().unwrap().clone();
    let st = shared.stats.lock().unwrap();
    s.cpu = st.cpu;
    s.cpu_temp = st.sensors.cpu_temp;
    drop(st);
    s
}

/// Switches between the floating island and the full-width top bar.
fn apply_style(app: &AppHandle, shared: &Shared, style: &str) {
    let bar = is_bar_style(style);
    shared.bar_mode.store(bar, Ordering::Relaxed);
    shared.bar_tall.store(style == "tall", Ordering::Relaxed);
    let Some(win) = app.get_webview_window("island") else { return };
    let p = island::place(&win, bar);
    *shared.place.lock().unwrap() = p;
    let hwnd = shared.island_hwnd.load(Ordering::Relaxed);
    util::log_line(&format!("style {style}: window {}x{} at {},{}", p.monitor.right - p.monitor.left, island::HEIGHT, p.x, p.y));
    if bar {
        // Like the taskbar: always in front, with its strip of the screen reserved.
        if let Err(e) = win.set_always_on_top(true) {
            util::log_line(&format!("set_always_on_top failed: {e}"));
        }
        let height = (bar::height(style == "tall") * p.scale).round() as i32;
        std::thread::spawn(move || {
            let h = windows::Win32::Foundation::HWND(hwnd as _);
            bar::appbar_register(h, p.monitor, height);
            std::thread::sleep(Duration::from_millis(600));
            bar::log_state(h);
        });
    } else {
        std::thread::spawn(move || {
            let h = windows::Win32::Foundation::HWND(hwnd as _);
            bar::appbar_unregister(h);
            std::thread::sleep(Duration::from_millis(600));
            bar::log_state(h);
        });
        let _ = win.set_always_on_top(false);
        let h = windows::Win32::Foundation::HWND(hwnd as _);
        if !h.is_invalid() {
            island::sink(h);
        }
    }
}

#[tauri::command]
fn get_style(shared: Shr) -> String {
    current_style(&shared).into()
}

/// Saves the new layout and restarts the shell in it. Switching in place isn't reliable: once the
/// window has lived on the desktop layer (island style), Windows refuses to make it topmost again
/// and Explorer won't reserve the bar's strip, while a fresh start in bar style does both.
fn switch_style(app: &AppHandle, shared: &Shared, style: &str) -> Result<(), String> {
    if style != "island" && !is_bar_style(style) {
        return Err("Unknown style".into());
    }
    let mut settings = Settings::load(&shared.dir);
    settings.style = style.to_string();
    settings.save(&shared.dir);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).arg("--after").arg(std::process::id().to_string()).spawn().map_err(|e| e.to_string())?;
    util::log_line(&format!("restarting in {style} style"));
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn set_style(style: String, app: AppHandle, shared: Shr) -> Result<(), String> {
    switch_style(&app, &shared, &style)
}

#[tauri::command]
async fn desktop_switch(index: usize) -> Result<(), String> {
    blocking(move || bar::switch_desktop(index)).await?
}

#[tauri::command]
fn desktop_new() {
    bar::new_desktop();
}

#[tauri::command]
async fn media_thumb(shared: Shr<'_>) -> Result<tauri::ipc::Response, String> {
    let preferred = shared.media_source.lock().unwrap().clone();
    // Raw bytes (an ArrayBuffer in the page; empty when there's no artwork).
    let bytes = blocking(move || media::manager().and_then(|m| media::thumbnail(&m, &preferred))).await?;
    Ok(tauri::ipc::Response::new(bytes.unwrap_or_default()))
}

/// toggle, next, prev, seek (`value` = seconds), shuffle, repeat, favorite, open.
#[tauri::command]
async fn media_cmd(action: String, value: Option<f64>, shared: Shr<'_>) -> Result<(), String> {
    let preferred = shared.media_source.lock().unwrap().clone();
    blocking(move || {
        let m = media::manager().ok_or("Nothing is playing")?;
        media::command(&m, &preferred, &action, value.unwrap_or(0.0))
    })
    .await?
}

/// Shows this media session instead of choosing automatically ("" = automatic again).
#[tauri::command]
fn media_source(id: String, shared: Shr) {
    *shared.media_source.lock().unwrap() = id.clone();
    let mut settings = Settings::load(&shared.dir);
    settings.media_source = id;
    settings.save(&shared.dir);
}

/// Sets the volume of the player being shown (not the whole PC).
#[tauri::command]
async fn media_app_volume(level: f32, shared: Shr<'_>) -> Result<(), String> {
    let exe = shared.media_exe.lock().unwrap().clone();
    blocking(move || audio::set_app_volume(&exe, level)).await?
}

#[tauri::command]
async fn media_lyrics(title: String, artist: String, album: String, duration: f64, shared: Shr<'_>) -> Result<Option<lyrics::Lyrics>, String> {
    let dir = shared.dir.clone();
    blocking(move || lyrics::fetch(&dir, &title, &artist, &album, duration)).await?
}

/// Installed Steam games, most recently played first.
#[tauri::command]
async fn games_list(app: AppHandle) -> Result<games::Library, String> {
    let lib = blocking(games::library).await?;
    // The covers live in Steam's folders (custom artwork and its cache); let the page load them.
    for dir in &lib.art_dirs {
        if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
            util::log_line(&format!("games: couldn't allow {} ({e})", dir.display()));
        }
    }
    Ok(lib)
}

/// play | library | folder
#[tauri::command]
async fn game_action(appid: u32, what: String) -> Result<(), String> {
    blocking(move || games::action(appid, &what)).await?
}

#[tauri::command]
fn gaze_status() -> gaze::Status {
    gaze::status()
}

/// Turns eye tracking on/off and/or the pause-when-away option.
#[tauri::command]
fn gaze_set(enabled: Option<bool>, pause_when_away: Option<bool>, app: AppHandle, shared: Shr) -> gaze::Status {
    let mut settings = Settings::load(&shared.dir);
    {
        let mut st = gaze::STATUS.lock().unwrap();
        if let Some(on) = enabled {
            st.enabled = on;
            settings.gaze = on;
        }
        if let Some(on) = pause_when_away {
            st.pause_when_away = on;
            settings.gaze_pause = on;
        }
    }
    settings.save(&shared.dir);
    let st = gaze::status();
    let _ = app.emit("gaze:status", st.clone());
    st
}

#[tauri::command]
async fn vpn_status() -> Result<vpn::Status, String> {
    blocking(vpn::status).await
}

/// up | down | exit (arg = exit node IP, "" for none) | open
#[tauri::command]
async fn vpn_tailscale(action: String, arg: Option<String>) -> Result<(), String> {
    blocking(move || vpn::tailscale_cmd(&action, arg.as_deref().unwrap_or(""))).await?
}

#[tauri::command]
fn vpn_radmin_open() -> Result<(), String> {
    vpn::radmin_open()
}

#[tauri::command]
async fn copy_text(text: String) -> Result<(), String> {
    blocking(move || arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())).await?
}

/// Wallpaper Engine's wallpapers; missing thumbnails are made in the background (`we:thumb`).
#[tauri::command]
async fn we_library(app: AppHandle, shared: Shr<'_>) -> Result<wpe::Library, String> {
    let dir = shared.dir.clone();
    let lib = {
        let dir = dir.clone();
        blocking(move || wpe::library(&dir)).await?
    };
    for d in &lib.dirs {
        let _ = app.asset_protocol_scope().allow_directory(d, true);
    }
    wpe::make_thumbs(app, &lib, dir);
    Ok(lib)
}

#[tauri::command]
async fn we_apply(dir: String) -> Result<(), String> {
    blocking(move || wpe::apply(&dir)).await?
}

/// pause | play | mute | unmute | stop | next
#[tauri::command]
async fn we_control(cmd: String) -> Result<(), String> {
    blocking(move || wpe::control(&cmd)).await?
}

/// Discord: voice call, notifications and Rich Presence (see discord.rs).
#[tauri::command]
fn discord_status() -> discord::Status {
    discord::status()
}

/// Saves the user's Discord application (the secret is kept only if one is given).
#[tauri::command]
fn discord_configure(client_id: String, secret: String) -> Result<(), String> {
    if !client_id.trim().chars().all(|c| c.is_ascii_digit()) {
        return Err("The Client ID is a long number (the Application ID in the developer portal)".into());
    }
    discord::send(discord::Cmd::Configure { client_id, secret })
}

/// authorize | mute | unmute | deafen | undeafen | leave | forget
#[tauri::command]
fn discord_action(action: String) -> Result<(), String> {
    use discord::Cmd;
    discord::send(match action.as_str() {
        "authorize" => Cmd::Authorize,
        "mute" => Cmd::Mute(true),
        "unmute" => Cmd::Mute(false),
        "deafen" => Cmd::Deaf(true),
        "undeafen" => Cmd::Deaf(false),
        "leave" => Cmd::Leave,
        "forget" => Cmd::Forget,
        _ => return Err(format!("unknown Discord action {action}")),
    })
}

#[tauri::command]
fn discord_options(presence: Option<bool>, notify: Option<bool>) -> Result<(), String> {
    discord::send(discord::Cmd::Options { presence, notify })
}

/// portal | app
#[tauri::command]
fn discord_open(what: String) -> Result<(), String> {
    discord::open(&what)
}

/// The full-screen Command Center: the island's window covers the whole monitor while it's open,
/// then goes back to its usual place. (Staying on top and catching clicks come from island_rect.)
#[tauri::command]
fn cc_mode(on: bool, app: AppHandle, shared: Shr) {
    let Some(win) = app.get_webview_window("island") else { return };
    if on {
        let m = shared.place.lock().unwrap().monitor;
        let _ = win.set_size(tauri::PhysicalSize::new((m.right - m.left) as u32, (m.bottom - m.top) as u32));
        let _ = win.set_position(tauri::PhysicalPosition::new(m.left, m.top));
        let _ = win.set_focus();
    } else {
        embed::hide_all(&app);
        *shared.place.lock().unwrap() = island::place(&win, shared.bar_mode.load(Ordering::Relaxed));
    }
}

/// Drives, the user's folders, and recently opened files and folders.
#[tauri::command]
async fn cc_places() -> Result<Vec<places::Place>, String> {
    blocking(places::list).await
}

/// open | reveal (show in File Explorer)
#[tauri::command]
async fn cc_open(path: String, reveal: Option<bool>) -> Result<(), String> {
    blocking(move || if reveal.unwrap_or(false) { places::reveal(&path) } else { places::open(&path) }).await?
}

/// Searches the web in the default browser (never Edge), for queries nothing on the PC matched.
#[tauri::command]
fn cc_web(query: String) -> Result<(), String> {
    util::open_web(&format!("https://www.google.com/search?q={}", util::url_encode(query.trim())))
}

/// Docks a website (discord | canvas | dim) over the given CSS-pixel rectangle of the Command Center.
/// `skin` (Discord only, default on) dresses Discord in the island skin.
#[tauri::command]
async fn cc_embed(site: String, x: f64, y: f64, w: f64, h: f64, skin: Option<bool>, app: AppHandle, shared: Shr<'_>) -> Result<(), String> {
    let p = *shared.place.lock().unwrap();
    embed::show(&app, &site, x, y, w, h, p.scale, (p.monitor.left, p.monitor.top), skin.unwrap_or(true))
}

/// Hides docked websites (leaving a section, or closing the Command Center).
#[tauri::command]
fn cc_embed_hide(app: AppHandle) {
    embed::hide_all(&app);
}

/// taskmgr | terminal | explorer | recycle | control | snip
#[tauri::command]
fn cc_tool(name: String) -> Result<(), String> {
    places::tool(&name)
}

/// The Command Center calendar (see calendar.rs): events overlapping [from, to) (UTC ms).
#[tauri::command]
async fn calendar_events(from: i64, to: i64, shared: Shr<'_>) -> Result<Vec<calendar::CalEvent>, String> {
    let dir = shared.dir.clone();
    blocking(move || calendar::events(&dir, from, to)).await
}

#[tauri::command]
fn calendar_sources(shared: Shr) -> Vec<calendar::Source> {
    calendar::sources(&shared.dir)
}

/// Adds a calendar from its iCal link (Google's secret address, iCloud's public link, Outlook's ICS).
#[tauri::command]
async fn calendar_add_feed(name: String, url: String, shared: Shr<'_>) -> Result<Vec<calendar::Source>, String> {
    let dir = shared.dir.clone();
    blocking(move || calendar::add_feed(&dir, &name, &url)).await?
}

#[tauri::command]
fn calendar_remove_feed(id: String, shared: Shr) -> Vec<calendar::Source> {
    calendar::remove_feed(&shared.dir, &id)
}

#[tauri::command]
fn calendar_set_source(id: String, on: Option<bool>, color: Option<String>, shared: Shr) -> Vec<calendar::Source> {
    calendar::set_source(&shared.dir, &id, on, color)
}

#[tauri::command]
fn calendar_save(event: calendar::Local, shared: Shr) -> Result<calendar::Local, String> {
    calendar::save_event(&shared.dir, event)
}

#[tauri::command]
fn calendar_delete(id: String, shared: Shr) {
    calendar::delete_event(&shared.dir, &id);
}

/// A web link in the default browser (never Edge): a calendar event's, a song's Apple Music page.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Not a web link".into());
    }
    util::open_web(&url)
}

/// The Music sheet's cover, year, genre and Apple Music link for a song (see music.rs).
#[tauri::command]
async fn music_info(title: String, artist: String, album: String) -> Result<music::Info, String> {
    blocking(move || music::info(&title, &artist, &album)).await?
}

/// The Command Center's Task Manager (see taskmgr.rs): the latest sample, None until the first.
#[tauri::command]
fn tm_snapshot() -> Option<taskmgr::Snapshot> {
    taskmgr::snapshot()
}

/// Ends processes, each named by (pid, creation time) so a reused PID is never hit.
#[tauri::command]
async fn tm_end(procs: Vec<(u32, i64)>) -> Result<usize, String> {
    blocking(move || taskmgr::end(&procs)).await?
}

#[tauri::command]
async fn tm_eco(procs: Vec<(u32, i64)>, on: bool) -> Result<(), String> {
    blocking(move || taskmgr::eco(&procs, on)).await?
}

#[tauri::command]
async fn tm_cpu_info() -> Result<sysinfo::CpuInfo, String> {
    blocking(sysinfo::cpu).await
}

#[tauri::command]
async fn tm_system() -> Result<sysinfo::SystemInfo, String> {
    blocking(|| {
        util::com_mta();
        sysinfo::system()
    })
    .await
}

#[tauri::command]
async fn tm_nvme() -> Result<Vec<sysinfo::NvmeHealth>, String> {
    blocking(sysinfo::nvme_health).await
}

#[tauri::command]
async fn tm_services(fresh: Option<bool>) -> Result<Vec<services::Service>, String> {
    blocking(move || services::list(fresh.unwrap_or(false))).await?
}

#[tauri::command]
async fn tm_service(name: String, action: String) -> Result<&'static str, String> {
    blocking(move || services::control(&name, &action)).await?
}

#[tauri::command]
async fn tm_sessions() -> Result<Vec<services::Session>, String> {
    blocking(services::sessions).await
}

#[tauri::command]
async fn tm_installed(shared: Shr<'_>) -> Result<Vec<installed::App>, String> {
    let start: Vec<(String, String)> = shared.apps.lock().unwrap().iter().filter_map(|a| a.icon.clone().map(|i| (a.name.to_lowercase(), i))).collect();
    let mut list = blocking(|| {
        util::com_mta();
        installed::list()
    })
    .await?;
    // Programs whose installer recorded no icon take their Start menu entry's (same name, or the
    // Start entry's name at the start of theirs: "7-Zip 25.01 (x64)" → "7-Zip File Manager" won't,
    // "Steam" → "Steam" will).
    for a in list.iter_mut().filter(|a| a.icon.is_none()) {
        let n = a.name.to_lowercase();
        let hit = start.iter().find(|(s, _)| *s == n).or_else(|| start.iter().filter(|(s, _)| s.len() >= 4 && n.starts_with(s.as_str())).max_by_key(|(s, _)| s.len()));
        if let Some((_, icon)) = hit {
            a.icon = Some(icon.clone());
        }
    }
    Ok(list)
}

#[tauri::command]
async fn tm_installed_action(id: String, what: String) -> Result<(), String> {
    blocking(move || installed::action(&id, &what)).await?
}

#[tauri::command]
async fn tm_conns() -> Result<Vec<conns::Conn>, String> {
    blocking(conns::list).await
}

#[tauri::command]
async fn tm_history() -> Result<taskmgr::History, String> {
    blocking(taskmgr::history).await
}

#[tauri::command]
fn tm_history_clear() {
    taskmgr::history_clear();
}

#[tauri::command]
async fn tm_run(command: String, admin: bool) -> Result<(), String> {
    blocking(move || {
        util::com_sta();
        taskmgr::run(&command, admin)
    })
    .await?
}

#[tauri::command]
fn disk_scan_start(root: String) -> Result<(), String> {
    diskscan::start(&root)
}

#[tauri::command]
fn disk_scan_status() -> diskscan::Status {
    diskscan::status()
}

#[tauri::command]
async fn disk_scan_view(path: String) -> Result<Option<diskscan::View>, String> {
    blocking(move || diskscan::view(&path)).await
}

#[tauri::command]
fn disk_scan_stop(clear: Option<bool>) {
    if clear.unwrap_or(false) {
        diskscan::clear();
    } else {
        diskscan::cancel();
    }
}

#[tauri::command]
fn bench_start(kind: String, drive: Option<String>) -> Result<(), String> {
    bench::start(&kind, drive)
}

#[tauri::command]
fn bench_status() -> bench::Status {
    bench::status()
}

#[tauri::command]
fn bench_cancel() {
    bench::cancel();
}

#[tauri::command]
async fn tm_startup() -> Result<Vec<taskmgr::Startup>, String> {
    blocking(taskmgr::startup).await
}

#[tauri::command]
async fn tm_startup_set(id: String, on: bool) -> Result<Vec<taskmgr::Startup>, String> {
    blocking(move || taskmgr::set_startup(&id, on).map(|_| taskmgr::startup())).await?
}

/// Canvas (school LMS) with the user's own access token (see canvas.rs).
#[tauri::command]
fn canvas_status(shared: Shr) -> canvas::Status {
    canvas::status(&shared.dir)
}

/// Saves the Canvas address and token (kept if left blank) after checking them with Canvas.
#[tauri::command]
async fn canvas_configure(domain: String, token: String, shared: Shr<'_>) -> Result<canvas::Status, String> {
    let dir = shared.dir.clone();
    blocking(move || canvas::configure(&dir, &domain, &token)).await?
}

/// Links Canvas through its calendar feed link instead of a token.
#[tauri::command]
async fn canvas_configure_feed(url: String, shared: Shr<'_>) -> Result<canvas::Status, String> {
    let dir = shared.dir.clone();
    blocking(move || canvas::configure_feed(&dir, &url)).await?
}

#[tauri::command]
async fn canvas_overview(shared: Shr<'_>) -> Result<canvas::Overview, String> {
    let dir = shared.dir.clone();
    blocking(move || canvas::overview(&dir)).await?
}

/// Ticks a planner item off (or back on); returns the planner override's id.
#[tauri::command]
async fn canvas_done(key: String, kind: String, id: u64, override_id: Option<u64>, done: bool, shared: Shr<'_>) -> Result<Option<u64>, String> {
    let dir = shared.dir.clone();
    blocking(move || canvas::set_done(&dir, &key, &kind, id, override_id, done)).await?
}

#[tauri::command]
fn canvas_open(url: String, shared: Shr) -> Result<(), String> {
    canvas::open(&shared.dir, &url)
}

#[tauri::command]
fn canvas_forget(shared: Shr) {
    canvas::forget(&shared.dir);
}

/// Destiny 2 through Bungie's API (see destiny.rs).
#[tauri::command]
fn destiny_status(shared: Shr) -> destiny::Status {
    destiny::status(&shared.dir)
}

/// Saves the API key (kept if left blank) and looks the Bungie Name up.
#[tauri::command]
async fn destiny_configure(key: String, name: String, shared: Shr<'_>) -> Result<destiny::Status, String> {
    let dir = shared.dir.clone();
    blocking(move || destiny::configure(&dir, &key, &name)).await?
}

#[tauri::command]
async fn destiny_profile(shared: Shr<'_>) -> Result<destiny::Profile, String> {
    let dir = shared.dir.clone();
    blocking(move || destiny::profile(&dir)).await?
}

#[tauri::command]
async fn destiny_xur(shared: Shr<'_>) -> Result<destiny::Xur, String> {
    let dir = shared.dir.clone();
    blocking(move || destiny::xur(&dir)).await?
}

#[tauri::command]
fn destiny_forget(shared: Shr) {
    destiny::forget(&shared.dir);
}

/// Bungie's page for API keys, in the default browser (never Edge).
#[tauri::command]
fn destiny_open() -> Result<(), String> {
    util::open_web("https://www.bungie.net/en/Application")
}

/// The Voicemeeter mixer (null while Voicemeeter isn't running or installed).
#[tauri::command]
fn vm_state() -> Option<vd_vm_core::VmState> {
    voicemeeter::state()
}

/// A strip or bus control, e.g. `Strip[2].Gain` or `Bus[0].Mute`.
#[tauri::command]
fn vm_set(name: String, value: f32) -> Result<(), String> {
    voicemeeter::set(&name, value)
}

#[tauri::command]
fn vm_levels(on: bool) -> Result<(), String> {
    voicemeeter::levels(on)
}

#[tauri::command]
fn vm_restart() -> Result<(), String> {
    voicemeeter::restart_engine()
}

/// Toggles the mic (strip 1); returns whether it's now muted.
#[tauri::command]
fn vm_mic() -> Result<bool, String> {
    voicemeeter::toggle_mic()
}

/// Which mixer apps are on this PC: { voicedeck, voicemeeter }.
#[tauri::command]
fn mixer_apps() -> serde_json::Value {
    serde_json::json!({ "voicedeck": voicemeeter::voicedeck_installed(), "voicemeeter": voicemeeter::installed() })
}

#[tauri::command]
fn voicedeck_open() -> Result<(), String> {
    voicemeeter::open_voicedeck()
}

#[tauri::command]
async fn audio_devices() -> Result<audio::Devices, String> {
    blocking(audio::devices).await
}

#[tauri::command]
async fn audio_default(id: String) -> Result<(), String> {
    blocking(move || audio::set_default(&id)).await?
}

#[tauri::command]
async fn audio_mic(level: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    blocking(move || audio::set_mic(level, muted)).await?
}

#[tauri::command]
async fn audio_apps() -> Result<Vec<audio::AppAudio>, String> {
    blocking(audio::apps).await
}

#[tauri::command]
async fn audio_app_set(pid: u32, level: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    blocking(move || audio::set_app(pid, level, muted)).await?
}

#[tauri::command]
async fn set_volume(level: f32) -> Result<(), String> {
    blocking(move || audio::set_level(level).map_err(|e| e.message().to_string())).await?
}

#[tauri::command]
async fn set_mute(muted: bool) -> Result<(), String> {
    blocking(move || audio::set_mute(muted).map_err(|e| e.message().to_string())).await?
}

#[tauri::command]
async fn set_radio(kind: String, on: bool) -> Result<(), String> {
    blocking(move || radios::set_radio(&kind, on)).await?
}

#[tauri::command]
fn get_stats(shared: Shr) -> stats::Stats {
    shared.stats_wanted.store(util::now_ms(), Ordering::Relaxed);
    shared.stats.lock().unwrap().clone()
}

#[tauri::command]
fn clip_list(shared: Shr) -> Vec<clipboard::Clip> {
    shared.clips.lock().unwrap().items.clone()
}

#[tauri::command]
async fn clip_copy(id: u64, shared: Shr<'_>) -> Result<(), String> {
    let item = shared.clips.lock().unwrap().items.iter().find(|c| c.id == id).cloned().ok_or("That item is gone")?;
    blocking(move || clipboard::copy(&item)).await?
}

#[tauri::command]
fn clip_pin(id: u64, shared: Shr) {
    shared.clips.lock().unwrap().pin(id);
}

#[tauri::command]
fn clip_delete(id: u64, shared: Shr) {
    shared.clips.lock().unwrap().delete(id);
}

#[tauri::command]
fn clip_clear(shared: Shr) {
    shared.clips.lock().unwrap().clear_unpinned();
}

#[tauri::command]
fn apps_list(shared: Shr) -> Vec<apps::AppEntry> {
    shared.apps.lock().unwrap().clone()
}

#[tauri::command]
fn app_launch(id: String) -> Result<(), String> {
    apps::launch(&id)
}

#[tauri::command]
async fn app_info(id: String, shared: Shr<'_>) -> Result<apps::AppInfo, String> {
    let dir = shared.dir.join("icons");
    blocking(move || apps::info(&id, &dir)).await?
}

#[tauri::command]
async fn app_action(id: String, what: String) -> Result<(), String> {
    blocking(move || apps::action(&id, &what)).await?
}

#[tauri::command]
async fn wallpaper_apply(request: tauri::ipc::Request<'_>, shared: Shr<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected image bytes".into());
    };
    let name = request.headers().get("x-name").and_then(|v| v.to_str().ok()).unwrap_or("wallpaper").to_string();
    let bytes = bytes.clone();
    let dir = shared.dir.clone();
    tauri::async_runtime::spawn_blocking(move || wallpaper::save_and_set(&dir, &name, &bytes)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn power(action: String, shared: Shr) -> Result<(), String> {
    power::run(&action, &shared.dir)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn island_rect(x: f64, y: f64, w: f64, h: f64, expanded: bool, raised: bool, bar: f64, active: Option<bool>, urgent: Option<bool>, app: AppHandle, shared: Shr) {
    *shared.geom.lock().unwrap() = island::Geom { x, y, w, h, expanded, bar_h: bar };
    // Back to normal memory use the moment the island is used; rest is handled by the status loop.
    let active = active.unwrap_or(expanded || raised);
    if shared.active.swap(active, Ordering::Relaxed) && !active {
        shared.idle_since.store(STARTED.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
    if active && shared.mem_low.swap(false, Ordering::Relaxed) {
        if let Some(win) = app.get_webview_window("island") {
            island::set_memory_low(&win, false);
        }
    }
    // At rest the island lives on the desktop layer, under apps; the panel and notifications come up
    // front, and an important notification goes above everything (the bar handles that in bar::ensure).
    let urgent = urgent.unwrap_or(false);
    let was_urgent = shared.urgent.swap(urgent, Ordering::Relaxed);
    let was = shared.raised.swap(raised, Ordering::Relaxed);
    if (was != raised || was_urgent != urgent) && !shared.bar_mode.load(Ordering::Relaxed) {
        let hwnd = windows::Win32::Foundation::HWND(shared.island_hwnd.load(Ordering::Relaxed) as _);
        if !hwnd.is_invalid() {
            if was_urgent != urgent {
                island::pin(hwnd, urgent);
            }
            if raised {
                island::raise(hwnd);
            } else {
                island::sink(hwnd);
            }
        }
    }
}

#[tauri::command]
async fn windows_list(shared: Shr<'_>) -> Result<Vec<running::RunningWindow>, String> {
    let dir = shared.dir.clone();
    let active = shared.prev_fg.load(Ordering::Relaxed);
    blocking(move || running::list(&dir, active)).await
}

#[tauri::command]
fn window_activate(hwnd: isize) -> Result<(), String> {
    running::activate(hwnd)
}

#[tauri::command]
fn window_minimize(hwnd: isize) {
    running::minimize(hwnd);
}

#[tauri::command]
fn window_close(hwnd: isize) {
    running::close(hwnd);
}

#[tauri::command]
fn tray_list() -> Vec<tray::TrayItem> {
    tray::list()
}

#[tauri::command]
fn tray_click(key: String, button: String) -> Result<(), String> {
    tray::click(&key, &button)
}

#[tauri::command]
fn release_focus(shared: Shr) {
    island::release_focus(&shared);
}

#[tauri::command]
fn set_taskbar(hidden: bool, shared: Shr) {
    apply_taskbar(&shared, hidden);
}

#[tauri::command]
fn open_settings(uri: String) -> Result<(), String> {
    util::open_settings(&uri)
}

#[tauri::command]
fn autostart_get(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn autostart_set(on: bool, app: AppHandle) -> Result<(), String> {
    let m = app.autolaunch();
    if on { m.enable() } else { m.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
fn update_status() -> updater::Status {
    updater::status()
}

#[tauri::command]
async fn update_check(app: AppHandle) -> Result<updater::Status, String> {
    blocking(move || updater::check(&app)).await?
}

#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    blocking(move || updater::install(&app)).await?
}

#[tauri::command]
fn update_auto(on: bool, app: AppHandle) -> updater::Status {
    updater::set_auto(&app, on)
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Page errors and diagnostics, written to island.log in the data folder.
#[tauri::command]
fn log(msg: String) {
    util::log_line(&msg);
}

/// Brings the island window forward and asks the page to toggle the panel, optionally on a tab.
fn summon(app: &AppHandle, tab: Option<&str>) {
    if let Some(w) = app.get_webview_window("island") {
        let _ = w.set_focus();
    }
    let _ = app.emit("island:toggle", tab);
}

// ---------- background work ----------

fn spawn_status(win: WebviewWindow, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        util::com_mta();
        let own = island::hwnd_of(&win);
        let mut mgr = media::manager();
        let mut tick: u64 = 0;
        let (mut wifi, mut bt, mut net) = Default::default();
        let mut battery = bar::Battery::default();
        loop {
            if tick % 10 == 0 {
                battery = bar::battery();
            }
            if mgr.is_none() && tick % 10 == 0 {
                mgr = media::manager();
            }
            let preferred = shared.media_source.lock().unwrap().clone();
            let mut media = mgr.as_ref().and_then(|m| media::snapshot(m, &preferred));
            if let Some(m) = media.as_mut() {
                m.app_volume = audio::app_volume(&m.exe);
            }
            *shared.media_exe.lock().unwrap() = media.as_ref().map(|m| m.exe.clone()).unwrap_or_default();
            shared.media_playing.store(media.as_ref().is_some_and(|m| m.playing), Ordering::Relaxed);
            let audio = audio::info();
            if tick % 3 == 0 {
                (wifi, bt) = radios::radios();
                net = radios::network();
            }
            // Fade out for fullscreen apps, unless the user deliberately opened the panel over one.
            let fullscreen = island::fullscreen_app(own);
            let fade = fullscreen.is_some() && !shared.geom.lock().unwrap().expanded;
            if fade != shared.fullscreen_hidden.load(Ordering::Relaxed) {
                shared.fullscreen_hidden.store(fade, Ordering::Relaxed);
                let _ = win.emit("island:fullscreen", fade);
                modules::publish(island_host::proto::topic::FULLSCREEN, serde_json::json!({ "active": fade }));
                util::log_line(&format!("fullscreen fade {fade} ({})", fullscreen.unwrap_or_default()));
            }
            let hidden = shared.taskbar_hidden.load(Ordering::Relaxed);
            if hidden && tick % 2 == 0 {
                taskbar::enforce();
            }
            // A few seconds after the island goes back to rest, let WebView2 drop its caches.
            let idle_ms = (STARTED.elapsed().as_millis() as u64).saturating_sub(shared.idle_since.load(Ordering::Relaxed));
            if !shared.active.load(Ordering::Relaxed) && idle_ms > 4000 && !shared.mem_low.swap(true, Ordering::Relaxed) {
                island::set_memory_low(&win, true);
            }
            if !shared.active.load(Ordering::Relaxed) {
                island::trim_at_rest(STARTED.elapsed().as_millis() as u64, shared.idle_since.load(Ordering::Relaxed));
            }
            if shared.bar_mode.load(Ordering::Relaxed) {
                let p = *shared.place.lock().unwrap();
                let h = windows::Win32::Foundation::HWND(shared.island_hwnd.load(Ordering::Relaxed) as _);
                let height = bar::height(shared.bar_tall.load(Ordering::Relaxed));
                // Over a fullscreen app the bar steps back, except while an important notification shows.
                bar::ensure(h, p.monitor, (height * p.scale).round() as i32, fade && !shared.urgent.load(Ordering::Relaxed));
            }
            *shared.status.lock().unwrap() = Status {
                media,
                audio,
                wifi: wifi.clone(),
                bluetooth: bt.clone(),
                net: net.clone(),
                taskbar_hidden: hidden,
                battery: battery.clone(),
                desktops: bar::desktops(),
                cpu: 0.0,
                cpu_temp: None,
            };
            tick += 1;
            std::thread::sleep(Duration::from_millis(1000));
        }
    });
}

/// Streams the playing app's output level (~30 times a second) so the equalizer bars move with
/// the music. Silent while nothing plays or the island is faded out for a fullscreen app.
fn spawn_meter(win: WebviewWindow, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        util::com_mta();
        let mut meter: Option<audio::Meter> = None;
        let mut exe = String::new();
        let mut found_at = std::time::Instant::now();
        let mut logged = String::new();
        loop {
            let want = shared.media_exe.lock().unwrap().clone();
            let live = !want.is_empty() && shared.media_playing.load(Ordering::Relaxed) && !shared.fullscreen_hidden.load(Ordering::Relaxed);
            if !live {
                meter = None;
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            // Look the sessions up again now and then: players open new ones on device changes.
            if meter.is_none() || want != exe || found_at.elapsed() > Duration::from_secs(3) {
                meter = audio::Meter::find(&want);
                exe = want;
                found_at = std::time::Instant::now();
                let note = format!("{exe}: {} session(s)", meter.as_ref().map_or(0, |m| m.sessions()));
                if note != logged {
                    util::log_line(&format!("level meter {note}"));
                    logged = note;
                }
            }
            if let Some(m) = &meter {
                let _ = win.emit("media:level", m.peak());
            }
            std::thread::sleep(Duration::from_millis(33));
        }
    });
}

fn build_tray(app: &tauri::App, shared: Arc<Shared>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open island", true, None::<&str>)?;
    let tb = CheckMenuItem::with_id(app, "taskbar", "Hide Windows taskbar", true, shared.taskbar_hidden.load(Ordering::Relaxed), None::<&str>)?;
    let auto = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, app.autolaunch().is_enabled().unwrap_or(false), None::<&str>)?;
    let upd = MenuItem::with_id(app, "update", "Check for updates", updater::status().state != "off", None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Island Shell", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &tb, &auto, &upd, &sep, &quit])?;
    *shared.tray_taskbar.lock().unwrap() = Some(tb);
    let mut builder = TrayIconBuilder::with_id("island").tooltip("Island Shell").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            "open" => {
                let _ = app.emit("island:toggle", ());
            }
            "taskbar" => {
                let hide = !shared.taskbar_hidden.load(Ordering::Relaxed);
                apply_taskbar(&shared, hide);
            }
            "autostart" => {
                let m = app.autolaunch();
                let _ = if m.is_enabled().unwrap_or(false) { m.disable() } else { m.enable() };
            }
            "update" => {
                // The page answers on the island: up to date, or the version that's ready to install.
                let app = app.clone();
                std::thread::spawn(move || {
                    util::com_mta();
                    let st = updater::check(&app).unwrap_or_else(|_| updater::status());
                    let _ = app.emit("update:asked", st);
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                let _ = tray.app_handle().emit("island:toggle", ());
            }
        })
        .build(app)?;
    Ok(())
}

unsafe extern "system" fn on_console_close(_ctrl: u32) -> windows::core::BOOL {
    taskbar::restore(&util::data_dir());
    windows::core::BOOL(0)
}

fn main() {
    let dir = util::data_dir();
    let _ = std::fs::create_dir_all(&dir);

    if std::env::args().any(|a| a == "--restore-taskbar") {
        taskbar::restore(&dir);
        return;
    }

    // Started by a layout switch: wait for the previous shell to finish exiting first.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pid) = args.iter().position(|a| a == "--after").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<u32>().ok()) {
        util::wait_for_exit(pid, 8000);
    }

    // Whatever happens, try to give the taskbar back.
    let panic_dir = dir.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        taskbar::restore(&panic_dir);
        prev(info);
    }));
    unsafe {
        let _ = windows::Win32::System::Console::SetConsoleCtrlHandler(Some(on_console_close), true);
    }

    let settings = Settings::load(&dir);
    let shared = Arc::new(Shared {
        dir: dir.clone(),
        status: Mutex::new(Status::default()),
        stats: Mutex::new(stats::Stats::default()),
        stats_wanted: AtomicU64::new(0),
        clips: Mutex::new(clipboard::History::load(&dir)),
        apps: Mutex::new(vec![]),
        geom: Mutex::new(island::Geom::default()),
        place: Mutex::new(island::Placement::default()),
        fullscreen_hidden: AtomicBool::new(false),
        urgent: AtomicBool::new(false),
        taskbar_hidden: AtomicBool::new(false),
        tray_taskbar: Mutex::new(None),
        prev_fg: AtomicIsize::new(0),
        island_hwnd: AtomicIsize::new(0),
        raised: AtomicBool::new(false),
        bar_mode: AtomicBool::new(is_bar_style(&settings.style)),
        bar_tall: AtomicBool::new(settings.style == "tall"),
        media_source: Mutex::new(settings.media_source.clone()),
        media_exe: Mutex::new(String::new()),
        media_playing: AtomicBool::new(false),
        active: AtomicBool::new(false),
        idle_since: AtomicU64::new(0),
        mem_low: AtomicBool::new(false),
    });

    let setup_shared = shared.clone();
    let event_shared = shared.clone();
    let exit_dir = dir.clone();
    let exit_shared = shared.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `island-shell.exe --quit` closes the running shell cleanly; any other launch toggles the panel.
            if args.iter().any(|a| a == "--quit") {
                app.exit(0);
            } else if let Some(i) = args.iter().position(|a| a == "--style") {
                // `island-shell.exe --style bar|tall|island` switches the running shell's layout.
                let style = args.get(i + 1).map(|s| s.as_str()).unwrap_or("");
                let shared = app.state::<Arc<Shared>>();
                let current = current_style(&shared);
                if style != current {
                    let _ = switch_style(app, &shared, style);
                }
            } else {
                let _ = app.emit("island:toggle", ());
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        summon(app, None);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        // Module UI files (src/modhost.js imports them from http://mod.localhost/<id>/…).
        .register_asynchronous_uri_scheme_protocol("mod", |_ctx, request, responder| {
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(modules::serve_ui(&path)));
        })
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            get_status,
            media_thumb,
            media_cmd,
            media_source,
            media_app_volume,
            media_lyrics,
            vpn_status,
            audio_devices,
            vm_state,
            we_library,
            we_apply,
            we_control,
            discord_status,
            discord_configure,
            discord_action,
            discord_options,
            discord_open,
            cc_mode,
            cc_places,
            cc_open,
            cc_tool,
            cc_web,
            cc_embed,
            cc_embed_hide,
            calendar_events,
            calendar_sources,
            calendar_add_feed,
            calendar_remove_feed,
            calendar_set_source,
            calendar_save,
            calendar_delete,
            open_link,
            canvas_status,
            tm_snapshot,
            music_info,
            tm_end,
            tm_eco,
            tm_startup,
            tm_cpu_info,
            tm_system,
            tm_nvme,
            tm_services,
            tm_service,
            tm_sessions,
            tm_installed,
            tm_installed_action,
            tm_conns,
            tm_history,
            tm_history_clear,
            tm_run,
            disk_scan_start,
            disk_scan_status,
            disk_scan_view,
            disk_scan_stop,
            bench_start,
            bench_status,
            bench_cancel,
            tm_startup_set,
            canvas_configure,
            canvas_configure_feed,
            canvas_overview,
            canvas_done,
            canvas_open,
            canvas_forget,
            destiny_status,
            destiny_configure,
            destiny_profile,
            destiny_xur,
            destiny_forget,
            destiny_open,
            vm_set,
            vm_levels,
            vm_restart,
            vm_mic,
            mixer_apps,
            voicedeck_open,
            audio_default,
            audio_mic,
            audio_apps,
            audio_app_set,
            games_list,
            game_action,
            gaze_status,
            gaze_set,
            vpn_tailscale,
            vpn_radmin_open,
            copy_text,
            set_volume,
            set_mute,
            set_radio,
            get_stats,
            clip_list,
            clip_copy,
            clip_pin,
            clip_delete,
            clip_clear,
            apps_list,
            app_launch,
            app_info,
            app_action,
            wallpaper_apply,
            power,
            island_rect,
            get_style,
            set_style,
            desktop_switch,
            desktop_new,
            windows_list,
            window_activate,
            window_minimize,
            window_close,
            tray_list,
            tray_click,
            release_focus,
            set_taskbar,
            open_settings,
            autostart_get,
            autostart_set,
            update_status,
            update_check,
            update_install,
            update_auto,
            modules::mod_list,
            modules::mod_call,
            modules::mod_set_enabled,
            modules::mod_restart,
            modules::mod_logs,
            modules::mod_theme,
            modules::mod_load_folder,
            modules::mod_remove,
            quit,
            log
        ])
        .setup(move |app| {
            // A running shell would already have received this launch through the single-instance
            // plugin; reaching setup with --quit means there was nothing to quit.
            if std::env::args().any(|a| a == "--quit" || a == "--style") {
                std::process::exit(0);
            }
            let shared = setup_shared;
            let win = app.get_webview_window("island").expect("island window");
            util::log_line(&format!(
                "Island Shell {} starting; {} clipboard items loaded",
                env!("CARGO_PKG_VERSION"),
                shared.clips.lock().unwrap().items.len()
            ));
            *shared.place.lock().unwrap() = island::place(&win, is_bar_style(&settings.style));
            island::show_quietly(&win);
            win.set_ignore_cursor_events(true)?;
            if let Some(h) = island::hwnd_of(&win) {
                shared.island_hwnd.store(h.0 as isize, Ordering::Relaxed);
            }
            apply_style(app.handle(), &shared, &settings.style);

            if settings.hide_taskbar {
                apply_taskbar(&shared, true);
            } else {
                // Clean up after a crash that happened while the taskbar was hidden.
                taskbar::restore(&shared.dir);
            }

            match app.global_shortcut().register(Shortcut::new(Some(Modifiers::ALT), Code::Backquote)) {
                Ok(()) => util::log_line("Alt+` registered"),
                Err(e) => util::log_line(&format!("Couldn't register Alt+`: {e}")),
            }
            let handle = app.handle().clone();
            // A Win tap opens Apps; Win+A/G/V/N/W open their tab (see winkey.rs).
            winkey::spawn(move |tab| summon(&handle, Some(tab.unwrap_or("apps"))));
            if settings.tray_host {
                tray::spawn(app.handle().clone());
            }

            island::spawn_cursor(win.clone(), shared.clone());
            applemusic::spawn();
            voicemeeter::spawn(app.handle().clone());
            discord::spawn(app.handle().clone(), shared.clone());
            canvas::spawn(app.handle().clone(), shared.dir.clone());
            embed::spawn_reaper(app.handle().clone());
            gaze::spawn(app.handle().clone(), shared.clone(), settings.gaze, settings.gaze_pause);
            spawn_meter(win.clone(), shared.clone());
            spawn_status(win, shared.clone());
            stats::spawn(shared.clone());
            taskmgr::history_spawn();
            clipboard::spawn(app.handle().clone(), shared.clone());
            apps::spawn(app.handle().clone(), shared.clone());
            updater::spawn(app.handle().clone());
            modules::init(app.handle(), &shared.dir);
            build_tray(app, shared)?;
            Ok(())
        })
        .on_window_event(move |win, event| match event {
            // Docked websites (embed.rs) are ordinary windows; only the island keeps itself open.
            WindowEvent::CloseRequested { api, .. } if win.label() == "island" => {
                api.prevent_close();
                let _ = win.emit("island:close", ());
            }
            WindowEvent::ScaleFactorChanged { .. } if win.label() == "island" => {
                let style = current_style(&event_shared);
                apply_style(win.app_handle(), &event_shared, style);
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building Island Shell");

    app.run(move |_app, event| {
        if let RunEvent::Exit = event {
            taskmgr::history_flush();
            // Step aside as tray host first so the taskbar restore talks to Explorer directly.
            tray::shutdown();
            // Give the bar's reserved screen strip back.
            bar::appbar_unregister(windows::Win32::Foundation::HWND(exit_shared.island_hwnd.load(Ordering::Relaxed) as _));
            taskbar::restore(&exit_dir);
            // Then let module backends save and stop (the job object ends any that don't).
            modules::shutdown();
        }
    });
}
