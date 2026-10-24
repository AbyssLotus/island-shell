//! Time-synced lyrics from LRCLIB (lrclib.net, a free, open lyrics database), cached on disk.
//!
//! Apple Music shows its own lyrics, but only inside its window, so the island looks songs up by
//! title, artist, album and length. Only those four values are sent. Misses are remembered for a
//! while so a song without lyrics isn't looked up on every play.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;

const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = "IslandShell/0.1 (personal Windows desktop shell)";
/// How long to wait before asking again about a song LRCLIB didn't have.
const RETRY_MISS_MS: u64 = 12 * 60 * 60 * 1000;

#[derive(Serialize, Deserialize, Clone)]
pub struct Line {
    /// Seconds into the song.
    pub t: f64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Lyrics {
    /// Timed lines, when LRCLIB has synced lyrics.
    pub lines: Vec<Line>,
    /// Untimed text, used when there are no synced lyrics.
    pub plain: String,
    pub instrumental: bool,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    found: Option<Lyrics>,
    at: u64,
}

pub fn fetch(dir: &Path, title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Lyrics>, String> {
    if title.trim().is_empty() {
        return Ok(None);
    }
    let cache_dir = dir.join("lyrics");
    let file = cache_dir.join(format!("{:016x}.json", fnv(&format!("{artist}\n{title}\n{album}"))));
    if let Some(c) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok()) {
        if c.found.is_some() || crate::util::now_ms().saturating_sub(c.at) < RETRY_MISS_MS {
            return Ok(c.found);
        }
    }
    let found = lookup(title, artist, album, duration)?;
    let _ = std::fs::create_dir_all(&cache_dir);
    let cached = Cached { found, at: crate::util::now_ms() };
    if let Ok(b) = serde_json::to_vec(&cached) {
        let _ = std::fs::write(&file, b);
    }
    Ok(cached.found)
}

fn lookup(title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Lyrics>, String> {
    // An exact match first: LRCLIB matches the duration within a couple of seconds. This endpoint
    // also asks outside sources on a miss and sometimes answers 503, so errors fall through to
    // the search below rather than failing the lookup.
    if duration > 0.0 {
        let url = format!(
            "{API}/get?track_name={}&artist_name={}&album_name={}&duration={}",
            enc(title),
            enc(artist),
            enc(album),
            duration.round() as i64
        );
        match get(&url) {
            Ok(Some(body)) => {
                if let Some(l) = serde_json::from_str::<Value>(&body).ok().as_ref().and_then(from_record) {
                    return Ok(Some(l));
                }
            }
            Ok(None) => {}
            Err(e) => crate::util::log_line(&format!("{e}; trying a search instead")),
        }
    }
    // Then a search with a cleaned-up title ("Song (feat. X)" → "Song") and the first artist only.
    let url = format!("{API}/search?track_name={}&artist_name={}", enc(&clean_title(title)), enc(&main_artist(artist)));
    let Some(body) = get(&url)? else { return Ok(None) };
    let results: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();
    let best = results
        .iter()
        .filter_map(|r| {
            let d = r["duration"].as_f64().unwrap_or(0.0);
            let off = if duration > 0.0 && d > 0.0 { (d - duration).abs() } else { 0.0 };
            if off > 8.0 {
                return None;
            }
            let synced = r["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty());
            Some((off + if synced { 0.0 } else { 20.0 }, r))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    Ok(best.and_then(|(_, r)| from_record(r)))
}

fn from_record(r: &Value) -> Option<Lyrics> {
    let lines = parse_lrc(r["syncedLyrics"].as_str().unwrap_or(""));
    let plain = r["plainLyrics"].as_str().unwrap_or("").trim().to_string();
    let instrumental = r["instrumental"].as_bool().unwrap_or(false);
    (!lines.is_empty() || !plain.is_empty() || instrumental).then_some(Lyrics { lines, plain, instrumental })
}

/// Parses "[01:23.45] words" lines (a line can carry several time tags).
fn parse_lrc(s: &str) -> Vec<Line> {
    let mut out = vec![];
    for raw in s.lines() {
        let mut rest = raw.trim();
        let mut times = vec![];
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            match parse_time(&stripped[..end]) {
                Some(t) => times.push(t),
                None => break, // a tag like [ar:Artist]
            }
            rest = &stripped[end + 1..];
        }
        for t in times {
            out.push(Line { t, text: rest.trim().to_string() });
        }
    }
    out.sort_by(|a, b| a.t.total_cmp(&b.t));
    out
}

fn parse_time(tag: &str) -> Option<f64> {
    let (m, s) = tag.split_once(':')?;
    let m: f64 = m.trim().parse().ok()?;
    let s: f64 = s.trim().parse().ok()?;
    Some(m * 60.0 + s)
}

/// Drops "(feat. …)", "[Remastered]" and the like from a title.
fn clean_title(t: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut group = String::new();
    for ch in t.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    group.clear();
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let g = group.to_lowercase();
                    let noise = ["feat", "ft.", "with ", "remaster", "version", "edit", "live", "mono", "stereo"];
                    if !noise.iter().any(|n| g.contains(n)) {
                        out.push_str(&format!("({group})"));
                    }
                }
            }
            _ if depth > 0 => group.push(ch),
            _ => out.push(ch),
        }
    }
    let out = out.split(" - ").next().unwrap_or(&out).trim().to_string();
    if out.is_empty() { t.to_string() } else { out }
}

fn main_artist(a: &str) -> String {
    let mut s = a;
    for sep in [" & ", ", ", " feat", " ft.", " x ", " and "] {
        if let Some(i) = s.find(sep) {
            s = &s[..i];
        }
    }
    s.trim().to_string()
}

/// GET → Some(body), None for 404 (LRCLIB's "not found").
fn get(url: &str) -> Result<Option<String>, String> {
    let e = |e: windows::core::Error| format!("lyrics lookup failed: {}", e.message());
    let client = HttpClient::new().map_err(e)?;
    let _ = client.DefaultRequestHeaders().and_then(|h| h.UserAgent()).and_then(|ua| ua.TryParseAdd(&HSTRING::from(USER_AGENT)));
    let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(e)?;
    let resp = client.GetAsync(&uri).and_then(|op| op.get()).map_err(e)?;
    let status = resp.StatusCode().map_err(e)?.0;
    if status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(format!("lyrics lookup failed: HTTP {status}"));
    }
    let body = resp.Content().and_then(|c| c.ReadAsStringAsync()).and_then(|op| op.get()).map_err(e)?;
    Ok(Some(body.to_string()))
}

/// Percent-encodes a query value.
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
