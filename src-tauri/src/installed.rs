//! Installed apps for the Task Manager's Installed Apps page: everything Settings > Apps lists.
//!
//! - Programs come from the Uninstall keys: the machine's (64- and 32-bit) and yours. Updates,
//!   system components and entries with no name are left out, as Settings does.
//! - Store apps come from the package manager (your main packages, not frameworks or Windows' own).
//! - Sizes are what the installer recorded; when it didn't, the install folder is measured in the
//!   background and the result shows on the next refresh.
//! - Uninstall runs the program's own uninstaller (which asks for administrator rights itself when
//!   it needs them); a Store app opens its page in Settings, where its Uninstall button is.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::sysinfo::{reg_dword, reg_str};
use crate::{running, util};

#[derive(Serialize, Clone, Default)]
pub struct App {
    /// "m64|{key}", "m32|{key}", "u|{key}" or "pkg|{family name}".
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// Unix seconds (from the installer's date, else when its entry last changed).
    pub installed: u64,
    /// Bytes; 0 when unknown (yet).
    pub size: u64,
    /// "Machine · 64-bit", "Machine · 32-bit", "Current user", "Microsoft Store".
    pub scope: &'static str,
    pub location: String,
    pub icon: Option<String>,
    pub can_uninstall: bool,
    pub can_modify: bool,
    pub help: String,
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Subkey names of a key, with when each last changed.
fn subkeys(root: HKEY, path: &str) -> Vec<(String, FILETIME)> {
    let mut k = HKEY::default();
    if unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ | KEY_WOW64_64KEY, &mut k) } != ERROR_SUCCESS {
        return vec![];
    }
    let k = Key(k);
    let mut count = 0u32;
    unsafe {
        let _ = RegQueryInfoKeyW(k.0, None, None, None, Some(&mut count), None, None, None, None, None, None, None);
    }
    let mut out = vec![];
    for i in 0..count {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let mut ft = FILETIME::default();
        if unsafe { RegEnumKeyExW(k.0, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, Some(&mut ft)) } == ERROR_SUCCESS {
            out.push((String::from_utf16_lossy(&name[..len as usize]), ft));
        }
    }
    out
}

fn filetime_secs(ft: FILETIME) -> u64 {
    let t = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
    (t / 10_000_000).saturating_sub(11_644_473_600)
}

/// "20250311" → Unix seconds at noon that day.
fn ymd_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (s[..4].parse().ok()?, s[4..6].parse().ok()?, s[6..].parse().ok()?);
    if !(1990..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + 43_200) as u64)
}

/// Folder sizes measured in the background, by path.
static MEASURED: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static MEASURING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn folder_size(dir: &Path, budget: &mut u32) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if *budget == 0 {
                return total;
            }
            *budget -= 1;
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn measure(paths: Vec<String>) {
    if paths.is_empty() || MEASURING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        for p in paths {
            // At most 200 000 files per folder, so one huge game can't hold up the rest.
            let mut budget = 200_000;
            let size = folder_size(Path::new(&p), &mut budget);
            MEASURED.lock().unwrap().get_or_insert_with(HashMap::new).insert(p, size);
        }
        MEASURING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

fn icon_of(display_icon: &str) -> Option<String> {
    let p = display_icon.trim().trim_matches('"');
    let p = match p.rsplit_once(',') {
        Some((a, b)) if b.trim().trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) => a.trim().trim_matches('"'),
        _ => p,
    };
    let l = p.to_ascii_lowercase();
    if (l.ends_with(".exe") || l.ends_with(".ico") || l.ends_with(".dll")) && Path::new(p).is_file() {
        running::exe_icon(&l)
    } else {
        None
    }
}

