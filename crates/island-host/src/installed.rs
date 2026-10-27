//! `installed.json` (§4.3): which modules are installed, at which version, and whether each is on.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Installed {
    pub schema: u32,
    #[serde(default)]
    pub modules: BTreeMap<String, Entry>,
}

impl Default for Installed {
    fn default() -> Self {
        Installed { schema: 1, modules: BTreeMap::new() }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Entry {
    pub version: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Loaded from a folder in developer mode (unsigned).
    #[serde(default)]
    pub dev: bool,
    /// RFC 3339.
    #[serde(default)]
    pub installed: String,
    /// "catalog", or the folder a developer-mode module was loaded from.
    #[serde(default)]
    pub source: String,
}

fn yes() -> bool {
    true
}

impl Installed {
    /// A missing or unreadable file is an empty set (a broken one is kept aside, not overwritten).
    pub fn load(path: &Path) -> Installed {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                let _ = std::fs::rename(path, path.with_extension("json.broken"));
                Installed::default()
            }),
            Err(_) => Installed::default(),
        }
    }

    /// Written to a temporary file first, so a crash mid-write can't leave half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_damage() {
        let dir = std::env::temp_dir().join(format!("island-installed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("installed.json");
        assert_eq!(Installed::load(&path), Installed::default());
        let mut set = Installed::default();
        set.modules.insert("destiny".into(), Entry { version: "1.0.0".into(), enabled: true, auto_update: true, dev: false, installed: "2026-11-02T18:00:00Z".into(), source: "catalog".into() });
        set.save(&path).unwrap();
        assert_eq!(Installed::load(&path), set);
        let parsed: Installed = serde_json::from_str(r#"{ "schema": 1, "modules": { "music": { "version": "1.0.0" } } }"#).unwrap();
        assert!(parsed.modules["music"].enabled && parsed.modules["music"].auto_update);
        std::fs::write(&path, b"{ nope").unwrap();
        assert_eq!(Installed::load(&path), Installed::default());
        assert!(path.with_extension("json.broken").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
