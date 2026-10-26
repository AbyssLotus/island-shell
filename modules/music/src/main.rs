//! Music, an Island Shell module backend (docs/design/modules.md); its UI is ui/music.js: the Music
//! tab, the Command Center's Music sheet, and what's playing on the island (a `media` activity the
//! island draws on its pill, peek, Controls card and top bar).
//!
//! - media.rs: what's playing, through Windows' system media controls, in any player Windows knows.
//! - applemusic.rs: Apple Music's own player bar through UI Automation (seek, shuffle, repeat,
//!   favorite and the quality badge).
//! - lyrics.rs: time-synced lyrics from LRCLIB. info.rs: the sheet's cover, year and genre from
//!   Apple's iTunes Search.
//!
//! A thread reads the player every second and sends `state` (what's playing, or null) when anything
//! but the clock moved, and publishes `media.state` for other modules (the Discord module's status).
//! It follows `presence` (the eye tracker) too: with "pause when I step away" on, the music pauses
//! once you've stepped away and plays again when you're back, unless someone else started it meanwhile.
//!
//! RPC methods: `state`, `cmd { action, value }`, `source { id }`, `thumb`,
//! `lyrics { title, artist, album, duration }`, `info { title, artist, album }`, `settings`,
//! `set { pause_when_away }`.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod info;
mod lyrics;

#[cfg(windows)]
mod applemusic;
#[cfg(windows)]
mod media;

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use island_module::Host;

static HOST: OnceLock<Host> = OnceLock::new();

