//! `module.json` (§4.2): the only thing the base reads before a module runs.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{topic, valid_id, API};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u32,
    pub description: String,
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publishes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscribes: Vec<String>,
    /// Plain-language list of everything it contacts, shown before install. Empty means nothing.
    pub talks_to: Vec<String>,
    /// Host names its backend reaches (`*.example.com` matches subdomains). The SDK's HTTP helper
    /// refuses any other host: discipline, not a sandbox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// Words a search for a missing feature should find it by ("lyrics" → Music).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// 0.1.x files (relative to the legacy folder) it takes over on its first run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub legacy: Vec<String>,
    /// `{ "<path>": "sha256-<base64>" }` for every file but module.json. Written by pack-modules.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Backend {
    pub exe: String,
    pub run: RunMode,
    /// For `while-process`: image names, like `Discord.exe` (case doesn't matter).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub process: Vec<String>,
    /// For `scheduled`: how often, like `15m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// For `on-demand`: how long without calls before it exits (default `60s`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    /// From island start to exit, while enabled.
    Always,
    /// While one of `process` runs, and 5 s after. A UI call also starts it (to open the app, or
    /// set it up); started that way, it exits after `idle` without calls once no process runs.
    WhileProcess,
    /// Woken every `every` for `island.tick`, and for UI calls; exits when idle.
    Scheduled,
    /// Started by the first UI call; exits after `idle` without calls.
    #[default]
    OnDemand,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Ui {
    pub entry: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css: Vec<String>,
    /// Import the entry at startup rather than when something of the module first shows: for
    /// modules whose chips, tiles or activities have to be up before anyone opens them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub eager: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Contributes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<Tab>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cubes: Vec<Cube>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<Site>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calendar: Vec<CalendarSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotkeys: Vec<Hotkey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<Setup>,
}

/// An island tab. Base tabs use multiples of 10 for `order`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Tab {
    pub id: String,
    pub title: String,
    /// An icon name from the island's set, or an `.svg` in the module.
    pub icon: String,
    pub order: f64,
    /// The panel's width while this tab shows, in CSS pixels (default 760).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

/// A Command Center cube (§4.12).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Cube {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub size: String,
    /// The slot it prefers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    /// Opens this docked site instead of a sheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// A real website docked into a Command Center sheet by the base's embed.rs.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Site {
    pub id: String,
    pub label: String,
    /// May hold `{settings.<key>}` from the module's settings.json.
    pub url: String,
    /// Where the skin applies, and where navigation may go (`*.example.com` matches subdomains).
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,
    #[serde(default)]
    pub default: bool,
    /// Docked, the site keeps the module's island view beside it (Discord's call controls).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub split: bool,
}

/// A section inside a base tab (Voicemeeter in Audio, VPN in Network, Wallpaper Engine in Wallpaper).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Section {
    pub tab: String,
    pub id: String,
    pub order: f64,
    /// Its name where the tab shows one (default: the module's name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// An alternative to the tab's own content, picked in a switch in its header (Wallpaper Engine's
    /// library beside the island's scenes), rather than a block under it. Only tabs with such a
    /// switch take one: `wallpaper`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub view: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CalendarSource {
    pub source: String,
    pub name: String,
    pub color: String,
}

/// Win + `key` opens `opens` (`tab:<id>` or `cube:<id>`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Hotkey {
    pub key: String,
    pub opens: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Setup {
    pub card: bool,
    #[serde(default)]
    pub preselect: bool,
}

/// Base tabs with a switch for module views (`sections[].view`).
pub const VIEW_TABS: [&str; 2] = ["wallpaper", "audio"];

