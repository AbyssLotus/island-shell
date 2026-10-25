//! The Command Center's calendar: one view of
//! - your own events (kept in calendar.json),
//! - any calendar feeds you add: Google Calendar, iCloud and Outlook each give a private iCal link,
//!   no account access needed (the links are stored DPAPI-encrypted, since anyone with one can read
//!   the calendar),
//! - Canvas due dates (when Canvas is connected),
//! - and the sources modules declare (`contributes.calendar`): a module replaces its events with
//!   `host.calendar`, and the last set is kept here so the calendar works while the module isn't running.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::ics::{self, VEvent, DAY};
use crate::util::{protect, unprotect};

const PALETTE: [&str; 8] = ["#7fb2ff", "#ff9a8a", "#9be38a", "#f5c26b", "#c79bff", "#6ee0d0", "#ff8fc8", "#d6e66b"];

#[derive(Serialize, Deserialize, Clone)]
struct Feed {
    id: String,
    name: String,
    /// DPAPI-encrypted, base64.
    url: String,
    color: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Local {
    #[serde(default)]
    pub id: String,
    pub title: String,
    /// UTC ms.
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub location: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default)]
    feeds: Vec<Feed>,
    #[serde(default)]
    events: Vec<Local>,
    /// Sources switched off in the calendar list.
    #[serde(default)]
    hidden: Vec<String>,
    /// Colors picked for the built-in sources.
    #[serde(default)]
    colors: HashMap<String, String>,
    /// Module sources by source id, with their last events.
    #[serde(default)]
    modules: HashMap<String, ModuleSource>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct ModuleSource {
    module: String,
    name: String,
    color: String,
    #[serde(default)]
    events: Vec<ModuleEvent>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct ModuleEvent {
    id: String,
    title: String,
    start: i64,
    end: i64,
    #[serde(default)]
    all_day: bool,
    #[serde(default)]
    url: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    notes: String,
}

/// Module sources whose module is installed and on (the rest stay cached but hidden).
static ACTIVE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn path(dir: &Path) -> PathBuf {
    dir.join("calendar.json")
}

fn load(dir: &Path) -> Saved {
    std::fs::read(path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(path(dir), b);
    }
}

#[derive(Serialize, Clone)]
pub struct Source {
    pub id: String,
    pub name: String,
    /// "local", "feed" or "module".
    pub kind: String,
    pub color: String,
    pub on: bool,
    /// Why a feed couldn't be read last time.
    pub error: String,
}

#[derive(Serialize, Clone)]
pub struct CalEvent {
    pub id: String,
    pub source: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub color: String,
    pub location: String,
    pub notes: String,
    pub url: String,
    pub editable: bool,
}

/// Parsed feeds, re-read at most every 15 minutes.
static FEEDS: Mutex<Option<HashMap<String, (Instant, Result<Vec<VEvent>, String>)>>> = Mutex::new(None);

fn feed_events(f: &Feed) -> Result<Vec<VEvent>, String> {
    if let Some((at, r)) = FEEDS.lock().unwrap().as_ref().and_then(|m| m.get(&f.id)) {
        if at.elapsed() < Duration::from_secs(900) {
            return r.clone();
        }
    }
    let r = ics::fetch(&unprotect(&f.url)).and_then(|text| {
        if text.contains("BEGIN:VCALENDAR") { Ok(ics::parse(&text)) } else { Err("That link didn't return a calendar".into()) }
    });
    FEEDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(f.id.clone(), (Instant::now(), r.clone()));
    r
}

fn builtin() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![("local", "My events", "#c9e9ff")]
}

