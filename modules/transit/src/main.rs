//! Transit, an Island Shell module backend (docs/design/modules.md); its UI is ui/transit.js: the
//! Transit tab, and the next departure on the pill around the time you usually leave.
//!
//! Any city whose transit agency publishes GTFS (its timetable, a zip of CSV files) and, ideally,
//! GTFS-realtime (live trip updates) works: the user gives the two addresses. Philadelphia is built
//! in: SEPTA's buses, trolleys and subway, and Regional Rail.
//!
//! - gtfs.rs reads a timetable into an index (every stop with the routes that serve it, the routes,
//!   the service calendars) and the timetable rows for the saved stops. Both are kept in the cache
//!   folder with the timetable itself, which is fetched again once a week.
//! - rt.rs decodes the live feed, read at most every 20 seconds while someone's looking.
//! - board.rs puts the two together into each stop's departures.
//!
//! A custom city's addresses may carry an API key, so they're DPAPI-encrypted in transit.json; their
//! host names go in settings.json, where the manifest's `{settings.hosts}` reads them.
//!
//! RPC methods: `status`, `city { city, name, schedule, live, header, key }`, `prepare { force }`,
//! `search { q }`, `add { feed, id }`, `remove { key }`, `update { key, routes, window }`,
//! `departures`.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod board;
mod gtfs;
mod rt;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use island_module::http::Request;
use island_module::secrets::{protect, unprotect};
use island_module::{json, now_ms, CallResult, Host, Init, Module, RpcError, Value};
use serde::{Deserialize, Serialize};

use gtfs::{Index, Visit};
use rt::TripUpdate;

const SEPTA_SCHEDULE: &str = "https://www3.septa.org/developer/gtfs_public.zip";
/// How long a timetable is used before it's fetched again.
const STALE_SECS: i64 = 7 * 86400;
/// How far ahead the departures go.
const AHEAD_SECS: i64 = 3 * 3600;

// ---------- settings ----------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    /// "philadelphia", "custom" or "" (not picked yet).
    #[serde(default)]
    city: String,
    #[serde(default)]
    custom: Option<Custom>,
    #[serde(default)]
    stops: Vec<SavedStop>,
}

/// Another city's feeds. The addresses (and the key) are DPAPI-encrypted.
#[derive(Serialize, Deserialize, Default, Clone)]
struct Custom {
    name: String,
    schedule: String,
    #[serde(default)]
    live: String,
    /// A header the live feed wants its API key in ("x-api-key"), if any.
    #[serde(default)]
    header: String,
    #[serde(default)]
    key: String,
}