/// Letters the base keeps for itself (§4.13).
pub const BASE_HOTKEYS: [&str; 4] = ["A", "V", "N", "W"];
pub const CUBE_SIZES: [&str; 4] = ["S", "M", "L", "W"];

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_slice(json).map_err(|e| format!("module.json: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    /// Every rule a manifest follows, so the base can trust its fields afterwards.
    pub fn validate(&self) -> Result<(), String> {
        let id = &self.id;
        if !valid_id(id) {
            return Err(format!("{id:?} isn't a valid module id"));
        }
        let ctx = |what: &str| format!("{id}: {what}");
        if self.name.trim().is_empty() || self.description.trim().is_empty() {
            return Err(ctx("needs a name and a description"));
        }
        semver::Version::parse(&self.version).map_err(|_| ctx(&format!("version {:?} isn't semver", self.version)))?;
        if self.api == 0 || self.api > API {
            return Err(ctx(&format!("is built for module API {}, which this version doesn't know (it speaks up to {API})", self.api)));
        }
        safe_path(&self.icon).map_err(|e| ctx(&format!("icon: {e}")))?;
        if let Some(b) = &self.backend {
            safe_path(&b.exe).map_err(|e| ctx(&format!("backend.exe: {e}")))?;
            match b.run {
                RunMode::WhileProcess if b.process.is_empty() || b.process.iter().any(|p| p.trim().is_empty() || p.contains(['/', '\\'])) => {
                    return Err(ctx("while-process needs process names"));
                }
                RunMode::Scheduled => {
                    let every = b.every.as_deref().and_then(duration).ok_or_else(|| ctx("scheduled needs every (like \"15m\")"))?;
                    if every < Duration::from_secs(60) {
                        return Err(ctx("every must be at least 1m"));
                    }
                }
                _ => {}
            }
            if let Some(idle) = &b.idle {
                duration(idle).ok_or_else(|| ctx(&format!("idle {idle:?} isn't a duration")))?;
            }
        }
        if let Some(ui) = &self.ui {
            safe_path(&ui.entry).map_err(|e| ctx(&format!("ui.entry: {e}")))?;
            if !ui.entry.ends_with(".js") {
                return Err(ctx("ui.entry must be a .js file"));
            }
            for css in &ui.css {
                safe_path(css).map_err(|e| ctx(&format!("ui.css: {e}")))?;
            }
        }
        let c = &self.contributes;
        for t in &c.tabs {
            if !valid_id(&t.id) || t.title.trim().is_empty() || !t.order.is_finite() {
                return Err(ctx(&format!("tab {:?} needs a valid id, a title and an order", t.id)));
            }
            check_icon(&t.icon).map_err(|e| ctx(&format!("tab {}: {e}", t.id)))?;
        }
        for cube in &c.cubes {
            if !valid_id(&cube.id) || cube.title.trim().is_empty() || !CUBE_SIZES.contains(&cube.size.as_str()) {
                return Err(ctx(&format!("cube {:?} needs a valid id, a title and a size of S, M, L or W", cube.id)));
            }
            check_icon(&cube.icon).map_err(|e| ctx(&format!("cube {}: {e}", cube.id)))?;
            if let Some(site) = &cube.site {
                if !c.sites.iter().any(|s| &s.id == site) {
                    return Err(ctx(&format!("cube {} opens site {site:?}, which it doesn't declare", cube.id)));
                }
            }
        }
        for s in &c.sites {
            if !valid_id(&s.id) || s.label.trim().is_empty() || !(s.url.starts_with("https://") || s.url.starts_with("{settings.")) || s.hosts.is_empty() {
                return Err(ctx(&format!("site {:?} needs a valid id, a label, an https url and its hosts", s.id)));
            }
            if s.hosts.iter().any(|h| !valid_host_pattern(h)) {
                return Err(ctx(&format!("site {}: hosts must be names like example.com or *.example.com", s.id)));
            }
            if let Some(skin) = &s.skin {
                safe_path(skin).map_err(|e| ctx(&format!("site {} skin: {e}", s.id)))?;
            }
        }
        for s in &c.sections {
            if !valid_id(&s.tab) || !valid_id(&s.id) || !s.order.is_finite() {
                return Err(ctx(&format!("section {:?} needs a tab, an id and an order", s.id)));
            }
            if s.view && !VIEW_TABS.contains(&s.tab.as_str()) {
                return Err(ctx(&format!("section {}: only {} can take a view", s.id, VIEW_TABS.join(", "))));
            }
        }
        for cal in &c.calendar {
            if !valid_id(&cal.source) || cal.name.trim().is_empty() || !valid_color(&cal.color) {
                return Err(ctx(&format!("calendar source {:?} needs an id, a name and a #rrggbb color", cal.source)));
            }
        }
        for h in &c.hotkeys {
            let ok_key = h.key.len() == 1 && h.key.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
            if !ok_key || BASE_HOTKEYS.contains(&h.key.as_str()) {
                return Err(ctx(&format!("hotkey {:?} must be one letter or digit the base doesn't use", h.key)));
            }
            if !valid_target(&h.opens) {
                return Err(ctx(&format!("hotkey {} opens {:?}; use tab:<id> or cube:<id>", h.key, h.opens)));
            }
        }
        for t in self.publishes.iter().chain(&self.subscribes) {
            if !topic::valid(t) {
                return Err(ctx(&format!("topic {t:?} isn't valid")));
            }
        }
        if self.hosts.iter().any(|h| !valid_host_pattern(h)) {
            return Err(ctx("hosts must be names like example.com or *.example.com"));
        }
        for l in &self.legacy {
            safe_path(l).map_err(|e| ctx(&format!("legacy: {e}")))?;
        }
        for (path, hash) in &self.files {
            safe_path(path).map_err(|e| ctx(&format!("files: {e}")))?;
            if path == "module.json" {
                return Err(ctx("files lists module.json itself"));
            }
            if !hash.starts_with("sha256-") || hash.len() != 7 + 44 {
                return Err(ctx(&format!("files: {path} needs a sha256-<base64> hash")));
            }
        }
        Ok(())
    }

    /// Every file the manifest refers to (which `files` must cover in a release).
    pub fn referenced_files(&self) -> Vec<String> {
        let mut out = vec![self.icon.clone()];
        if let Some(b) = &self.backend {
            out.push(b.exe.clone());
        }
        if let Some(ui) = &self.ui {
            out.push(ui.entry.clone());
            out.extend(ui.css.iter().cloned());
        }
        out.extend(self.contributes.sites.iter().filter_map(|s| s.skin.clone()));
        out.extend(self.contributes.tabs.iter().map(|t| t.icon.clone()).filter(|i| i.ends_with(".svg")));
        out.extend(self.contributes.cubes.iter().map(|c| c.icon.clone()).filter(|i| i.ends_with(".svg")));
        out.sort();
        out.dedup();
        out
    }

    pub fn idle(&self) -> Duration {
        self.backend.as_ref().and_then(|b| b.idle.as_deref()).and_then(duration).unwrap_or(Duration::from_secs(60))
    }

    pub fn every(&self) -> Option<Duration> {
        self.backend.as_ref().and_then(|b| b.every.as_deref()).and_then(duration)
    }
}

/// A relative path inside the module folder: forward slashes, no `..`, no drive or root.
pub fn safe_path(p: &str) -> Result<(), String> {
    let bad = p.is_empty()
        || p.len() > 200
        || p.starts_with('/')
        || p.contains(['\\', ':', '\0'])
        || p.chars().any(char::is_control)
        || p.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']));
    if bad {
        Err(format!("{p:?} isn't a plain relative path"))
    } else {
        Ok(())
    }
}

fn check_icon(icon: &str) -> Result<(), String> {
    if icon.ends_with(".svg") {
        safe_path(icon)
    } else if !icon.is_empty() && icon.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        Ok(())
    } else {
        Err(format!("icon {icon:?} must be an icon name or an .svg in the module"))
    }
}

fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `tab:<id>` or `cube:<id>`.
pub fn valid_target(t: &str) -> bool {
    match t.split_once(':') {
        Some(("tab" | "cube", id)) => valid_id(id),
        _ => false,
    }
}

/// `example.com` or `*.example.com`.
pub fn valid_host_pattern(h: &str) -> bool {
    if settings_key(h).is_some() {
        return true;
    }
    valid_host(h.strip_prefix("*.").unwrap_or(h))
}

/// A plain host name: lowercase labels with a dot somewhere.
pub fn valid_host(name: &str) -> bool {
    name.contains('.') && name.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
}

/// `{settings.<key>}`: a host the user chose (a school's Canvas address), read from the module's
/// settings.json when it's needed.
pub fn settings_key(pattern: &str) -> Option<&str> {
    let key = pattern.strip_prefix("{settings.")?.strip_suffix('}')?;
    (!key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).then_some(key)
}

/// Host patterns with `{settings.<key>}` filled in from `settings` (left out while unset or not a
/// plain host name).
pub fn resolve_hosts(patterns: &[String], settings: &serde_json::Value) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|p| match settings_key(p) {
            Some(k) => settings[k].as_str().map(|v| v.trim().to_ascii_lowercase()).filter(|v| valid_host(v)),
            None => Some(p.clone()),
        })
        .collect()
}

