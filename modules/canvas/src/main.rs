//! Canvas (the school LMS), an Island Shell module backend (docs/design/modules.md); its UI is
//! ui/canvas.js. Two ways in:
//!
//! - **Calendar feed** (works when a school blocks access tokens): the private .ics link Canvas
//!   gives every student under Calendar → Calendar Feed, meant for calendar apps. It has every due
//!   date and course event with its course and link, but no grades, submission status or
//!   announcements. "Done" ticks are kept on this PC (`canvas-done.json`).
//! - **Access token** (Account → Settings → New Access Token), when the school allows them:
//!   everything below.
//!
//! - What's coming up (Canvas's own planner: assignments, quizzes, discussions…) with submission
//!   status, and marking items done the way the Canvas app's checkbox does (planner overrides).
//! - Missing work, current grades per course, recent announcements.
//! - Notifications from a check every 10 minutes (the `scheduled` tick): due in under 3 hours (shown
//!   on top of everything), newly graded work with the score, new announcements.
//! - Due dates in the island's calendar (source `canvas`), refreshed with every read.
//!
//! With the feed, notifications are due-soon reminders and newly posted assignments.
//!
//! The token or feed link is DPAPI-encrypted in `canvas.json`; what's already been notified is in
//! `canvas-seen.json`. All three files are taken over from 0.1.x. The school's address also goes in
//! settings.json, where the manifest's `{settings.domain}` (its one allowed host, and the docked
//! site) reads it.
//!
//! RPC methods: `status`, `configure { domain, token }`, `configure_feed { url }`, `overview`,
//! `done { key, kind, id, override_id, done }`, `forget`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use island_module::http::{Request, Response};
use island_module::secrets::{protect, unprotect};
use island_module::{json, local_offset_ms, now_ms, CalendarEvent, CallResult, Host, Init, Module, Notify, RpcError, Value};
use serde::{Deserialize, Serialize};

/// How soon before a due date the reminder comes.
const REMIND_MS: i64 = 3 * 3600 * 1000;

// ---------- settings ----------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    #[serde(default)]
    domain: String,
    /// DPAPI-encrypted, base64.
    #[serde(default)]
    token: String,
    /// The calendar feed link, DPAPI-encrypted (it works like a password: anyone with it sees the calendar).
    #[serde(default)]
    feed: String,
    #[serde(default)]
    name: String,
}

