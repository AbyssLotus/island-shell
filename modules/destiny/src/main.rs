//! Destiny 2, through Bungie's API with the user's own API key (bungie.net/en/Application). An
//! Island Shell module backend (docs/design/modules.md); its UI is ui/destiny.js.
//!
//! - Your characters: class, race, Power, emblem, time played, and what each is doing right now.
//! - The selected character's equipped subclass, weapons and armor, with icons and Power.
//! - Xûr: where he is and what exotics he's selling.
//! - The weekly reset in the island's calendar (Tuesdays, 17:00 UTC), once Destiny is set up.
//!
//! All of it is public data, so the API key alone is enough: no Bungie sign-in. Names and icons come
//! from Bungie's manifest one definition at a time (`/Destiny2/Manifest/{type}/{hash}/`) and are kept
//! in the module's cache folder, so only the first look at a new item costs a request. The key is
//! stored DPAPI-encrypted in the module's data folder (`destiny.json`, taken over from 0.1.x).
//!
//! RPC methods: `status`, `configure { key, name }`, `profile`, `xur`, `forget`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use island_module::http::Request;
use island_module::secrets::{protect, unprotect};
use island_module::{json, CalendarEvent, CallResult, Host, Init, Module, RpcError, Value};
use serde::{Deserialize, Serialize};

const API: &str = "https://www.bungie.net/Platform";
const ROOT: &str = "https://www.bungie.net";
const XUR: u32 = 2190858386;
const DAY_MS: i64 = 86_400_000;

/// Equipment slots worth showing, in order (inventory bucket hashes).
const SLOTS: [(u32, &str); 9] = [
    (3284755031, "Subclass"),
    (1498876634, "Kinetic"),
    (2465295065, "Energy"),
    (953998645, "Power"),
    (3448274439, "Helmet"),
    (3551918588, "Arms"),
    (14239492, "Chest"),
    (20886954, "Legs"),
    (1585787867, "Class item"),
];

// ---------- settings ----------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    /// DPAPI-encrypted, base64.
    #[serde(default)]
    key: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    membership_type: i64,
    #[serde(default)]
    membership_id: String,
}

