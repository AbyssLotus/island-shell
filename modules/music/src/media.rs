//! Now-playing info and transport controls through Windows' system media controls (GSMTC).
//! (Moved from the island's base into the Music module; bringing the player forward is the
//! island's job, since it's the one in front when you ask.)
//!
//! Which player a session is (Apple Music, Spotify, Amazon Music, YouTube Music, a browser…) is
//! players.rs, from its app id and the name Windows has for the app.
//!
//! Apple Music gets extra treatment: its album is split out of the artist field, and seeking,
//! shuffle, repeat, favorite and the quality badge come from its own player bar (applemusic.rs).
//! Everything else gets whatever its media session supports.

use serde::Serialize;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackControls as Controls,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Media::MediaPlaybackAutoRepeatMode as RepeatMode;
use windows::Storage::Streams::DataReader;

use crate::applemusic::{self, Cmd};
use crate::players::{self, Player};

/// A media session, for the source switcher.
#[derive(Serialize, Clone, Default)]
pub struct SessionInfo {
    pub id: String,
    pub app: String,
    pub title: String,
    pub playing: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    /// The session's app id (AUMID).
    pub id: String,
    /// "apple", "spotify", "amazon", "youtube", "browser" or "other" (players.rs).
    pub source: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    /// When `position` was measured (Unix ms), so the page can carry it forward accurately.
    pub sampled_at: u64,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
    /// None when the player doesn't offer it.
    pub shuffle: Option<bool>,
    /// "off", "all" or "one"; None when the player doesn't offer it.
    pub repeat: Option<String>,
    /// Apple Music's Favorite.
    pub favorite: Option<bool>,
    /// "Lossless", "Dolby Atmos"… (Apple Music).
    pub badge: String,
    /// True when the user picked this source instead of the automatic choice.
    pub pinned: bool,
    pub sessions: Vec<SessionInfo>,
    pub key: String,
    /// Process image name(s) of the player, `|`-separated, for its volume and level meter (players.rs).
    pub exe: String,
    /// Apple Music's main window, to bring it forward (0 otherwise).
    pub hwnd: isize,
}

pub fn manager() -> Option<Manager> {
    Manager::RequestAsync().ok()?.get().ok()
}

fn filetime_now() -> i64 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (d.as_nanos() / 100) as i64 + 116_444_736_000_000_000
}

fn id_of(s: &Session) -> String {
    s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default()
}

/// The player behind a session's app id (players.rs), with the name Windows has for the app.
pub fn player(id: &str) -> Player {
    players::identify(id, display_name(id).as_deref())
}

fn source_of(id: &str) -> &'static str {
    player(id).source
}

fn is_playing(s: &Session) -> bool {
    s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).is_ok_and(|st| st == PlaybackStatus::Playing)
}

/// The session to show: the one the user picked, otherwise whatever is playing (Apple Music first,
/// then the other music apps, other players, and browsers last), otherwise a paused one in that
/// order, otherwise whatever Windows considers current.
fn pick(mgr: &Manager, preferred: &str) -> Option<Session> {
    let sessions: Vec<Session> = mgr.GetSessions().ok()?.into_iter().collect();
    if !preferred.is_empty() {
        if let Some(s) = sessions.iter().find(|s| id_of(s) == preferred) {
            return Some(s.clone());
        }
    }
    let current = mgr.GetCurrentSession().ok().map(|s| id_of(&s));
    sessions.into_iter().min_by_key(|s| {
        let id = id_of(s);
        (!is_playing(s), player(&id).rank, current.as_deref() != Some(id.as_str()))
    })
}

