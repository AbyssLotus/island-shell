//! Notion, an Island Shell module backend (docs/design/modules.md); its UI is ui/notion.js, a
//! section in Today.
//!
//! The user makes an internal integration in Notion (notion.so/profile/integrations), pastes its
//! secret here, and shares the databases they want with it (••• → Connections). Then, for each
//! database they pick (Notion's API calls the table inside a database a *data source*):
//!
//! - **To-dos in Today**: its pages that aren't done (a checkbox or a status property says which
//!   are), with their dates. Ticking one off here ticks it off in Notion.
//! - **Dates in the calendar**: its pages with a date, from two months back to six ahead (calendar
//!   source `notion`), done ones marked "Done".
//!
//! Which properties mean "done" and "date" is guessed from their types and names, and the user can
//! change it. Everything is read again every 10 minutes (the `scheduled` tick) and when Today
//! shows. The secret is DPAPI-encrypted in notion.json.
//!
//! RPC methods: `status`, `configure { token }`, `forget`, `databases`, `choose { sources }`,
//! `todos { fresh }`, `done { id, source, done }`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use island_module::http::Request;
use island_module::secrets::{protect, unprotect};
use island_module::{json, local_offset_ms, now_ms, CalendarEvent, CallResult, Host, Init, Module, RpcError, Value};
use serde::{Deserialize, Serialize};

const API: &str = "https://api.notion.com/v1";
/// The API version with data sources (databases can hold several since 2025).
const VERSION: &str = "2025-09-03";

// ---------- settings ----------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    /// DPAPI-encrypted, base64.
    #[serde(default)]
    token: String,
    #[serde(default)]
    workspace: String,
    #[serde(default)]
    sources: Vec<Source>,
}

/// A database the user picked, and which of its properties mean what (property ids, which survive
/// renaming).
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
struct Source {
    /// The data source's id.
    id: String,
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    todos: bool,
    #[serde(default)]
    calendar: bool,
    #[serde(default)]
    title_prop: String,
    #[serde(default)]
    done_prop: String,
    /// "checkbox" or "status" ("" when it has neither).
    #[serde(default)]
    done_kind: String,
    /// A status property's options that mean done (its "Complete" group).
    #[serde(default)]
    done_names: Vec<String>,
    /// The status to go back to when a to-do is unticked.
    #[serde(default)]
    open_name: String,
    #[serde(default)]
    date_prop: String,
}

fn load(dir: &Path) -> Saved {
    std::fs::read(dir.join("notion.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(dir.join("notion.json"), b);
    }
}

#[derive(Serialize, Default)]
struct Status {
    configured: bool,
    workspace: String,
    /// Databases picked, for to-dos and for the calendar.
    todos: usize,
    calendar: usize,
}

fn status(dir: &Path) -> Status {
    let s = load(dir);
    Status {
        configured: !s.token.is_empty(),
        workspace: s.workspace,
        todos: s.sources.iter().filter(|x| x.todos).count(),
        calendar: s.sources.iter().filter(|x| x.calendar).count(),
    }
}

// ---------- HTTP ----------

struct Api {
    token: String,
}

impl Api {
    fn from(dir: &Path) -> Result<Api, String> {
        let token = unprotect(&load(dir).token);
        if token.is_empty() {
            return Err("Notion isn't connected yet.".into());
        }
        Ok(Api { token })
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let url = format!("{API}{path}");
        let mut req = match body {
            Some(b) => Request::post(&url, "application/json", b.to_string()),
            None => Request::get(&url),
        };
        req.method = method.into();
        let resp = req.header("Authorization", &format!("Bearer {}", self.token)).header("Notion-Version", VERSION).send()?;
        let v = resp.json();
        if resp.ok() {
            return Ok(v);
        }
        Err(problem(resp.status, v["code"].as_str().unwrap_or(""), v["message"].as_str().unwrap_or("")))
    }
}

/// What a failed request means for the user.
fn problem(status: u16, code: &str, message: &str) -> String {
    match (status, code) {
        (401, _) | (_, "unauthorized") => "Notion didn't accept the secret. Copy it again from your integration's page.".into(),
        (403, _) | (_, "restricted_resource") => "Your integration isn't allowed to do that. On its page, under Capabilities, turn on Read content and Update content.".into(),
        (404, _) | (_, "object_not_found") => "Notion can't see that database anymore. Share it with your integration again (••• → Connections).".into(),
        (429, _) | (_, "rate_limited") => "Notion asked to slow down. It'll try again in a few minutes.".into(),
        _ if !message.is_empty() => format!("Notion: {message}"),
        _ => format!("Notion answered HTTP {status}"),
    }
}

// ---------- reading Notion's objects ----------

/// Rich text as plain text.
fn plain(rt: &Value) -> String {
    rt.as_array().map(|a| a.iter().filter_map(|t| t["plain_text"].as_str()).collect::<String>()).unwrap_or_default()
}

/// A page's property by its id.
fn prop<'a>(page: &'a Value, id: &str) -> Option<&'a Value> {
    page["properties"].as_object()?.values().find(|v| v["id"].as_str() == Some(id))
}