/// Whether `host` is allowed by a list of patterns (`*.x.com` also matches `x.com`).
pub fn host_allowed(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns.iter().any(|p| match p.strip_prefix("*.") {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == *p,
    })
}

/// `90s`, `15m`, `2h`.
pub fn duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_secs(n * 60)),
        "h" => Some(Duration::from_secs(n * 3600)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> &'static str {
        r##"{
          "id": "canvas", "name": "Canvas", "version": "1.0.0", "api": 1,
          "description": "Due dates, grades and announcements from your school's Canvas.",
          "icon": "ui/icon.svg",
          "backend": { "exe": "canvas-module.exe", "run": "scheduled", "every": "15m" },
          "ui": { "entry": "ui/canvas.js", "css": ["ui/canvas.css"] },
          "contributes": {
            "tabs": [{ "id": "canvas", "title": "Canvas", "icon": "cap", "order": 60 }],
            "sites": [{ "id": "canvas", "label": "Canvas", "url": "{settings.baseUrl}", "hosts": ["*.instructure.com"], "skin": "skins/canvas-skin.css" }],
            "cubes": [{ "id": "canvas", "title": "Canvas", "icon": "cap", "size": "S", "slot": "canvas", "site": "canvas" }],
            "calendar": [{ "source": "canvas", "name": "Canvas", "color": "#f5c26b" }],
            "setup": { "card": true }
          },
          "talksTo": ["Your school's Canvas"],
          "hosts": ["*.instructure.com"],
          "legacy": ["canvas.json"]
        }"##
    }

    #[test]
    fn a_real_manifest_parses() {
        let m = Manifest::parse(canvas().as_bytes()).unwrap();
        assert_eq!(m.backend.as_ref().unwrap().run, RunMode::Scheduled);
        assert_eq!(m.every(), Some(Duration::from_secs(900)));
        assert_eq!(m.idle(), Duration::from_secs(60));
        assert_eq!(m.talks_to, vec!["Your school's Canvas"]);
        assert_eq!(m.referenced_files(), vec!["canvas-module.exe", "skins/canvas-skin.css", "ui/canvas.css", "ui/canvas.js", "ui/icon.svg"]);
        // And it survives a round trip through serde.
        let again = Manifest::parse(&serde_json::to_vec(&m).unwrap()).unwrap();
        assert_eq!(again, m);
    }

    fn broken(edit: impl Fn(&mut serde_json::Value)) -> String {
        let mut v: serde_json::Value = serde_json::from_str(canvas()).unwrap();
        edit(&mut v);
        Manifest::parse(&serde_json::to_vec(&v).unwrap()).unwrap_err()
    }

    #[test]
    fn bad_manifests_are_refused() {
        assert!(broken(|v| v["id"] = "Canvas".into()).contains("valid module id"));
        assert!(broken(|v| v["version"] = "1.0".into()).contains("semver"));
        assert!(broken(|v| v["api"] = 9.into()).contains("API"));
        assert!(broken(|v| v["icon"] = "../evil.svg".into()).contains("icon"));
        assert!(broken(|v| v["backend"]["exe"] = "C:/x.exe".into()).contains("backend.exe"));
        assert!(broken(|v| v["backend"]["every"] = "10s".into()).contains("at least"));
        assert!(broken(|v| v["backend"]["run"] = "while-process".into()).contains("process names"));
        assert!(broken(|v| v["ui"]["entry"] = "ui/x.css".into()).contains(".js"));
        assert!(broken(|v| v["contributes"]["cubes"][0]["size"] = "XL".into()).contains("size"));
        assert!(broken(|v| v["contributes"]["cubes"][0]["site"] = "dim".into()).contains("doesn't declare"));
        assert!(broken(|v| v["contributes"]["sites"][0]["url"] = "http://x.com".into()).contains("https"));
        assert!(broken(|v| v["contributes"]["calendar"][0]["color"] = "gold".into()).contains("color"));
        assert!(broken(|v| v["contributes"]["hotkeys"] = serde_json::json!([{ "key": "A", "opens": "tab:canvas" }])).contains("hotkey"));
        assert!(broken(|v| v["contributes"]["hotkeys"] = serde_json::json!([{ "key": "C", "opens": "canvas" }])).contains("tab:<id>"));
        assert!(broken(|v| v["publishes"] = serde_json::json!(["Media State"])).contains("topic"));
        assert!(broken(|v| v["files"] = serde_json::json!({ "module.json": format!("sha256-{}", "A".repeat(44)) })).contains("module.json itself"));
        assert!(broken(|v| v["files"] = serde_json::json!({ "ui/canvas.js": "abc" })).contains("sha256"));
        assert!(broken(|v| { v.as_object_mut().unwrap().remove("talksTo"); }).contains("talksTo"));
    }

    #[test]
    fn paths() {
        for ok in ["module.json", "ui/canvas.js", "skins/a-b.css", "x/y/z.svg"] {
            assert!(safe_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/abs", "..", "a/../b", "a//b", "./a", "a\\b", "C:x", "a/b.", "a/ b/c "] {
            assert!(safe_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hosts() {
        let p = vec!["*.bungie.net".to_string(), "lrclib.net".to_string()];
        assert!(host_allowed(&p, "www.bungie.net"));
        assert!(host_allowed(&p, "bungie.net"));
        assert!(host_allowed(&p, "LRCLIB.net"));
        assert!(!host_allowed(&p, "evilbungie.net"));
        assert!(!host_allowed(&p, "api.lrclib.net"));
        assert!(valid_host_pattern("*.instructure.com") && !valid_host_pattern("*") && !valid_host_pattern("localhost") && !valid_host_pattern("Ex.com"));
        assert!(valid_host_pattern("{settings.domain}") && !valid_host_pattern("{settings.}") && !valid_host_pattern("{settings.a.b}"));
        let pats = vec!["*.instructure.com".to_string(), "{settings.domain}".to_string()];
        assert_eq!(resolve_hosts(&pats, &serde_json::json!({ "domain": "Canvas.School.edu" })), vec!["*.instructure.com", "canvas.school.edu"]);
        assert_eq!(resolve_hosts(&pats, &serde_json::json!({})), vec!["*.instructure.com"]);
        assert_eq!(resolve_hosts(&pats, &serde_json::json!({ "domain": "evil.com/x" })), vec!["*.instructure.com"]);
    }

    #[test]
    fn durations() {
        assert_eq!(duration("90s"), Some(Duration::from_secs(90)));
        assert_eq!(duration("15m"), Some(Duration::from_secs(900)));
        assert_eq!(duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(duration("15"), None);
        assert_eq!(duration("m"), None);
        assert_eq!(duration("1d"), None);
    }
}
