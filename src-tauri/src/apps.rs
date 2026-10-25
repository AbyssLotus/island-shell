//! Installed apps from the shell's AppsFolder (the same list as Start > All apps), with icons
//! cached as PNG files and launched through `shell:AppsFolder`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::{Interface, HSTRING, PCWSTR, PSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Storage::EnhancedStorage::PKEY_Link_TargetParsingPath;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    IContextMenu, IEnumShellItems, IShellItem, IShellItem2, IShellItemImageFactory, SHCreateItemFromParsingName, SHGetIDListFromObject, SHGetKnownFolderItem,
    SHGetKnownFolderPath, ShellExecuteExW, BHID_EnumItems, BHID_SFUIObject, CMF_NORMAL, FOLDERID_AppsFolder, GCS_VERBW, KF_FLAG_DEFAULT, SEE_MASK_INVOKEIDLIST,
    SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID, SW_SHOWNORMAL};

use crate::{util, Shared};

#[derive(Serialize, Clone)]
pub struct AppEntry {
    pub name: String,
    pub id: String,
    pub icon: Option<String>,
    /// Lower-case full path when the app is a plain .exe, used to match it to open windows.
    pub exe: Option<String>,
}

/// AppsFolder ids for desktop apps look like `{6D809377-…}\Mozilla Firefox\firefox.exe`
/// (a known-folder GUID plus a relative path) or a plain absolute path.
fn resolve_exe(id: &str) -> Option<String> {
    exe_path(id).map(|p| p.to_lowercase())
}

/// The program file behind a desktop app's id, spelled as Windows spells it.
fn exe_path(id: &str) -> Option<String> {
    if !id.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    if let Some(rest) = id.strip_prefix('{') {
        let close = rest.find('}')?;
        let guid = windows::core::GUID::try_from(&rest[..close]).ok()?;
        let base = unsafe { SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None) }.ok().map(util::take_pwstr)?;
        let tail = rest[close + 1..].trim_start_matches('\\');
        return Some(format!("{base}\\{tail}"));
    }
    (id.len() > 2 && id.as_bytes()[1] == b':').then(|| id.to_string())
}

const SKIP_WORDS: [&str; 10] = ["uninstall", "readme", "read me", "help", "documentation", "manual", "release notes", "website", "license", "changelog"];

fn is_noise(name: &str, id: &str) -> bool {
    let n = name.to_lowercase();
    let i = id.to_lowercase();
    SKIP_WORDS.iter().any(|w| n.contains(w))
        || i.starts_with("http")
        || [".url", ".chm", ".txt", ".pdf", ".html", ".htm", ".rtf", ".ini", ".log"].iter().any(|e| i.ends_with(e))
}

unsafe fn bitmap_png(hbm: HBITMAP) -> Option<Vec<u8>> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
    if got == 0 {
        let _ = DeleteObject(hbm.into());
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let hdc = GetDC(None);
    let lines = GetDIBits(hdc, hbm, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    ReleaseDC(None, hdc);
    let _ = DeleteObject(hbm.into());
    if lines == 0 {
        return None;
    }
    let has_alpha = buf.chunks(4).any(|p| p[3] != 0);
    for p in buf.chunks_mut(4) {
        let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
        if !has_alpha {
            p.copy_from_slice(&[r as u8, g as u8, b as u8, 255]);
        } else if a == 0 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            // Shell bitmaps are premultiplied BGRA.
            let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
            p.copy_from_slice(&[un(r), un(g), un(b), a as u8]);
        }
    }
    util::encode_png(w as u32, h as u32, &buf)
}

fn icon_file(dir: &Path, id: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    dir.join(format!("{h:016x}.png"))
}

