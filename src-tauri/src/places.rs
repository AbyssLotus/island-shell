//! Places for the Command Center to jump to besides apps and windows: drives, the user's own
//! folders (Desktop, Documents, Downloads…) and what Windows lists as recently opened (its Recent
//! folder of shortcuts, resolved to the real files and folders). Plus opening them, showing them in
//! File Explorer, and a short allowlist of Windows tools.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;
use windows::core::{GUID, HSTRING};
use windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::UI::Shell::{
    IShellItem2, SHCreateItemFromParsingName, SHGetKnownFolderPath, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
    FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_Recent, FOLDERID_Videos, KF_FLAG_DEFAULT,
};

use crate::util;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

#[derive(Serialize, Clone, Default)]
pub struct Place {
    pub name: String,
    pub path: String,
    /// "drive", "folder" or "file".
    pub kind: String,
    /// Drive label, or which known folder this is.
    pub sub: String,
    pub free_gb: Option<f64>,
    pub total_gb: Option<f64>,
    /// When it was last opened (recent items), Unix ms.
    pub when: u64,
}

pub(crate) fn known(id: &GUID) -> Option<String> {
    unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok().map(util::take_pwstr).filter(|p| !p.is_empty())
}

fn drives() -> Vec<Place> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = vec![];
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let kind = unsafe { GetDriveTypeW(&HSTRING::from(root.as_str())) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let (mut free, mut total) = (0u64, 0u64);
        let sized = unsafe { GetDiskFreeSpaceExW(&HSTRING::from(root.as_str()), None, Some(&mut total), Some(&mut free)) }.is_ok() && total > 0;
        let mut label = [0u16; 64];
        let _ = unsafe { GetVolumeInformationW(&HSTRING::from(root.as_str()), Some(&mut label), None, None, None, None) };
        let label = String::from_utf16_lossy(&label[..label.iter().position(|&c| c == 0).unwrap_or(0)]);
        out.push(Place {
            name: format!("{}:", (b'A' + i as u8) as char),
            path: root,
            kind: "drive".into(),
            sub: if label.is_empty() { if kind == DRIVE_REMOVABLE { "Removable drive".into() } else { "Local disk".into() } } else { label },
            free_gb: sized.then(|| free as f64 / 1_073_741_824.0),
            total_gb: sized.then(|| total as f64 / 1_073_741_824.0),
            when: 0,
        });
    }
    out
}

fn folders() -> Vec<Place> {
    [(&FOLDERID_Desktop, "Desktop"), (&FOLDERID_Downloads, "Downloads"), (&FOLDERID_Documents, "Documents"), (&FOLDERID_Pictures, "Pictures"), (&FOLDERID_Videos, "Videos"), (&FOLDERID_Music, "Music"), (&FOLDERID_Profile, "Home")]
        .iter()
        .filter_map(|(id, name)| {
            let path = known(id)?;
            Some(Place { name: name.to_string(), sub: path.clone(), path, kind: "folder".into(), ..Default::default() })
        })
        .collect()
}

/// The target a shortcut points to.
pub(crate) fn link_target(lnk: &Path) -> Option<String> {
    let item: IShellItem2 = unsafe { SHCreateItemFromParsingName(&HSTRING::from(lnk.as_os_str()), None) }.ok()?;
    let t = unsafe { item.GetString(&PKEY_Link_TargetParsingPath) }.ok().map(util::take_pwstr)?;
    (!t.is_empty()).then_some(t)
}

/// Recently opened files and folders, newest first (at most 40 files and 20 folders).
fn recent() -> Vec<Place> {
    let Some(dir) = known(&FOLDERID_Recent) else { return vec![] };
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut links: Vec<(u64, std::path::PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| {
            let t = e.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
            Some((t, e.path()))
        })
        .collect();
    links.sort_by(|a, b| b.0.cmp(&a.0));
    let (mut files, mut dirs, mut seen) = (vec![], vec![], std::collections::HashSet::new());
    for (when, lnk) in links.into_iter().take(160) {
        if files.len() >= 40 && dirs.len() >= 20 {
            break;
        }
        let Some(target) = link_target(&lnk) else { continue };
        if !seen.insert(target.to_lowercase()) {
            continue;
        }
        let p = Path::new(&target);
        let Ok(meta) = std::fs::metadata(p) else { continue }; // gone, or a network place that's offline
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| target.clone());
        let parent = p.parent().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        let place = Place { name, path: target.clone(), sub: parent, kind: if meta.is_dir() { "folder" } else { "file" }.into(), when, ..Default::default() };
        if meta.is_dir() {
            if dirs.len() < 20 {
                dirs.push(place);
            }
        } else if files.len() < 40 {
            files.push(place);
        }
    }
    dirs.extend(files);
    dirs
}

/// Everything at once: drives, known folders, then recent folders and files.
pub fn list() -> Vec<Place> {
    let mut out = drives();
    out.extend(folders());
    out.extend(recent());
    out
}

/// A local path the Command Center offered (it has to exist; no URLs or commands).
fn checked(path: &str) -> Result<&Path, String> {
    let p = Path::new(path);
    let local = path.len() > 2 && path.as_bytes()[1] == b':' || path.starts_with("\\\\");
    if !local || !p.exists() {
        return Err(format!("{path} isn't there any more"));
    }
    Ok(p)
}

/// Opens a file or folder the way double-clicking it in File Explorer would.
pub fn open(path: &str) -> Result<(), String> {
    checked(path)?;
    util::shell_open(path)
}

/// Shows a file or folder selected in File Explorer.
pub fn reveal(path: &str) -> Result<(), String> {
    checked(path)?;
    std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Windows tools the Command Center can start by name.
pub fn tool(name: &str) -> Result<(), String> {
    let target = match name {
        "taskmgr" => "taskmgr.exe",
        "terminal" => "wt.exe",
        "explorer" => "explorer.exe",
        "recycle" => "shell:RecycleBinFolder",
        "control" => "control.exe",
        "snip" => "ms-screenclip:",
        _ => return Err(format!("unknown tool {name}")),
    };
    util::shell_open(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_local_paths() {
        assert!(checked("C:\\Windows").is_ok());
        assert!(checked("https://example.com").is_err());
        assert!(checked("calc.exe").is_err());
        assert!(checked("C:\\definitely\\not\\here\\x.txt").is_err());
    }

    /// Prints what this PC has (drives, folders, recent items).
    #[test]
    #[ignore = "needs a real PC: prints its drives, folders and recent items"]
    fn live_places() {
        crate::util::com_mta();
        for p in list() {
            println!("{:6} {:30} {}", p.kind, p.name, p.path);
        }
    }
}