/// A line in island.log (as `[mod:music] …`).
pub(crate) fn log_line(msg: &str) {
    if let Some(h) = HOST.get() {
        h.info(msg);
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Percent-encodes a query value.
pub(crate) fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// What's saved in the module's settings.json.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, PartialEq, Debug)]
struct Settings {
    /// The media session the user picked instead of the automatic choice ("" = automatic).
    #[serde(default)]
    source: String,
    /// Pause the music when you step away from the screen (with an eye tracker), play it when you're back.
    #[serde(default)]
    pause_when_away: bool,
}

impl Settings {
    /// From the module's settings.json; the first time, from 0.1.x's choices in the island's own
    /// settings.json (read, not taken: it's still the island's file).
    fn load(init: &island_module::Value, legacy: &std::path::Path) -> Settings {
        if init.as_object().is_some_and(|o| !o.is_empty()) {
            return serde_json::from_value(init.clone()).unwrap_or_default();
        }
        let old: island_module::Value = std::fs::read(legacy.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Settings { source: old["media_source"].as_str().unwrap_or("").to_string(), pause_when_away: old["gaze_pause"].as_bool().unwrap_or(false) }
    }
}

#[cfg(windows)]
mod module {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use island_module::{json, CallResult, Host, Init, Module, RpcError, Value};

    use super::{fnv, Settings, HOST};
    use crate::media::{self, MediaInfo};

    #[derive(Default)]
    struct Away {
        /// We paused the music because you stepped away.
        paused_by_us: bool,
        paused_at: Option<Instant>,
        away: bool,
    }

    #[derive(Default)]
    pub struct Music {
        settings: Mutex<Settings>,
        settings_path: Mutex<std::path::PathBuf>,
        state: Arc<Mutex<Option<MediaInfo>>>,
        poke: Mutex<Option<Sender<()>>>,
        away: Arc<Mutex<Away>>,
        /// A `presence` topic has arrived: there's an eye tracker (or something like it).
        tracker: AtomicBool,
    }

    impl Music {
        fn preferred(&self) -> String {
            self.settings.lock().unwrap().source.clone()
        }

        fn save(&self) {
            let s = self.settings.lock().unwrap().clone();
            if let Ok(b) = serde_json::to_vec_pretty(&s) {
                let _ = std::fs::write(&*self.settings_path.lock().unwrap(), b);
            }
        }

        /// Reads the player again right away (after a command, so the island follows quickly).
        fn poke(&self) {
            if let Some(tx) = self.poke.lock().unwrap().as_ref() {
                let _ = tx.send(());
            }
        }

        fn command(&self, action: &str, value: f64) -> Result<(), String> {
            let m = media::manager().ok_or("Nothing is playing")?;
            let r = media::command(&m, &self.preferred(), action, value);
            self.poke();
            r
        }

        /// The artwork of what's playing, saved in the cache folder for the UI (`ctx.cacheUrl`).
        fn thumb(&self, cache: &std::path::Path) -> Option<String> {
            let key = self.state.lock().unwrap().as_ref()?.key.clone();
            let bytes = media::thumbnail(&media::manager()?, &self.preferred())?;
            let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" };
            let name = format!("art-{:016x}.{ext}", fnv(&key));
            let path = cache.join(&name);
            if !path.is_file() {
                std::fs::write(&path, &bytes).ok()?;
                // Keep the latest few covers; older ones go.
                let mut old: Vec<_> = std::fs::read_dir(cache).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("art-")).collect();
                old.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
                for e in old.into_iter().skip(8) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            Some(name)
        }

        fn presence(&self, state: &str) {
            let mut a = self.away.lock().unwrap();
            let away = state == "away";
            if away == a.away {
                return;
            }
            a.away = away;
            let playing = self.state.lock().unwrap().as_ref().is_some_and(|m| m.playing);
            if away && playing && !a.paused_by_us && self.settings.lock().unwrap().pause_when_away {
                a.paused_by_us = true;
                a.paused_at = Some(Instant::now());
                drop(a);
                super::log_line("you stepped away: pausing the music");
                let _ = self.command("pause", 0.0);
            } else if !away && a.paused_by_us {
                a.paused_by_us = false;
                drop(a);
                super::log_line("you're back: playing the music again");
                let _ = self.command("play", 0.0);
            }
        }
    }

    /// Whether `m` is news compared with `last`: anything but the position moving on as it should.
    fn changed(last: &Option<MediaInfo>, m: &Option<MediaInfo>) -> bool {
        let strip = |m: &MediaInfo| {
            let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
            v["position"] = Value::Null;
            v["sampled_at"] = Value::Null;
            v
        };
        match (last, m) {
            (None, None) => false,
            (Some(a), Some(b)) => {
                if strip(a) != strip(b) {
                    return true;
                }
                // Where the last reading says the song should be by now; a jump is a seek.
                let expect = a.position + if a.playing { b.sampled_at.saturating_sub(a.sampled_at) as f64 / 1000.0 } else { 0.0 };
                (b.position - expect).abs() > 2.5
            }
            _ => true,
        }
    }

    /// `media.state` changes when the song, play state or start time does (a seek); the start time
    /// in 3-second steps, and not while paused (paused, the clock keeps moving it).
    fn topic_sig(m: &Option<MediaInfo>) -> Option<(String, bool, i64)> {
        m.as_ref().map(|m| (m.key.clone(), m.playing, if m.playing { (m.sampled_at as i64 - (m.position * 1000.0) as i64) / 3000 } else { 0 }))
    }

    fn poll(host: Host, music: Arc<Music>, rx: Receiver<()>) {
        super::com_mta();
        let mut mgr = media::manager();
        let mut tick: u64 = 0;
        let mut sig = None;
        loop {
            if mgr.is_none() && tick % 10 == 0 {
                mgr = media::manager();
            }
            let m = mgr.as_ref().and_then(|g| media::snapshot(g, &music.preferred()));
            let news = changed(&music.state.lock().unwrap(), &m);
            let playing = m.as_ref().is_some_and(|m| m.playing);
            *music.state.lock().unwrap() = m.clone();
            if news {
                host.emit("state", &m);
            }
            let s = topic_sig(&m);
            if s != sig {
                sig = s;
                host.publish("media.state", media::topic(m.as_ref()));
            }
            // Playing again while you're still away means someone else started it: not ours to resume.
            {
                let mut a = music.away.lock().unwrap();
                if a.paused_by_us && playing && a.paused_at.is_some_and(|t| t.elapsed() > Duration::from_secs(4)) {
                    a.paused_by_us = false;
                    super::log_line("the music was started again while you were away");
                }
            }
            tick += 1;
            match rx.recv_timeout(Duration::from_millis(1000)) {
                // A command just ran: give the player a moment to report it.
                Ok(()) => std::thread::sleep(Duration::from_millis(250)),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// The module, shared with its reading thread.
    #[derive(Default)]
    pub struct Shell(pub Arc<Music>);

    impl Module for Shell {
        fn init(&self, host: &Host, init: &Init) -> Result<(), RpcError> {
            let _ = HOST.set(host.clone());
            let m = &self.0;
            *m.settings.lock().unwrap() = Settings::load(&init.settings, &host.legacy_dir());
            *m.settings_path.lock().unwrap() = host.data_dir().join("settings.json");
            m.save();
            crate::applemusic::spawn();
            let (tx, rx) = channel();
            *m.poke.lock().unwrap() = Some(tx);
            let (h, music) = (host.clone(), m.clone());
            std::thread::spawn(move || poll(h, music, rx));
            Ok(())
        }

        fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
            let m = &self.0;
            match method {
                "state" => Ok(serde_json::to_value(m.state.lock().unwrap().clone()).unwrap_or(Value::Null)),
                // toggle, play, pause, next, prev, seek (`value` = seconds), shuffle, repeat, favorite.
                "cmd" => m.command(params["action"].as_str().unwrap_or(""), params["value"].as_f64().unwrap_or(0.0)).map(|_| Value::Null).map_err(RpcError::failed),
                // Shows this media session instead of choosing automatically ("" = automatic again).
                "source" => {
                    m.settings.lock().unwrap().source = params["id"].as_str().unwrap_or("").to_string();
                    m.save();
                    m.poke();
                    Ok(Value::Null)
                }
                "thumb" => Ok(m.thumb(&host.cache_dir()).map(Value::String).unwrap_or(Value::Null)),
                "lyrics" => {
                    let s = |k: &str| params[k].as_str().unwrap_or("").to_string();
                    crate::lyrics::fetch(&host.cache_dir(), &s("title"), &s("artist"), &s("album"), params["duration"].as_f64().unwrap_or(0.0))
                        .map(|l| serde_json::to_value(l).unwrap_or(Value::Null))
                        .map_err(RpcError::offline)
                }
                "info" => {
                    let s = |k: &str| params[k].as_str().unwrap_or("").to_string();
                    crate::info::info(&s("title"), &s("artist"), &s("album")).map(|i| serde_json::to_value(i).unwrap_or(Value::Null)).map_err(RpcError::offline)
                }
                "settings" => Ok(json!({ "pause_when_away": m.settings.lock().unwrap().pause_when_away, "tracker": m.tracker.load(Ordering::Relaxed) })),
                "set" => {
                    if let Some(on) = params["pause_when_away"].as_bool() {
                        m.settings.lock().unwrap().pause_when_away = on;
                        m.save();
                    }
                    Ok(json!({ "pause_when_away": m.settings.lock().unwrap().pause_when_away, "tracker": m.tracker.load(Ordering::Relaxed) }))
                }
                _ => Err(RpcError::unknown_method(method)),
            }
        }

        fn event(&self, host: &Host, topic: &str, data: Value) {
            if topic == "presence" {
                if !self.0.tracker.swap(true, Ordering::Relaxed) {
                    host.emit("tracker", true);
                }
                let st = data["state"].as_str().unwrap_or("").to_string();
                let music = self.0.clone();
                // Pausing waits on the player; events come in order on one thread, so not here.
                std::thread::spawn(move || music.presence(&st));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn song(pos: f64, at: u64, playing: bool) -> Option<MediaInfo> {
            Some(MediaInfo { title: "Song".into(), key: "k".into(), position: pos, sampled_at: at, playing, ..Default::default() })
        }

        #[test]
        fn only_news_is_sent() {
            assert!(!changed(&None, &None));
            assert!(changed(&None, &song(0.0, 0, true)));
            // Playing on as expected: nothing to say.
            assert!(!changed(&song(10.0, 1000, true), &song(12.0, 3000, true)));
            // A seek, a pause, a new song.
            assert!(changed(&song(10.0, 1000, true), &song(60.0, 3000, true)));
            assert!(changed(&song(10.0, 1000, true), &song(12.0, 3000, false)));
            let mut other = song(0.0, 3000, true);
            other.as_mut().unwrap().key = "other".into();
            assert!(changed(&song(10.0, 1000, true), &other));
            // Paused, the clock moving isn't news.
            assert!(!changed(&song(10.0, 1000, false), &song(10.0, 9000, false)));
        }

        #[test]
        fn the_topic_follows_seeks_not_the_clock() {
            assert_eq!(topic_sig(&song(10.0, 100_000, true)), topic_sig(&song(11.0, 101_000, true)));
            assert_ne!(topic_sig(&song(10.0, 100_000, true)), topic_sig(&song(40.0, 101_000, true)));
            assert_eq!(topic_sig(&song(10.0, 100_000, false)), topic_sig(&song(10.0, 500_000, false)));
        }
    }
}

/// COM for this thread (WinRT media controls, UI Automation).
#[cfg(windows)]
pub(crate) fn com_mta() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
}

/// The lowercased path of the program that owns a window.
#[cfg(windows)]
pub(crate) fn exe_of_window(h: windows::Win32::Foundation::HWND) -> String {
    let mut pid = 0u32;
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(h, Some(&mut pid)) };
    if pid == 0 {
        return String::new();
    }
    island_win::process::path_of(pid).to_lowercase()
}

#[cfg(windows)]
fn main() {
    island_module::run(module::Shell::default());
}

/// The players are Windows' (media controls, Apple Music's window); elsewhere there's nothing to read.
#[cfg(not(windows))]
fn main() {
    eprintln!("The Music module needs Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "music");
    }

    #[test]
    fn settings_from_0_1() {
        let dir = std::env::temp_dir().join(format!("music-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{ "media_source": "Spotify.exe", "gaze_pause": true, "style": "island" }"#).unwrap();
        let s = Settings::load(&serde_json::json!({}), &dir);
        assert_eq!(s, Settings { source: "Spotify.exe".into(), pause_when_away: true });
        // Once the module has its own, 0.1.x's don't count any more; the island's file stays.
        let own = Settings::load(&serde_json::json!({ "source": "", "pause_when_away": false }), &dir);
        assert_eq!(own, Settings::default());
        assert!(dir.join("settings.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