pub fn list() -> Vec<App> {
    let mut out = vec![];
    let mut seen = HashSet::new();
    let mut to_measure = vec![];
    let measured = MEASURED.lock().unwrap().clone().unwrap_or_default();
    for (root, path, scope, tag) in [
        (HKEY_LOCAL_MACHINE, UNINSTALL, "Machine · 64-bit", "m64"),
        (HKEY_LOCAL_MACHINE, UNINSTALL32, "Machine · 32-bit", "m32"),
        (HKEY_CURRENT_USER, UNINSTALL, "Current user", "u"),
    ] {
        for (key, ft) in subkeys(root, path) {
            let p = format!(r"{path}\{key}");
            let s = |n: &str| reg_str(root, &p, n).unwrap_or_default();
            let name = s("DisplayName");
            if name.is_empty()
                || reg_dword(root, &p, "SystemComponent") == Some(1)
                || !s("ParentKeyName").is_empty()
                || ["Update", "Hotfix", "Security Update"].contains(&s("ReleaseType").as_str())
            {
                continue;
            }
            let version = s("DisplayVersion");
            if !seen.insert((name.to_lowercase(), version.clone())) {
                continue; // the same program listed in two places
            }
            let location = s("InstallLocation").trim().trim_matches('"').to_string();
            let mut size = reg_dword(root, &p, "EstimatedSize").unwrap_or(0) as u64 * 1024;
            if size == 0 && !location.is_empty() && Path::new(&location).is_dir() {
                match measured.get(&location) {
                    Some(&m) => size = m,
                    None => to_measure.push(location.clone()),
                }
            }
            let uninstall = s("UninstallString");
            out.push(App {
                id: format!("{tag}|{key}"),
                publisher: s("Publisher"),
                installed: ymd_secs(&s("InstallDate")).unwrap_or_else(|| filetime_secs(ft)),
                size,
                scope,
                icon: icon_of(&s("DisplayIcon")),
                can_uninstall: !uninstall.is_empty() && reg_dword(root, &p, "NoRemove") != Some(1),
                can_modify: !s("ModifyPath").is_empty() && reg_dword(root, &p, "NoModify") != Some(1),
                help: [s("URLInfoAbout"), s("HelpLink")].into_iter().find(|u| u.starts_with("http")).unwrap_or_default(),
                location,
                version,
                name,
            });
        }
    }
    out.extend(store_apps(&measured, &mut to_measure));
    measure(to_measure);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn store_apps(measured: &HashMap<String, u64>, to_measure: &mut Vec<String>) -> Vec<App> {
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    let mut out = vec![];
    let Ok(pm) = PackageManager::new() else { return out };
    let Ok(pkgs) = pm.FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), PackageTypes::Main) else { return out };
    let Ok(it) = pkgs.First() else { return out };
    while it.HasCurrent().unwrap_or(false) {
        if let Ok(p) = it.Current() {
            let keep = !p.IsFramework().unwrap_or(true)
                && !p.IsResourcePackage().unwrap_or(true)
                && !p.IsStub().unwrap_or(false)
                && p.SignatureKind().map(|k| k != PackageSignatureKind::System).unwrap_or(false);
            let name = p.DisplayName().map(|s| s.to_string()).unwrap_or_default();
            if keep && !name.is_empty() && !name.starts_with("ms-resource:") {
                let id = p.Id().ok();
                let version = id.as_ref().and_then(|i| i.Version().ok()).map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)).unwrap_or_default();
                let family = id.as_ref().and_then(|i| i.FamilyName().ok()).map(|s| s.to_string()).unwrap_or_default();
                let location = p.InstalledPath().map(|s| s.to_string()).unwrap_or_default();
                // DateTime: 100 ns ticks since 1601.
                let installed = p.InstalledDate().map(|d| ((d.UniversalTime / 10_000_000) as u64).saturating_sub(11_644_473_600)).unwrap_or(0);
                let size = match measured.get(&location) {
                    Some(&m) => m,
                    None => {
                        if !location.is_empty() {
                            to_measure.push(location.clone());
                        }
                        0
                    }
                };
                out.push(App {
                    id: format!("pkg|{family}"),
                    name,
                    publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
                    version,
                    installed,
                    size,
                    scope: "Microsoft Store",
                    location,
                    icon: None,
                    can_uninstall: true,
                    can_modify: false,
                    help: String::new(),
                });
            }
        }
        if !it.MoveNext().unwrap_or(false) {
            break;
        }
    }
    out
}