fn load(dir: &Path) -> Saved {
    std::fs::read(dir.join("canvas.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(dir.join("canvas.json"), b);
    }
    // The address, where the manifest's {settings.domain} finds it (empty when forgotten).
    let _ = std::fs::write(dir.join("settings.json"), json!({ "domain": s.domain }).to_string());
}

#[derive(Serialize, Clone, Default)]
struct Status {
    configured: bool,
    /// "feed" (calendar feed only) or "token".
    mode: String,
    domain: String,
    name: String,
}

fn status(dir: &Path) -> Status {
    let s = load(dir);
    let mode = if !s.feed.is_empty() { "feed" } else { "token" };
    Status { configured: !s.domain.is_empty() && (!s.token.is_empty() || !s.feed.is_empty()), mode: mode.into(), domain: s.domain, name: s.name }
}

/// "https://school.instructure.com/courses/1" or "canvas.school.edu" → "canvas.school.edu".
fn clean_domain(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches("https://").trim_start_matches("http://");
    let host = s.split(['/', '?', '#']).next().unwrap_or("").trim().to_ascii_lowercase();
    let ok = host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') && !host.starts_with('.') && !host.ends_with('.');
    ok.then_some(host)
}

// ---------- HTTP ----------

struct Api {
    domain: String,
    token: String,
}

impl Api {
    fn from(dir: &Path) -> Result<Api, String> {
        let s = load(dir);
        let token = unprotect(&s.token);
        if s.domain.is_empty() || token.is_empty() {
            return Err("Canvas isn't set up yet.".into());
        }
        Ok(Api { domain: s.domain, token })
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let url = format!("https://{}/api/v1{path}", self.domain);
        let mut req = match body {
            Some(b) => Request::post(&url, "application/json", b.to_string()),
            None => Request::get(&url),
        };
        req.method = method.into();
        let resp = req.header("Authorization", &format!("Bearer {}", self.token)).header("Accept", "application/json").send()?;
        // Canvas may prefix JSON with "while(1);" (anti-hijacking).
        let v: Value = serde_json::from_str(resp.text().trim_start_matches("while(1);")).unwrap_or(Value::Null);
        match resp.status {
            200..=299 => Ok(v),
            401 => Err("Canvas didn't accept the access token (it may have expired or been deleted).".into()),
            403 => Err("Canvas says this token isn't allowed to see that.".into()),
            404 => Err(format!("{} doesn't look like a Canvas address.", self.domain)),
            s => Err(format!("Canvas answered HTTP {s}{}", v["errors"][0]["message"].as_str().map(|m| format!(": {m}")).unwrap_or_default())),
        }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        self.call("GET", path, None)
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with("http") { path.to_string() } else { format!("https://{}{path}", self.domain) }
    }
}

/// A plain GET (the feed needs no sign-in; its link is the key).
fn fetch(url: &str) -> Result<String, String> {
    let resp: Response = Request::get(url).send()?;
    if !resp.ok() {
        return Err(if resp.status == 404 || resp.status == 401 { "Canvas didn't recognize that feed link (it may have been reset).".into() } else { format!("Canvas answered HTTP {}", resp.status) });
    }
    Ok(resp.text())
}

// ---------- setup ----------

/// "webcal://school.instructure.com/feeds/calendars/user_….ics" → the https link, if it is one.
fn clean_feed(input: &str) -> Option<String> {
    let s = input.trim();
    let s = s.strip_prefix("webcal://").map(|r| format!("https://{r}")).unwrap_or_else(|| s.to_string());
    let host = clean_domain(&s)?;
    let path = s.strip_prefix(&format!("https://{host}"))?;
    (path.starts_with("/feeds/calendars/") && path.split('?').next().unwrap_or("").ends_with(".ics") && !s.contains(['"', ' ', '<', '>'])).then_some(s)
}

// ---------- overview ----------

#[derive(Serialize, Clone, Default)]
struct Course {
    id: u64,
    name: String,
    code: String,
    color: String,
    score: Option<f64>,
    grade: String,
    url: String,
}

#[derive(Serialize, Clone, Default)]
struct Task {
    /// Unique per item ("assignment:12", or the feed's UID).
    key: String,
    /// "assignment", "quiz", "discussion_topic", "wiki_page", "planner_note", "calendar_event"…
    kind: String,
    id: u64,
    title: String,
    course_id: u64,
    /// ISO date it's due (or its to-do date).
    due: String,
    points: Option<f64>,
    url: String,
    submitted: bool,
    graded: bool,
    missing: bool,
    late: bool,
    /// Ticked off (in Canvas's planner; with the feed, on this PC).
    done: bool,
    /// A date without a time (feed events).
    all_day: bool,
    override_id: Option<u64>,
    /// For graded work: the assignment behind it (quizzes have their own id).
    #[serde(skip)]
    assignment_id: Option<u64>,
}

#[derive(Serialize, Clone, Default)]
struct Announcement {
    id: u64,
    title: String,
    course_id: u64,
    posted: String,
    url: String,
    preview: String,
}

#[derive(Serialize, Clone, Default)]
struct Overview {
    name: String,
    domain: String,
    courses: Vec<Course>,
    /// From an hour ago through the next three weeks.
    upcoming: Vec<Task>,
    missing: Vec<Task>,
    announcements: Vec<Announcement>,
    /// From the calendar feed: due dates only (no grades, status, announcements or missing work).
    limited: bool,
    at: u64,
    /// Graded items from the planner window (for the grade notifications).
    #[serde(skip)]
    graded: Vec<Task>,
}

fn iso(ms: i64) -> String {
    // Canvas takes ISO 8601; build it without a date library.
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Unix ms from an ISO 8601 UTC date ("2026-10-14T23:59:00Z").
fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |a: usize, l: usize| s.get(a..a + l)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86400) + h * 3600 + mi * 60 + se) * 1000)
}

fn task_from(api: &Api, it: &Value) -> Task {
    let p = &it["plannable"];
    let sub = &it["submissions"];
    let flag = |k: &str| sub[k].as_bool().unwrap_or(false);
    Task {
        key: format!("{}:{}", it["plannable_type"].as_str().unwrap_or(""), it["plannable_id"].as_u64().unwrap_or(0)),
        kind: it["plannable_type"].as_str().unwrap_or("").to_string(),
        id: it["plannable_id"].as_u64().unwrap_or(0),
        title: p["title"].as_str().or(p["name"].as_str()).unwrap_or("Untitled").to_string(),
        course_id: it["course_id"].as_u64().unwrap_or(0),
        due: p["due_at"].as_str().or(it["plannable_date"].as_str()).unwrap_or("").to_string(),
        points: p["points_possible"].as_f64(),
        url: it["html_url"].as_str().map(|u| api.url(u)).unwrap_or_default(),
        submitted: flag("submitted"),
        graded: flag("graded"),
        missing: flag("missing"),
        late: flag("late"),
        done: it["planner_override"]["marked_complete"].as_bool().unwrap_or(false),
        all_day: false,
        override_id: it["planner_override"]["id"].as_u64(),
        assignment_id: p["assignment_id"].as_u64().or_else(|| (it["plannable_type"] == "assignment").then(|| it["plannable_id"].as_u64()).flatten()),
    }
}