pub fn snapshot(mgr: &Manager, preferred: &str) -> Option<MediaInfo> {
    let s = pick(mgr, preferred)?;
    let id = id_of(&s);
    let who = player(&id);
    let source = who.source;
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
    let mut artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
    let mut album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
    if title.is_empty() && artist.is_empty() {
        return None;
    }
    // Apple Music reports "Artist — Album" as the artist.
    if source == "apple" && album.is_empty() {
        if let Some((a, b)) = artist.split_once(" \u{2014} ") {
            (artist, album) = (a.trim().to_string(), b.trim().to_string());
        }
    }
    let info = s.GetPlaybackInfo().ok()?;
    let playing = info.PlaybackStatus().map(|st| st == PlaybackStatus::Playing).unwrap_or(false);
    let controls = info.Controls().ok();
    let flag = |f: fn(&Controls) -> windows::core::Result<bool>| controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false);
    let mut m = MediaInfo {
        can_next: flag(Controls::IsNextEnabled),
        can_prev: flag(Controls::IsPreviousEnabled),
        can_seek: flag(Controls::IsPlaybackPositionEnabled),
        shuffle: flag(Controls::IsShuffleEnabled).then(|| info.IsShuffleActive().and_then(|v| v.Value()).unwrap_or(false)),
        repeat: flag(Controls::IsRepeatEnabled).then(|| {
            match info.AutoRepeatMode().and_then(|v| v.Value()) {
                Ok(RepeatMode::Track) => "one",
                Ok(RepeatMode::List) => "all",
                _ => "off",
            }
            .to_string()
        }),
        ..Default::default()
    };

    if let Ok(tl) = s.GetTimelineProperties() {
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
        let mut pos = tl.Position().map(|t| t.Duration).unwrap_or(0);
        if playing {
            if let Ok(updated) = tl.LastUpdatedTime() {
                let dt = filetime_now() - updated.UniversalTime;
                if dt > 0 && dt < 36_000_000_000 {
                    pos += dt;
                }
            }
        }
        m.duration = (end - start).max(0) as f64 / 1e7;
        m.position = (pos - start).max(0) as f64 / 1e7;
        if m.duration > 0.0 {
            m.position = m.position.min(m.duration);
        }
    }
    if source == "apple" {
        let am = applemusic::state();
        if am.connected {
            m.can_seek = true;
            m.can_prev = true;
            m.shuffle = am.shuffle;
            m.repeat = am.repeat;
            m.favorite = am.favorite;
            m.badge = am.badge;
            m.hwnd = am.hwnd;
        }
    }
    m.sessions = mgr
        .GetSessions()
        .map(|list| {
            list.into_iter()
                .map(|o| {
                    let oid = id_of(&o);
                    let title = o
                        .TryGetMediaPropertiesAsync()
                        .and_then(|p| p.get())
                        .and_then(|p| p.Title())
                        .map(|h| h.to_string())
                        .unwrap_or_default();
                    SessionInfo { app: player(&oid).name, playing: is_playing(&o), id: oid, title }
                })
                .collect()
        })
        .unwrap_or_default();
    m.key = format!("{id}|{title}|{artist}");
    m.app = who.name;
    m.exe = who.exe;
    m.pinned = !preferred.is_empty() && preferred == id;
    m.source = source.to_string();
    m.sampled_at = crate::now_ms();
    Some(MediaInfo { title, artist, album, id, playing, ..m })
}

