//! Eye tracking through Tobii Stream Engine, the library Tobii Experience installs alongside the
//! tracker (loaded at run time, so the island still works on PCs without one).
//!
//! - Looking at the island counts like hovering it: the pill opens its preview, and a notification
//!   waits until it has been seen. The page gets `gaze:island` true/false (with dwell times so a
//!   passing glance doesn't flicker it), never raw gaze points.
//! - Presence (someone in front of the tracker) goes to modules as the `presence` topic: "looking"
//!   (eyes on the island), "present", or "away" (nobody there for 10 s). The Music module pauses
//!   the music on "away" and plays it again on return, if you asked it to.
//!
//! Gaze is only used live, in this process; nothing is stored or sent. Stream Engine is opened for
//! "interactive" use, which is what Tobii licenses consumer trackers for.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::{s, w};
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

use crate::{util, Shared};

/// How long eyes must rest on the island before it counts, and be gone before it stops counting.
const DWELL_IN: Duration = Duration::from_millis(330);
const DWELL_OUT: Duration = Duration::from_millis(550);
/// Tracking isn't pixel-exact (about a finger's width at arm's length), so the target is padded.
const PAD_X: f64 = 70.0;
const PAD_Y: f64 = 50.0;
/// Seen this long in one go counts as being there (shorter blips are the tracker flickering).
const BACK_RESUME: Duration = Duration::from_millis(1000);
/// Not there for this long shows as away; this long pauses the music.
const AWAY_STATUS: Duration = Duration::from_secs(2);
const AWAY_PAUSE: Duration = Duration::from_secs(10);

#[derive(Serialize, Clone, Default)]
pub struct Status {
    /// Tobii's software is installed.
    pub available: bool,
    /// A tracker is connected and streaming.
    pub connected: bool,
    /// Someone is in front of it.
    pub present: bool,
    /// The user's switch for all eye-tracking features.
    pub enabled: bool,
}

#[repr(C)]
struct GazePoint {
    timestamp_us: i64,
    validity: c_int,
    xy: [f32; 2],
}

type UrlReceiver = unsafe extern "C" fn(*const c_char, *mut c_void);
type GazeCb = unsafe extern "C" fn(*const GazePoint, *mut c_void);
type PresenceCb = unsafe extern "C" fn(c_int, i64, *mut c_void);

/// The Stream Engine functions we use (signatures from tobii.h, API 4.x).
struct Api {
    api_create: unsafe extern "C" fn(*mut *mut c_void, *const c_void, *const c_void) -> c_int,
    enumerate: unsafe extern "C" fn(*mut c_void, UrlReceiver, *mut c_void) -> c_int,
    device_create: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void) -> c_int,
    device_destroy: unsafe extern "C" fn(*mut c_void) -> c_int,
    device_reconnect: unsafe extern "C" fn(*mut c_void) -> c_int,
    gaze_subscribe: unsafe extern "C" fn(*mut c_void, GazeCb, *mut c_void) -> c_int,
    presence_subscribe: unsafe extern "C" fn(*mut c_void, PresenceCb, *mut c_void) -> c_int,
    process: unsafe extern "C" fn(*mut c_void) -> c_int,
}

const DLL: &str = r"C:\Program Files\Tobii\Tobii EyeX\tobii_stream_engine.dll";
const FIELD_OF_USE_INTERACTIVE: c_int = 1;
const VALID: c_int = 1;
const PRESENT: c_int = 2;

impl Api {
    fn load() -> Option<Api> {
        if !std::path::Path::new(DLL).is_file() {
            return None;
        }
        unsafe {
            let lib: HMODULE = LoadLibraryW(w!("C:\\Program Files\\Tobii\\Tobii EyeX\\tobii_stream_engine.dll")).ok()?;
            macro_rules! f {
                ($name:expr) => {
                    std::mem::transmute(GetProcAddress(lib, $name)?)
                };
            }
            Some(Api {
                api_create: f!(s!("tobii_api_create")),
                enumerate: f!(s!("tobii_enumerate_local_device_urls")),
                device_create: f!(s!("tobii_device_create")),
                device_destroy: f!(s!("tobii_device_destroy")),
                device_reconnect: f!(s!("tobii_device_reconnect")),
                gaze_subscribe: f!(s!("tobii_gaze_point_subscribe")),
                presence_subscribe: f!(s!("tobii_user_presence_subscribe")),
                process: f!(s!("tobii_device_process_callbacks")),
            })
        }
    }
}

/// What the callbacks deliver (they run on our thread, inside `process`).
#[derive(Default)]
struct Feed {
    /// Latest valid gaze point, normalized to the tracker's display (0–1).
    point: Option<(f32, f32)>,
    point_at: Option<Instant>,
    presence: Option<bool>,
}