pub fn sources(dir: &Path) -> Vec<Source> {
    let s = load(dir);
    let errors: HashMap<String, String> = FEEDS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.iter().filter_map(|(k, (_, r))| r.as_ref().err().map(|e| (k.clone(), e.clone()))).collect())
        .unwrap_or_default();
    let mut out: Vec<Source> = builtin()
        .into_iter()
        .map(|(id, name, color)| Source { id: id.into(), name: name.into(), kind: id.into(), color: s.colors.get(id).cloned().unwrap_or_else(|| color.into()), on: !s.hidden.iter().any(|h| h == id), error: String::new() })
        .collect();
    out.extend(s.feeds.iter().map(|f| Source { id: f.id.clone(), name: f.name.clone(), kind: "feed".into(), color: f.color.clone(), on: !s.hidden.contains(&f.id), error: errors.get(&f.id).cloned().unwrap_or_default() }));
    let active = ACTIVE.lock().unwrap().clone().unwrap_or_default();
    let mut mods: Vec<_> = s.modules.iter().filter(|(id, _)| active.contains(*id)).collect();
    mods.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    out.extend(mods.into_iter().map(|(id, m)| Source { id: id.clone(), name: m.name.clone(), kind: "module".into(), color: s.colors.get(id).cloned().unwrap_or_else(|| m.color.clone()), on: !s.hidden.contains(id), error: String::new() }));
    out
}

/// The module sources that are live now: `(module, source, name, color)` for every enabled module.
/// New sources are added; known ones keep their events and the user's color.
pub fn set_module_sources(dir: &Path, list: &[(String, String, String, String)]) {
    let mut s = load(dir);
    let mut dirty = false;
    for (module, source, name, color) in list {
        let e = s.modules.entry(source.clone()).or_insert_with(|| {
            dirty = true;
            ModuleSource { module: module.clone(), name: name.clone(), color: color.clone(), events: vec![] }
        });
        if e.name != *name || e.module != *module {
            e.name = name.clone();
            e.module = module.clone();
            dirty = true;
        }
    }
    if dirty {
        save(dir, &s);
    }
    *ACTIVE.lock().unwrap() = Some(list.iter().map(|(_, source, ..)| source.clone()).collect());
}

/// Forgets a removed module's sources and their events.
pub fn forget_module(dir: &Path, module: &str) {
    let mut s = load(dir);
    s.modules.retain(|_, m| m.module != module);
    save(dir, &s);
}

/// `host.calendar`: replaces all of a source's events (RFC 3339 times; bad ones are skipped).
pub fn put_module_events(dir: &Path, module: &str, source: &str, events: &[island_host::proto::msg::CalendarEvent]) {
    let mut s = load(dir);
    let Some(src) = s.modules.get_mut(source).filter(|m| m.module == module) else { return };
    src.events = events
        .iter()
        .take(2000)
        .filter_map(|e| {
            let (start, all_day) = parse_rfc3339(&e.start)?;
            let end = e.end.as_deref().and_then(parse_rfc3339).map(|(t, _)| t).unwrap_or(start);
            Some(ModuleEvent {
                id: e.id.chars().take(200).collect(),
                title: e.title.chars().take(200).collect(),
                start,
                end: end.max(start),
                all_day: e.all_day || all_day,
                url: e.url.clone().filter(|u| u.starts_with("https://")).unwrap_or_default(),
                color: e.color.clone().filter(|c| c.starts_with('#') && c.len() <= 9).unwrap_or_default(),
                kind: e.kind.clone().unwrap_or_default(),
                location: e.location.as_deref().unwrap_or("").chars().take(120).collect(),
                notes: e.notes.as_deref().unwrap_or("").chars().take(200).collect(),
            })
        })
        .collect();
    save(dir, &s);
}

/// Unix ms from RFC 3339 (`2026-10-27T17:00:00Z`, `…+02:00`, fractions allowed), or a bare date
/// (`2026-10-27`, which is an all-day date at local midnight).
pub fn parse_rfc3339(t: &str) -> Option<(i64, bool)> {
    let t = t.trim();
    let n = |a: usize, l: usize| t.get(a..a + l).filter(|x| x.bytes().all(|b| b.is_ascii_digit())).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (n(0, 4)?, n(5, 2)?, n(8, 2)?);
    if t.as_bytes().get(4) != Some(&b'-') || t.as_bytes().get(7) != Some(&b'-') || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let days = ics::days_from_ymd(y, mo, d);
    if t.len() == 10 {
        return Some((ics::to_utc(days * DAY), true));
    }
    if !matches!(t.as_bytes().get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let (h, mi, se) = (n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let mut rest = &t[19..];
    let mut frac_ms = 0;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        frac_ms = format!("{digits:0<3}")[..3].parse::<i64>().ok()?;
        rest = &r[digits.len()..];
    }
    let offset_min = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (oh * 60 + om)
        }
    };
    Some((days * DAY + ((h * 60 + mi - offset_min) * 60 + se) * 1000 + frac_ms, false))
}

