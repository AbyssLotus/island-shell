//! Destiny 2, through Bungie's API with the user's own API key (bungie.net/en/Application).
//!
//! - Your characters: class, race, Power, emblem, time played, and what each is doing right now.
//! - The selected character's equipped subclass, weapons and armor, with icons and Power.
//! - Xûr: where he is and what exotics he's selling (when he's here, Friday to the weekly reset).
//!
//! All of it is public data, so the API key alone is enough: no Bungie sign-in. Names and icons
//! come from Bungie's manifest one definition at a time (`/Destiny2/Manifest/{type}/{hash}/`) and are
//! cached in `destiny-defs.json`, so only the first look at a new item costs a request. The key is
//! stored encrypted with DPAPI in `destiny.json`, next to the Bungie Name it looks up.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Storage::Streams::UnicodeEncoding;
use windows::Web::Http::Filters::{HttpBaseProtocolFilter, HttpCacheReadBehavior};
use windows::Web::Http::{HttpClient, HttpStringContent};

use crate::util::{log_line, protect, unprotect};

const API: &str = "https://www.bungie.net/Platform";
const ROOT: &str = "https://www.bungie.net";
const UA: &str = "IslandShell/0.1 (personal Windows desktop shell)";
const XUR: u32 = 2190858386;

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

fn saved_path(dir: &Path) -> PathBuf {
    dir.join("destiny.json")
}

fn load(dir: &Path) -> Saved {
    std::fs::read(saved_path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Saved) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(saved_path(dir), b);
    }
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub configured: bool,
    /// Bungie Name, e.g. "Guardian#1234".
    pub name: String,
}

pub fn status(dir: &Path) -> Status {
    let s = load(dir);
    Status { configured: !s.key.is_empty() && !s.membership_id.is_empty(), name: s.name }
}

// ---------- HTTP ----------

fn request(key: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
    let e = |e: windows::core::Error| e.message().to_string();
    // Bungie's answers carry cache headers; always ask for the current state.
    let filter = HttpBaseProtocolFilter::new().map_err(e)?;
    let _ = filter.CacheControl().and_then(|c| c.SetReadBehavior(HttpCacheReadBehavior::NoCache));
    let client = HttpClient::Create(&filter).map_err(e)?;
    let headers = client.DefaultRequestHeaders().map_err(e)?;
    let _ = headers.UserAgent().and_then(|ua| ua.TryParseAdd(&HSTRING::from(UA)));
    headers.Append(&HSTRING::from("X-API-Key"), &HSTRING::from(key)).map_err(e)?;
    let uri = Uri::CreateUri(&HSTRING::from(format!("{API}{path}"))).map_err(e)?;
    let resp = match body {
        Some(b) => {
            let content = HttpStringContent::CreateFromStringWithEncodingAndMediaType(&HSTRING::from(b), UnicodeEncoding::Utf8, &HSTRING::from("application/json")).map_err(e)?;
            client.PostAsync(&uri, &content).and_then(|op| op.get()).map_err(e)?
        }
        None => client.GetAsync(&uri).and_then(|op| op.get()).map_err(e)?,
    };
    let status = resp.StatusCode().map_err(e)?.0;
    let text = resp.Content().and_then(|c| c.ReadAsStringAsync()).and_then(|op| op.get()).map_err(e)?.to_string();
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("Bungie answered HTTP {status} without data (maintenance?)"))?;
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

static DEFS: Mutex<Option<HashMap<String, Def>>> = Mutex::new(None);

