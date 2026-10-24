//! Hides the Windows taskbar while the shell runs and puts it back afterwards.
//!
//! The taskbar is switched to auto-hide (so maximized windows use the full screen) and its
//! windows are hidden. The user's original auto-hide setting is written to a file before the
//! first change, so a crash can't make the change permanent: the next launch (or
//! `island-shell.exe --restore-taskbar`) restores from that file.

use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetClassNameW, GetForegroundWindow, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNA,
};

fn restore_file(dir: &Path) -> PathBuf {
    dir.join("taskbar-restore.txt")
}

/// Explorer's taskbar windows. Island Shell's own tray host also has class Shell_TrayWnd, so it
/// is skipped here.
fn bars() -> Vec<HWND> {
    let mut v = crate::tray::explorer_trays();
    unsafe {
        let mut prev: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_SecondaryTrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            v.push(h);
            prev = Some(h);
        }
    }
    v
}

fn appbar() -> APPBARDATA {
    let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
    if let Some(h) = bars().first() {
        d.hWnd = *h;
    }
    d
}

fn get_state() -> u32 {
    let mut d = appbar();
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut d) as u32 }
}

fn set_state(state: u32) {
    let mut d = appbar();
    d.lParam = LPARAM(state as isize);
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}

pub fn hide(dir: &Path) {
    let file = restore_file(dir);
    if !file.exists() {
        let _ = std::fs::write(&file, get_state().to_string());
    }
    set_state(ABS_AUTOHIDE);
    for h in bars() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

pub fn restore(dir: &Path) {
    let file = restore_file(dir);
    if let Ok(s) = std::fs::read_to_string(&file) {
        if let Ok(state) = s.trim().parse::<u32>() {
            set_state(state);
        }
        let _ = std::fs::remove_file(&file);
    }
    for h in bars() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNA);
        }
    }
}

/// Explorer re-shows the taskbar now and then (restarts, edge hover). Hide it again unless the
/// user is in the middle of using Start, the tray or another piece of shell UI.
pub fn enforce() {
    unsafe {
        let fg = GetForegroundWindow();
        let mut buf = [0u16; 64];
        let n = GetClassNameW(fg, &mut buf);
        let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        const SHELL_UI: [&str; 6] = [
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "Windows.UI.Core.CoreWindow",
            "XamlExplorerHostIslandWindow",
            "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland",
        ];
        if SHELL_UI.contains(&class.as_str()) {
            return;
        }
        for h in bars() {
            if IsWindowVisible(h).as_bool() {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}
