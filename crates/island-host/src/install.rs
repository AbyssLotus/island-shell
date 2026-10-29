//! Putting a checked package in place (handoff §4.1, M3.2): unpacked into `<root>/.staging/<id>`,
//! then swapped in, with the old version kept in `<root>/.trash/<id>` until the new one is there.
//! If the swap is cut short (a crash, a power cut), `recover` at the next start finishes it or puts
//! the old version back, so a module is never left half installed.
//!
//! The module must not be running while it's swapped (its exe would be locked); the caller stops it.
//! An app the module opened may still have its folder open as its working folder (base 0.3.2 and
//! earlier started modules in their code folder, and Discord opened from the Discord module kept
//! it). The folder can't move then, but what's in it can: the contents are swapped instead.

use std::ffi::OsString;
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
        if let Err(e) = std::fs::rename(&dest, &trash) {
            if !held(&e) {
                return Err(io(&format!("move the old {id} aside"), e));
            }
            swap_contents(&dest, &staging, &trash).map_err(|e| io(&format!("swap in the new {id} (is it still running?)"), e))?;
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_dir_all(&trash);
            return Ok(dest);
        }
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
/// place goes back, and so does one whose place has no module.json (a contents swap cut short);
/// otherwise the trash and any staging folders are cleared.
pub fn recover(root: &Path) -> Vec<String> {
    let mut restored = vec![];
    if let Ok(rd) = std::fs::read_dir(root.join(".trash")) {
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let dest = root.join(&id);
            if !dest.exists() && std::fs::rename(e.path(), &dest).is_ok() {
                restored.push(id);
                continue;
            }
            if dest.is_dir() && !dest.join(MANIFEST).exists() && e.path().join(MANIFEST).exists() {
                restore(&dest, &e.path());
                restored.push(id);
            }
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    let _ = std::fs::remove_dir_all(root.join(".staging"));
    restored
}

const MANIFEST: &str = "module.json";

/// A sharing violation or access denied: something has the folder itself open.
fn held(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5 | 32)) || e.kind() == std::io::ErrorKind::PermissionDenied
}

/// What's in a folder, module.json first.
fn entries(dir: &Path) -> std::io::Result<Vec<OsString>> {
    let mut names = std::fs::read_dir(dir)?.map(|e| e.map(|e| e.file_name())).collect::<std::io::Result<Vec<_>>>()?;
    names.sort_by_key(|n| n != MANIFEST);
    Ok(names)
}

/// Replaces what's in `dest` with what's in `staging`, the old contents going to `trash`. module.json
/// leaves first and comes in last, so `dest` has one only while it holds a whole version (that's how
/// `recover` tells a swap was cut short). A file that won't move puts everything back.
fn swap_contents(dest: &Path, staging: &Path, trash: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(trash)?;
    for name in entries(dest)? {
        if let Err(e) = std::fs::rename(dest.join(&name), trash.join(&name)) {
            restore(dest, trash);
            return Err(e);
        }
    }
    for name in entries(staging)?.iter().rev() {
        if let Err(e) = std::fs::rename(staging.join(name), dest.join(name)) {
            restore(dest, trash);
            return Err(e);
        }
    }
    Ok(())
}

/// Empties `dest` and moves the old version back in from `trash`, module.json last.
fn restore(dest: &Path, trash: &Path) {
    for name in entries(dest).unwrap_or_default() {
        let p = dest.join(name);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    for name in entries(trash).unwrap_or_default().iter().rev() {
        let _ = std::fs::rename(trash.join(name), dest.join(name));
    }
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

    /// What an app started in the folder holds: the folder open, without letting it be deleted or moved.
    #[cfg(windows)]
    fn hold(dir: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE (no FILE_SHARE_DELETE), FILE_FLAG_BACKUP_SEMANTICS for a folder.
        std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).custom_flags(0x0200_0000).open(dir).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn an_update_goes_in_while_an_app_holds_the_folder() {
        let r = root("held");
        let dir = install(&r, &pkg("1.0.0")).unwrap();
        let held = hold(&dir);
        assert!(std::fs::rename(&dir, r.join("moved")).is_err(), "the folder should be held");
        assert_eq!(install(&r, &pkg("1.1.0")).unwrap(), dir);
        assert_eq!(version_at(&dir), "1.1.0");
        assert!(dir.join("ui/sample.js").is_file() && dir.join("sample-module.exe").is_file());
        assert!(!r.join(".trash/sample").exists() && !r.join(".staging/sample").exists());
        drop(held);
        // An empty folder left behind by a removal that couldn't delete it takes a fresh install too.
        remove(&r, "sample").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let held = hold(&dir);
        install(&r, &pkg("1.2.0")).unwrap();
        assert_eq!(version_at(&dir), "1.2.0");
        drop(held);
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn a_contents_swap_cut_short_rolls_back() {
        let r = root("contents");
        let dir = install(&r, &pkg("1.0.0")).unwrap();
        // The old contents moved aside and part of the new ones in, then the power went out.
        let (trash, staging) = (r.join(".trash/sample"), r.join(".staging/sample"));
        std::fs::create_dir_all(&trash).unwrap();
        for name in entries(&dir).unwrap() {
            std::fs::rename(dir.join(&name), trash.join(&name)).unwrap();
        }
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/sample.js"), "part of 1.1.0").unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        assert_eq!(recover(&r), ["sample"]);
        assert_eq!(version_at(&dir), "1.0.0");
        assert_ne!(std::fs::read_to_string(dir.join("ui/sample.js")).unwrap(), "part of 1.1.0");
        assert!(!trash.exists() && !r.join(".staging").exists());
        // A swap that did finish (module.json in place) only leaves the trash to clear.
        std::fs::create_dir_all(&trash).unwrap();
        std::fs::write(trash.join(MANIFEST), "{}").unwrap();
        assert!(recover(&r).is_empty());
        assert_eq!(version_at(&dir), "1.0.0");
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
