//! Which player a media session belongs to, from its app id (an AUMID such as
//! `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify`, or an exe name such as `Spotify.exe`) and the
//! name Windows has for that app, when it has one: what to call it, where its sound comes from (for
//! the equalizer and its volume) and which one wins when several are playing.
//!
//! Apple Music, Spotify, Amazon Music and YouTube Music are known by name, whether they're the
//! app from the Microsoft Store, the one from the company's website, or (YouTube Music) the
//! open-source desktop apps or YouTube Music installed as an app from Chrome or Edge. A YouTube
//! Music *tab* in a browser shares the browser's session, so it shows as the browser.

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// "apple", "spotify", "amazon", "youtube", "browser" or "other".
    pub source: &'static str,
    pub name: String,
    /// Process image names, `|`-separated, for its audio sessions (empty when unknown).
    pub exe: String,
    /// Lower wins when several play: music apps, then other players, then browsers.
    pub rank: u8,
}

struct Known {
    source: &'static str,
    name: &'static str,
    /// Lowercase parts of an app id or of its display name.
    ids: &'static [&'static str],
    exe: &'static str,
    rank: u8,
}

const MUSIC: &[Known] = &[
    // Apple Music plays through its background agent, not the app window's process.
    Known { source: "apple", name: "Apple Music", ids: &["applemusic", "apple music"], exe: "amplibraryagent.exe|applemusic.exe", rank: 0 },
    Known { source: "spotify", name: "Spotify", ids: &["spotify"], exe: "spotify.exe", rank: 1 },
    Known { source: "amazon", name: "Amazon Music", ids: &["amazonmusic", "amazon music", "amazonmobilellc"], exe: "amazon music.exe|amazonmusic.exe", rank: 1 },
    // th-ch's "YouTube Music" and the "YouTube Music Desktop App" (ytmdesktop), or the installed web app.
    Known {
        source: "youtube",
        name: "YouTube Music",
        ids: &["youtube music", "youtube-music", "youtubemusic", "ytmdesktop", "th-ch"],
        exe: "youtube music.exe|youtube-music.exe|youtube music desktop app.exe",
        rank: 1,
    },
];

/// Browsers by the first part of their session's app id: (id, name, exe).
const BROWSERS: &[(&str, &str, &str)] = &[
    ("chrome", "Chrome", "chrome.exe"),
    ("msedge", "Edge", "msedge.exe"),
    ("brave", "Brave", "brave.exe"),
    ("opera", "Opera", "opera.exe"),
    ("vivaldi", "Vivaldi", "vivaldi.exe"),
    ("firefox", "Firefox", "firefox.exe"),
];

/// Firefox and the browsers built on it, which name their session with a hash of where they're installed.
const FIREFOX_FAMILY: &str = "firefox.exe|zen.exe|librewolf.exe|floorp.exe|waterfox.exe";

fn hashed(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// The id's first part, lowercase: "chrome" from `Chrome`, `chrome.exe` or `Chrome._crx_abc`.
fn head(id: &str) -> String {
    let file = id.rsplit('\\').next().unwrap_or(id);
    file.split(['.', '!']).next().unwrap_or(file).to_ascii_lowercase()
}

fn browser(id: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let h = head(id);
    BROWSERS.iter().find(|b| b.0 == h)
}

/// `display`: the app's name as Windows knows it (its Start menu name, or a Firefox-family
/// browser's registration), if it has one.
pub fn identify(id: &str, display: Option<&str>) -> Player {
    let l = id.to_ascii_lowercase();
    let d = display.map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    // An app installed from a browser (Chrome._crx_…) plays through the browser.
    let web_app = l.contains("_crx_");
    for k in MUSIC {
        if k.ids.iter().any(|p| l.contains(p) || d.contains(p)) {
            let exe = match browser(id) {
                Some(b) if web_app => b.2.to_string(),
                _ => k.exe.to_string(),
            };
            return Player { source: k.source, name: k.name.into(), exe, rank: k.rank };
        }
    }
    let named = |fallback: &str| display.filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_string()).unwrap_or_else(|| fallback.to_string());
    if hashed(id) {
        return Player { source: "browser", name: named("Firefox"), exe: FIREFOX_FAMILY.into(), rank: 3 };
    }
    if let Some(&(_, name, exe)) = browser(id) {
        // A web app other than a music one is still the browser, under its own name.
        return Player { source: if web_app { "other" } else { "browser" }, name: named(name), exe: exe.into(), rank: if web_app { 2 } else { 3 } };
    }
    let exe = if l.ends_with(".exe") { l.rsplit('\\').next().unwrap_or(&l).to_string() } else { String::new() };
    Player { source: "other", name: named(&friendly(id)), exe, rank: 2 }
}