fn scan(icon_dir: &Path) -> Vec<AppEntry> {
    util::com_sta();
    let mut out: Vec<AppEntry> = vec![];
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None::<&IBindCtx>, &BHID_EnumItems) else { return out };
        loop {
            let mut slot = [None];
            let mut fetched = 0u32;
            if items.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(util::take_pwstr).unwrap_or_default();
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).map(util::take_pwstr).unwrap_or_default();
            if name.is_empty() || id.is_empty() || is_noise(&name, &id) {
                continue;
            }
            let file = icon_file(icon_dir, &id);
            let icon = if file.exists() {
                Some(file)
            } else {
                item.cast::<IShellItemImageFactory>()
                    .ok()
                    .and_then(|f| f.GetImage(SIZE { cx: 64, cy: 64 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok())
                    .and_then(|hbm| bitmap_png(hbm))
                    .and_then(|png| std::fs::write(&file, png).ok())
                    .map(|_| file)
            };
            // Shortcut-based apps (Discord, Steam…) have an AUMID-style id; their target exe is a property.
            let exe = resolve_exe(&id).or_else(|| {
                item.cast::<IShellItem2>()
                    .ok()
                    .and_then(|i2| i2.GetString(&PKEY_Link_TargetParsingPath).ok())
                    .map(util::take_pwstr)
                    .filter(|p| p.to_ascii_lowercase().ends_with(".exe"))
                    .map(|p| p.to_lowercase())
            });
            out.push(AppEntry { name, id, icon: icon.map(|p| p.to_string_lossy().into_owned()), exe });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    out
}

pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let dir = shared.dir.join("icons");
        let _ = std::fs::create_dir_all(&dir);
        let list = scan(&dir);
        *shared.apps.lock().unwrap() = list;
        let _ = app.emit("apps:ready", ());
    });
}

/* ---------- details and actions, for the Command Center's Apps sheet ---------- */

/// What the Apps sheet shows about the chosen app, read the first time it's chosen.
#[derive(Serialize, Clone, Default)]
pub struct AppInfo {
    /// "desktop" (a program file), "store" (a packaged app) or "other" (a link, a Control Panel page…).
    pub kind: &'static str,
    /// The program file, as Windows spells it.
    pub path: Option<String>,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    /// Whether its Start menu entry offers "Run as administrator".
    pub admin: bool,
    /// Whether there's somewhere to show it in File Explorer (its program file or its shortcut).
    pub location: bool,
    /// A 256-px icon for the big tile.
    pub icon: Option<String>,
}

static INFO: Mutex<Option<HashMap<String, AppInfo>>> = Mutex::new(None);

/// System.AppUserModel.PackageFullName (`Publisher.App_1.2.3.0_x64__hash`), which the windows
/// crate doesn't name.
const PKEY_PACKAGE_FULL_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 21 };

fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.contains(['"', '\n', '\r', '\0']) {
        return Err("Invalid app id".into());
    }
    Ok(())
}

unsafe fn apps_item(id: &str) -> Option<IShellItem> {
    let name = match id.strip_prefix("exe:") {
        Some(path) => path.to_string(),
        None => format!("shell:AppsFolder\\{id}"),
    };
    SHCreateItemFromParsingName(&HSTRING::from(name), None::<&IBindCtx>).ok()
}

/// CompanyName, ProductVersion (else FileVersion) and FileDescription from a program's version resource.
fn version_info(path: &str) -> (Option<String>, Option<String>, Option<String>) {
    unsafe {
        let file = HSTRING::from(path);
        let size = GetFileVersionInfoSizeW(&file, None);
        if size == 0 {
            return (None, None, None);
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(&file, None, size, buf.as_mut_ptr() as _).is_err() {
            return (None, None, None);
        }
        let query = |sub: &str| -> Option<(*const u16, u32)> {
            let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            let ok = VerQueryValueW(buf.as_ptr() as _, &HSTRING::from(sub), &mut p, &mut len).as_bool();
            (ok && !p.is_null() && len > 0).then_some((p as *const u16, len))
        };
        // The first language the resource lists; most programs carry only one.
        let lang = query("\\VarFileInfo\\Translation")
            .filter(|&(_, len)| len >= 4)
            .map(|(p, _)| format!("{:04x}{:04x}", *p, *p.add(1)))
            .unwrap_or_else(|| "040904b0".into());
        let text = |name: &str| -> Option<String> {
            let (p, len) = query(&format!("\\StringFileInfo\\{lang}\\{name}"))?;
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len as usize));
            let s = s.trim_end_matches('\0').trim().to_string();
            (!s.is_empty()).then_some(s)
        };
        // Some programs write their version as "3, 3, 1, 9".
        let version = text("ProductVersion").or_else(|| text("FileVersion")).map(|v| v.replace(", ", "."));
        (text("CompanyName"), version, text("FileDescription"))
    }
}