/// Text from an announcement's HTML (tags dropped, entities for the common characters).
fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in html.chars() {
        match c {
            '<' => tag = true,
            '>' => {
                tag = false;
                out.push(' ');
            }
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect()
}

fn token_overview(api: &Api, saved: &Saved) -> Result<Overview, String> {
    let courses_v = api.get("/courses?enrollment_state=active&include[]=total_scores&per_page=100")?;
    let colors = api.get("/users/self/colors").unwrap_or(Value::Null);
    let courses: Vec<Course> = courses_v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["name"].is_string())
        .map(|c| {
            let id = c["id"].as_u64().unwrap_or(0);
            let e = c["enrollments"].as_array().and_then(|a| a.iter().find(|e| e["type"] == "student")).cloned().unwrap_or(Value::Null);
            Course {
                id,
                name: c["name"].as_str().unwrap_or("").to_string(),
                code: c["course_code"].as_str().unwrap_or("").to_string(),
                color: colors["custom_colors"][format!("course_{id}")].as_str().unwrap_or("").to_string(),
                score: e["computed_current_score"].as_f64(),
                grade: e["computed_current_grade"].as_str().unwrap_or("").to_string(),
                url: api.url(&format!("/courses/{id}")),
            }
        })
        .collect();
    let t = now_ms();
    let items = api.get(&format!("/planner/items?start_date={}&end_date={}&per_page=100", iso(t - 14 * 86_400_000), iso(t + 21 * 86_400_000)))?;
    let all: Vec<Task> = items.as_array().into_iter().flatten().map(|it| task_from(api, it)).collect();
    let mut upcoming: Vec<Task> = all.iter().filter(|x| parse_iso(&x.due).is_some_and(|d| d >= t - 3_600_000)).cloned().collect();
    upcoming.sort_by_key(|x| parse_iso(&x.due).unwrap_or(0));
    let graded: Vec<Task> = all.iter().filter(|x| x.graded).cloned().collect();
    let missing: Vec<Task> = api
        .get("/users/self/missing_submissions?include[]=planner_overrides&filter[]=submittable&per_page=100")
        .unwrap_or(Value::Null)
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["planner_override"]["marked_complete"].as_bool() != Some(true))
        .map(|a| Task {
            key: format!("assignment:{}", a["id"].as_u64().unwrap_or(0)),
            kind: "assignment".into(),
            id: a["id"].as_u64().unwrap_or(0),
            title: a["name"].as_str().unwrap_or("Untitled").to_string(),
            course_id: a["course_id"].as_u64().unwrap_or(0),
            due: a["due_at"].as_str().unwrap_or("").to_string(),
            points: a["points_possible"].as_f64(),
            url: a["html_url"].as_str().map(|u| api.url(u)).unwrap_or_default(),
            missing: true,
            override_id: a["planner_override"]["id"].as_u64(),
            ..Default::default()
        })
        .collect();
    let codes: Vec<String> = courses.iter().map(|c| format!("context_codes[]=course_{}", c.id)).collect();
    let announcements: Vec<Announcement> = if codes.is_empty() {
        vec![]
    } else {
        api.get(&format!("/announcements?{}&start_date={}&end_date={}&per_page=20", codes.join("&"), iso(t - 21 * 86_400_000), iso(t + 86_400_000)))
            .unwrap_or(Value::Null)
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| Announcement {
                id: a["id"].as_u64().unwrap_or(0),
                title: a["title"].as_str().unwrap_or("").to_string(),
                course_id: a["context_code"].as_str().and_then(|c| c.trim_start_matches("course_").parse().ok()).unwrap_or(0),
                posted: a["posted_at"].as_str().unwrap_or("").to_string(),
                url: a["html_url"].as_str().map(|u| api.url(u)).unwrap_or_default(),
                preview: plain(a["message"].as_str().unwrap_or("")),
            })
            .collect()
    };
    Ok(Overview { name: saved.name.clone(), domain: api.domain.clone(), courses, upcoming, missing, announcements, limited: false, at: now_ms() as u64, graded })
}

// ---------- calendar feed ----------

/// "Fri Oct 16" for a moment, in this PC's time.
fn local_day(utc_ms: i64) -> String {
    let local = utc_ms + local_offset_ms();
    let days = local.div_euclid(86_400_000);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let d = iso(local);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let m: usize = d[5..7].parse().unwrap_or(1);
    format!("{wd} {} {}", months[m.clamp(1, 12) - 1], d[8..10].trim_start_matches('0'))
}