/// A Notion page's link with no dashes in its id: https://www.notion.so/<id>.
fn link(id: &str) -> String {
    format!("https://www.notion.so/{}", id.replace('-', ""))
}

/// A property of a database's schema, for picking what means "done" and "date".
#[derive(Serialize, Clone, Debug, PartialEq)]
struct Prop {
    id: String,
    name: String,
    kind: String,
}

fn props_of(ds: &Value) -> Vec<Prop> {
    let mut v: Vec<Prop> = ds["properties"]
        .as_object()
        .map(|o| o.values().map(|p| Prop { id: p["id"].as_str().unwrap_or("").into(), name: p["name"].as_str().unwrap_or("").into(), kind: p["type"].as_str().unwrap_or("").into() }).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// A status property's done options (its "Complete" group) and the one to go back to.
fn status_names(schema: &Value) -> (Vec<String>, String) {
    let options = schema["status"]["options"].as_array().cloned().unwrap_or_default();
    let name_of = |id: &str| options.iter().find(|o| o["id"].as_str() == Some(id)).and_then(|o| o["name"].as_str()).map(String::from);
    let groups = schema["status"]["groups"].as_array().cloned().unwrap_or_default();
    let group = |names: &[&str]| groups.iter().find(|g| names.contains(&g["name"].as_str().unwrap_or("").to_lowercase().as_str()));
    let ids = |g: &Value| g["option_ids"].as_array().map(|a| a.iter().filter_map(|i| i.as_str()).filter_map(name_of).collect::<Vec<_>>()).unwrap_or_default();
    let done = group(&["complete", "completed", "done"]).or(groups.last()).map(ids).unwrap_or_default();
    let open = group(&["to-do", "to do", "todo", "not started"]).or(groups.first()).map(ids).and_then(|v| v.into_iter().next());
    let open = open.or_else(|| options.first().and_then(|o| o["name"].as_str()).map(String::from)).unwrap_or_default();
    (done, open)
}

/// What a data source's properties most likely mean: its title, a checkbox (or status) for done
/// and a date, preferring names that say so.
fn guess(ds: &Value) -> Source {
    let schema = ds["properties"].as_object().cloned().unwrap_or_default();
    let all: Vec<&Value> = schema.values().collect();
    let named = |kind: &str, words: &[&str]| {
        let of: Vec<&&Value> = all.iter().filter(|p| p["type"] == kind).collect();
        of.iter().find(|p| words.iter().any(|w| p["name"].as_str().unwrap_or("").to_lowercase().contains(w))).or(of.first()).map(|p| (**p).clone())
    };
    let mut s = Source {
        id: ds["id"].as_str().unwrap_or("").into(),
        title: Some(plain(&ds["title"])).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled".into()),
        url: ds["parent"]["database_id"].as_str().map(link).unwrap_or_default(),
        title_prop: all.iter().find(|p| p["type"] == "title").and_then(|p| p["id"].as_str()).unwrap_or("").into(),
        ..Default::default()
    };
    if let Some(p) = named("checkbox", &["done", "complete", "finished", "checked"]) {
        s.done_kind = "checkbox".into();
        s.done_prop = p["id"].as_str().unwrap_or("").into();
    } else if let Some(p) = named("status", &["status"]) {
        s.done_kind = "status".into();
        s.done_prop = p["id"].as_str().unwrap_or("").into();
        (s.done_names, s.open_name) = status_names(&p);
    }
    if let Some(p) = named("date", &["due", "deadline", "date", "when", "scheduled"]) {
        s.date_prop = p["id"].as_str().unwrap_or("").into();
    }
    s
}

fn is_done(page: &Value, s: &Source) -> bool {
    match (s.done_kind.as_str(), prop(page, &s.done_prop)) {
        ("checkbox", Some(v)) => v["checkbox"].as_bool().unwrap_or(false),
        ("status", Some(v)) => v["status"]["name"].as_str().is_some_and(|n| s.done_names.iter().any(|d| d == n)),
        _ => false,
    }
}

fn title_of(page: &Value, s: &Source) -> String {
    let v = prop(page, &s.title_prop).or_else(|| page["properties"].as_object()?.values().find(|v| v["type"] == "title"));
    Some(v.map(|v| plain(&v["title"])).unwrap_or_default()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Untitled".into())
}

/// The page's date (start, end), as Notion writes them: a bare date or RFC 3339 with an offset.
fn date_of(page: &Value, s: &Source) -> Option<(String, Option<String>)> {
    let d = &prop(page, &s.date_prop)?["date"];
    Some((d["start"].as_str()?.to_string(), d["end"].as_str().map(String::from)))
}

/// The query for a to-do database's open pages, soonest first.
fn open_query(s: &Source) -> Value {
    let filter = match s.done_kind.as_str() {
        "checkbox" => Some(json!({ "property": s.done_prop, "checkbox": { "equals": false } })),
        "status" if !s.done_names.is_empty() => Some(json!({ "and": s.done_names.iter().map(|n| json!({ "property": s.done_prop, "status": { "does_not_equal": n } })).collect::<Vec<_>>() })),
        _ => None,
    };
    let sorts = if s.date_prop.is_empty() { json!([{ "timestamp": "created_time", "direction": "ascending" }]) } else { json!([{ "property": s.date_prop, "direction": "ascending" }]) };
    let mut q = json!({ "sorts": sorts, "page_size": 100 });
    if let Some(f) = filter {
        q["filter"] = f;
    }
    q
}

/// The query for a database's pages dated between two days.
fn dated_query(s: &Source, from: &str, to: &str, cursor: Option<&str>) -> Value {
    let mut q = json!({
        "filter": { "and": [
            { "property": s.date_prop, "date": { "on_or_after": from } },
            { "property": s.date_prop, "date": { "on_or_before": to } }
        ] },
        "page_size": 100
    });
    if let Some(c) = cursor {
        q["start_cursor"] = json!(c);
    }
    q
}

// ---------- dates ----------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn ymd(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// "2026-10-27" plus `n` days.
fn add_days(date: &str, n: i64) -> Option<String> {
    let p = |a: usize, l: usize| date.get(a..a + l)?.parse::<i64>().ok();
    Some(ymd(days_from_civil(p(0, 4)?, p(5, 2)?, p(8, 2)?) + n))
}

/// Today's date on this PC.
fn today() -> String {
    ymd((now_ms() + local_offset_ms()).div_euclid(86_400_000))
}

// ---------- to-dos and calendar events ----------

#[derive(Serialize, Clone, Debug, PartialEq)]
struct Todo {
    id: String,
    /// The data source it's from.
    source: String,
    list: String,
    title: String,
    /// Its date, as Notion has it (a bare date, or RFC 3339).
    due: Option<String>,
    url: String,
}

#[derive(Serialize, Clone, Default)]
struct Problem {
    source: String,
    title: String,
    error: String,
}

#[derive(Serialize, Clone, Default)]
struct Overview {
    todos: Vec<Todo>,
    problems: Vec<Problem>,
}

fn todo_from(page: &Value, s: &Source) -> Todo {
    let id = page["id"].as_str().unwrap_or("").to_string();
    Todo {
        url: page["url"].as_str().filter(|u| u.starts_with("https://")).map(String::from).unwrap_or_else(|| link(&id)),
        title: title_of(page, s),
        due: date_of(page, s).map(|d| d.0),
        source: s.id.clone(),
        list: s.title.clone(),
        id,
    }
}

/// Undated to-dos last; otherwise by date, then by name.
fn sort_todos(v: &mut [Todo]) {
    v.sort_by(|a, b| match (&a.due, &b.due) {
        (Some(x), Some(y)) => x[..10.min(x.len())].cmp(&y[..10.min(y.len())]).then(x.cmp(y)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }.then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
}

fn event_from(page: &Value, s: &Source) -> Option<CalendarEvent> {
    let (start, end) = date_of(page, s)?;
    let all_day = start.len() == 10;
    // A date range's end day is part of it; the calendar wants the moment it ends.
    let end = end.and_then(|e| if e.len() == 10 { add_days(&e, 1) } else { Some(e) });
    Some(CalendarEvent {
        id: format!("{}:{}", s.id, page["id"].as_str().unwrap_or("")),
        title: title_of(page, s),
        start,
        end,
        all_day,
        url: page["url"].as_str().map(String::from),
        kind: Some(if s.todos { "due" } else { "event" }.into()),
        location: Some(s.title.clone()),
        notes: (s.todos && is_done(page, s)).then(|| "Done".into()),
        ..Default::default()
    })
}

// ---------- the module ----------

#[derive(Default)]
struct Notion {
    /// The last to-dos, read again at most once a minute.
    cache: Mutex<Option<(Instant, Overview)>>,
}

fn to<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

impl Notion {
    fn dir(host: &Host) -> PathBuf {
        host.data_dir()
    }

    fn configure(&self, host: &Host, token: &str) -> Result<Status, String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("Paste your integration's secret first.".into());
        }
        let me = Api { token: token.into() }.call("GET", "/users/me", None)?;
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        saved.token = protect(token);
        saved.workspace = me["bot"]["workspace_name"].as_str().unwrap_or("").to_string();
        save(&dir, &saved);
        *self.cache.lock().unwrap() = None;
        host.info(format!("connected to {}", if saved.workspace.is_empty() { "Notion" } else { &saved.workspace }));
        Ok(status(&dir))
    }

    fn forget(&self, host: &Host) {
        save(&Self::dir(host), &Saved::default());
        *self.cache.lock().unwrap() = None;
        host.calendar("notion", vec![]);
    }

    /// Every database shared with the integration, with what each property most likely means (or
    /// what the user picked before).
    fn databases(&self, host: &Host) -> Result<Value, String> {
        let dir = Self::dir(host);
        let api = Api::from(&dir)?;
        let saved = load(&dir);
        let mut out = vec![];
        let mut cursor: Option<String> = None;
        for _ in 0..3 {
            let mut q = json!({ "filter": { "property": "object", "value": "data_source" }, "page_size": 100 });
            if let Some(c) = &cursor {
                q["start_cursor"] = json!(c);
            }
            let r = api.call("POST", "/search", Some(&q))?;
            for ds in r["results"].as_array().into_iter().flatten() {
                let g = guess(ds);
                let picked = saved.sources.iter().find(|s| s.id == g.id).cloned();
                out.push(json!({ "source": picked.clone().map(|p| Source { title: g.title.clone(), ..p }).unwrap_or(g), "picked": picked.is_some(), "props": props_of(ds), "statuses": statuses(ds) }));
            }
            cursor = r["next_cursor"].as_str().map(String::from);
            if !r["has_more"].as_bool().unwrap_or(false) || cursor.is_none() {
                break;
            }
        }
        Ok(json!(out))
    }

    /// The databases the user picked, and what their properties mean.
    fn choose(&self, host: &Host, sources: Vec<Source>) -> Result<Status, String> {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        saved.sources = sources
            .into_iter()
            .map(|mut s| {
                // A to-do list needs to know what done means, a calendar needs a date.
                s.todos &= !s.done_kind.is_empty() && !s.done_prop.is_empty();
                s.calendar &= !s.date_prop.is_empty();
                s
            })
            .filter(|s| !s.id.is_empty() && (s.todos || s.calendar))
            .take(20)
            .collect();
        save(&dir, &saved);
        *self.cache.lock().unwrap() = None;
        if let Err(e) = self.calendar(host) {
            host.warn(format!("calendar: {e}"));
        }
        Ok(status(&dir))
    }

    /// Open to-dos from every to-do database, read again at most once a minute (or now, with `fresh`).
    fn todos(&self, host: &Host, fresh: bool) -> Result<Overview, String> {
        if !fresh {
            if let Some((at, o)) = self.cache.lock().unwrap().as_ref() {
                if at.elapsed() < Duration::from_secs(60) {
                    return Ok(o.clone());
                }
            }
        }
        let dir = Self::dir(host);
        let api = Api::from(&dir)?;
        let mut o = Overview::default();
        for s in load(&dir).sources.iter().filter(|s| s.todos) {
            match api.call("POST", &format!("/data_sources/{}/query", s.id), Some(&open_query(s))) {
                Ok(r) => o.todos.extend(r["results"].as_array().into_iter().flatten().filter(|p| !is_done(p, s)).map(|p| todo_from(p, s))),
                Err(e) => o.problems.push(Problem { source: s.id.clone(), title: s.title.clone(), error: e }),
            }
        }
        sort_todos(&mut o.todos);
        o.todos.truncate(300);
        *self.cache.lock().unwrap() = Some((Instant::now(), o.clone()));
        Ok(o)
    }

    fn set_done(&self, host: &Host, id: &str, source: &str, done: bool) -> Result<(), String> {
        let dir = Self::dir(host);
        let s = load(&dir).sources.into_iter().find(|s| s.id == source).ok_or("That database isn't picked anymore.")?;
        let value = match s.done_kind.as_str() {
            "checkbox" => json!({ "checkbox": done }),
            "status" => {
                let name = if done { s.done_names.first().cloned().unwrap_or_default() } else { s.open_name.clone() };
                if name.is_empty() {
                    return Err("That status property has no option to set.".into());
                }
                json!({ "status": { "name": name } })
            }
            _ => return Err("That database has nothing that says a to-do is done.".into()),
        };
        Api::from(&dir)?.call("PATCH", &format!("/pages/{id}"), Some(&json!({ "properties": { s.done_prop.clone(): value } })))?;
        *self.cache.lock().unwrap() = None;
        Ok(())
    }

    /// Dated pages from the calendar databases, two months back to six ahead.
    fn calendar(&self, host: &Host) -> Result<(), String> {
        let dir = Self::dir(host);
        let saved = load(&dir);
        if !saved.sources.iter().any(|s| s.calendar) {
            host.calendar("notion", vec![]);
            return Ok(());
        }
        let api = Api::from(&dir)?;
        let t = today();
        let (from, to) = (add_days(&t, -62).unwrap_or_default(), add_days(&t, 183).unwrap_or_default());
        let mut events = vec![];
        for s in saved.sources.iter().filter(|s| s.calendar) {
            let mut cursor: Option<String> = None;
            for _ in 0..5 {
                let r = api.call("POST", &format!("/data_sources/{}/query", s.id), Some(&dated_query(s, &from, &to, cursor.as_deref())))?;
                events.extend(r["results"].as_array().into_iter().flatten().filter_map(|p| event_from(p, s)));
                cursor = r["next_cursor"].as_str().map(String::from);
                if !r["has_more"].as_bool().unwrap_or(false) || cursor.is_none() {
                    break;
                }
            }
        }
        host.calendar("notion", events);
        Ok(())
    }
}

/// A database's status properties and their options, for picking what done means by hand.
fn statuses(ds: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for p in ds["properties"].as_object().into_iter().flat_map(|o| o.values()).filter(|p| p["type"] == "status") {
        let (done, open) = status_names(p);
        let options: Vec<&str> = p["status"]["options"].as_array().into_iter().flatten().filter_map(|o| o["name"].as_str()).collect();
        out.insert(p["id"].as_str().unwrap_or("").into(), json!({ "done": done, "open": open, "options": options }));
    }
    Value::Object(out)
}

impl Module for Notion {
    fn init(&self, _host: &Host, _init: &Init) -> Result<(), RpcError> {
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        let failed = |e: String| RpcError::failed(e);
        match method {
            "status" => Ok(to(&status(&Self::dir(host)))),
            "configure" => self.configure(host, params["token"].as_str().unwrap_or("")).map(|s| to(&s)).map_err(failed),
            "forget" => {
                self.forget(host);
                Ok(Value::Null)
            }
            "databases" => self.databases(host).map_err(failed),
            "choose" => {
                let sources: Vec<Source> = serde_json::from_value(params["sources"].clone()).map_err(|e| RpcError::bad_params(e.to_string()))?;
                self.choose(host, sources).map(|s| to(&s)).map_err(failed)
            }
            "todos" => self.todos(host, params["fresh"].as_bool().unwrap_or(false)).map(|o| to(&o)).map_err(failed),
            "done" => {
                let p = &params;
                self.set_done(host, p["id"].as_str().unwrap_or(""), p["source"].as_str().unwrap_or(""), p["done"].as_bool().unwrap_or(true)).map(|_| Value::Null).map_err(failed)
            }
            _ => Err(RpcError::unknown_method(method)),
        }
    }

    /// Every 10 minutes: the to-dos and the calendar, read again.
    fn tick(&self, host: &Host) -> Result<(), RpcError> {
        if !status(&Self::dir(host)).configured {
            return Ok(());
        }
        self.todos(host, true).map_err(RpcError::failed)?;
        self.calendar(host).map_err(RpcError::failed)
    }
}

fn main() {
    island_module::run(Notion::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A data source as Notion's API returns it (trimmed), with a checkbox, a status and two dates.
    fn tasks() -> Value {
        json!({
            "object": "data_source", "id": "ds-1",
            "title": [{ "plain_text": "Tasks" }],
            "parent": { "type": "database_id", "database_id": "aaaa-bbbb" },
            "properties": {
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                "Created": { "id": "c%3D", "name": "Created", "type": "date", "date": {} },
                "Due date": { "id": "d%3F", "name": "Due date", "type": "date", "date": {} },
                "Done": { "id": "x%40", "name": "Done", "type": "checkbox", "checkbox": {} },
                "Status": { "id": "s%21", "name": "Status", "type": "status", "status": {
                    "options": [{ "id": "o1", "name": "Not started" }, { "id": "o2", "name": "Doing" }, { "id": "o3", "name": "Shipped" }],
                    "groups": [{ "name": "To-do", "option_ids": ["o1"] }, { "name": "In progress", "option_ids": ["o2"] }, { "name": "Complete", "option_ids": ["o3"] }]
                } }
            }
        })
    }

    fn page(id: &str, title: &str, done: bool, status: &str, due: Option<(&str, Option<&str>)>) -> Value {
        json!({
            "object": "page", "id": id, "url": format!("https://www.notion.so/Page-{id}"),
            "properties": {
                "Name": { "id": "title", "type": "title", "title": [{ "plain_text": title }] },
                "Done": { "id": "x%40", "type": "checkbox", "checkbox": done },
                "Status": { "id": "s%21", "type": "status", "status": { "name": status } },
                "Due date": { "id": "d%3F", "type": "date", "date": due.map(|(s, e)| json!({ "start": s, "end": e })) }
            }
        })
    }

    #[test]
    fn guesses_what_properties_mean() {
        let s = guess(&tasks());
        assert_eq!((s.title.as_str(), s.url.as_str(), s.title_prop.as_str()), ("Tasks", "https://www.notion.so/aaaabbbb", "title"));
        // A checkbox named Done wins over the status; "Due date" over "Created".
        assert_eq!((s.done_kind.as_str(), s.done_prop.as_str(), s.date_prop.as_str()), ("checkbox", "x%40", "d%3F"));
        // Without the checkbox, the status, with its Complete group.
        let mut ds = tasks();
        ds["properties"].as_object_mut().unwrap().remove("Done");
        let s = guess(&ds);
        assert_eq!((s.done_kind.as_str(), s.done_names.clone(), s.open_name.as_str()), ("status", vec!["Shipped".to_string()], "Not started"));
        assert_eq!(props_of(&tasks()).iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Created", "Done", "Due date", "Name", "Status"]);
    }

    #[test]
    fn open_to_dos_and_their_query() {
        let s = guess(&tasks());
        assert_eq!(open_query(&s)["filter"], json!({ "property": "x%40", "checkbox": { "equals": false } }));
        assert_eq!(open_query(&s)["sorts"][0]["property"], "d%3F");
        let status = Source { done_kind: "status".into(), done_prop: "s%21".into(), done_names: vec!["Shipped".into(), "Won't do".into()], ..s.clone() };
        assert_eq!(open_query(&status)["filter"]["and"][1], json!({ "property": "s%21", "status": { "does_not_equal": "Won't do" } }));
        assert!(is_done(&page("p", "x", false, "Shipped", None), &status));
        assert!(!is_done(&page("p", "x", false, "Doing", None), &status));
        let mut v = vec![
            todo_from(&page("1", "No date", false, "", None), &s),
            todo_from(&page("2", "Later", false, "", Some(("2026-11-02", None))), &s),
            todo_from(&page("3", "Sooner", false, "", Some(("2026-10-28T09:00:00.000-04:00", None))), &s),
        ];
        sort_todos(&mut v);
        assert_eq!(v.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Sooner", "Later", "No date"]);
        assert_eq!((v[0].list.as_str(), v[0].url.as_str()), ("Tasks", "https://www.notion.so/Page-3"));
    }

    #[test]
    fn dated_pages_become_calendar_events() {
        let s = Source { todos: true, calendar: true, ..guess(&tasks()) };
        let e = event_from(&page("p1", "Trip", true, "", Some(("2026-10-27", Some("2026-10-29")))), &s).unwrap();
        // An all-day range ends after its last day.
        assert_eq!((e.start.as_str(), e.end.as_deref(), e.all_day), ("2026-10-27", Some("2026-10-30"), true));
        assert_eq!((e.notes.as_deref(), e.kind.as_deref(), e.location.as_deref()), (Some("Done"), Some("due"), Some("Tasks")));
        let e = event_from(&page("p2", "Call", false, "", Some(("2026-10-27T15:00:00.000-04:00", None))), &s).unwrap();
        assert!(!e.all_day && e.end.is_none() && e.notes.is_none());
        assert!(event_from(&page("p3", "Someday", false, "", None), &s).is_none());
        assert_eq!(dated_query(&s, "2026-09-01", "2027-04-01", Some("c1"))["filter"]["and"][1]["date"]["on_or_before"], "2027-04-01");
    }

    #[test]
    fn dates() {
        assert_eq!(add_days("2026-12-31", 1).as_deref(), Some("2027-01-01"));
        assert_eq!(add_days("2028-03-01", -1).as_deref(), Some("2028-02-29"));
        assert_eq!(add_days("nope", 1), None);
        assert_eq!(ymd(days_from_civil(2026, 10, 27)), "2026-10-27");
    }

    #[test]
    fn problems_read_plainly() {
        assert!(problem(401, "unauthorized", "API token is invalid.").contains("secret"));
        assert!(problem(404, "object_not_found", "").contains("Connections"));
        assert_eq!(problem(400, "validation_error", "Could not find property with name or id: x."), "Notion: Could not find property with name or id: x.");
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "notion");
    }
}