/// The verbs on an item's context menu (the one Start shows), such as "open" and "runas".
unsafe fn verbs(item: &IShellItem) -> Vec<String> {
    let mut out = vec![];
    let Ok(cm) = item.BindToHandler::<_, IContextMenu>(None::<&IBindCtx>, &BHID_SFUIObject) else { return out };
    let Ok(menu) = CreatePopupMenu() else { return out };
    if cm.QueryContextMenu(menu, 0, 1, 0x7fff, CMF_NORMAL).is_ok() {
        for i in 0..GetMenuItemCount(Some(menu)).max(0) {
            let cmd = GetMenuItemID(menu, i);
            if cmd == u32::MAX || cmd == 0 {
                continue;
            }
            let mut buf = [0u16; 80];
            if cm.GetCommandString((cmd - 1) as usize, GCS_VERBW, None, PSTR(buf.as_mut_ptr() as *mut u8), buf.len() as u32).is_ok() {
                let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                if n > 0 {
                    out.push(String::from_utf16_lossy(&buf[..n]));
                }
            }
        }
    }
    let _ = DestroyMenu(menu);
    out
}

pub fn info(id: &str, icon_dir: &Path) -> Result<AppInfo, String> {
    valid_id(id)?;
    if let Some(i) = INFO.lock().unwrap().as_ref().and_then(|m| m.get(id)) {
        return Ok(i.clone());
    }
    util::com_sta();
    let mut out = AppInfo { kind: "other", ..Default::default() };
    unsafe {
        let item = apps_item(id);
        let item2 = item.as_ref().and_then(|i| i.cast::<IShellItem2>().ok());
        out.path = if let Some(p) = id.strip_prefix("exe:") {
            Some(p.to_string())
        } else {
            exe_path(id).or_else(|| {
                item2
                    .as_ref()
                    .and_then(|i| i.GetString(&PKEY_Link_TargetParsingPath).ok())
                    .map(util::take_pwstr)
                    .filter(|p| p.to_ascii_lowercase().ends_with(".exe"))
            })
        };
        if let Some(path) = out.path.as_deref() {
            out.kind = "desktop";
            (out.publisher, out.version, out.description) = version_info(path);
        } else if id.contains('!') {
            out.kind = "store";
            // Publisher.App_1.2.3.0_x64__hash: the version is the second part.
            if let Some(full) = item2.as_ref().and_then(|i| i.GetString(&PKEY_PACKAGE_FULL_NAME).ok()).map(util::take_pwstr) {
                out.version = full.split('_').nth(1).filter(|v| !v.is_empty()).map(String::from);
            }
            out.publisher = Some(if id.to_ascii_lowercase().starts_with("microsoft.") { "Microsoft Corporation".into() } else { "Microsoft Store app".into() });
        }
        if let Some(item) = item.as_ref() {
            let vs = verbs(item);
            let has = |v: &str| vs.iter().any(|x| x.eq_ignore_ascii_case(v));
            out.admin = has("runas");
            out.location = out.path.is_some() || has("openfilelocation");
            let file = icon_file(icon_dir, id).with_extension("256.png");
            out.icon = if file.exists() {
                Some(file.to_string_lossy().into_owned())
            } else {
                item.cast::<IShellItemImageFactory>()
                    .ok()
                    .and_then(|f| f.GetImage(SIZE { cx: 256, cy: 256 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok())
                    .and_then(|hbm| bitmap_png(hbm))
                    .and_then(|png| std::fs::write(&file, png).ok())
                    .map(|_| file.to_string_lossy().into_owned())
            };
        }
    }
    INFO.lock().unwrap().get_or_insert_with(HashMap::new).insert(id.to_string(), out.clone());
    Ok(out)
}

/// admin (run as administrator: Windows asks first) | location (the program file in File Explorer)
/// | uninstall (Settings' Installed apps, where you finish it yourself).
pub fn action(id: &str, what: &str) -> Result<(), String> {
    valid_id(id)?;
    match what {
        "admin" => invoke_verb(id, "runas").map_err(|e| format!("Couldn't start it as administrator: {e}")),
        "location" => match info(id, &util::data_dir().join("icons"))?.path {
            Some(path) => crate::places::reveal(&path),
            // No program file (a Control Panel page…): its shortcut, the way Start does it.
            None => invoke_verb(id, "openfilelocation"),
        },
        "uninstall" => {
            // A Store app's own Settings page has its Uninstall button; desktop apps are in the list.
            let pfn = id.split_once('!').map(|(f, _)| f).filter(|_| !id.contains('\\'));
            util::shell_open(&pfn.map_or("ms-settings:appsfeatures".into(), |f| format!("ms-settings:appsfeatures-app?{f}")))
        }
        _ => Err(format!("Unknown action {what}")),
    }
}

/// Runs one of the verbs on the app's Start menu entry, the same as choosing it from Start's menu.
fn invoke_verb(id: &str, verb: &str) -> Result<(), String> {
    unsafe {
        util::com_sta();
        let item = apps_item(id).ok_or("That app isn't installed anymore")?;
        let pidl = SHGetIDListFromObject(&item).map_err(|e| e.message())?;
        let verb = HSTRING::from(verb);
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpIDList: pidl as *mut _,
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let r = ShellExecuteExW(&mut sei);
        CoTaskMemFree(Some(pidl as *const _));
        match r {
            Ok(()) => Ok(()),
            // Saying No to Windows' question isn't an error.
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_CANCELLED.to_hresult() => Ok(()),
            Err(e) => Err(e.message()),
        }
    }
}

pub fn launch(id: &str) -> Result<(), String> {
    if id.contains(['"', '\n', '\r']) {
        return Err("Invalid app id".into());
    }
    // Pinned running programs that aren't in the Start menu are stored as "exe:<full path>".
    if let Some(path) = id.strip_prefix("exe:") {
        let p = std::path::Path::new(path);
        if !p.is_file() || !path.to_ascii_lowercase().ends_with(".exe") {
            return Err("That program isn't installed there anymore".into());
        }
        return std::process::Command::new("explorer.exe").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{id}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test apps::tests::info_live -- --ignored --nocapture`: details and menu verbs for a few
    /// installed apps of each kind.
    #[test]
    #[ignore = "needs a real PC: prints its installed apps"]
    fn info_live() {
        let dir = std::env::temp_dir().join("island-apps-test");
        let _ = std::fs::create_dir_all(&dir);
        let list = scan(&dir);
        let want = ["Discord", "Steam", "Calculator", "Notepad", "Terminal", "Firefox", "Visual Studio Code", "Command Prompt", "Photos", "Control Panel"];
        for a in list.iter().filter(|a| want.iter().any(|n| a.name.contains(n))) {
            let i = info(&a.id, &dir).unwrap();
            let v = unsafe { apps_item(&a.id).map(|it| verbs(&it)).unwrap_or_default() };
            println!("{} [{}]\n   kind={} path={:?} version={:?} publisher={:?} admin={} location={} icon={} verbs={v:?}", a.name, a.id, i.kind, i.path, i.version, i.publisher, i.admin, i.location, i.icon.is_some());
        }
    }
}