fn defs_path(dir: &Path) -> PathBuf {
    dir.join("destiny-defs.json")
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

/// Makes sure every (type, hash) has a definition, fetching missing ones (six at a time).
fn ensure_defs(dir: &Path, key: &str, want: &[(&'static str, u32)]) {
    let missing: Vec<(&'static str, u32)> = {
        let mut g = DEFS.lock().unwrap();
        let defs = g.get_or_insert_with(|| std::fs::read(defs_path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
        let mut seen = HashSet::new();
        want.iter().filter(|(t, h)| *h != 0 && !defs.contains_key(&format!("{t}:{h}")) && seen.insert((*t, *h))).copied().collect()
    };
    if missing.is_empty() {
        return;
    }
    let fetched: Vec<(String, Def)> = std::thread::scope(|s| {
        let jobs: Vec<_> = missing
            .chunks(missing.len().div_ceil(6))
            .map(|chunk| {
                s.spawn(move || {
                    crate::util::com_mta();
                    chunk.iter().filter_map(|(t, h)| request(key, &format!("/Destiny2/Manifest/{t}/{h}/"), None).ok().map(|v| (format!("{t}:{h}"), def_from(&v)))).collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
    });
    let mut g = DEFS.lock().unwrap();
    let defs = g.get_or_insert_with(HashMap::new);
    defs.extend(fetched);
    if let Ok(b) = serde_json::to_vec(defs) {
        let _ = std::fs::write(defs_path(dir), b);
    }
}

fn def(t: &str, h: u32) -> Def {
    DEFS.lock().unwrap().as_ref().and_then(|d| d.get(&format!("{t}:{h}")).cloned()).unwrap_or_default()
}

fn img(path: &str) -> String {
    if path.is_empty() { String::new() } else { format!("{ROOT}{path}") }
}

// ---------- setup ----------

/// Saves the key and looks the Bungie Name up ("Name#1234").
pub fn configure(dir: &Path, key: &str, bungie_name: &str) -> Result<Status, String> {
    let mut saved = load(dir);
    let key = if key.trim().is_empty() { unprotect(&saved.key) } else { key.trim().to_string() };
    if key.is_empty() {
        return Err("Paste your API key first.".into());
    }
    let (name, code) = bungie_name.trim().rsplit_once('#').ok_or("Your Bungie Name ends in # and four digits, like Guardian#1234.")?;
    let code: u32 = code.trim().parse().map_err(|_| "The part after # should be digits, like Guardian#1234.".to_string())?;
    let body = serde_json::json!({ "displayName": name.trim(), "displayNameCode": code }).to_string();
    let found = request(&key, "/Destiny2/SearchDestinyPlayerByBungieName/-1/", Some(&body))?;
    let cards = found.as_array().cloned().unwrap_or_default();
    // With Cross Save, the account whose type matches crossSaveOverride holds the characters.
    let card = cards
        .iter()
        .find(|c| c["crossSaveOverride"].as_i64().is_some_and(|o| o != 0 && Some(o) == c["membershipType"].as_i64()))
        .or(cards.first())
        .ok_or("No Destiny account with that Bungie Name.")?;
    saved.key = protect(&key);
    saved.membership_type = card["membershipType"].as_i64().unwrap_or(0);
    saved.membership_id = card["membershipId"].as_str().unwrap_or("").to_string();
    saved.name = format!("{}#{:04}", card["bungieGlobalDisplayName"].as_str().unwrap_or(name.trim()), card["bungieGlobalDisplayNameCode"].as_u64().unwrap_or(code as u64));
    save(dir, &saved);
    log_line(&format!("destiny: linked {} (type {})", saved.name, saved.membership_type));
    *PROFILE.lock().unwrap() = None;
    Ok(status(dir))
}

pub fn forget(dir: &Path) {
    save(dir, &Saved::default());
    *PROFILE.lock().unwrap() = None;
    *XUR_CACHE.lock().unwrap() = None;
}

// ---------- profile ----------

#[derive(Serialize, Clone, Default)]
pub struct Item {
    pub slot: String,
    pub name: String,
    pub kind: String,
    pub icon: String,
    pub exotic: bool,
    pub power: Option<u64>,
}

#[derive(Serialize, Clone, Default)]
pub struct Character {
    pub id: String,
    pub class: String,
    pub race: String,
    pub light: u64,
    pub emblem: String,
    pub emblem_bg: String,
    /// ISO date.
    pub last_played: String,
    pub minutes_played: u64,
    /// What it's doing now ("Crucible · Control", "Orbit"), when online.
    pub activity: String,
    pub activity_since: String,
    pub items: Vec<Item>,
}

#[derive(Serialize, Clone, Default)]
pub struct Profile {
    pub name: String,
    pub guardian_rank: u64,
    pub characters: Vec<Character>,
    /// Unix ms when this was read.
    pub at: u64,
}

static PROFILE: Mutex<Option<(Instant, Profile)>> = Mutex::new(None);

fn creds(dir: &Path) -> Result<(String, Saved), String> {
    let saved = load(dir);
    let key = unprotect(&saved.key);
    if key.is_empty() || saved.membership_id.is_empty() {
        return Err("Destiny isn't set up yet.".into());
    }
    Ok((key, saved))
}

/// Your characters, most recently played first. Re-read at most every 20 seconds.
pub fn profile(dir: &Path) -> Result<Profile, String> {
    if let Some((at, p)) = PROFILE.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(20) {
            return Ok(p.clone());
        }
    }
    let (key, saved) = creds(dir)?;
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
    ensure_defs(dir, &key, &want);
    let mut characters: Vec<Character> = chars
        .iter()
        .map(|(id, c)| {
            let act = &v["characterActivities"]["data"][id];
            let a = def("DestinyActivityDefinition", h(&act["currentActivityHash"]));
            let mode = def("DestinyActivityModeDefinition", h(&act["currentActivityModeHash"]));
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
                    let d = def("DestinyInventoryItemDefinition", h(&it["itemHash"]));
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
    let p = Profile {
        name: saved.name.clone(),
        guardian_rank: v["profile"]["data"]["currentGuardianRank"].as_u64().unwrap_or(0),
        characters,
        at: crate::util::now_ms(),
    };
    *PROFILE.lock().unwrap() = Some((Instant::now(), p.clone()));
    Ok(p)
}

// ---------- Xûr ----------

#[derive(Serialize, Clone, Default)]
pub struct Xur {
    /// Selling right now (Bungie lists him all week these days, without a location).
    pub here: bool,
    pub location: String,
    /// ISO date his stock refreshes next.
    pub leaves: String,
    pub items: Vec<Item>,
}

static XUR_CACHE: Mutex<Option<(Instant, Xur)>> = Mutex::new(None);

/// What Xûr sells that isn't a weapon or armor (his sparrow, engrams, quest steps…).
const NOT_GEAR: [&str; 12] = ["Vehicle", "Engram", "Quest", "Bounty", "Ship", "Emblem", "Shader", "Ornament", "Emote", "Transmat", "Currency", "Material"];

/// Xûr from Bungie's public vendor data (re-read at most every 20 minutes).
pub fn xur(dir: &Path) -> Result<Xur, String> {
    if let Some((at, x)) = XUR_CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(20 * 60) {
            return Ok(x.clone());
        }
    }
    let (key, _) = creds(dir)?;
    let v = request(&key, "/Destiny2/Vendors/?components=400,402", None)?;
    let vendor = &v["vendors"]["data"][XUR.to_string()];
    let mut x = Xur::default();
    if vendor.is_object() && vendor["enabled"].as_bool() != Some(false) {
        x.here = true;
        x.leaves = vendor["nextRefreshDate"].as_str().unwrap_or("").to_string();
        let sales: Vec<u32> = v["sales"]["data"][XUR.to_string()]["saleItems"]
            .as_object()
            .map(|o| o.values().map(|s| s["itemHash"].as_u64().unwrap_or(0) as u32).collect())
            .unwrap_or_default();
        let mut want: Vec<(&'static str, u32)> = vec![("DestinyVendorDefinition", XUR)];
        want.extend(sales.iter().map(|h| ("DestinyInventoryItemDefinition", *h)));
        ensure_defs(dir, &key, &want);
        let places = def("DestinyVendorDefinition", XUR).places;
        if let Some(dest) = vendor["vendorLocationIndex"].as_u64().and_then(|i| places.get(i as usize)) {
            ensure_defs(dir, &key, &[("DestinyDestinationDefinition", *dest)]);
            x.location = def("DestinyDestinationDefinition", *dest).name;
        }
        // His exotic gear (not the currencies, engrams and catalysts around it).
        x.items = sales
            .iter()
            .map(|h| def("DestinyInventoryItemDefinition", *h))
            .filter(|d| d.tier == 6 && !d.name.is_empty() && !d.kind.is_empty() && !NOT_GEAR.iter().any(|k| d.kind.contains(k)))
            .map(|d| Item { slot: String::new(), name: d.name, kind: d.kind, icon: img(&d.icon), exotic: true, power: None })
            .collect();
    }
    *XUR_CACHE.lock().unwrap() = Some((Instant::now(), x.clone()));
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_parse() {
        let v = serde_json::json!({
            "displayProperties": { "name": "Ace of Spades", "icon": "/common/destiny2_content/icons/ace.jpg" },
            "itemTypeDisplayName": "Hand Cannon",
            "inventory": { "tierType": 6 }
        });
        let d = def_from(&v);
        assert_eq!((d.name.as_str(), d.kind.as_str(), d.tier), ("Ace of Spades", "Hand Cannon", 6));
        assert_eq!(img(&d.icon), "https://www.bungie.net/common/destiny2_content/icons/ace.jpg");
        let vendor = def_from(&serde_json::json!({ "displayProperties": { "name": "Xûr" }, "locations": [{ "destinationHash": 1 }, { "destinationHash": 2 }] }));
        assert_eq!(vendor.places, vec![1, 2]);
    }

    /// Against Bungie's real API (checks the plumbing): the manifest index needs no key; a player
    /// search without one is refused with a readable message.
    #[test]
    #[ignore = "needs the network: calls bungie.net"]
    fn live_without_key() {
        crate::util::com_mta();
        let m = request("", "/Destiny2/Manifest/", None).expect("manifest");
        println!("manifest version {}", m["version"]);
        assert!(m["version"].is_string());
        let body = serde_json::json!({ "displayName": "Guardian", "displayNameCode": 1 }).to_string();
        let e = request("", "/Destiny2/SearchDestinyPlayerByBungieName/-1/", Some(&body));
        println!("search without a key: {:?}", e.as_ref().err());
    }

    #[test]
    fn errors_explain_themselves() {
        let e = bungie_error(&serde_json::json!({ "ErrorCode": 2101, "ErrorStatus": "ApiInvalidOrExpiredKey", "Message": "x" }));
        assert!(e.contains("API key"));
        let e = bungie_error(&serde_json::json!({ "ErrorCode": 99, "ErrorStatus": "Other", "Message": "Something" }));
        assert_eq!(e, "Bungie: Something");
    }
}