/// When a stop's next departure goes on the pill: on these weekdays (0 Monday), between two times.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
struct Window {
    days: Vec<u8>,
    from: String,
    to: String,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
struct SavedStop {
    /// "<feed>:<stop id>".
    key: String,
    feed: String,
    id: String,
    name: String,
    #[serde(default)]
    code: String,
    /// The stop, or a station and its platforms.
    ids: Vec<String>,
    /// Only these route ids (empty: every route).
    #[serde(default)]
    routes: Vec<String>,
    #[serde(default)]
    window: Option<Window>,
    /// The kind of vehicle most of its routes are (GTFS route_type), for its icon.
    #[serde(default)]
    kind: u16,
}

fn load(dir: &Path) -> Saved {
    std::fs::read(dir.join("transit.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(dir.join("transit.json"), b);
    }
}

/// "https://api.example.org/x?y" → "api.example.org".
fn host_of(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://")?;
    let h = rest.split(['/', '?', '#', ':']).next()?.to_ascii_lowercase();
    (h.contains('.') && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')).then_some(h)
}

/// A stop name or a search, the same way: lowercase words, "street" as "st" (and the like), no "&".
fn words(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "and")
        .map(|w| match w {
            "street" => "st",
            "avenue" => "ave",
            "road" => "rd",
            "boulevard" => "blvd",
            "station" | "stn" => "sta",
            "saint" => "st",
            w => w,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- feeds ----------

/// One timetable and its live feeds.
#[derive(Clone, Debug, PartialEq)]
struct Feed {
    id: String,
    name: String,
    schedule: String,
    /// The timetable's zip inside the download, when it's in one (SEPTA's holds two).
    inner: Option<String>,
    live: Vec<String>,
    header: Option<(String, String)>,
}

fn feeds(s: &Saved) -> Vec<Feed> {
    match s.city.as_str() {
        "philadelphia" => vec![
            Feed { id: "septa-bus".into(), name: "SEPTA buses, trolleys and subway".into(), schedule: SEPTA_SCHEDULE.into(), inner: Some("google_bus.zip".into()), live: vec!["https://www3.septa.org/gtfsrt/septa-pa-us/Trip/rtTripUpdates.pb".into()], header: None },
            Feed { id: "septa-rail".into(), name: "SEPTA Regional Rail".into(), schedule: SEPTA_SCHEDULE.into(), inner: Some("google_rail.zip".into()), live: vec!["https://www3.septa.org/gtfsrt/septarail-pa-us/Trip/rtTripUpdates.pb".into()], header: None },
        ],
        "custom" => s
            .custom
            .iter()
            .map(|c| {
                let live = unprotect(&c.live);
                let key = unprotect(&c.key);
                Feed { id: "custom".into(), name: c.name.clone(), schedule: unprotect(&c.schedule), inner: None, live: if live.is_empty() { vec![] } else { vec![live] }, header: (!c.header.is_empty() && !key.is_empty()).then(|| (c.header.clone(), key)) }
            })
            .collect(),
        _ => vec![],
    }
}

fn city_name(s: &Saved) -> String {
    match s.city.as_str() {
        "philadelphia" => "Philadelphia".into(),
        "custom" => s.custom.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
        _ => String::new(),
    }
}

// ---------- what's kept per feed ----------

struct Loaded {
    index: Index,
    visits: HashMap<String, Vec<Visit>>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Fetched {
    /// Unix seconds.
    at: i64,
    url: String,
}

#[derive(Serialize, Clone, Default)]
struct FeedState {
    id: String,
    name: String,
    /// "missing", "working", "ready" or "error".
    state: String,
    /// What it's doing, or what went wrong.
    text: String,
    stale: bool,
    stops: usize,
}

#[derive(Default)]
struct Transit {
    loaded: Mutex<HashMap<String, Arc<Loaded>>>,
    working: Mutex<HashMap<String, String>>,
    errors: Mutex<HashMap<String, String>>,
    /// One timetable download at a time.
    prep: Mutex<()>,
    /// Live feeds by address: when they were read and what they said.
    live: Mutex<HashMap<String, (Instant, Arc<HashMap<String, TripUpdate>>)>>,
}

fn read_json<T: for<'a> Deserialize<'a>>(p: &Path) -> Option<T> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), String> {
    std::fs::write(p, serde_json::to_vec(v).map_err(|e| e.to_string())?).map_err(|e| format!("couldn't save {}: {e}", p.display()))
}

fn get(url: &str, header: Option<&(String, String)>) -> Result<Vec<u8>, String> {
    let mut r = Request::get(url);
    if let Some((k, v)) = header {
        r = r.header(k, v);
    }
    let resp = r.send()?;
    if !resp.ok() {
        let host = host_of(url).unwrap_or_default();
        return Err(match resp.status {
            401 | 403 => format!("{host} turned the request down (HTTP {}): it may need an API key", resp.status),
            404 => format!("{host} has nothing at that address (HTTP 404)"),
            s => format!("{host} answered HTTP {s}"),
        });
    }
    Ok(resp.body)
}

impl Transit {
    fn dir(host: &Host) -> PathBuf {
        host.data_dir()
    }

    fn feed_dir(host: &Host, feed: &str) -> PathBuf {
        let d = host.cache_dir().join(feed);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn status(&self, host: &Host) -> Value {
        let saved = load(&Self::dir(host));
        let now = now_ms() / 1000;
        let working = self.working.lock().unwrap().clone();
        let errors = self.errors.lock().unwrap().clone();
        let states: Vec<FeedState> = feeds(&saved)
            .iter()
            .map(|f| {
                let dir = Self::feed_dir(host, &f.id);
                let fetched: Option<Fetched> = read_json(&dir.join("fetched.json"));
                let have = dir.join("index.json").is_file() && fetched.as_ref().is_some_and(|x| x.url == f.schedule);
                let (state, text) = if let Some(w) = working.get(&f.id) {
                    ("working", w.clone())
                } else if let Some(e) = errors.get(&f.id) {
                    ("error", e.clone())
                } else if have {
                    ("ready", String::new())
                } else {
                    ("missing", String::new())
                };
                FeedState { id: f.id.clone(), name: f.name.clone(), state: state.into(), text, stale: have && fetched.is_some_and(|x| now - x.at > STALE_SECS), stops: saved.stops.iter().filter(|s| s.feed == f.id).count() }
            })
            .collect();
        json!({
            "city": saved.city, "name": city_name(&saved), "feeds": states, "stops": saved.stops,
            "presets": [{ "id": "philadelphia", "name": "Philadelphia", "sub": "SEPTA buses, trolleys, subway and Regional Rail" }],
            "custom": saved.custom.as_ref().map(|c| json!({ "name": c.name, "header": c.header, "live": !c.live.is_empty() })),
        })
    }

    /// Picks the city: the preset, or another city's feeds.
    fn set_city(&self, host: &Host, p: &Value) -> Result<Value, String> {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        let city = p["city"].as_str().unwrap_or("");
        let mut hosts: Vec<String> = vec![];
        match city {
            "philadelphia" => saved.custom = None,
            "custom" => {
                let (schedule, live) = (p["schedule"].as_str().unwrap_or("").trim().to_string(), p["live"].as_str().unwrap_or("").trim().to_string());
                let sh = host_of(&schedule).ok_or("The timetable address should be an https link to a GTFS zip.")?;
                hosts.push(sh);
                if !live.is_empty() {
                    hosts.push(host_of(&live).ok_or("The live feed address should be an https link to a GTFS-realtime feed.")?);
                }
                let name = p["name"].as_str().unwrap_or("").trim();
                saved.custom = Some(Custom {
                    name: if name.is_empty() { host_of(&schedule).unwrap_or_default() } else { name.chars().take(60).collect() },
                    schedule: protect(&schedule),
                    live: if live.is_empty() { String::new() } else { protect(&live) },
                    header: p["header"].as_str().unwrap_or("").trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect(),
                    key: protect(p["key"].as_str().unwrap_or("").trim()),
                });
            }
            "" => saved.custom = None,
            other => return Err(format!("{other} isn't a city this module knows")),
        }
        if saved.city != city {
            saved.stops.clear();
        }
        saved.city = city.into();
        // The hosts the manifest's {settings.hosts} allows, before anything's fetched.
        write_json(&dir.join("settings.json"), &json!({ "hosts": hosts }))?;
        save(&dir, &saved);
        self.loaded.lock().unwrap().clear();
        self.errors.lock().unwrap().clear();
        self.live.lock().unwrap().clear();
        Ok(self.status(host))
    }

    /// A feed's index and saved stops' rows, from memory or the cache folder.
    fn loaded(&self, host: &Host, feed: &str) -> Option<Arc<Loaded>> {
        if let Some(l) = self.loaded.lock().unwrap().get(feed) {
            return Some(l.clone());
        }
        let dir = Self::feed_dir(host, feed);
        let index: Index = read_json(&dir.join("index.json"))?;
        let visits: HashMap<String, Vec<Visit>> = read_json(&dir.join("visits.json")).unwrap_or_default();
        let l = Arc::new(Loaded { index, visits });
        self.loaded.lock().unwrap().insert(feed.into(), l.clone());
        Some(l)
    }

    fn progress(&self, host: &Host, feed: &str, text: &str) {
        self.working.lock().unwrap().insert(feed.into(), text.into());
        host.emit("progress", json!({ "feed": feed, "text": text }));
    }

    /// Fetches and reads every timetable that's missing, or older than a week (all of them with
    /// `force`). Blocks until done; the UI follows along through `status` and `progress` events.
    fn prepare(&self, host: &Host, force: bool) -> Result<Value, String> {
        let _one = self.prep.lock().unwrap_or_else(|e| e.into_inner());
        let saved = load(&Self::dir(host));
        let now = now_ms() / 1000;
        let mut downloads: HashMap<String, Arc<Vec<u8>>> = HashMap::new();
        for f in feeds(&saved) {
            let dir = Self::feed_dir(host, &f.id);
            let fetched: Option<Fetched> = read_json(&dir.join("fetched.json"));
            let fresh = dir.join("index.json").is_file() && fetched.as_ref().is_some_and(|x| x.url == f.schedule && now - x.at <= STALE_SECS);
            if fresh && !force {
                continue;
            }
            self.errors.lock().unwrap().remove(&f.id);
            let r = (|| -> Result<(), String> {
                let bytes = match downloads.get(&f.schedule) {
                    Some(b) => b.clone(),
                    None => {
                        self.progress(host, &f.id, "Downloading the timetable");
                        let b = Arc::new(get(&f.schedule, None)?);
                        downloads.insert(f.schedule.clone(), b.clone());
                        b
                    }
                };
                self.progress(host, &f.id, "Reading the timetable");
                let zip = match &f.inner {
                    Some(name) => gtfs::inner(&bytes, name)?,
                    None => bytes.to_vec(),
                };
                let want: HashSet<String> = saved.stops.iter().filter(|s| s.feed == f.id).flat_map(|s| s.ids.iter().cloned()).collect();
                let (index, visits) = gtfs::build(&zip, &want)?;
                std::fs::write(dir.join("schedule.zip"), &zip).map_err(|e| e.to_string())?;
                write_json(&dir.join("index.json"), &index)?;
                write_json(&dir.join("visits.json"), &visits)?;
                write_json(&dir.join("fetched.json"), &Fetched { at: now, url: f.schedule.clone() })?;
                host.info(format!("{}: {} stops, {} routes", f.name, index.stops.len(), index.routes.len()));
                self.loaded.lock().unwrap().insert(f.id.clone(), Arc::new(Loaded { index, visits }));
                Ok(())
            })();
            self.working.lock().unwrap().remove(&f.id);
            if let Err(e) = r {
                host.warn(format!("{}: {e}", f.name));
                self.errors.lock().unwrap().insert(f.id.clone(), e);
            }
            host.emit("progress", json!({ "feed": f.id, "text": "" }));
        }
        Ok(self.status(host))
    }

    /// Stops whose name has every word of `q`, or whose number is `q`, in every timetable.
    fn search(&self, host: &Host, q: &str) -> Result<Value, String> {
        let saved = load(&Self::dir(host));
        let raw = q.trim().to_lowercase();
        let q = words(&raw);
        if q.is_empty() {
            return Ok(json!([]));
        }
        let parts: Vec<&str> = q.split(' ').collect();
        let mut hits: Vec<(u8, String, Value)> = vec![];
        let mut any = false;
        for f in feeds(&saved) {
            let Some(l) = self.loaded(host, &f.id) else { continue };
            any = true;
            let ix = &l.index;
            let has: HashSet<&str> = ix.stops.iter().map(|s| s.id.as_str()).collect();
            for s in &ix.stops {
                // A platform shows through its station.
                if !s.parent.is_empty() && has.contains(s.parent.as_str()) {
                    continue;
                }
                let name = words(&s.name);
                let score = if !s.code.is_empty() && s.code.to_lowercase() == raw {
                    0
                } else if parts.iter().all(|w| name.contains(w)) {
                    if name.starts_with(&q) { 1 } else { 2 }
                } else {
                    continue;
                };
                if s.routes.is_empty() {
                    continue;
                }
                let routes: Vec<Value> = s.routes.iter().take(12).filter_map(|r| ix.routes.get(r).map(|x| json!({ "id": r, "label": x.label(), "color": x.color, "text": x.text, "kind": x.kind }))).collect();
                let key = format!("{}:{}", f.id, s.id);
                hits.push((score, s.name.clone(), json!({ "feed": f.id, "feedName": f.name, "id": s.id, "code": s.code, "name": s.name, "station": s.station, "routes": routes, "saved": saved.stops.iter().any(|x| x.key == key) })));
            }
        }
        if !any {
            return Err("The timetable isn't here yet.".into());
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(json!(hits.into_iter().take(40).map(|h| h.2).collect::<Vec<_>>()))
    }

    /// Saves a stop, reading its timetable rows if it's new to the cache.
    fn add(&self, host: &Host, feed: &str, id: &str) -> Result<Value, String> {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        let key = format!("{feed}:{id}");
        if saved.stops.iter().any(|s| s.key == key) {
            return Ok(self.status(host));
        }
        if saved.stops.len() >= 12 {
            return Err("That's 12 stops already; remove one first.".into());
        }
        let l = self.loaded(host, feed).ok_or("The timetable isn't here yet.")?;
        let stop = l.index.stops.iter().find(|s| s.id == id).ok_or("That stop isn't in the timetable anymore.")?.clone();
        let ids = l.index.ids_of(id);
        let missing: HashSet<String> = ids.iter().filter(|i| !l.visits.contains_key(*i)).cloned().collect();
        if !missing.is_empty() {
            let fdir = Self::feed_dir(host, feed);
            self.progress(host, feed, "Reading this stop's timetable");
            let r = std::fs::read(fdir.join("schedule.zip")).map_err(|e| e.to_string()).and_then(|b| gtfs::visits(&b, &missing));
            self.working.lock().unwrap().remove(feed);
            host.emit("progress", json!({ "feed": feed, "text": "" }));
            let mut visits = l.visits.clone();
            for i in &missing {
                visits.insert(i.clone(), vec![]);
            }
            visits.extend(r?);
            write_json(&fdir.join("visits.json"), &visits)?;
            self.loaded.lock().unwrap().insert(feed.into(), Arc::new(Loaded { index: l.index.clone(), visits }));
        }
        // Its icon: what most of its routes are.
        let mut kinds: HashMap<u16, usize> = HashMap::new();
        for r in &stop.routes {
            if let Some(x) = l.index.routes.get(r) {
                *kinds.entry(x.kind).or_default() += 1;
            }
        }
        let kind = kinds.into_iter().max_by_key(|(k, n)| (*n, *k)).map(|(k, _)| k).unwrap_or(3);
        saved.stops.push(SavedStop { key, feed: feed.into(), id: id.into(), name: stop.name, code: stop.code, ids, routes: vec![], window: None, kind });
        save(&dir, &saved);
        Ok(self.status(host))
    }

    fn remove(&self, host: &Host, key: &str) -> Value {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        saved.stops.retain(|s| s.key != key);
        save(&dir, &saved);
        self.status(host)
    }

    fn update(&self, host: &Host, key: &str, p: &Value) -> Result<Value, String> {
        let dir = Self::dir(host);
        let mut saved = load(&dir);
        let s = saved.stops.iter_mut().find(|s| s.key == key).ok_or("That stop isn't saved.")?;
        if let Some(r) = p.get("routes") {
            s.routes = serde_json::from_value(r.clone()).map_err(|e| e.to_string())?;
        }
        if let Some(w) = p.get("window") {
            s.window = if w.is_null() { None } else { Some(serde_json::from_value(w.clone()).map_err(|e| e.to_string())?) };
        }
        if let Some(i) = p.get("index").and_then(|v| v.as_u64()) {
            // A new place in the list.
            let i = (i as usize).min(saved.stops.len() - 1);
            let at = saved.stops.iter().position(|s| s.key == key).unwrap_or(0);
            let s = saved.stops.remove(at);
            saved.stops.insert(i, s);
        }
        save(&dir, &saved);
        Ok(self.status(host))
    }

    /// A feed's live trip updates, read again at most every 20 seconds.
    fn live(&self, f: &Feed) -> Result<Arc<HashMap<String, TripUpdate>>, String> {
        let mut all = HashMap::new();
        for url in &f.live {
            if let Some((at, v)) = self.live.lock().unwrap().get(url) {
                if at.elapsed() < Duration::from_secs(20) {
                    all.extend(v.iter().map(|(k, t)| (k.clone(), t.clone())));
                    continue;
                }
            }
            let feed = rt::decode(&get(url, f.header.as_ref())?)?;
            let v = Arc::new(feed.trips);
            self.live.lock().unwrap().insert(url.clone(), (Instant::now(), v.clone()));
            all.extend(v.iter().map(|(k, t)| (k.clone(), t.clone())));
        }
        Ok(Arc::new(all))
    }

    /// Every saved stop's next departures.
    fn departures(&self, host: &Host) -> Value {
        let saved = load(&Self::dir(host));
        let now = now_ms() / 1000;
        let all = feeds(&saved);
        let mut live: HashMap<String, Result<Arc<HashMap<String, TripUpdate>>, String>> = HashMap::new();
        let stops: Vec<Value> = saved
            .stops
            .iter()
            .map(|s| {
                let Some(f) = all.iter().find(|f| f.id == s.feed) else { return json!({ "key": s.key, "departures": [], "error": "Its city isn't picked anymore." }) };
                let Some(l) = self.loaded(host, &s.feed) else { return json!({ "key": s.key, "departures": [], "error": "The timetable isn't here yet." }) };
                let rt = live.entry(f.id.clone()).or_insert_with(|| if f.live.is_empty() { Ok(Arc::new(HashMap::new())) } else { self.live(f) });
                let empty = HashMap::new();
                let (updates, live_ok, live_error) = match rt {
                    Ok(m) => (&**m, !f.live.is_empty(), String::new()),
                    Err(e) => (&empty, false, e.clone()),
                };
                let ids: HashSet<String> = s.ids.iter().cloned().collect();
                let visits: Vec<Visit> = s.ids.iter().flat_map(|i| l.visits.get(i).cloned().unwrap_or_default()).collect();
                let mut b = board::board(&visits, &ids, &l.index, updates, now, AHEAD_SECS, &s.routes);
                b.truncate(8);
                json!({ "key": s.key, "departures": b, "live": live_ok, "liveError": live_error })
            })
            .collect();
        json!({ "at": now, "stops": stops })
    }

    /// The routes at a saved stop, for picking which to show.
    fn routes(&self, host: &Host, key: &str) -> Result<Value, String> {
        let saved = load(&Self::dir(host));
        let s = saved.stops.iter().find(|s| s.key == key).ok_or("That stop isn't saved.")?;
        let l = self.loaded(host, &s.feed).ok_or("The timetable isn't here yet.")?;
        let mut ids: Vec<String> = s.ids.iter().flat_map(|i| l.visits.get(i).into_iter().flatten().map(|v| v.route.clone())).collect::<HashSet<_>>().into_iter().collect();
        ids.sort_by_key(|r| l.index.routes.get(r).map(|x| (x.label().len(), x.label().to_string())).unwrap_or_default());
        Ok(json!(ids.iter().filter_map(|r| l.index.routes.get(r).map(|x| json!({ "id": r, "label": x.label(), "long": x.long, "color": x.color, "text": x.text, "kind": x.kind }))).collect::<Vec<_>>()))
    }
}

impl Module for Transit {
    fn init(&self, _host: &Host, _init: &Init) -> Result<(), RpcError> {
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, p: Value) -> CallResult {
        let failed = |e: String| RpcError::failed(e);
        match method {
            "status" => Ok(self.status(host)),
            "city" => self.set_city(host, &p).map_err(failed),
            "prepare" => self.prepare(host, p["force"].as_bool().unwrap_or(false)).map_err(failed),
            "search" => self.search(host, p["q"].as_str().unwrap_or("")).map_err(failed),
            "add" => self.add(host, p["feed"].as_str().unwrap_or(""), p["id"].as_str().unwrap_or("")).map_err(failed),
            "remove" => Ok(self.remove(host, p["key"].as_str().unwrap_or(""))),
            "update" => self.update(host, p["key"].as_str().unwrap_or(""), &p).map_err(failed),
            "routes" => self.routes(host, p["key"].as_str().unwrap_or("")).map_err(failed),
            "departures" => Ok(self.departures(host)),
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}

fn main() {
    island_module::run(Transit::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn philadelphia_is_two_feeds_from_one_download() {
        let f = feeds(&Saved { city: "philadelphia".into(), ..Default::default() });
        assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["septa-bus", "septa-rail"]);
        assert!(f.iter().all(|x| x.schedule == SEPTA_SCHEDULE && x.live.len() == 1));
        assert_eq!(f[1].inner.as_deref(), Some("google_rail.zip"));
        assert!(feeds(&Saved::default()).is_empty());
    }

    #[test]
    fn searches_read_like_stop_names() {
        assert_eq!(words("30th Street Station"), "30th st sta");
        assert_eq!(words("Chestnut St & 34th St - FS"), "chestnut st 34th st fs");
        assert_eq!(words("Broad and Walnut"), "broad walnut");
    }

    #[test]
    fn hosts_from_addresses() {
        assert_eq!(host_of("https://API.Example.org:443/gtfs.zip?key=1").as_deref(), Some("api.example.org"));
        assert_eq!(host_of("http://example.org/x"), None);
        assert_eq!(host_of("https://evil host/x"), None);
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "transit");
        // SEPTA's feeds are reachable out of the box; another city's through settings.json.
        let hosts = island_module::proto::manifest::resolve_hosts(&m.hosts, &json!({ "hosts": ["gtfs.example.org"] }));
        assert!(hosts.iter().any(|h| h == "www3.septa.org") && hosts.iter().any(|h| h == "gtfs.example.org"));
    }
}