#[derive(Default, Debug)]
struct Event {
    uid: String,
    summary: String,
    /// UTC ms, and whether it's a date without a time.
    start: Option<(i64, bool)>,
    url: String,
}

/// Unix ms from an iCalendar date or date-time: "20261014T235900Z" (UTC), "20261014T235900"
/// (local, or with a TZID, which Canvas sets to the school's zone), "20261014" (all day).
fn ics_time(value: &str, params: &str, offset: i64) -> Option<(i64, bool)> {
    let v = value.trim();
    let ymd = format!("{}-{}-{}", v.get(0..4)?, v.get(4..6)?, v.get(6..8)?);
    if v.len() == 8 || params.contains("VALUE=DATE") && !params.contains("DATE-TIME") {
        return Some((parse_iso(&format!("{ymd}T00:00:00Z"))? - offset, true));
    }
    let t = v.get(9..15)?;
    let utc = parse_iso(&format!("{ymd}T{}:{}:{}Z", &t[0..2], &t[2..4], &t[4..6]))?;
    Some((if v.ends_with('Z') { utc } else { utc - offset }, false))
}

fn ics_unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

fn parse_ics(text: &str, offset: i64) -> Vec<Event> {
    // Long lines are folded: a line break followed by a space or tab continues the line.
    let unfolded = text.replace("\r\n ", "").replace("\r\n\t", "").replace("\n ", "").replace("\n\t", "");
    let mut out = vec![];
    let mut cur: Option<Event> = None;
    for line in unfolded.lines() {
        let line = line.trim_end_matches('\r');
        match line {
            "BEGIN:VEVENT" => cur = Some(Event::default()),
            "END:VEVENT" => out.extend(cur.take()),
            _ => {
                let Some(ev) = cur.as_mut() else { continue };
                let Some((head, value)) = line.split_once(':') else { continue };
                let (name, params) = head.split_once(';').unwrap_or((head, ""));
                match name {
                    "UID" => ev.uid = value.to_string(),
                    "SUMMARY" => ev.summary = ics_unescape(value),
                    "DTSTART" => ev.start = ics_time(value, params, offset),
                    "URL" => ev.url = value.to_string(),
                    _ => {}
                }
            }
        }
    }
    out
}

/// "Lab 6: AVL Trees [CS 101-001]" → ("Lab 6: AVL Trees", "CS 101-001").
fn split_course(summary: &str) -> (String, String) {
    match summary.trim().strip_suffix(']').and_then(|s| s.rsplit_once(" [")) {
        Some((title, course)) => (title.trim().to_string(), course.trim().to_string()),
        None => (summary.trim().to_string(), String::new()),
    }
}

/// A stable id for a course name (kept under 2^52 so the page's numbers stay exact).
fn course_id(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    h & ((1 << 52) - 1)
}