unsafe extern "C" fn on_url(url: *const c_char, data: *mut c_void) {
    let v = &mut *(data as *mut Vec<CString>);
    v.push(CStr::from_ptr(url).to_owned());
}

unsafe extern "C" fn on_gaze(g: *const GazePoint, data: *mut c_void) {
    let (g, feed) = (&*g, &mut *(data as *mut Feed));
    if g.validity == VALID {
        feed.point = Some((g.xy[0], g.xy[1]));
        feed.point_at = Some(Instant::now());
    }
}

unsafe extern "C" fn on_presence(status: c_int, _ts: i64, data: *mut c_void) {
    (&mut *(data as *mut Feed)).presence = Some(status == PRESENT);
}

pub static STATUS: Mutex<Status> = Mutex::new(Status { available: false, connected: false, present: false, enabled: false });

pub fn status() -> Status {
    STATUS.lock().unwrap().clone()
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let changed = {
        let mut st = STATUS.lock().unwrap();
        let before = serde_json::to_string(&*st).unwrap_or_default();
        f(&mut st);
        serde_json::to_string(&*st).unwrap_or_default() != before
    };
    if changed {
        let _ = app.emit("gaze:status", status());
    }
}

pub fn spawn(app: AppHandle, shared: Arc<Shared>, enabled: bool) {
    STATUS.lock().unwrap().enabled = enabled;
    std::thread::spawn(move || {
        util::com_mta();
        let Some(api) = Api::load() else {
            util::log_line("eye tracking: Tobii Stream Engine not installed");
            return;
        };
        update(&app, |s| s.available = true);
        let mut handle: *mut c_void = std::ptr::null_mut();
        if unsafe { (api.api_create)(&mut handle, std::ptr::null(), std::ptr::null()) } != 0 {
            util::log_line("eye tracking: couldn't start Stream Engine");
            return;
        }
        loop {
            if !status().enabled {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            match connect(&api, handle) {
                Some((dev, url)) => {
                    util::log_line(&format!("eye tracking: connected to {url}"));
                    run(&app, &shared, &api, dev);
                    unsafe { (api.device_destroy)(dev) };
                    update(&app, |s| {
                        s.connected = false;
                        s.present = false;
                    });
                    let _ = app.emit("gaze:island", false);
                }
                None => std::thread::sleep(Duration::from_secs(5)),
            }
        }
    });
}

fn connect(api: &Api, handle: *mut c_void) -> Option<(*mut c_void, String)> {
    unsafe {
        let mut urls: Vec<CString> = vec![];
        (api.enumerate)(handle, on_url, &mut urls as *mut _ as *mut c_void);
        let url = urls.into_iter().next()?;
        let mut dev = std::ptr::null_mut();
        if (api.device_create)(handle, url.as_ptr(), FIELD_OF_USE_INTERACTIVE, &mut dev) != 0 || dev.is_null() {
            return None;
        }
        Some((dev, url.to_string_lossy().into_owned()))
    }
}

/// Streams from one connected device until it's lost or the user turns eye tracking off.
fn run(app: &AppHandle, shared: &Shared, api: &Api, dev: *mut c_void) {
    // The callbacks write into this box through the user-data pointer.
    let mut feed = Box::new(Feed::default());
    let ptr = &mut *feed as *mut Feed as *mut c_void;
    unsafe {
        if (api.gaze_subscribe)(dev, on_gaze, ptr) != 0 {
            util::log_line("eye tracking: gaze stream refused");
            return;
        }
        // Presence is a nice-to-have; not every tracker offers it.
        let _ = (api.presence_subscribe)(dev, on_presence, ptr);
    }
    update(app, |s| {
        s.connected = true;
        s.present = true;
    });

    let mut smooth: Option<(f64, f64)> = None;
    let mut on_island = false;
    let mut edge_since = Instant::now(); // when the raw on/off reading last flipped
    let mut raw_on = false;
    let mut seen_raw = true;
    let mut seen_since = Instant::now();
    let mut last_there = Instant::now();
    let mut away_for = Duration::ZERO;
    let mut published = "";
    let mut logged_presence: Option<bool> = None;
    let mut presence_logs = 0;
    let mut failures = 0;
    let mut first_look = true;
    loop {
        if !status().enabled {
            // Turned off on purpose: whatever waited for you to come back can carry on.
            crate::modules::publish(island_host::proto::topic::PRESENCE, serde_json::json!({ "state": "present" }));
            return;
        }
        if unsafe { (api.process)(dev) } != 0 {
            failures += 1;
            if failures % 60 == 1 && unsafe { (api.device_reconnect)(dev) } != 0 && failures > 300 {
                util::log_line("eye tracking: tracker lost");
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        failures = 0;
        let now = Instant::now();

        // Where the eyes are, in screen pixels, lightly smoothed (raw gaze jitters).
        let fresh = feed.point_at.is_some_and(|t| now - t < Duration::from_millis(200));
        let target = if fresh { feed.point.map(|p| to_screen(shared, p)) } else { None };
        smooth = match (smooth, target) {
            (Some((sx, sy)), Some((x, y))) => Some((sx + (x - sx) * 0.35, sy + (y - sy) * 0.35)),
            (_, t) => t,
        };
        let looking = smooth.is_some_and(|p| island_rect(shared).is_some_and(|r| hit(&r, p)));
        if looking != raw_on {
            raw_on = looking;
            edge_since = now;
        }
        let settle = if raw_on { DWELL_IN } else { DWELL_OUT };
        if raw_on != on_island && now - edge_since >= settle {
            on_island = raw_on;
            let _ = app.emit("gaze:island", on_island);
            if on_island && first_look {
                first_look = false;
                util::log_line("eye tracking: eyes on the island (first time this run)");
            }
        }

        // Presence. The tracker's own signal flickers both ways (a moment of "away" while you sit
        // there, a moment of "present" at an empty chair), so only a full second of being seen
        // counts as being there, and the away clock runs from the last time that happened.
        // Valid gaze in the last moment also counts (blinks and eye jumps leave short gaps), because
        // this tracker's presence signal sometimes says "away" with someone sitting right there.
        let gaze_recent = feed.point_at.is_some_and(|t| now - t < Duration::from_millis(1500));
        let seen = feed.presence == Some(true) || gaze_recent;
        if feed.presence != logged_presence && presence_logs < 20 {
            presence_logs += 1;
            logged_presence = feed.presence;
            util::log_line(&format!("eye tracking: tracker reports {}", match feed.presence { Some(true) => "present", Some(false) => "away", None => "nothing yet" }));
        }
        if seen != seen_raw {
            seen_raw = seen;
            seen_since = now;
        }
        if seen_raw && now - seen_since >= BACK_RESUME {
            last_there = now;
        }
        let gone = now - last_there;
        let present = gone < AWAY_STATUS;
        if present != status().present {
            update(app, |s| s.present = present);
            if !present {
                util::log_line("eye tracking: away (nobody in front of the tracker)");
            } else {
                util::log_line(&format!("eye tracking: back after {:.0} s away", away_for.as_secs_f32()));
            }
        }
        if !present {
            away_for = gone;
        }
        // What modules hear: away once nobody's been there for a while, looking while your eyes are
        // on the island, present otherwise.
        let state = if gone >= AWAY_PAUSE { "away" } else if on_island { "looking" } else { "present" };
        if state != published {
            published = state;
            crate::modules::publish(island_host::proto::topic::PRESENCE, serde_json::json!({ "state": state }));
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Normalized gaze → screen pixels on the island's monitor (where the tracker is mounted).
fn to_screen(shared: &Shared, (x, y): (f32, f32)) -> (f64, f64) {
    let m = shared.place.lock().unwrap().monitor;
    (m.left as f64 + x as f64 * (m.right - m.left) as f64, m.top as f64 + y as f64 * (m.bottom - m.top) as f64)
}

/// The island's rectangle on screen, padded, or None while it's hidden (bar style, no dropdown).
fn island_rect(shared: &Shared) -> Option<RECT> {
    let g = *shared.geom.lock().unwrap();
    if g.w <= 0.0 || shared.fullscreen_hidden.load(Ordering::Relaxed) {
        return None;
    }
    let p = *shared.place.lock().unwrap();
    let mut wr = RECT { left: p.x, top: p.y, ..Default::default() };
    let h = shared.island_hwnd.load(Ordering::Relaxed);
    if h != 0 {
        let _ = unsafe { GetWindowRect(windows::Win32::Foundation::HWND(h as _), &mut wr) };
    }
    let s = p.scale;
    Some(RECT {
        left: (wr.left as f64 + (g.x - PAD_X) * s) as i32,
        // The island hugs the top edge, where trackers are least sure: count anything above it.
        top: p.monitor.top - 200,
        right: (wr.left as f64 + (g.x + g.w + PAD_X) * s) as i32,
        bottom: (wr.top as f64 + (g.y + g.h + PAD_Y) * s) as i32,
    })
}

fn hit(r: &RECT, (x, y): (f64, f64)) -> bool {
    x >= r.left as f64 && x <= r.right as f64 && y >= r.top as f64 && y <= r.bottom as f64
}