/// Every event overlapping [from, to), from the sources that are switched on.
pub fn events(dir: &Path, from: i64, to: i64) -> Vec<CalEvent> {
    let s = load(dir);
    let on = |id: &str| !s.hidden.iter().any(|h| h == id);
    let color = |id: &str, def: &str| s.colors.get(id).cloned().unwrap_or_else(|| def.to_string());
    let mut out = vec![];
    if on("local") {
        let c = color("local", "#c9e9ff");
        out.extend(s.events.iter().filter(|e| e.start < to && e.end.max(e.start + 1) > from).map(|e| CalEvent {
            id: e.id.clone(),
            source: "local".into(),
            title: e.title.clone(),
            start: e.start,
            end: e.end,
            all_day: e.all_day,
            color: if e.color.is_empty() { c.clone() } else { e.color.clone() },
            location: e.location.clone(),
            notes: e.notes.clone(),
            url: String::new(),
            editable: true,
        }));
    }
    for f in s.feeds.iter().filter(|f| on(&f.id)) {
        let Ok(list) = feed_events(f) else { continue };
        // Moved or cancelled single occurrences replace the series' own.
        let moved: HashSet<(String, i64)> = list.iter().filter_map(|e| e.recurrence_id.map(|r| (e.uid.clone(), r))).collect();
        for e in &list {
            if e.cancelled && e.recurrence_id.is_none() {
                continue;
            }
            let dur = e.end - e.start;
            for start in ics::occurrences(e, from, to) {
                if e.recurrence_id.is_none() && moved.contains(&(e.uid.clone(), start)) {
                    continue;
                }
                if e.cancelled {
                    continue;
                }
                out.push(CalEvent {
                    id: format!("{}:{}:{start}", f.id, e.uid),
                    source: f.id.clone(),
                    title: if e.summary.is_empty() { "(No title)".into() } else { e.summary.clone() },
                    start,
                    end: start + dur,
                    all_day: e.all_day,
                    color: f.color.clone(),
                    location: e.location.clone(),
                    notes: e.description.chars().take(1500).collect(),
                    url: e.url.clone(),
                    editable: false,
                });
            }
        }
    }
    let active = ACTIVE.lock().unwrap().clone().unwrap_or_default();
    for (id, m) in s.modules.iter().filter(|(id, _)| active.contains(*id) && on(id)) {
        let c = color(id, &m.color);
        out.extend(m.events.iter().filter(|e| e.start < to && e.end.max(e.start + 1) > from).map(|e| CalEvent {
            id: format!("{id}:{}", e.id),
            source: id.clone(),
            title: e.title.clone(),
            start: e.start,
            end: e.end,
            all_day: e.all_day,
            color: if e.color.is_empty() { c.clone() } else { e.color.clone() },
            location: if e.location.is_empty() { m.name.clone() } else { e.location.clone() },
            notes: e.notes.clone(),
            url: e.url.clone(),
            editable: false,
        }));
    }
    out.sort_by_key(|e| (e.start, !e.all_day));
    out
}

/// "webcal://…" → "https://…".
fn clean_url(url: &str) -> Option<String> {
    let u = url.trim();
    let u = u.strip_prefix("webcal://").map(|r| format!("https://{r}")).unwrap_or_else(|| u.to_string());
    (u.starts_with("https://") && !u.contains([' ', '"', '<', '>'])).then_some(u)
}

