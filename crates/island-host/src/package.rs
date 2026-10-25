//! Module packages: the zip a release carries for each module (handoff §4.4). tools/pack-modules
//! writes them and the base installs them; both sides go through here, so they can't disagree.
//!
//! A package holds `module.json` and exactly the files its `files` list names, each matching its
//! sha256. Anything else (an extra file, a missing one, a path that climbs out, a wrong hash) and
//! the package is refused whole.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use island_module_proto::manifest::{safe_path, Manifest};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::ui::sri;

/// The most files a package may hold, and the most it may unpack to.
pub const MAX_FILES: usize = 512;
pub const MAX_UNPACKED: u64 = 256 << 20;

/// A checked package.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    /// `module.json` exactly as packed.
    pub json: Vec<u8>,
    /// Every other file, by its path in the module.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Writes a package: `module.json` first, then the files in path order, with a fixed date and
/// permissions, so the same files always make the same bytes.
pub fn write(json: &[u8], files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).last_modified_time(DateTime::default()).unix_permissions(0o644);
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let e = |e: zip::result::ZipError| e.to_string();
    z.start_file("module.json", opts).map_err(e)?;
    z.write_all(json).map_err(|e| e.to_string())?;
    for (path, bytes) in files {
        z.start_file(path.as_str(), opts).map_err(e)?;
        z.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(z.finish().map_err(e)?.into_inner())
}

/// Reads and checks a package (see the module notes).
pub fn read(zip: &[u8]) -> Result<Package, String> {
    let mut z = ZipArchive::new(Cursor::new(zip)).map_err(|e| format!("it isn't a zip: {e}"))?;
    if z.len() > MAX_FILES {
        return Err(format!("it holds more than {MAX_FILES} files"));
    }
    let mut json = None;
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        if name != "module.json" {
            safe_path(&name).map_err(|e| format!("{name}: {e}"))?;
        }
        total += f.size();
        if total > MAX_UNPACKED {
            return Err("it unpacks to more than 256 MB".into());
        }
        let mut bytes = Vec::with_capacity(f.size() as usize);
        f.by_ref().take(MAX_UNPACKED).read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
        if name == "module.json" {
            json = Some(bytes);
        } else if files.insert(name.clone(), bytes).is_some() {
            return Err(format!("{name} is in it twice"));
        }
    }
    let json = json.ok_or("it has no module.json")?;
    let manifest: Manifest = serde_json::from_slice(&json).map_err(|e| format!("module.json: {e}"))?;
    manifest.validate().map_err(|e| format!("module.json: {e}"))?;
    for (path, want) in &manifest.files {
        let got = files.get(path).ok_or_else(|| format!("{path} is in its manifest but not in the package"))?;
        if sri(got) != *want {
            return Err(format!("{path} doesn't match its hash"));
        }
    }
    if let Some(extra) = files.keys().find(|p| !manifest.files.contains_key(*p)) {
        return Err(format!("{extra} isn't in its manifest"));
    }
    let needed = manifest.referenced_files().into_iter().chain(manifest.backend.as_ref().map(|b| b.exe.clone()));
    for path in needed {
        if !files.contains_key(&path) {
            return Err(format!("{path} is missing"));
        }
    }
    Ok(Package { manifest, json, files })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small valid package: (module.json, files).
    pub(crate) fn sample(id: &str, version: &str) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
        let mut files = BTreeMap::new();
        files.insert("ui/icon.svg".to_string(), b"<svg/>".to_vec());
        files.insert(format!("ui/{id}.js"), b"export function activate() {}".to_vec());
        files.insert(format!("{id}-module.exe"), b"MZ".to_vec());
        let hashes: serde_json::Map<String, serde_json::Value> = files.iter().map(|(p, b)| (p.clone(), sri(b).into())).collect();
        let json = serde_json::json!({
            "id": id, "name": "Sample", "version": version, "api": 1, "description": "A sample.", "icon": "ui/icon.svg",
            "backend": { "exe": format!("{id}-module.exe"), "run": "on-demand" },
            "ui": { "entry": format!("ui/{id}.js") },
            "talksTo": [], "files": hashes
        });
        (serde_json::to_vec_pretty(&json).unwrap(), files)
    }

    #[test]
    fn round_trip_and_same_bytes() {
        let (json, files) = sample("sample", "1.0.0");
        let a = write(&json, &files).unwrap();
        assert_eq!(a, write(&json, &files).unwrap(), "the same files make the same zip");
        let p = read(&a).unwrap();
        assert_eq!((p.manifest.id.as_str(), p.files.len(), p.json == json), ("sample", 3, true));
    }

    #[test]
    fn refuses_bad_packages() {
        let (json, files) = sample("sample", "1.0.0");
        // An extra file.
        let mut extra = files.clone();
        extra.insert("ui/extra.js".into(), b"alert(1)".to_vec());
        assert!(read(&write(&json, &extra).unwrap()).unwrap_err().contains("isn't in its manifest"));
        // A changed file.
        let mut changed = files.clone();
        changed.insert("ui/sample.js".into(), b"evil()".to_vec());
        assert!(read(&write(&json, &changed).unwrap()).unwrap_err().contains("hash"));
        // A missing one.
        let mut missing = files.clone();
        missing.remove("sample-module.exe");
        assert!(read(&write(&json, &missing).unwrap()).unwrap_err().contains("not in the package"));
        // A path that climbs out.
        let mut out = files.clone();
        out.insert("../evil.exe".into(), b"MZ".to_vec());
        assert!(read(&write(&json, &out).unwrap()).is_err());
        // Not a zip, or no manifest.
        assert!(read(b"nope").is_err());
        assert!(read(&write(b"{}", &BTreeMap::new()).unwrap()).unwrap_err().contains("module.json"));
    }
}
