//! Support for the top-bar style: virtual desktops, battery, and reserving the bar's strip of the
//! screen as an app bar (like the taskbar does) so maximized windows fit underneath it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_D,
    VK_LCONTROL, VK_LEFT, VK_LWIN, VK_RIGHT,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::WM_USER;

/// Height of the bar in CSS pixels; the page uses the same value.
pub const BAR_HEIGHT: f64 = 40.0;
/// The "tall" layout's bar height.
pub const TALL_BAR_HEIGHT: f64 = 48.0;

/// Logical height of the bar for the regular or the tall layout.
pub fn height(tall: bool) -> f64 {
    if tall { TALL_BAR_HEIGHT } else { BAR_HEIGHT }
}

const VD_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VirtualDesktops";

#[derive(Serialize, Clone)]
pub struct Desktops {
    pub count: usize,
    pub current: usize,
    pub names: Vec<String>,
}

impl Default for Desktops {
    fn default() -> Self {
        Desktops { count: 1, current: 0, names: vec![String::new()] }
    }
}

#[derive(Serialize, Clone, Default)]
pub struct Battery {
    pub present: bool,
    pub percent: u8,
    pub charging: bool,
}

fn reg_bin(sub: &str, value: &str) -> Option<Vec<u8>> {
    unsafe {
        let (sub, value) = (HSTRING::from(sub), HSTRING::from(value));
        let mut size = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, &sub, &value, RRF_RT_REG_BINARY, None, None, Some(&mut size)).is_err() || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if RegGetValueW(HKEY_CURRENT_USER, &sub, &value, RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).is_err() {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }
}