pub fn add_feed(dir: &Path, name: &str, url: &str) -> Result<Vec<Source>, String> {
    let url = clean_url(url).ok_or("Paste the calendar's iCal link (it starts with https:// or webcal://)")?;
    let text = ics::fetch(&url)?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err("That link didn't return a calendar. Use the private or public iCal (.ics) address".into());
    }
    let mut s = load(dir);
    let id = format!("feed{}", crate::util::now_ms());
    let name = if name.trim().is_empty() {
        text.lines().find_map(|l| l.strip_prefix("X-WR-CALNAME:")).map(|n| ics::unescape(n.trim())).unwrap_or_else(|| "Calendar".into())
    } else {
        name.trim().to_string()
    };
    let color = PALETTE[s.feeds.len() % PALETTE.len()].to_string();
    FEEDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.clone(), (Instant::now(), Ok(ics::parse(&text))));
    s.feeds.push(Feed { id, name, url: protect(&url), color });
    save(dir, &s);
    Ok(sources(dir))
}

pub fn remove_feed(dir: &Path, id: &str) -> Vec<Source> {
    let mut s = load(dir);
    s.feeds.retain(|f| f.id != id);
    s.hidden.retain(|h| h != id);
    save(dir, &s);
    if let Some(m) = FEEDS.lock().unwrap().as_mut() {
        m.remove(id);
    }
    sources(dir)
}

/// Shows/hides a source, or changes its color.
pub fn set_source(dir: &Path, id: &str, on: Option<bool>, color: Option<String>) -> Vec<Source> {
    let mut s = load(dir);
    if let Some(on) = on {
        s.hidden.retain(|h| h != id);
        if !on {
            s.hidden.push(id.to_string());
        }
    }
    if let Some(c) = color.filter(|c| c.starts_with('#') && c.len() <= 9) {
        match s.feeds.iter_mut().find(|f| f.id == id) {
            Some(f) => f.color = c,
            None => {
                s.colors.insert(id.to_string(), c);
            }
        }
    }
    save(dir, &s);
    sources(dir)
}

pub fn save_event(dir: &Path, mut ev: Local) -> Result<Local, String> {
    if ev.title.trim().is_empty() {
        return Err("Give the event a name".into());
    }
    if ev.end < ev.start {
        return Err("It ends before it starts".into());
    }
    ev.title = ev.title.trim().chars().take(200).collect();
    let mut s = load(dir);
    if ev.id.is_empty() {
        ev.id = format!("local{}", crate::util::now_ms());
        s.events.push(ev.clone());
    } else if let Some(x) = s.events.iter_mut().find(|x| x.id == ev.id) {
        *x = ev.clone();
    } else {
        return Err("That event is gone".into());
    }
    save(dir, &s);
    Ok(ev)
}

pub fn delete_event(dir: &Path, id: &str) {
    let mut s = load(dir);
    s.events.retain(|e| e.id != id);
    save(dir, &s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("2026-10-27T17:00:00Z"), Some((1_793_120_400_000, false)));
        assert_eq!(parse_rfc3339("2026-10-27T19:00:00+02:00"), Some((1_793_120_400_000, false)));
        assert_eq!(parse_rfc3339("2026-10-27T12:00:00.250-05:00"), Some((1_793_120_400_250, false)));
        assert_eq!(parse_rfc3339("2026-10-27T17:00:00.5Z"), Some((1_793_120_400_500, false)));
        assert!(parse_rfc3339("2026-10-27").is_some_and(|(_, all_day)| all_day));
        for bad in ["", "2026-13-01T00:00:00Z", "2026/10/27", "2026-10-27T17:00:00", "2026-10-27T17:00:00+2", "tomorrow"] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn feed_links() {
        assert_eq!(clean_url("webcal://p01-caldav.icloud.com/published/2/abc").as_deref(), Some("https://p01-caldav.icloud.com/published/2/abc"));
        assert!(clean_url("https://calendar.google.com/calendar/ical/x/private-y/basic.ics").is_some());
        assert!(clean_url("http://insecure.example/cal.ics").is_none());
        assert!(clean_url("file:///c:/x.ics").is_none());
    }
}