/// Splits "\"C:\\x\\un.exe\" /S" or "MsiExec.exe /X{…}" into (program, arguments).
fn split_command(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    let l = c.to_ascii_lowercase();
    if let Some(i) = l.find(".exe") {
        return (c[..i + 4].to_string(), c[i + 4..].trim().to_string());
    }
    match c.split_once(' ') {
        Some((a, b)) => (a.to_string(), b.trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// uninstall | modify | location | help
pub fn action(id: &str, what: &str) -> Result<(), String> {
    let (tag, key) = id.split_once('|').ok_or("Unknown app")?;
    if key.is_empty() || key.contains(['"', '\n', '\r', '\0']) || key.contains(r"..\") {
        return Err("Unknown app".into());
    }
    if tag == "pkg" {
        return match what {
            "uninstall" => util::shell_open(&format!("ms-settings:appsfeatures-app?{key}")),
            "location" => {
                let a = list().into_iter().find(|a| a.id == id).ok_or("That app isn't installed anymore")?;
                crate::places::open(&a.location)
            }
            _ => Err("Not for Store apps".into()),
        };
    }
    let (root, path) = match tag {
        "m64" => (HKEY_LOCAL_MACHINE, UNINSTALL),
        "m32" => (HKEY_LOCAL_MACHINE, UNINSTALL32),
        "u" => (HKEY_CURRENT_USER, UNINSTALL),
        _ => return Err("Unknown app".into()),
    };
    let p = format!(r"{path}\{key}");
    let s = |n: &str| reg_str(root, &p, n).unwrap_or_default();
    match what {
        "location" => {
            let l = s("InstallLocation").trim().trim_matches('"').to_string();
            if l.is_empty() {
                return Err("The installer didn't say where it put this one".into());
            }
            crate::places::open(&l)
        }
        "help" => {
            let u = [s("URLInfoAbout"), s("HelpLink")].into_iter().find(|u| u.starts_with("http")).ok_or("No web page for this one")?;
            util::open_web(&u)
        }
        "uninstall" | "modify" => {
            let cmd = if what == "modify" { s("ModifyPath") } else { s("UninstallString") };
            if cmd.is_empty() {
                return Err("This one can't be changed from here".into());
            }
            let (file, args) = split_command(&util_expand(&cmd));
            let (f, a, v) = (HSTRING::from(file.as_str()), HSTRING::from(args.as_str()), HSTRING::from("open"));
            let mut sei = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_NOASYNC,
                lpVerb: PCWSTR(v.as_ptr()),
                lpFile: PCWSTR(f.as_ptr()),
                lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(a.as_ptr()) },
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            match unsafe { ShellExecuteExW(&mut sei) } {
                Ok(()) => {
                    util::log_line(&format!("Task Manager: {what} {key}"));
                    Ok(())
                }
                Err(e) if e.code() == windows::Win32::Foundation::ERROR_CANCELLED.to_hresult() => Ok(()),
                Err(e) => Err(format!("Couldn't start its uninstaller: {}", e.message())),
            }
        }
        _ => Err(format!("Unknown action {what}")),
    }
}

/// %VARS% from this process's environment.
fn util_expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => {
                match std::env::var(&after[..b]) {
                    Ok(v) if b > 0 => out.push_str(&v),
                    _ => out.push_str(&rest[a..a + b + 2]),
                }
                rest = &after[b + 1..];
            }
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_dates() {
        assert_eq!(split_command("\"C:\\Program Files\\X\\unins000.exe\" /SILENT"), ("C:\\Program Files\\X\\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(split_command("MsiExec.exe /X{1234}"), ("MsiExec.exe".into(), "/X{1234}".into()));
        assert_eq!(split_command("C:\\Program Files\\Y\\uninstall.exe"), ("C:\\Program Files\\Y\\uninstall.exe".into(), String::new()));
        assert_eq!(ymd_secs("19700101"), None);
        assert_eq!(ymd_secs("20000101"), Some(946_684_800 + 43_200));
        assert_eq!(ymd_secs("2024-1-1"), None);
    }

    #[test]
    #[ignore = "needs a real PC: prints its installed apps and sizes"]
    fn live() {
        let t = std::time::Instant::now();
        let l = list();
        println!("{} apps in {:?}", l.len(), t.elapsed());
        for a in l.iter().take(40) {
            println!("{:<44} {:<26} {:<16} {:>10} {:>7} MB  {:<18} icon={}", a.name.chars().take(44).collect::<String>(), a.publisher.chars().take(26).collect::<String>(), a.version, a.installed, a.size >> 20, a.scope, a.icon.is_some());
        }
        println!("store: {}", l.iter().filter(|a| a.scope == "Microsoft Store").count());
        std::thread::sleep(std::time::Duration::from_secs(8));
        let l2 = list();
        println!("unknown sizes before {} after {}", l.iter().filter(|a| a.size == 0).count(), l2.iter().filter(|a| a.size == 0).count());
    }
}
