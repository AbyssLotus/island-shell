//! Open app windows, using the same rules the taskbar uses to decide what gets a button.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::{icons, util};

#[derive(Serialize, Clone)]
pub struct RunningWindow {
    pub hwnd: isize,
    pub title: String,
    /// Lower-case full path of the process image.
    pub exe: String,
    /// Explicit AppUserModelID, when the window has one (UWP apps, Chrome, Electron apps…).
    pub aumid: Option<String>,
    /// Cached PNG of the window or exe icon.
    pub icon: Option<String>,
    pub minimized: bool,
    /// The window the user was in before opening the island.
    pub active: bool,
}

/// Icon files by exe path, so each program's icon is only extracted once per run.
static ICON_CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

pub fn hwnd(v: isize) -> HWND {
    HWND(v as _)
}

/// Lower-case full path of a process image, or empty when it can't be read.
pub fn exe_of(pid: u32) -> String {
    exe_path_of(pid).to_lowercase()
}

/// Full path of a process image as Windows spells it, or empty when it can't be read.
pub fn exe_path_of(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

pub fn exe_of_window(h: HWND) -> (u32, String) {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    (pid, exe_of(pid))
}

fn aumid_of(h: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(h).ok()?;
        let pv = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let p = PropVariantToStringAlloc(&pv).ok()?;
        Some(util::take_pwstr(p)).filter(|s| !s.is_empty())
    }
}

fn title_of(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

/// The same test the taskbar applies: visible, not cloaked (other desktops, suspended UWP),
/// unowned or explicitly an app window, not a tool window, and titled.
pub fn is_task_window(h: HWND) -> bool {
    unsafe {
        if !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let app = ex & WS_EX_APPWINDOW.0 != 0;
        if !app {
            if ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 {
                return false;
            }
            if GetWindow(h, GW_OWNER).map(|o| !o.is_invalid()).unwrap_or(false) {
                return false;
            }
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        cloaked == 0 && GetWindowTextLengthW(h) > 0
    }
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> windows::core::BOOL {
    let list = &mut *(lp.0 as *mut Vec<HWND>);
    list.push(h);
    windows::core::BOOL(1)
}

/// Cached PNG of a program's icon: from one of its windows when there is one, else from the exe.
fn icon_for(exe: &str, h: Option<HWND>, dir: &Path) -> Option<String> {
    let mut cache = ICON_CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(v) = cache.get(exe) {
        return v.clone();
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in exe.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let file = dir.join(format!("{hash:016x}.png"));
    let path = if file.exists() {
        Some(file)
    } else {
        h.and_then(icons::window_icon_png)
            .or_else(|| icons::exe_icon_png(exe))
            .and_then(|png| std::fs::write(&file, png).ok())
            .map(|_| file)
    };
    let path = path.map(|p| p.to_string_lossy().into_owned());
    cache.insert(exe.to_string(), path.clone());
    path
}

pub fn list(data_dir: &Path, active: isize) -> Vec<RunningWindow> {
    let mut handles: Vec<HWND> = vec![];
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let own_pid = std::process::id();
    let dir = data_dir.join("winicons");
    let _ = std::fs::create_dir_all(&dir);
    let mut out = vec![];
    for h in handles {
        if !is_task_window(h) {
            continue;
        }
        let (pid, exe) = exe_of_window(h);
        if pid == own_pid {
            continue;
        }
        out.push(RunningWindow {
            hwnd: h.0 as isize,
            title: title_of(h),
            icon: if exe.is_empty() { None } else { icon_for(&exe, Some(h), &dir) },
            exe,
            aumid: aumid_of(h),
            minimized: unsafe { IsIconic(h).as_bool() },
            active: h.0 as isize == active,
        });
    }
    out
}

/// Processes that own a taskbar window (for the Task Manager's Apps). A Store app's window
/// belongs to ApplicationFrameHost, so the app's own process is found among its child windows.
pub fn task_window_pids() -> HashSet<u32> {
    let mut handles: Vec<HWND> = vec![];
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let mut out = HashSet::new();
    for h in handles {
        if !is_task_window(h) {
            continue;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        out.insert(pid);
        let mut class = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut class) };
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "ApplicationFrameWindow" {
            let mut kids: Vec<HWND> = vec![];
            unsafe {
                let _ = EnumChildWindows(Some(h), Some(collect), LPARAM(&mut kids as *mut _ as isize));
            }
            for k in kids {
                let mut kp = 0u32;
                unsafe { GetWindowThreadProcessId(k, Some(&mut kp)) };
                out.insert(kp);
            }
        }
    }
    out
}

/// A program's icon for places without a window to take it from (the audio mixer).
pub fn exe_icon(exe: &str) -> Option<String> {
    let dir = util::data_dir().join("winicons");
    let _ = std::fs::create_dir_all(&dir);
    icon_for(exe, None, &dir)
}

pub fn activate(v: isize) -> Result<(), String> {
    let h = hwnd(v);
    if !is_task_window(h) {
        return Err("That window is gone".into());
    }
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
    Ok(())
}

pub fn minimize(v: isize) {
    let h = hwnd(v);
    if is_task_window(h) {
        unsafe {
            let _ = ShowWindow(h, SW_MINIMIZE);
        }
    }
}

pub fn close(v: isize) {
    let h = hwnd(v);
    if is_task_window(h) {
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}
