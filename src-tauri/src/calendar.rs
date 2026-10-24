//! The Command Center's calendar: one view of
//! - your own events (kept in calendar.json),
//! - any calendar feeds you add: Google Calendar, iCloud and Outlook each give a private iCal link,
//!   no account access needed (the links are stored DPAPI-encrypted, since anyone with one can read
//!   the calendar),
//! - Canvas due dates (when Canvas is connected) and Destiny's weekly reset (when Destiny is).

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
}

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
    /// "local", "feed", "canvas" or "destiny".
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
    let r = crate::canvas::fetch(&unprotect(&f.url)).and_then(|text| {
        if text.contains("BEGIN:VCALENDAR") { Ok(ics::parse(&text)) } else { Err("That link didn't return a calendar".into()) }
    });
    FEEDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(f.id.clone(), (Instant::now(), r.clone()));
    r
}

fn builtin(dir: &Path) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut v = vec![("local", "My events", "#c9e9ff")];
    if crate::canvas::status(dir).configured {
        v.push(("canvas", "Canvas", "#f5c26b"));
    }
    if crate::destiny::status(dir).configured {
        v.push(("destiny", "Destiny 2", "#c79bff"));
    }
    v
}

pub fn sources(dir: &Path) -> Vec<Source> {
    let s = load(dir);
    let errors: HashMap<String, String> = FEEDS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.iter().filter_map(|(k, (_, r))| r.as_ref().err().map(|e| (k.clone(), e.clone()))).collect())
        .unwrap_or_default();
    let mut out: Vec<Source> = builtin(dir)
        .into_iter()
        .map(|(id, name, color)| Source { id: id.into(), name: name.into(), kind: id.into(), color: s.colors.get(id).cloned().unwrap_or_else(|| color.into()), on: !s.hidden.iter().any(|h| h == id), error: String::new() })
        .collect();
    out.extend(s.feeds.iter().map(|f| Source { id: f.id.clone(), name: f.name.clone(), kind: "feed".into(), color: f.color.clone(), on: !s.hidden.contains(&f.id), error: errors.get(&f.id).cloned().unwrap_or_default() }));
    out
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
    if on("canvas") && crate::canvas::status(dir).configured {
        if let Ok(o) = crate::canvas::overview(dir) {
            let c = color("canvas", "#f5c26b");
            for t in o.upcoming.iter().chain(o.missing.iter()) {
                let Some(due) = crate::canvas::parse_iso(&t.due) else { continue };
                if due < from || due >= to {
                    continue;
                }
                let course = o.courses.iter().find(|x| x.id == t.course_id);
                out.push(CalEvent {
                    id: format!("canvas:{}", t.key),
                    source: "canvas".into(),
                    title: t.title.clone(),
                    start: due,
                    end: due,
                    all_day: t.all_day,
                    color: course.map(|x| x.color.clone()).filter(|x| !x.is_empty()).unwrap_or_else(|| c.clone()),
                    location: course.map(|x| if x.code.is_empty() { x.name.clone() } else { x.code.clone() }).unwrap_or_default(),
                    notes: if t.done || t.submitted { "Done".into() } else if t.missing { "Missing".into() } else { String::new() },
                    url: t.url.clone(),
                    editable: false,
                });
            }
        }
    }
    if on("destiny") && crate::destiny::status(dir).configured {
        // Weekly reset: Tuesdays 17:00 UTC.
        let c = color("destiny", "#c79bff");
        let mut day = from.div_euclid(DAY);
        while day * DAY < to {
            let t = day * DAY + 17 * 3_600_000;
            if ics::weekday(day) == 1 && t >= from && t < to {
                out.push(CalEvent { id: format!("destiny:{day}"), source: "destiny".into(), title: "Weekly reset".into(), start: t, end: t, all_day: false, color: c.clone(), location: "Destiny 2".into(), notes: String::new(), url: String::new(), editable: false });
            }
            day += 1;
        }
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
    let text = crate::canvas::fetch(&url)?;
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
    fn feed_links() {
        assert_eq!(clean_url("webcal://p01-caldav.icloud.com/published/2/abc").as_deref(), Some("https://p01-caldav.icloud.com/published/2/abc"));
        assert!(clean_url("https://calendar.google.com/calendar/ical/x/private-y/basic.ics").is_some());
        assert!(clean_url("http://insecure.example/cal.ics").is_none());
        assert!(clean_url("file:///c:/x.ics").is_none());
    }
}