fn reg_sz(sub: &str, value: &str) -> Option<String> {
    unsafe {
        let (sub, value) = (HSTRING::from(sub), HSTRING::from(value));
        let mut buf = [0u16; 260];
        let mut size = (buf.len() * 2) as u32;
        RegGetValueW(HKEY_CURRENT_USER, &sub, &value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).ok().ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

fn guid_string(b: &[u8]) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        u16::from_le_bytes([b[4], b[5]]),
        u16::from_le_bytes([b[6], b[7]]),
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Windows' virtual desktops, read from the registry Explorer keeps them in. With only one desktop
/// Windows stores no list at all.
pub fn desktops() -> Desktops {
    let ids: Vec<Vec<u8>> = reg_bin(VD_KEY, "VirtualDesktopIDs").map(|b| b.chunks_exact(16).map(|c| c.to_vec()).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Desktops::default();
    }
    let current = reg_bin(VD_KEY, "CurrentVirtualDesktop")
        .or_else(|| {
            let mut sid = 0u32;
            unsafe { ProcessIdToSessionId(std::process::id(), &mut sid) }.ok()?;
            reg_bin(&format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo\\{sid}\\VirtualDesktops"), "CurrentVirtualDesktop")
        })
        .and_then(|cur| ids.iter().position(|id| cur.len() >= 16 && id[..] == cur[..16]))
        .unwrap_or(0);
    let names = ids.iter().map(|id| reg_sz(&format!("{VD_KEY}\\Desktops\\{}", guid_string(id)), "Name").unwrap_or_default()).collect();
    Desktops { count: ids.len(), current, names }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    if vk == VK_LEFT || vk == VK_RIGHT {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

/// Presses a shortcut (keys down in order, up in reverse).
fn chord(keys: &[VIRTUAL_KEY]) {
    let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, false)).collect();
    inputs.extend(keys.iter().rev().map(|&k| key(k, true)));
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Switches desktops with Win+Ctrl+Left/Right, one step at a time (there's no public API).
pub fn switch_desktop(target: usize) -> Result<(), String> {
    let d = desktops();
    if target >= d.count {
        return Err("That desktop doesn't exist".into());
    }
    let delta = target as i64 - d.current as i64;
    let arrow = if delta > 0 { VK_RIGHT } else { VK_LEFT };
    for _ in 0..delta.unsigned_abs() {
        chord(&[VK_LWIN, VK_LCONTROL, arrow]);
        std::thread::sleep(Duration::from_millis(90));
    }
    Ok(())
}

pub fn new_desktop() {
    chord(&[VK_LWIN, VK_LCONTROL, VK_D]);
}

pub fn desktop_manager() -> Option<IVirtualDesktopManager> {
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL).ok() }
}

/// Windows shows a window only on the desktop it belongs to. After a desktop switch, move our
/// window onto the new current desktop so the island/bar is always there.
pub fn follow_current_desktop(vdm: &IVirtualDesktopManager, own: HWND) {
    unsafe {
        if vdm.IsWindowOnCurrentVirtualDesktop(own).map(|b| b.as_bool()).unwrap_or(true) {
            return;
        }
        let current = reg_bin(VD_KEY, "CurrentVirtualDesktop").filter(|b| b.len() >= 16).map(|b| {
            GUID::from_values(
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                u16::from_le_bytes([b[4], b[5]]),
                u16::from_le_bytes([b[6], b[7]]),
                [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
            )
        });
        // Fall back to whatever desktop the window the user is in lives on.
        let current = current.or_else(|| vdm.GetWindowDesktopId(GetForegroundWindow()).ok());
        if let Some(id) = current {
            let _ = vdm.MoveWindowToDesktop(own, &id);
        }
    }
}

pub fn battery() -> Battery {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return Battery::default();
    }
    let present = s.BatteryFlag != 128 && s.BatteryFlag != 255 && s.BatteryLifePercent <= 100;
    Battery { present, percent: s.BatteryLifePercent.min(100), charging: s.ACLineStatus == 1 }
}

static APPBAR: AtomicBool = AtomicBool::new(false);
static LAST_RESERVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_MOVE_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Reserves `height_px` at the top of the monitor for the bar. Run off the UI thread: the shell
/// may message our window while it handles this.
pub fn appbar_register(hwnd: HWND, monitor: RECT, height_px: i32) {
    unsafe {
        let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, uCallbackMessage: WM_USER + 0x3A7, ..Default::default() };
        if !APPBAR.swap(true, Ordering::Relaxed) {
            SHAppBarMessage(ABM_NEW, &mut d);
        }
        d.uEdge = ABE_TOP;
        d.rc = RECT { left: monitor.left, top: monitor.top, right: monitor.right, bottom: monitor.top + height_px };
        SHAppBarMessage(ABM_QUERYPOS, &mut d);
        d.rc.bottom = d.rc.top + height_px;
        SHAppBarMessage(ABM_SETPOS, &mut d);
    }
}

/// Keeps the bar in front with its screen strip reserved, re-applying either if something undid it
/// (a style switch racing the island's desktop placement, Explorer restarting…). While a fullscreen
/// app runs the bar steps out of the topmost band instead, so it never sits over a game.
pub fn ensure(hwnd: HWND, monitor: RECT, height_px: i32, fullscreen: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowRect, SetWindowPos, SystemParametersInfoW, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SPI_GETWORKAREA,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WS_EX_TOPMOST,
    };
    if hwnd.is_invalid() {
        return;
    }
    unsafe {
        let topmost = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
        if fullscreen {
            if topmost {
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            return;
        }
        if !topmost {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        // The bar belongs flush with the top-left of the monitor; put it back if something moved it.
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() && (r.left != monitor.left || r.top != monitor.top) {
            let _ = SetWindowPos(hwnd, None, monitor.left, monitor.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            let now = crate::util::now_ms();
            if now.saturating_sub(LAST_MOVE_LOG.load(Ordering::Relaxed)) > 10_000 {
                LAST_MOVE_LOG.store(now, Ordering::Relaxed);
                crate::util::log_line(&format!("bar window had moved to {},{}; put back at {},{}", r.left, r.top, monitor.left, monitor.top));
            }
        }
        // Re-reserve the strip if it went missing (e.g. Explorer restarted), at most every 10s.
        let mut wa = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let now = crate::util::now_ms();
        if wa.top < monitor.top + height_px && now.saturating_sub(LAST_RESERVE.load(Ordering::Relaxed)) > 10_000 {
            LAST_RESERVE.store(now, Ordering::Relaxed);
            appbar_register(hwnd, monitor, height_px);
        }
    }
}

/// Diagnostics: whether the bar window is topmost and what the desktop work area is now.
pub fn log_state(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SystemParametersInfoW, GWL_EXSTYLE, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WS_EX_TOPMOST};
    unsafe {
        let topmost = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
        let mut wa = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let mut r = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r);
        crate::util::log_line(&format!(
            "bar state: topmost={topmost} work area {},{} -> {},{}; window {},{} -> {},{}",
            wa.left, wa.top, wa.right, wa.bottom, r.left, r.top, r.right, r.bottom
        ));
    }
}

pub fn appbar_unregister(hwnd: HWND) {
    if APPBAR.swap(false, Ordering::Relaxed) {
        unsafe {
            let mut d = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() };
            SHAppBarMessage(ABM_REMOVE, &mut d);
        }
    }
}
