//! Eye tracking, an Island Shell module backend (docs/design/modules.md), through Tobii Stream
//! Engine, the library Tobii Experience installs alongside the tracker (loaded at run time, so the
//! module runs, quietly, on PCs without one). Its UI is ui/gaze.js: a Controls tile and a setting.
//!
//! It publishes the `presence` topic (§4.7): "looking" while your eyes rest on the island (with
//! dwell times, so a passing glance doesn't flicker it), "present", "away" once nobody has been in
//! front of the tracker for 10 seconds, or "off" without a tracker. The island treats "looking"
//! like hovering (the pill opens its peek, a notification waits until it's been seen); the Music
//! module pauses on "away" if you asked it to. Where the island is comes from `island.geometry`.
//!
//! Gaze is only used live, in this process; nothing is stored or sent, and never a gaze point.
//! Stream Engine is opened for "interactive" use, which is what Tobii licenses consumer trackers for.
//!
//! RPC methods: `status`, `set { enabled }`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};

/// What the UI sees.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
struct Status {
    /// Tobii's software is installed.
    available: bool,
    /// A tracker is connected and streaming.
    connected: bool,
    /// Someone is in front of it.
    present: bool,
    /// The user's switch for eye tracking.
    enabled: bool,
}

/// A rectangle in physical screen pixels.
#[derive(Deserialize, Clone, Copy, Default, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// The island on screen (`island.geometry`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
struct Geometry {
    rect: Rect,
    monitor: Rect,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    visible: bool,
}

fn one() -> f64 {
    1.0
}

/// How long eyes must rest on the island before it counts, and be gone before it stops counting.
#[cfg_attr(not(windows), allow(dead_code))]
mod timing {
    use std::time::Duration;
    pub const DWELL_IN: Duration = Duration::from_millis(330);
    pub const DWELL_OUT: Duration = Duration::from_millis(550);
    /// Seen this long in one go counts as being there (shorter blips are the tracker flickering).
    pub const BACK_RESUME: Duration = Duration::from_millis(1000);
    /// Not there for this long shows as away; this long is "away" for everyone else (the music pauses).
    pub const AWAY_STATUS: Duration = Duration::from_secs(2);
    pub const AWAY_PAUSE: Duration = Duration::from_secs(10);
}

/// Tracking isn't pixel-exact (about a finger's width at arm's length), so the target is padded
/// (in the island's CSS pixels).
const PAD_X: f64 = 70.0;
const PAD_Y: f64 = 50.0;

/// The island's rectangle to look for, padded, as (left, top, right, bottom); None while it's
/// hidden (bar style without its dropdown, or faded for a fullscreen app).
fn target(g: &Geometry, fullscreen: bool) -> Option<(f64, f64, f64, f64)> {
    if !g.visible || g.rect.w <= 0.0 || fullscreen {
        return None;
    }
    let s = g.scale;
    // The island hugs the top edge, where trackers are least sure: count anything above it.
    Some((g.rect.x - PAD_X * s, g.monitor.y - 200.0, g.rect.x + g.rect.w + PAD_X * s, g.rect.y + g.rect.h + PAD_Y * s))
}

/// Normalized gaze → screen pixels on the island's monitor (where the tracker is mounted).
fn to_screen(m: &Rect, (x, y): (f32, f32)) -> (f64, f64) {
    (m.x + x as f64 * m.w, m.y + y as f64 * m.h)
}

fn hit(r: &(f64, f64, f64, f64), (x, y): (f64, f64)) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

#[cfg(windows)]
mod tracker {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use island_module::{json, CallResult, Host, Init, Module, RpcError, Value};
    use windows::core::{s, w};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    use super::timing::*;
    use super::{hit, target, to_screen, Geometry, Status};

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

    /// What the tracker thread and the calls share.
    #[derive(Default)]
    pub struct Gaze {
        status: Mutex<Status>,
        geometry: Mutex<Option<Geometry>>,
        fullscreen: AtomicBool,
        settings_path: Mutex<std::path::PathBuf>,
        published: Mutex<&'static str>,
    }