fn local_done(dir: &Path) -> Vec<String> {
    std::fs::read(dir.join("canvas-done.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn feed_overview(dir: &Path, saved: &Saved) -> Result<Overview, String> {
    let text = fetch(&unprotect(&saved.feed))?;
    let events = parse_ics(&text, local_offset_ms());
    let done = local_done(dir);
    let t = now_ms();
    let mut courses: Vec<Course> = vec![];
    let mut upcoming: Vec<Task> = vec![];
    for ev in &events {
        let Some((start, all_day)) = ev.start else { continue };
        let (title, course) = split_course(&ev.summary);
        let cid = if course.is_empty() { 0 } else { course_id(&course) };
        // Courses with anything from a month back to two months ahead count as current.
        if cid != 0 && start > t - 30 * 86_400_000 && start < t + 60 * 86_400_000 && !courses.iter().any(|c| c.id == cid) {
            courses.push(Course { id: cid, name: course.clone(), code: course.clone(), url: format!("https://{}/calendar", saved.domain), ..Default::default() });
        }
        // All-day events stay listed through their day.
        let shown_from = if all_day { t - 86_400_000 } else { t - 3_600_000 };
        if start < shown_from || start > t + 21 * 86_400_000 {
            continue;
        }
        let key = ev.uid.clone();
        upcoming.push(Task {
            done: done.contains(&key),
            key,
            kind: if ev.uid.contains("assignment") { "assignment".into() } else { "calendar_event".into() },
            title,
            course_id: cid,
            due: iso(start),
            all_day,
            url: if ev.url.starts_with(&format!("https://{}/", saved.domain)) { ev.url.clone() } else { format!("https://{}/calendar", saved.domain) },
            ..Default::default()
        });
    }
    upcoming.sort_by_key(|x| parse_iso(&x.due).unwrap_or(0));
    courses.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Overview { name: saved.name.clone(), domain: saved.domain.clone(), courses, upcoming, limited: true, at: now_ms() as u64, ..Default::default() })
}

/// The overview's due dates as calendar events: coloured by course, the course as their place.
fn calendar_events(o: &Overview) -> Vec<CalendarEvent> {
    let offset = local_offset_ms();
    o.upcoming
        .iter()
        .chain(o.missing.iter())
        .filter_map(|t| {
            let due = parse_iso(&t.due)?;
            let course = o.courses.iter().find(|x| x.id == t.course_id);
            Some(CalendarEvent {
                id: t.key.clone(),
                title: t.title.clone(),
                // An all-day date is the local day, written as a bare date.
                start: if t.all_day { iso(due + offset)[..10].to_string() } else { t.due.clone() },
                all_day: t.all_day,
                url: (!t.url.is_empty()).then(|| t.url.clone()),
                color: course.map(|x| x.color.clone()).filter(|x| !x.is_empty()),
                kind: Some("due".into()),
                location: course.map(|x| if x.code.is_empty() { x.name.clone() } else { x.code.clone() }),
                notes: if t.done || t.submitted { Some("Done".into()) } else if t.missing { Some("Missing".into()) } else { None },
                ..Default::default()
            })
        })
        .take(500)
        .collect()
}

// ---------- notifications ----------

#[derive(Serialize, Deserialize, Default)]
struct Seen {
    #[serde(default)]
    started: bool,
    #[serde(default)]
    announcements: Vec<u64>,
    #[serde(default)]
    graded: Vec<String>,
    #[serde(default)]
    reminded: Vec<String>,
    /// Feed items already known (to notice newly posted ones).
    #[serde(default)]
    known: Vec<String>,
}

fn keep_last<T>(v: &mut Vec<T>, n: usize) {
    if v.len() > n {
        v.drain(..v.len() - n);
    }
}

/// What's worth a notification since the last check. The first check only remembers what's there,
/// so connecting Canvas doesn't set off a pile of old news.
fn notices(dir: &Path, o: &Overview, api: Option<&Api>, t: i64) -> Vec<Notify> {
    let path = dir.join("canvas-seen.json");
    let mut seen: Seen = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let course = |id: u64| o.courses.iter().find(|c| c.id == id).map(|c| if c.code.is_empty() { c.name.clone() } else { c.code.clone() }).unwrap_or_default();
    let note = |title: String, body: String, urgent: bool| Notify {
        title,
        body,
        icon: Some("cap".into()),
        urgent,
        // Due-soon reminders show on top of everything (fullscreen games too); the rest above windows.
        top: true,
        action: Some("tab:canvas".into()),
        sound: Some(if urgent { "urgent" } else { "canvas" }.into()),
    };
    let mut out = vec![];
    for a in &o.announcements {
        if !seen.announcements.contains(&a.id) {
            seen.announcements.push(a.id);
            out.push(note(course(a.course_id), a.title.clone(), false));
        }
    }
    for g in &o.graded {
        let key = format!("{}:{}", g.kind, g.id);
        if seen.graded.contains(&key) {
            continue;
        }
        seen.graded.push(key);
        let mut body = g.title.clone();
        if seen.started {
            if let (Some(aid), Some(api)) = (g.assignment_id, api) {
                if let Ok(s) = api.get(&format!("/courses/{}/assignments/{aid}/submissions/self", g.course_id)) {
                    match (s["score"].as_f64(), g.points) {
                        (Some(sc), Some(p)) if p > 0.0 => body = format!("{} · {sc}/{p}", g.title),
                        (Some(sc), _) => body = format!("{} · {sc}", g.title),
                        _ => {}
                    }
                }
            }
        }
        out.push(note(format!("Graded · {}", course(g.course_id)), body, false));
    }
    // From the feed: assignments posted since the last check (Canvas can't say more without a token).
    if o.limited {
        for x in o.upcoming.iter().filter(|x| x.kind == "assignment") {
            if seen.known.contains(&x.key) {
                continue;
            }
            seen.known.push(x.key.clone());
            let due = parse_iso(&x.due).map(|d| format!(" · due {}", local_day(d))).unwrap_or_default();
            out.push(note(format!("New · {}", course(x.course_id)), format!("{}{due}", x.title), false));
        }
    }
    for x in &o.upcoming {
        let key = format!("{}:{}", x.key, x.due);
        let due = parse_iso(&x.due).unwrap_or(0);
        if x.submitted || x.done || x.graded || x.all_day || due <= t || due - t > REMIND_MS || seen.reminded.contains(&key) {
            continue;
        }
        seen.reminded.push(key);
        let mins = (due - t) / 60_000;
        let when = if mins >= 60 { format!("{}h {}m", mins / 60, mins % 60) } else { format!("{mins} min") };
        out.push(note(format!("Due in {when} · {}", course(x.course_id)), x.title.clone(), true));
    }
    let first = !seen.started;
    seen.started = true;
    keep_last(&mut seen.announcements, 300);
    keep_last(&mut seen.graded, 500);
    keep_last(&mut seen.reminded, 300);
    keep_last(&mut seen.known, 600);
    if let Ok(b) = serde_json::to_vec(&seen) {
        let _ = std::fs::write(path, b);
    }
    if first {
        vec![]
    } else {
        out
    }
}

// ---------- the module ----------

#[derive(Default)]
struct Canvas {
    /// The last overview, re-read at most once a minute.
    overview: Mutex<Option<(Instant, Overview)>>,
}

impl Canvas {
    fn dir(host: &Host) -> PathBuf {
        host.data_dir()
    }

    /// After (re)linking: the first check should only take note of what's there.
    fn linked(&self, dir: &Path) {
        let _ = std::fs::remove_file(dir.join("canvas-seen.json"));
        *self.overview.lock().unwrap() = None;
    }

    fn configure(&self, host: &Host, domain: &str, token: &str) -> Result<Status, String> {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        let domain = clean_domain(domain).ok_or("That doesn't look like a web address (e.g. school.instructure.com).")?;
        let token = if token.trim().is_empty() { unprotect(&saved.token) } else { token.trim().to_string() };
        if token.is_empty() {
            return Err("Paste your access token first.".into());
        }
        // The address goes in settings.json first: it's the one host this module may reach.
        let _ = std::fs::write(dir.join("settings.json"), json!({ "domain": domain }).to_string());
        let api = Api { domain: domain.clone(), token: token.clone() };
        let me = match api.get("/users/self") {
            Ok(me) => me,
            Err(e) => {
                save(&dir, &saved);
                return Err(e);
            }
        };
        saved.domain = domain;
        saved.token = protect(&token);
        saved.feed.clear();
        saved.name = me["short_name"].as_str().or(me["name"].as_str()).unwrap_or("").to_string();
        save(&dir, &saved);
        host.info(format!("linked {} with a token", saved.domain));
        self.linked(&dir);
        Ok(status(&dir))
    }

    /// Links Canvas through the calendar feed (no access token needed).
    fn configure_feed(&self, host: &Host, url: &str) -> Result<Status, String> {
        let dir = Self::dir(host);
        let feed = clean_feed(url).ok_or("That isn't a Canvas calendar feed link. In Canvas, open Calendar and click Calendar Feed (bottom right); it ends in .ics.")?;
        let previous = load(&dir);
        let _ = std::fs::write(dir.join("settings.json"), json!({ "domain": clean_domain(&feed).unwrap_or_default() }).to_string());
        let text = fetch(&feed).and_then(|t| if t.contains("BEGIN:VCALENDAR") { Ok(t) } else { Err("Canvas didn't send a calendar for that link.".into()) });
        let text = match text {
            Ok(t) => t,
            Err(e) => {
                save(&dir, &previous);
                return Err(e);
            }
        };
        let name = text.lines().find_map(|l| l.strip_prefix("X-WR-CALNAME:")).unwrap_or("").replace(" Calendar (Canvas)", "").trim().to_string();
        let saved = Saved { domain: clean_domain(&feed).unwrap_or_default(), token: String::new(), feed: protect(&feed), name };
        save(&dir, &saved);
        host.info(format!("linked {} with its calendar feed", saved.domain));
        self.linked(&dir);
        Ok(status(&dir))
    }

    fn forget(&self, host: &Host) {
        let dir = Self::dir(host);
        save(&dir, &Saved::default());
        let _ = std::fs::remove_file(dir.join("canvas-done.json"));
        self.linked(&dir);
        host.calendar("canvas", vec![]);
    }

    /// Everything for the tab, re-read at most once a minute (or now, with `fresh`). Each read also
    /// refreshes the calendar's Canvas due dates.
    fn overview(&self, host: &Host, fresh: bool) -> Result<Overview, String> {
        if !fresh {
            if let Some((at, o)) = self.overview.lock().unwrap().as_ref() {
                if at.elapsed() < Duration::from_secs(60) {
                    return Ok(o.clone());
                }
            }
        }
        let dir = Self::dir(host);
        let saved = load(&dir);
        let o = if !saved.feed.is_empty() { feed_overview(&dir, &saved)? } else { token_overview(&Api::from(&dir)?, &saved)? };
        *self.overview.lock().unwrap() = Some((Instant::now(), o.clone()));
        host.calendar("canvas", calendar_events(&o));
        Ok(o)
    }

    /// Ticks an item off (or back on): in Canvas's planner, like the checkbox in the Canvas app; with
    /// the calendar feed (which can't be written to), on this PC.
    fn set_done(&self, host: &Host, key: &str, kind: &str, id: u64, override_id: Option<u64>, done: bool) -> Result<Option<u64>, String> {
        let dir = Self::dir(host);
        if !load(&dir).feed.is_empty() {
            let mut keys = local_done(&dir);
            keys.retain(|k| k != key);
            if done {
                keys.push(key.to_string());
            }
            keep_last(&mut keys, 500);
            let _ = std::fs::write(dir.join("canvas-done.json"), serde_json::to_vec(&keys).unwrap_or_default());
            *self.overview.lock().unwrap() = None;
            return Ok(None);
        }
        let api = Api::from(&dir)?;
        let v = match override_id {
            Some(o) => api.call("PUT", &format!("/planner/overrides/{o}"), Some(&json!({ "marked_complete": done })))?,
            None => api.call("POST", "/planner/overrides", Some(&json!({ "plannable_type": kind, "plannable_id": id, "marked_complete": done })))?,
        };
        *self.overview.lock().unwrap() = None;
        Ok(v["id"].as_u64().or(override_id))
    }
}

fn to<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

impl Module for Canvas {
    fn init(&self, host: &Host, _init: &Init) -> Result<(), RpcError> {
        let dir = Self::dir(host);
        // 0.1.x kept these in the app's own folder.
        for name in ["canvas.json", "canvas-seen.json", "canvas-done.json"] {
            match host.take_legacy(name, &dir.join(name)) {
                Ok(true) => host.info(format!("took over {name} from 0.1.x")),
                Ok(false) => {}
                Err(e) => host.warn(format!("couldn't take over {name}: {e}")),
            }
        }
        // settings.json follows canvas.json (a taken-over one has no settings.json yet).
        let saved = load(&dir);
        let set: Value = std::fs::read(dir.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        if set["domain"].as_str().unwrap_or("") != saved.domain {
            save(&dir, &saved);
        }
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        let failed = |e: String| RpcError::failed(e);
        match method {
            "status" => Ok(to(&status(&Self::dir(host)))),
            "configure" => self.configure(host, params["domain"].as_str().unwrap_or(""), params["token"].as_str().unwrap_or("")).map(|s| to(&s)).map_err(failed),
            "configure_feed" => self.configure_feed(host, params["url"].as_str().unwrap_or("")).map(|s| to(&s)).map_err(failed),
            "overview" => self.overview(host, false).map(|o| to(&o)).map_err(failed),
            "done" => {
                let p = &params;
                self.set_done(host, p["key"].as_str().unwrap_or(""), p["kind"].as_str().unwrap_or(""), p["id"].as_u64().unwrap_or(0), p["override_id"].as_u64(), p["done"].as_bool().unwrap_or(false))
                    .map(|id| json!(id))
                    .map_err(failed)
            }
            "forget" => {
                self.forget(host);
                Ok(Value::Null)
            }
            _ => Err(RpcError::unknown_method(method)),
        }
    }

    /// Every 10 minutes: a fresh read (which also refreshes the calendar) and its notifications,
    /// one at a time a few seconds apart, so each gets read.
    fn tick(&self, host: &Host) -> Result<(), RpcError> {
        let dir = Self::dir(host);
        if !status(&dir).configured {
            return Ok(());
        }
        let o = self.overview(host, true).map_err(RpcError::failed)?;
        let api = Api::from(&dir).ok();
        for (i, n) in notices(&dir, &o, api.as_ref(), now_ms()).into_iter().take(4).enumerate() {
            if i > 0 {
                std::thread::sleep(Duration::from_secs(7));
            }
            host.notify(n);
        }
        Ok(())
    }
}

fn main() {
    island_module::run(Canvas::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains() {
        assert_eq!(clean_domain("https://school.instructure.com/courses/12?x=1").as_deref(), Some("school.instructure.com"));
        assert_eq!(clean_domain(" Canvas.School.edu ").as_deref(), Some("canvas.school.edu"));
        assert_eq!(clean_domain("not a domain"), None);
        assert_eq!(clean_domain("evil.com\"&calc"), None);
    }

    #[test]
    fn dates_round_trip() {
        let ms = parse_iso("2026-10-14T23:59:00Z").unwrap();
        assert_eq!(iso(ms), "2026-10-14T23:59:00Z");
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso(parse_iso("2024-02-29T12:00:00Z").unwrap()), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn feed_links() {
        assert_eq!(
            clean_feed("webcal://school.instructure.com/feeds/calendars/user_AbC123.ics").as_deref(),
            Some("https://school.instructure.com/feeds/calendars/user_AbC123.ics")
        );
        assert!(clean_feed("https://school.instructure.com/feeds/calendars/user_AbC123.ics").is_some());
        assert!(clean_feed("https://school.instructure.com/courses/1").is_none());
        assert!(clean_feed("https://evil.example/feeds/calendars/x.ics\"").is_none());
    }

    #[test]
    fn calendar_feed_parses() {
        let ics = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Student Calendar (Canvas)\r\nBEGIN:VEVENT\r\nDTSTART:20261015T035900Z\r\nSUMMARY:Lab 6: AVL Trees [CS 101-001]\r\nUID:event-assignment-456\r\nURL;VALUE=URI:https://school.instructure.com/calendar?include_contexts=course_1#assignment_4\r\n 56\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261016\r\nSUMMARY:No class\\, reading day [MATH 201]\r\nUID:event-calendar-event-9\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = parse_ics(ics, -4 * 3_600_000);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].uid, "event-assignment-456");
        assert_eq!(ev[0].start, Some((parse_iso("2026-10-15T03:59:00Z").unwrap(), false)));
        assert!(ev[0].url.ends_with("#assignment_456"), "{}", ev[0].url);
        assert_eq!(split_course(&ev[0].summary), ("Lab 6: AVL Trees".to_string(), "CS 101-001".to_string()));
        assert_eq!(split_course(&ev[1].summary).0, "No class, reading day");
        // All-day: local midnight (UTC-4 here) = 04:00 UTC.
        assert_eq!(ev[1].start, Some((parse_iso("2026-10-16T04:00:00Z").unwrap(), true)));
        assert_eq!(course_id("CS 101-001"), course_id("CS 101-001"));
        assert!(course_id("CS 101-001") < 1 << 52);
    }

    #[test]
    fn announcement_text() {
        assert_eq!(plain("<p>Exam moved to <b>Friday</b>&nbsp;&amp; bring a pencil</p>"), "Exam moved to Friday & bring a pencil");
    }

    #[test]
    fn due_dates_reach_the_calendar() {
        let o = Overview {
            courses: vec![Course { id: 1, name: "Signals".into(), code: "EE 101".into(), color: "#4f8cf7".into(), ..Default::default() }],
            upcoming: vec![
                Task { key: "assignment:7".into(), title: "Problem set 4".into(), course_id: 1, due: "2026-10-27T03:59:00Z".into(), submitted: true, ..Default::default() },
                Task { key: "event-9".into(), title: "Reading day".into(), due: "2026-10-28T04:00:00Z".into(), all_day: true, ..Default::default() },
            ],
            missing: vec![Task { key: "assignment:3".into(), title: "Lab 2".into(), course_id: 1, due: "2026-10-20T03:59:00Z".into(), missing: true, ..Default::default() }],
            ..Default::default()
        };
        let ev = calendar_events(&o);
        assert_eq!(ev.len(), 3);
        assert_eq!((ev[0].start.as_str(), ev[0].location.as_deref(), ev[0].notes.as_deref(), ev[0].color.as_deref()), ("2026-10-27T03:59:00Z", Some("EE 101"), Some("Done"), Some("#4f8cf7")));
        assert!(ev[1].all_day && ev[1].start.len() == 10 && ev[1].location.is_none());
        assert_eq!(ev[2].notes.as_deref(), Some("Missing"));
    }

    #[test]
    fn first_check_only_takes_note() {
        let dir = std::env::temp_dir().join(format!("canvas-seen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = parse_iso("2026-10-26T12:00:00Z").unwrap();
        let mut o = Overview {
            announcements: vec![Announcement { id: 1, title: "Welcome".into(), ..Default::default() }],
            upcoming: vec![Task { key: "assignment:9".into(), title: "Quiz".into(), due: iso(t + 3_600_000), ..Default::default() }],
            ..Default::default()
        };
        assert!(notices(&dir, &o, None, t).is_empty());
        // Next time: only what's new, and a due-soon reminder once.
        o.announcements.push(Announcement { id: 2, title: "Exam moved".into(), ..Default::default() });
        o.upcoming.push(Task { key: "assignment:10".into(), title: "Lab".into(), due: iso(t + 7_200_000), ..Default::default() });
        let n = notices(&dir, &o, None, t);
        assert_eq!(n.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(), ["Exam moved", "Lab"]);
        assert!(!n[0].urgent && n[1].urgent && n[1].title.starts_with("Due in 2h 0m"));
        assert!(notices(&dir, &o, None, t).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "canvas");
    }
}