fn load(dir: &Path) -> Saved {
    std::fs::read(dir.join("destiny.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(dir.join("destiny.json"), b);
    }
}

#[derive(Serialize, Clone, Default)]
struct Status {
    configured: bool,
    /// Bungie Name, e.g. "Guardian#1234".
    name: String,
}

fn status(dir: &Path) -> Status {
    let s = load(dir);
    Status { configured: !s.key.is_empty() && !s.membership_id.is_empty(), name: s.name }
}

// ---------- HTTP ----------

fn request(key: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
    let url = format!("{API}{path}");
    let req = match body {
        Some(b) => Request::post(&url, "application/json", b),
        None => Request::get(&url),
    };
    let resp = req.header("X-API-Key", key).send()?;
    let v: Value = serde_json::from_slice(&resp.body).map_err(|_| format!("Bungie answered HTTP {} without data (maintenance?)", resp.status))?;
    // Bungie wraps everything: ErrorCode 1 = success, anything else explains itself in Message.
    match v["ErrorCode"].as_i64() {
        Some(1) => Ok(v["Response"].clone()),
        _ => Err(bungie_error(&v)),
    }
}

fn bungie_error(v: &Value) -> String {
    let status = v["ErrorStatus"].as_str().unwrap_or("");
    let msg = v["Message"].as_str().unwrap_or("unknown error");
    match status {
        "ApiInvalidOrExpiredKey" | "ApiKeyMissingFromRequest" => "Bungie didn't accept the API key.".into(),
        "OriginHeaderDoesNotMatchKey" => "Your Bungie app has an Origin Header set; clear that field at bungie.net/en/Application.".into(),
        "SystemDisabled" => "Bungie's API is down for maintenance.".into(),
        "DestinyPrivacyRestriction" => "Your Destiny profile is private on bungie.net (Privacy settings).".into(),
        _ => format!("Bungie: {msg}"),
    }
}

// ---------- manifest definitions (cached on disk) ----------

#[derive(Serialize, Deserialize, Clone, Default)]
struct Def {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    icon: String,
    /// "Hand Cannon", "Helmet", …
    #[serde(default, skip_serializing_if = "String::is_empty")]
    kind: String,
    /// Rarity (6 = Exotic, 5 = Legendary).
    #[serde(default, skip_serializing_if = "is_zero")]
    tier: u32,
    /// Destination (for activities).
    #[serde(default, skip_serializing_if = "is_zero")]
    dest: u32,
    /// Destinations by location index (for vendors).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    places: Vec<u32>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn def_from(v: &Value) -> Def {
    let dp = |k: &str| v["displayProperties"][k].as_str().filter(|s| !s.is_empty()).or(v["originalDisplayProperties"][k].as_str()).unwrap_or("").to_string();
    Def {
        name: dp("name"),
        icon: dp("icon"),
        kind: v["itemTypeDisplayName"].as_str().unwrap_or("").to_string(),
        tier: v["inventory"]["tierType"].as_u64().unwrap_or(0) as u32,
        dest: v["destinationHash"].as_u64().unwrap_or(0) as u32,
        places: v["locations"].as_array().map(|a| a.iter().map(|l| l["destinationHash"].as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default(),
    }
}

fn img(path: &str) -> String {
    if path.is_empty() { String::new() } else { format!("{ROOT}{path}") }
}

// ---------- the module ----------

#[derive(Serialize, Clone, Default)]
struct Item {
    slot: String,
    name: String,
    kind: String,
    icon: String,
    exotic: bool,
    power: Option<u64>,
}

#[derive(Serialize, Clone, Default)]
struct Character {
    id: String,
    class: String,
    race: String,
    light: u64,
    emblem: String,
    emblem_bg: String,
    /// ISO date.
    last_played: String,
    minutes_played: u64,
    /// What it's doing now ("Crucible · Control", "Orbit"), when online.
    activity: String,
    activity_since: String,
    items: Vec<Item>,
}

#[derive(Serialize, Clone, Default)]
struct Profile {
    name: String,
    guardian_rank: u64,
    characters: Vec<Character>,
    /// Unix ms when this was read.
    at: u64,
}

#[derive(Serialize, Clone, Default)]
struct Xur {
    /// Selling right now (Bungie lists him all week these days, without a location).
    here: bool,
    location: String,
    /// ISO date his stock refreshes next.
    leaves: String,
    items: Vec<Item>,
}

/// What Xûr sells that isn't a weapon or armor (his sparrow, engrams, quest steps…).
const NOT_GEAR: [&str; 12] = ["Vehicle", "Engram", "Quest", "Bounty", "Ship", "Emblem", "Shader", "Ornament", "Emote", "Transmat", "Currency", "Material"];

#[derive(Default)]
struct Destiny {
    defs: Mutex<Option<HashMap<String, Def>>>,
    profile: Mutex<Option<(Instant, Profile)>>,
    xur: Mutex<Option<(Instant, Xur)>>,
}

impl Destiny {
    fn defs_path(host: &Host) -> PathBuf {
        host.cache_dir().join("defs.json")
    }

    /// Makes sure every (type, hash) has a definition, fetching missing ones (six at a time).
    fn ensure_defs(&self, host: &Host, key: &str, want: &[(&'static str, u32)]) {
        let path = Self::defs_path(host);
        let missing: Vec<(&'static str, u32)> = {
            let mut g = self.defs.lock().unwrap();
            let defs = g.get_or_insert_with(|| std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
            let mut seen = HashSet::new();
            want.iter().filter(|(t, h)| *h != 0 && !defs.contains_key(&format!("{t}:{h}")) && seen.insert((*t, *h))).copied().collect()
        };
        if missing.is_empty() {
            return;
        }
        let fetched: Vec<(String, Def)> = std::thread::scope(|s| {
            let jobs: Vec<_> = missing
                .chunks(missing.len().div_ceil(6))
                .map(|chunk| s.spawn(move || chunk.iter().filter_map(|(t, h)| request(key, &format!("/Destiny2/Manifest/{t}/{h}/"), None).ok().map(|v| (format!("{t}:{h}"), def_from(&v)))).collect::<Vec<_>>()))
                .collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
        });
        let mut g = self.defs.lock().unwrap();
        let defs = g.get_or_insert_with(HashMap::new);
        defs.extend(fetched);
        if let Ok(b) = serde_json::to_vec(defs) {
            let _ = std::fs::write(&path, b);
        }
    }

    fn def(&self, t: &str, h: u32) -> Def {
        self.defs.lock().unwrap().as_ref().and_then(|d| d.get(&format!("{t}:{h}")).cloned()).unwrap_or_default()
    }

    fn creds(dir: &Path) -> Result<(String, Saved), RpcError> {
        let saved = load(dir);
        let key = unprotect(&saved.key);
        if key.is_empty() || saved.membership_id.is_empty() {
            return Err(RpcError::not_set_up("Destiny isn't set up yet."));
        }
        Ok((key, saved))
    }

    /// Saves the key (kept if left blank) and looks the Bungie Name up ("Name#1234").
    fn configure(&self, host: &Host, key: &str, bungie_name: &str) -> Result<Status, String> {
        let dir = host.data_dir();
        let mut saved = load(&dir);
        let key = if key.trim().is_empty() { unprotect(&saved.key) } else { key.trim().to_string() };
        if key.is_empty() {
            return Err("Paste your API key first.".into());
        }
        let (name, code) = bungie_name.trim().rsplit_once('#').ok_or("Your Bungie Name ends in # and four digits, like Guardian#1234.")?;
        let code: u32 = code.trim().parse().map_err(|_| "The part after # should be digits, like Guardian#1234.".to_string())?;
        let body = json!({ "displayName": name.trim(), "displayNameCode": code }).to_string();
        let found = request(&key, "/Destiny2/SearchDestinyPlayerByBungieName/-1/", Some(&body))?;
        let cards = found.as_array().cloned().unwrap_or_default();
        // With Cross Save, the account whose type matches crossSaveOverride holds the characters.
        let card = cards
            .iter()
            .find(|c| c["crossSaveOverride"].as_i64().is_some_and(|o| o != 0 && Some(o) == c["membershipType"].as_i64()))
            .or(cards.first())
            .ok_or("No Destiny account with that Bungie Name.")?;
        saved.key = protect(&key);
        if saved.key.is_empty() {
            return Err("Windows couldn't encrypt the key, so it wasn't saved.".into());
        }
        saved.membership_type = card["membershipType"].as_i64().unwrap_or(0);
        saved.membership_id = card["membershipId"].as_str().unwrap_or("").to_string();
        saved.name = format!("{}#{:04}", card["bungieGlobalDisplayName"].as_str().unwrap_or(name.trim()), card["bungieGlobalDisplayNameCode"].as_u64().unwrap_or(code as u64));
        save(&dir, &saved);
        host.info(format!("linked {} (type {})", saved.name, saved.membership_type));
        *self.profile.lock().unwrap() = None;
        push_resets(host, true);
        Ok(status(&dir))
    }

    fn forget(&self, host: &Host) {
        save(&host.data_dir(), &Saved::default());
        *self.profile.lock().unwrap() = None;
        *self.xur.lock().unwrap() = None;
        push_resets(host, false);
    }

    /// Your characters, most recently played first. Re-read at most every 20 seconds.
    fn profile(&self, host: &Host) -> Result<Profile, RpcError> {
        if let Some((at, p)) = self.profile.lock().unwrap().as_ref() {
            if at.elapsed() < Duration::from_secs(20) {
                return Ok(p.clone());
            }
        }
        let (key, saved) = Self::creds(&host.data_dir())?;
        let v = request(&key, &format!("/Destiny2/{}/Profile/{}/?components=100,200,204,205,300", saved.membership_type, saved.membership_id), None)?;
        let chars = v["characters"]["data"].as_object().cloned().ok_or("Bungie didn't share your characters (is your profile private on bungie.net?)")?;
        // Everything that needs a name or icon, fetched in one go.
        let mut want: Vec<(&'static str, u32)> = vec![];
        let h = |x: &Value| x.as_u64().unwrap_or(0) as u32;
        for id in chars.keys() {
            let act = &v["characterActivities"]["data"][id];
            want.push(("DestinyActivityDefinition", h(&act["currentActivityHash"])));
            want.push(("DestinyActivityModeDefinition", h(&act["currentActivityModeHash"])));
            for it in v["characterEquipment"]["data"][id]["items"].as_array().into_iter().flatten() {
                if SLOTS.iter().any(|(b, _)| *b == h(&it["bucketHash"])) {
                    want.push(("DestinyInventoryItemDefinition", h(&it["itemHash"])));
                }
            }
        }
        self.ensure_defs(host, &key, &want);
        let mut characters: Vec<Character> = chars
            .iter()
            .map(|(id, c)| {
                let act = &v["characterActivities"]["data"][id];
                let a = self.def("DestinyActivityDefinition", h(&act["currentActivityHash"]));
                let mode = self.def("DestinyActivityModeDefinition", h(&act["currentActivityModeHash"]));
                let activity = match (a.name.as_str(), mode.name.as_str()) {
                    ("", m) => m.to_string(),
                    (n, "") => n.to_string(),
                    (n, m) if n == m => n.to_string(),
                    (n, m) => format!("{m} · {n}"),
                };
                let mut items: Vec<Item> = v["characterEquipment"]["data"][id]["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|it| {
                        let (_, slot) = SLOTS.iter().find(|(b, _)| *b == h(&it["bucketHash"]))?;
                        let d = self.def("DestinyInventoryItemDefinition", h(&it["itemHash"]));
                        let inst = it["itemInstanceId"].as_str().unwrap_or("");
                        // Subclasses carry a meaningless "power" of their own; only gear has one worth showing.
                        let power = v["itemComponents"]["instances"]["data"][inst]["primaryStat"]["value"].as_u64().filter(|p| *p > 0 && *slot != "Subclass");
                        Some(Item { slot: slot.to_string(), name: d.name, kind: d.kind, icon: img(&d.icon), exotic: d.tier == 6, power })
                    })
                    .collect();
                items.sort_by_key(|i| SLOTS.iter().position(|(_, s)| *s == i.slot));
                Character {
                    id: id.clone(),
                    class: ["Titan", "Hunter", "Warlock"].get(c["classType"].as_u64().unwrap_or(9) as usize).unwrap_or(&"Guardian").to_string(),
                    race: ["Human", "Awoken", "Exo"].get(c["raceType"].as_u64().unwrap_or(9) as usize).unwrap_or(&"").to_string(),
                    light: c["light"].as_u64().unwrap_or(0),
                    emblem: img(c["emblemPath"].as_str().unwrap_or("")),
                    emblem_bg: img(c["emblemBackgroundPath"].as_str().unwrap_or("")),
                    last_played: c["dateLastPlayed"].as_str().unwrap_or("").to_string(),
                    minutes_played: c["minutesPlayedTotal"].as_str().and_then(|m| m.parse().ok()).unwrap_or(0),
                    activity: if h(&act["currentActivityHash"]) == 0 { String::new() } else { activity },
                    activity_since: act["dateActivityStarted"].as_str().unwrap_or("").to_string(),
                    items,
                }
            })
            .collect();
        characters.sort_by(|a, b| b.last_played.cmp(&a.last_played));
        let p = Profile { name: saved.name.clone(), guardian_rank: v["profile"]["data"]["currentGuardianRank"].as_u64().unwrap_or(0), characters, at: now_ms() as u64 };
        *self.profile.lock().unwrap() = Some((Instant::now(), p.clone()));
        Ok(p)
    }

    /// Xûr from Bungie's public vendor data (re-read at most every 20 minutes).
    fn xur(&self, host: &Host) -> Result<Xur, RpcError> {
        if let Some((at, x)) = self.xur.lock().unwrap().as_ref() {
            if at.elapsed() < Duration::from_secs(20 * 60) {
                return Ok(x.clone());
            }
        }
        let (key, _) = Self::creds(&host.data_dir())?;
        let v = request(&key, "/Destiny2/Vendors/?components=400,402", None)?;
        let vendor = &v["vendors"]["data"][XUR.to_string()];
        let mut x = Xur::default();
        if vendor.is_object() && vendor["enabled"].as_bool() != Some(false) {
            x.here = true;
            x.leaves = vendor["nextRefreshDate"].as_str().unwrap_or("").to_string();
            let sales: Vec<u32> = v["sales"]["data"][XUR.to_string()]["saleItems"].as_object().map(|o| o.values().map(|s| s["itemHash"].as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default();
            let mut want: Vec<(&'static str, u32)> = vec![("DestinyVendorDefinition", XUR)];
            want.extend(sales.iter().map(|h| ("DestinyInventoryItemDefinition", *h)));
            self.ensure_defs(host, &key, &want);
            let places = self.def("DestinyVendorDefinition", XUR).places;
            if let Some(dest) = vendor["vendorLocationIndex"].as_u64().and_then(|i| places.get(i as usize)) {
                self.ensure_defs(host, &key, &[("DestinyDestinationDefinition", *dest)]);
                x.location = self.def("DestinyDestinationDefinition", *dest).name;
            }
            // His exotic gear (not the currencies, engrams and catalysts around it).
            x.items = sales
                .iter()
                .map(|h| self.def("DestinyInventoryItemDefinition", *h))
                .filter(|d| d.tier == 6 && !d.name.is_empty() && !d.kind.is_empty() && !NOT_GEAR.iter().any(|k| d.kind.contains(k)))
                .map(|d| Item { slot: String::new(), name: d.name, kind: d.kind, icon: img(&d.icon), exotic: true, power: None })
                .collect();
        }
        *self.xur.lock().unwrap() = Some((Instant::now(), x.clone()));
        Ok(x)
    }
}

impl Module for Destiny {
    fn init(&self, host: &Host, _init: &Init) -> Result<(), RpcError> {
        let data = host.data_dir();
        // 0.1.x kept the key in the app's own folder; the definitions cache comes along too.
        match host.take_legacy("destiny.json", &data.join("destiny.json")) {
            Ok(true) => host.info("took over destiny.json from 0.1.x"),
            Ok(false) => {}
            Err(e) => host.warn(format!("couldn't take over destiny.json: {e}")),
        }
        let _ = host.take_legacy("destiny-defs.json", &Self::defs_path(host));
        push_resets(host, status(&data).configured);
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "status" => Ok(to(&status(&host.data_dir()))),
            "configure" => {
                let key = params["key"].as_str().unwrap_or("");
                let name = params["name"].as_str().unwrap_or("");
                self.configure(host, key, name).map(|s| to(&s)).map_err(RpcError::failed)
            }
            "profile" => self.profile(host).map(|p| to(&p)),
            "xur" => self.xur(host).map(|x| to(&x)),
            "forget" => {
                self.forget(host);
                Ok(Value::Null)
            }
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}

fn to<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

// ---------- the weekly reset in the calendar ----------

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Tuesdays at 17:00 UTC, from four weeks back to half a year ahead (the island keeps the last set,
/// so the calendar has them while this module isn't running).
fn resets(now: i64) -> Vec<CalendarEvent> {
    let today = now.div_euclid(DAY_MS);
    (today - 28..today + 183)
        .filter(|d| (d + 4).rem_euclid(7) == 2)
        .map(|d| {
            let (y, m, day) = civil(d);
            CalendarEvent { id: format!("reset-{y:04}{m:02}{day:02}"), title: "Weekly reset".into(), start: format!("{y:04}-{m:02}-{day:02}T17:00:00Z"), kind: Some("reset".into()), ..Default::default() }
        })
        .collect()
}

fn push_resets(host: &Host, configured: bool) {
    host.calendar("destiny", if configured { resets(now_ms()) } else { vec![] });
}

/// (year, month, day) from days since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn main() {
    island_module::run(Destiny::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_parse() {
        let v = json!({
            "displayProperties": { "name": "Ace of Spades", "icon": "/common/destiny2_content/icons/ace.jpg" },
            "itemTypeDisplayName": "Hand Cannon",
            "inventory": { "tierType": 6 }
        });
        let d = def_from(&v);
        assert_eq!((d.name.as_str(), d.kind.as_str(), d.tier), ("Ace of Spades", "Hand Cannon", 6));
        assert_eq!(img(&d.icon), "https://www.bungie.net/common/destiny2_content/icons/ace.jpg");
        let vendor = def_from(&json!({ "displayProperties": { "name": "Xûr" }, "locations": [{ "destinationHash": 1 }, { "destinationHash": 2 }] }));
        assert_eq!(vendor.places, vec![1, 2]);
    }

    #[test]
    fn errors_explain_themselves() {
        let e = bungie_error(&json!({ "ErrorCode": 2101, "ErrorStatus": "ApiInvalidOrExpiredKey", "Message": "x" }));
        assert!(e.contains("API key"));
        let e = bungie_error(&json!({ "ErrorCode": 99, "ErrorStatus": "Other", "Message": "Something" }));
        assert_eq!(e, "Bungie: Something");
    }

    #[test]
    fn manifest_is_valid() {
        let m = island_module::proto::Manifest::parse(include_bytes!("../module.json")).unwrap();
        assert_eq!(m.id, "destiny");
        assert_eq!(m.version, env!("CARGO_PKG_VERSION"), "module.json and Cargo.toml versions agree");
        assert!(m.hosts.iter().all(|h| API.contains(h.as_str())));
    }

    #[test]
    fn resets_fall_on_tuesdays_at_five_utc() {
        // Friday 2026-10-23, 13:41 UTC.
        let now = 1_792_762_860_000;
        let r = resets(now);
        assert_eq!(r.len(), 30);
        assert_eq!(r[0].start, "2026-09-29T17:00:00Z");
        assert!(r.iter().any(|e| e.start == "2026-10-27T17:00:00Z"));
        assert!(r.windows(2).all(|w| w[0].start < w[1].start));
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_753), (2026, 10, 27));
    }
}
