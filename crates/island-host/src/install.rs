//! Putting a checked package in place (handoff §4.1, M3.2): unpacked into `<root>/.staging/<id>`,
//! then swapped in, with the old version kept in `<root>/.trash/<id>` until the new one is there.
//! If the swap is cut short (a crash, a power cut), `recover` at the next start finishes it or puts
//! the old version back, so a module is never left half installed.
//!
//! The module must not be running while it's swapped (its exe would be locked); the caller stops it.

use std::path::{Path, PathBuf};

use crate::package::Package;

/// Module API versions this base runs: the current one and the one before.
pub fn supported_api() -> Vec<u32> {
    let n = island_module_proto::API;
    if n > 1 {
        vec![n, n - 1]
    } else {
        vec![n]
    }
}

/// Installs `pkg` as `<root>/<id>`; returns that folder.
pub fn install(root: &Path, pkg: &Package) -> Result<PathBuf, String> {
    let id = &pkg.manifest.id;
    if !supported_api().contains(&pkg.manifest.api) {
        return Err(format!("{} needs module API {}, and this version of Island Shell runs {:?}: update Island Shell first", pkg.manifest.name, pkg.manifest.api, supported_api()));
    }
    let staging = root.join(".staging").join(id);
    let trash = root.join(".trash").join(id);
    let dest = root.join(id);
    let io = |what: &str, e: std::io::Error| format!("Couldn't {what}: {e}");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| io("make the staging folder", e))?;
    std::fs::write(staging.join("module.json"), &pkg.json).map_err(|e| io("write module.json", e))?;
    for (path, bytes) in &pkg.files {
        let file = staging.join(path);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io("unpack the module", e))?;
        }
        std::fs::write(&file, bytes).map_err(|e| io(&format!("write {path}"), e))?;
    }
    let _ = std::fs::remove_dir_all(&trash);
    if dest.exists() {
        std::fs::create_dir_all(root.join(".trash")).map_err(|e| io("make the trash folder", e))?;
        std::fs::rename(&dest, &trash).map_err(|e| io(&format!("move the old {id} aside (is it still running?)"), e))?;
    }
    if let Err(e) = std::fs::rename(&staging, &dest) {
        if trash.exists() {
            let _ = std::fs::rename(&trash, &dest);
        }
        return Err(io(&format!("put the new {id} in place"), e));
    }
    let _ = std::fs::remove_dir_all(&trash);
    Ok(dest)
}

/// Finishes or undoes swaps a crash cut short: an old version in the trash with nothing in its
/// place goes back; otherwise the trash and any staging folders are cleared.
pub fn recover(root: &Path) -> Vec<String> {
    let mut restored = vec![];
    if let Ok(rd) = std::fs::read_dir(root.join(".trash")) {
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let dest = root.join(&id);
            if !dest.exists() && std::fs::rename(e.path(), &dest).is_ok() {
                restored.push(id);
            } else {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let _ = std::fs::remove_dir_all(root.join(".staging"));
    restored
}

/// Removes a module's code (its data and keys stay unless the caller forgets them too).
pub fn remove(root: &Path, id: &str) -> Result<(), String> {
    if !island_module_proto::valid_id(id) {
        return Err("That isn't a module".into());
    }
    match std::fs::remove_dir_all(root.join(id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't remove {id} (is it still running?): {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{self, tests::sample};

    fn root(name: &str) -> PathBuf {
        let r = std::env::temp_dir().join(format!("island-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        std::fs::create_dir_all(&r).unwrap();
        r
    }

    fn pkg(version: &str) -> Package {
        let (json, files) = sample("sample", version);
        package::read(&package::write(&json, &files).unwrap()).unwrap()
    }

    fn version_at(dir: &Path) -> String {
        let m: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("module.json")).unwrap()).unwrap();
        m["version"].as_str().unwrap().to_string()
    }

    #[test]
    fn install_then_update() {
        let r = root("update");
        let dir = install(&r, &pkg("1.0.0")).unwrap();
        assert_eq!(version_at(&dir), "1.0.0");
        assert!(dir.join("ui/sample.js").is_file() && dir.join("sample-module.exe").is_file());
        install(&r, &pkg("1.1.0")).unwrap();
        assert_eq!(version_at(&dir), "1.1.0");
        assert!(!r.join(".trash/sample").exists() && !r.join(".staging/sample").exists());
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn an_interrupted_swap_rolls_back() {
        let r = root("interrupted");
        install(&r, &pkg("1.0.0")).unwrap();
        // The old version moved aside, then the power went out before the new one was in place.
        std::fs::create_dir_all(r.join(".trash")).unwrap();
        std::fs::rename(r.join("sample"), r.join(".trash/sample")).unwrap();
        std::fs::create_dir_all(r.join(".staging/sample")).unwrap();
        assert_eq!(recover(&r), ["sample"]);
        assert_eq!(version_at(&r.join("sample")), "1.0.0");
        assert!(!r.join(".staging").exists() && !r.join(".trash/sample").exists());
        // A swap that did finish only leaves the trash to clear.
        install(&r, &pkg("1.1.0")).unwrap();
        std::fs::create_dir_all(r.join(".trash/sample")).unwrap();
        assert!(recover(&r).is_empty());
        assert_eq!(version_at(&r.join("sample")), "1.1.0");
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn api_mismatch_and_remove() {
        let r = root("api");
        let mut p = pkg("2.0.0");
        p.manifest.api = island_module_proto::API + 1;
        assert!(install(&r, &p).unwrap_err().contains("update Island Shell first"));
        assert!(!r.join("sample").exists());
        install(&r, &pkg("1.0.0")).unwrap();
        remove(&r, "sample").unwrap();
        assert!(!r.join("sample").exists());
        remove(&r, "sample").unwrap();
        assert!(remove(&r, "../x").is_err());
        let _ = std::fs::remove_dir_all(r);
    }
}