/// A name from the id alone: `Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic` → "Media
/// Player", `foobar2000.exe` → "Foobar2000", `AppleInc.AppleMusicWin_…!App` → "Apple Music Win" →
/// "Apple Music".
pub fn friendly(id: &str) -> String {
    let mut s = id.rsplit('!').next().unwrap_or(id).to_string();
    if let Some(p) = s.rfind('\\') {
        s = s[p + 1..].to_string();
    }
    if s.to_ascii_lowercase().ends_with(".exe") {
        s.truncate(s.len() - 4);
    }
    match s.to_ascii_lowercase().as_str() {
        "microsoft.zunemusic" | "zunemusic" => return "Media Player".into(),
        "app" => {
            let pkg = id.split('_').next().unwrap_or(id);
            s = pkg.rsplit('.').next().unwrap_or(pkg).to_string();
        }
        _ => {}
    }
    let last = s.rsplit('.').next().unwrap_or(&s).to_string();
    // "AppleMusicWin" -> "Apple Music"
    let mut words: Vec<String> = vec![];
    for ch in last.chars() {
        if ch.is_uppercase() && words.last().is_some_and(|w| w.chars().last().is_some_and(|p| p.is_lowercase())) {
            words.push(String::new());
        }
        match words.last_mut() {
            Some(w) => w.push(ch),
            None => words.push(ch.to_string()),
        }
    }
    while words.len() > 1 && matches!(words.last().map(|w| w.as_str()), Some("Win" | "Desktop" | "App" | "Windows")) {
        words.pop();
    }
    let name = words.join(" ");
    let mut c = name.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, display: Option<&str>) -> (&'static str, String, String, u8) {
        let p = identify(id, display);
        (p.source, p.name, p.exe, p.rank)
    }

    #[test]
    fn music_apps_from_the_store_and_the_web() {
        assert_eq!(p("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App", None).0, "apple");
        assert_eq!(p("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App", None).2, "amplibraryagent.exe|applemusic.exe");
        assert_eq!(p("Spotify.exe", None), ("spotify", "Spotify".into(), "spotify.exe".into(), 1));
        assert_eq!(p("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", None).0, "spotify");
        assert_eq!(p("Amazon Music.exe", None), ("amazon", "Amazon Music".into(), "amazon music.exe|amazonmusic.exe".into(), 1));
        assert_eq!(p("AmazonMobileLLC.AmazonMusic_kc6t79cpj4tp0!AmazonMobileLLC.AmazonMusic", None).1, "Amazon Music");
        // An id Windows only has a name for.
        assert_eq!(p("{6D809377-6AF0-444B-8957-A3773F02200E}\\Amazon\\Amazon Music\\Amazon Music.exe", None).0, "amazon");
        assert_eq!(p("Electron.App.12345", Some("Amazon Music")).0, "amazon");
    }

    #[test]
    fn youtube_music_in_its_apps_but_not_in_a_tab() {
        assert_eq!(p("com.github.th-ch.youtube-music", None).1, "YouTube Music");
        assert_eq!(p("YouTube Music.exe", None).0, "youtube");
        assert_eq!(p("app.ytmd", Some("YouTube Music Desktop App")).0, "youtube");
        // Installed from Chrome or Edge: named by Windows, playing through the browser.
        assert_eq!(p("Chrome._crx_cinhimbnkkaeohfgghhklpknlkffjgod", Some("YouTube Music")), ("youtube", "YouTube Music".into(), "chrome.exe".into(), 1));
        assert_eq!(p("MSEdge._crx_cinhimbnkkaeohfgghhklpknlkffjgod", Some("YouTube Music")).2, "msedge.exe");
        // A tab is just the browser.
        assert_eq!(p("Chrome", None), ("browser", "Chrome".into(), "chrome.exe".into(), 3));
    }

    #[test]
    fn browsers_and_everything_else() {
        assert_eq!(p("MSEdge", None).1, "Edge");
        assert_eq!(p("308046B0AF4A39CB", None), ("browser", "Firefox".into(), FIREFOX_FAMILY.into(), 3));
        assert_eq!(p("F0DC299D809B9700", Some("Zen Browser")).1, "Zen Browser");
        assert_eq!(p("Chrome._crx_abcdef", Some("Twitch")), ("other", "Twitch".into(), "chrome.exe".into(), 2));
        assert_eq!(p("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic", None).1, "Media Player");
        assert_eq!(p("C:\\Program Files\\foobar2000\\foobar2000.exe", None), ("other", "Foobar2000".into(), "foobar2000.exe".into(), 2));
        assert_eq!(p("VLC.exe", Some("VLC media player")).1, "VLC media player");
        // Music apps outrank other players, which outrank browsers.
        assert!(identify("Spotify.exe", None).rank < identify("VLC.exe", None).rank);
        assert!(identify("VLC.exe", None).rank < identify("Chrome", None).rank);
    }
}