    impl Gaze {
        fn status(&self) -> Status {
            self.status.lock().unwrap().clone()
        }

        fn update(&self, host: &Host, f: impl FnOnce(&mut Status)) {
            let st = {
                let mut st = self.status.lock().unwrap();
                let before = st.clone();
                f(&mut st);
                (*st != before).then(|| st.clone())
            };
            if let Some(st) = st {
                host.emit("status", &st);
            }
        }

        /// The `presence` topic, when it changes.
        fn publish(&self, host: &Host, state: &'static str) {
            let mut p = self.published.lock().unwrap();
            if *p != state {
                *p = state;
                host.publish("presence", json!({ "state": state }));
            }
        }

        fn save(&self) {
            let enabled = self.status.lock().unwrap().enabled;
            let _ = std::fs::write(&*self.settings_path.lock().unwrap(), json!({ "enabled": enabled }).to_string());
        }
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

    fn spawn(host: Host, gaze: Arc<Gaze>) {
        std::thread::spawn(move || {
            let Some(api) = Api::load() else {
                host.info("Tobii Stream Engine isn't installed");
                gaze.publish(&host, "off");
                return;
            };
            gaze.update(&host, |s| s.available = true);
            let mut handle: *mut c_void = std::ptr::null_mut();
            if unsafe { (api.api_create)(&mut handle, std::ptr::null(), std::ptr::null()) } != 0 {
                host.warn("couldn't start Stream Engine");
                gaze.publish(&host, "off");
                return;
            }
            loop {
                if !gaze.status().enabled {
                    gaze.publish(&host, "off");
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                match connect(&api, handle) {
                    Some((dev, url)) => {
                        host.info(format!("connected to {url}"));
                        run(&host, &gaze, &api, dev);
                        unsafe { (api.device_destroy)(dev) };
                        gaze.update(&host, |s| {
                            s.connected = false;
                            s.present = false;
                        });
                        gaze.publish(&host, "off");
                    }
                    None => {
                        gaze.publish(&host, "off");
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        });
    }

    /// Streams from one connected device until it's lost or the user turns eye tracking off.
    fn run(host: &Host, gaze: &Gaze, api: &Api, dev: *mut c_void) {
        // The callbacks write into this box through the user-data pointer.
        let mut feed = Box::new(Feed::default());
        let ptr = &mut *feed as *mut Feed as *mut c_void;
        unsafe {
            if (api.gaze_subscribe)(dev, on_gaze, ptr) != 0 {
                host.warn("gaze stream refused");
                return;
            }
            // Presence is a nice-to-have; not every tracker offers it.
            let _ = (api.presence_subscribe)(dev, on_presence, ptr);
        }
        gaze.update(host, |s| {
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
        let mut logged_presence: Option<bool> = None;
        let mut presence_logs = 0;
        let mut failures = 0;
        let mut first_look = true;
        loop {
            if !gaze.status().enabled {
                return;
            }
            if unsafe { (api.process)(dev) } != 0 {
                failures += 1;
                if failures % 60 == 1 && unsafe { (api.device_reconnect)(dev) } != 0 && failures > 300 {
                    host.warn("tracker lost");
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            failures = 0;
            let now = Instant::now();

            // Where the eyes are, in screen pixels, lightly smoothed (raw gaze jitters).
            let geo = *gaze.geometry.lock().unwrap();
            let fresh = feed.point_at.is_some_and(|t| now - t < Duration::from_millis(200));
            let point = if fresh { feed.point.zip(geo).map(|(p, g)| to_screen(&g.monitor, p)) } else { None };
            smooth = match (smooth, point) {
                (Some((sx, sy)), Some((x, y))) => Some((sx + (x - sx) * 0.35, sy + (y - sy) * 0.35)),
                (_, t) => t,
            };
            let rect = geo.and_then(|g| target(&g, gaze.fullscreen.load(Ordering::Relaxed)));
            let looking = smooth.is_some_and(|p| rect.is_some_and(|r| hit(&r, p)));
            if looking != raw_on {
                raw_on = looking;
                edge_since = now;
            }
            let settle = if raw_on { DWELL_IN } else { DWELL_OUT };
            if raw_on != on_island && now - edge_since >= settle {
                on_island = raw_on;
                if on_island && first_look {
                    first_look = false;
                    host.info("eyes on the island (first time this run)");
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
                host.info(format!("tracker reports {}", match feed.presence { Some(true) => "present", Some(false) => "away", None => "nothing yet" }));
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
            if present != gaze.status().present {
                gaze.update(host, |s| s.present = present);
                if !present {
                    host.info("away (nobody in front of the tracker)");
                } else {
                    host.info(format!("back after {:.0} s away", away_for.as_secs_f32()));
                }
            }
            if !present {
                away_for = gone;
            }
            // What the island and other modules hear: away once nobody's been there for a while,
            // looking while your eyes are on the island, present otherwise.
            gaze.publish(host, if gone >= AWAY_PAUSE { "away" } else if on_island { "looking" } else { "present" });
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    /// The module, shared with its tracker thread.
    #[derive(Default)]
    pub struct Shell(pub Arc<Gaze>);

    impl Module for Shell {
        fn init(&self, host: &Host, init: &Init) -> Result<(), RpcError> {
            let g = &self.0;
            // On, unless turned off here, or in 0.1.x (the island's own settings.json, read only).
            let enabled = init.settings["enabled"].as_bool().unwrap_or_else(|| {
                let old: Value = std::fs::read(host.legacy_dir().join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                old["gaze"].as_bool().unwrap_or(true)
            });
            g.status.lock().unwrap().enabled = enabled;
            *g.settings_path.lock().unwrap() = host.data_dir().join("settings.json");
            g.save();
            spawn(host.clone(), g.clone());
            Ok(())
        }

        fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
            let g = &self.0;
            match method {
                "status" => Ok(serde_json::to_value(g.status()).unwrap_or(Value::Null)),
                "set" => {
                    if let Some(on) = params["enabled"].as_bool() {
                        g.update(host, |s| s.enabled = on);
                        g.save();
                    }
                    Ok(serde_json::to_value(g.status()).unwrap_or(Value::Null))
                }
                _ => Err(RpcError::unknown_method(method)),
            }
        }

        fn event(&self, _host: &Host, topic: &str, data: Value) {
            match topic {
                "island.geometry" => *self.0.geometry.lock().unwrap() = serde_json::from_value(data).ok(),
                "fullscreen" => self.0.fullscreen.store(data["active"].as_bool().unwrap_or(false), Ordering::Relaxed),
                _ => {}
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    island_module::run(tracker::Shell::default());
}

/// Tobii Stream Engine is Windows-only.
#[cfg(not(windows))]
fn main() {
    eprintln!("The eye tracking module needs Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(visible: bool) -> Geometry {
        Geometry { rect: Rect { x: 1180.0, y: 8.0, w: 200.0, h: 38.0 }, monitor: Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 }, scale: 1.5, visible }
    }

    #[test]
    fn the_island_is_padded_and_reaches_the_top() {
        let r = target(&geo(true), false).unwrap();
        assert_eq!(r, (1180.0 - 105.0, -200.0, 1380.0 + 105.0, 46.0 + 75.0));
        assert!(hit(&r, (1100.0, 0.0)));
        assert!(!hit(&r, (1000.0, 20.0)));
        // Hidden, or under a fullscreen app: nothing to look at.
        assert!(target(&geo(false), false).is_none());
        assert!(target(&geo(true), true).is_none());
    }

    #[test]
    fn gaze_lands_on_the_islands_monitor() {
        let m = Rect { x: -1920.0, y: 0.0, w: 1920.0, h: 1080.0 };
        assert_eq!(to_screen(&m, (0.5, 0.25)), (-960.0, 270.0));
    }

    #[test]
    fn geometry_reads_from_the_topic() {
        let g: Geometry = serde_json::from_value(serde_json::json!({ "rect": { "x": 1, "y": 2, "w": 3, "h": 4 }, "monitor": { "x": 0, "y": 0, "w": 10, "h": 10 }, "visible": true })).unwrap();
        assert_eq!(g.scale, 1.0);
        assert!(g.visible);
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "gaze");
    }
}