/// The artwork's encoded bytes (JPEG/PNG), sent to the page as-is: no base64 copy.
pub fn thumbnail(mgr: &Manager, preferred: &str) -> Option<Vec<u8>> {
    let s = pick(mgr, preferred)?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

/// Waits for a Try…Async call: `done!(s.TrySkipNextAsync())`.
macro_rules! done {
    ($op:expr) => {
        finish($op.and_then(|o| o.get()))
    };
}

fn finish(r: windows::core::Result<bool>) -> Result<(), String> {
    match r {
        Ok(true) => Ok(()),
        Ok(false) => Err("The player ignored that".into()),
        Err(e) => Err(e.message().to_string()),
    }
}

/// `value` is the position in seconds for "seek".
pub fn command(mgr: &Manager, preferred: &str, action: &str, value: f64) -> Result<(), String> {
    let s = pick(mgr, preferred).ok_or("Nothing is playing")?;
    let id = id_of(&s);
    let apple = source_of(&id) == "apple" && applemusic::state().connected;
    match action {
        "toggle" => done!(s.TryTogglePlayPauseAsync()),
        "pause" => done!(s.TryPauseAsync()),
        "play" => done!(s.TryPlayAsync()),
        "next" => done!(s.TrySkipNextAsync()),
        "prev" if apple => applemusic::send(Cmd::Prev),
        "prev" => done!(s.TrySkipPreviousAsync()),
        "seek" if apple => applemusic::send(Cmd::Seek(value)),
        "seek" => done!(s.TryChangePlaybackPositionAsync((value.max(0.0) * 1e7) as i64)),
        "shuffle" if apple => applemusic::send(Cmd::Shuffle),
        "shuffle" => {
            let on = s.GetPlaybackInfo().and_then(|i| i.IsShuffleActive()).and_then(|v| v.Value()).unwrap_or(false);
            done!(s.TryChangeShuffleActiveAsync(!on))
        }
        "repeat" if apple => applemusic::send(Cmd::Repeat),
        "repeat" => {
            let next = match s.GetPlaybackInfo().and_then(|i| i.AutoRepeatMode()).and_then(|v| v.Value()) {
                Ok(RepeatMode::List) => RepeatMode::Track,
                Ok(RepeatMode::Track) => RepeatMode::None,
                _ => RepeatMode::List,
            };
            done!(s.TryChangeAutoRepeatModeAsync(next))
        }
        "favorite" if apple => applemusic::send(Cmd::Favorite),
        "favorite" => Err("Only Apple Music has favorites here".into()),
        _ => Err(format!("Unknown action {action}")),
    }
}

/// Firefox-family browsers (Firefox, Zen, LibreWolf…) name their media session with a hash of where
/// they're installed ("F0DC299D809B9700"). Their notification registration, "ZenToast-<hash>" under
/// HKCU\Software\Classes\AppUserModelId, carries the name to show ("Zen Browser").
fn hashed_app_name(id: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    static NAMES: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let key = id.to_ascii_uppercase();
    if let Some(v) = NAMES.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return v.clone();
    }
    const ROOT: &str = r"Software\Classes\AppUserModelId";
    let mut found = None;
    unsafe {
        let mut k = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(ROOT), None, KEY_READ, &mut k) == ERROR_SUCCESS {
            for i in 0..2048 {
                let mut name = [0u16; 512];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                    break;
                }
                let sub = String::from_utf16_lossy(&name[..len as usize]);
                if sub.to_ascii_uppercase().ends_with(&format!("-{key}")) {
                    found = island_win::reg::user_str(&format!(r"{ROOT}\{sub}"), "DisplayName");
                    if found.is_some() {
                        break;
                    }
                }
            }
            let _ = RegCloseKey(k);
        }
    }
    NAMES.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, found.clone());
    found
}

/// The name Windows has for a session's app: a Firefox-family browser's registration, or the app's
/// own display name (its Start menu name: "YouTube Music" for the one installed from Chrome).
/// Looked up once per app id.
fn display_name(id: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static NAMES: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
    if id.is_empty() {
        return None;
    }
    if let Some(v) = NAMES.lock().unwrap().get_or_insert_with(HashMap::new).get(id) {
        return v.clone();
    }
    let found = hashed_app_name(id).or_else(|| {
        windows::ApplicationModel::AppInfo::GetFromAppUserModelId(&windows::core::HSTRING::from(id))
            .and_then(|a| a.DisplayInfo())
            .and_then(|d| d.DisplayName())
            .map(|n| n.to_string())
            .ok()
            .filter(|n| !n.trim().is_empty())
    });
    NAMES.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.to_string(), found.clone());
    found
}

/// `media.state` for modules (§4.7): what's playing, or null when nothing is.
pub fn topic(m: Option<&MediaInfo>) -> serde_json::Value {
    match m {
        Some(m) => serde_json::json!({
            "playing": m.playing, "app": m.app, "source": m.source, "title": m.title, "artist": m.artist, "album": m.album,
            "duration": m.duration, "position": m.position, "sampled_at": m.sampled_at, "key": m.key,
        }),
        None => serde_json::Value::Null,
    }
}
