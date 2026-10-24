//! Tray (notification area) icons for the island.
//!
//! Windows 11 draws its tray inside Explorer, so the icons can't be read from outside. Instead,
//! like RetroBar and other taskbar replacements, Island Shell registers a second window of class
//! `Shell_TrayWnd` and keeps it first in z-order. `Shell_NotifyIcon` finds it with FindWindow and
//! sends it every icon add/change/remove as WM_COPYDATA. Every message is forwarded to Explorer's
//! real tray and Explorer's answer is returned, so Explorer, app bars and the taskbar keep working.
//! If Island Shell exits or crashes, the window disappears and apps go straight to Explorer again,
//! which already has every icon.

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GetCursorPos, GetMessageW, IsWindow,
    PostMessageW, RegisterClassW, RegisterWindowMessageW, SendMessageTimeoutW, SendNotifyMessageW, SetTimer, SetWindowPos, ShowWindow, HICON,
    HWND_BROADCAST, HWND_TOPMOST, MSG, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_COPYDATA, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER,
    WM_USER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::{icons, running, util};

const SIGNATURE: u32 = 0x3475_3423;
const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;
const NIN_SELECT: u32 = WM_USER;

/// NOTIFYICONDATA as Shell_NotifyIcon packs it for cross-process delivery (32-bit handles).
#[repr(C)]
#[derive(Clone, Copy)]
struct Nid32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    hicon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: [u8; 16],
    balloon_icon: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TrayData {
    signature: u32,
    message: u32,
    nid: Nid32,
}

#[derive(Clone)]
struct Icon {
    key: String,
    hwnd: u32,
    uid: u32,
    callback: u32,
    version: u32,
    tip: String,
    png: Option<String>,
    hidden: bool,
    pid: u32,
    exe: String,
    order: u64,
}

#[derive(Serialize, Clone)]
pub struct TrayItem {
    pub key: String,
    pub tip: String,
    pub icon: Option<String>,
    pub exe: String,
}

static ICONS: Mutex<Vec<Icon>> = Mutex::new(Vec::new());
static OWN: AtomicIsize = AtomicIsize::new(0);
static ORDER: AtomicU64 = AtomicU64::new(0);
static CHANGED: OnceLock<Sender<()>> = OnceLock::new();

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

fn notify() {
    if let Some(tx) = CHANGED.get() {
        let _ = tx.send(());
    }
}

/// Our own Shell_TrayWnd, if the tray host is running.
pub fn own_tray() -> Option<HWND> {
    let v = OWN.load(Ordering::Relaxed);
    (v != 0).then(|| HWND(v as _))
}

/// Every Shell_TrayWnd that belongs to Explorer (never ours).
pub fn explorer_trays() -> Vec<HWND> {
    let own = own_tray();
    let mut out = vec![];
    let mut prev: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_TrayWnd"), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            if Some(h) != own {
                out.push(h);
            }
            prev = Some(h);
        }
    }
    out
}

fn forward(msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    let target = *explorer_trays().first()?;
    let mut res = 0usize;
    let ok = unsafe { SendMessageTimeoutW(target, msg, wp, lp, SMTO_ABORTIFHUNG, 4000, Some(&mut res)) };
    (ok.0 != 0).then_some(LRESULT(res as isize))
}

fn handle(d: &TrayData) -> bool {
    if d.signature != SIGNATURE {
        return false;
    }
    let n = &d.nid;
    let key = if n.flags & NIF_GUID != 0 {
        n.guid.iter().map(|b| format!("{b:02x}")).collect::<String>()
    } else {
        format!("{:x}:{}", n.hwnd, n.uid)
    };
    let mut list = ICONS.lock().unwrap();
    let idx = list.iter().position(|i| i.key == key);
    match d.message {
        NIM_ADD | NIM_MODIFY => {
            let i = match idx {
                Some(i) => i,
                None => {
                    list.push(Icon {
                        key,
                        hwnd: n.hwnd,
                        uid: n.uid,
                        callback: 0,
                        version: 0,
                        tip: String::new(),
                        png: None,
                        hidden: false,
                        pid: 0,
                        exe: String::new(),
                        order: ORDER.fetch_add(1, Ordering::Relaxed),
                    });
                    list.len() - 1
                }
            };
            let e = &mut list[i];
            e.hwnd = n.hwnd;
            e.uid = n.uid;
            if n.flags & NIF_MESSAGE != 0 {
                e.callback = n.callback;
            }
            if n.flags & NIF_TIP != 0 {
                e.tip = wstr(&n.tip);
            }
            if n.flags & NIF_ICON != 0 {
                if let Some(png) = icons::hicon_to_png(HICON(n.hicon as isize as _)) {
                    e.png = Some(format!("data:image/png;base64,{}", B64.encode(png)));
                }
            }
            if n.flags & NIF_STATE != 0 && n.state_mask & NIS_HIDDEN != 0 {
                e.hidden = n.state & NIS_HIDDEN != 0;
            }
            if e.pid == 0 {
                let (pid, exe) = running::exe_of_window(HWND(n.hwnd as isize as _));
                e.pid = pid;
                e.exe = exe.rsplit('\\').next().unwrap_or(&exe).trim_end_matches(".exe").to_string();
            }
        }
        NIM_DELETE => {
            if let Some(i) = idx {
                list.remove(i);
            }
        }
        NIM_SETVERSION => {
            if let Some(i) = idx {
                list[i].version = n.version;
            }
        }
        _ => return false,
    }
    drop(list);
    notify();
    true
}

fn broadcast_taskbar_created() {
    unsafe {
        let msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        let _ = SendNotifyMessageW(HWND_BROADCAST, msg, WPARAM(0), LPARAM(0));
    }
}

static BEHIND_TICKS: AtomicU64 = AtomicU64::new(0);
static LAST_BROADCAST: AtomicU64 = AtomicU64::new(0);
static TICK: AtomicU64 = AtomicU64::new(0);

/// Runs 4× a second. Explorer raises its own (hidden) taskbar every so often, which puts its
/// Shell_TrayWnd in front of ours; quietly step back in front. Only if we stayed behind long
/// enough to have missed icon updates, ask apps to re-send their icons — at most once a minute.
unsafe fn maintain(own: HWND) {
    let tick = TICK.fetch_add(1, Ordering::Relaxed);
    if FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok() != Some(own) {
        let _ = SetWindowPos(own, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let behind = BEHIND_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        let now = util::now_ms();
        if behind >= 4 && now.saturating_sub(LAST_BROADCAST.load(Ordering::Relaxed)) > 60_000 {
            LAST_BROADCAST.store(now, Ordering::Relaxed);
            util::log_line("tray host was behind Explorer for a while; asking apps to re-send their icons");
            broadcast_taskbar_created();
        }
    } else {
        BEHIND_TICKS.store(0, Ordering::Relaxed);
    }
    if tick % 8 != 0 {
        return;
    }
    let mut list = ICONS.lock().unwrap();
    let before = list.len();
    list.retain(|i| IsWindow(Some(HWND(i.hwnd as isize as _))).as_bool());
    let changed = list.len() != before;
    drop(list);
    if changed {
        notify();
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA => {
            let cds = &*(lp.0 as *const COPYDATASTRUCT);
            let mut handled = false;
            if cds.dwData == 1 && !cds.lpData.is_null() && cds.cbData as usize >= 32 {
                let mut d: TrayData = std::mem::zeroed();
                let n = (cds.cbData as usize).min(std::mem::size_of::<TrayData>());
                std::ptr::copy_nonoverlapping(cds.lpData as *const u8, &mut d as *mut TrayData as *mut u8, n);
                handled = handle(&d);
            }
            let res = forward(msg, wp, lp);
            match res {
                // Explorer refuses re-adds of icons it already has; the icon is there, so report success.
                Some(r) if handled && r.0 == 0 => LRESULT(1),
                Some(r) => r,
                None => LRESULT(handled as isize),
            }
        }
        WM_TIMER => {
            maintain(h);
            LRESULT(0)
        }
        m if m == WM_COMMAND || (WM_USER..0xC000).contains(&m) => forward(msg, wp, lp).unwrap_or(LRESULT(0)),
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel::<()>();
    if CHANGED.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            std::thread::sleep(Duration::from_millis(150));
            while rx.try_recv().is_ok() {}
            let _ = app.emit("tray:changed", ());
        }
    });
    std::thread::spawn(|| unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = w!("Shell_TrayWnd");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: module.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                util::log_line(&format!("tray host failed to start: {e}"));
                return;
            }
        };
        OWN.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        SetTimer(Some(hwnd), 1, 250, None);
        LAST_BROADCAST.store(util::now_ms(), Ordering::Relaxed);
        let first = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).ok() == Some(hwnd);
        util::log_line(&format!("tray host started (first in z-order: {first})"));
        broadcast_taskbar_created();
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(5));
            let n = ICONS.lock().unwrap().iter().filter(|i| !i.hidden).count();
            util::log_line(&format!("tray host has {n} icons after re-registration"));
        });
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

/// Closes our tray window so apps (and SHAppBarMessage) talk to Explorer directly again.
pub fn shutdown() {
    if let Some(h) = own_tray() {
        OWN.store(0, Ordering::Relaxed);
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

pub fn list() -> Vec<TrayItem> {
    let mut v: Vec<Icon> = ICONS.lock().unwrap().iter().filter(|i| !i.hidden && i.png.is_some()).cloned().collect();
    v.sort_by_key(|i| i.order);
    v.into_iter().map(|i| TrayItem { key: i.key, tip: i.tip, icon: i.png, exe: i.exe }).collect()
}

/// Sends the icon's owner the same mouse messages Explorer sends for a click.
pub fn click(key: &str, button: &str) -> Result<(), String> {
    let icon = ICONS.lock().unwrap().iter().find(|i| i.key == key).cloned().ok_or("That tray icon is gone")?;
    if icon.callback == 0 {
        return Err("This icon doesn't respond to clicks".into());
    }
    let target = HWND(icon.hwnd as isize as _);
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        if icon.pid != 0 {
            let _ = AllowSetForegroundWindow(icon.pid);
        }
    }
    let post = |m: u32| unsafe {
        let (wp, lp) = if icon.version >= 4 {
            let wp = ((pt.y as u16 as usize) << 16) | pt.x as u16 as usize;
            let lp = (((icon.uid & 0xFFFF) << 16) | (m & 0xFFFF)) as isize;
            (WPARAM(wp), LPARAM(lp))
        } else {
            (WPARAM(icon.uid as usize), LPARAM(m as isize))
        };
        let _ = PostMessageW(Some(target), icon.callback, wp, lp);
    };
    post(WM_MOUSEMOVE);
    match button {
        "right" => {
            post(WM_RBUTTONDOWN);
            post(WM_RBUTTONUP);
            if icon.version >= 3 {
                post(WM_CONTEXTMENU);
            }
        }
        "double" => {
            post(WM_LBUTTONDOWN);
            post(WM_LBUTTONUP);
            post(WM_LBUTTONDBLCLK);
            post(WM_LBUTTONUP);
        }
        _ => {
            post(WM_LBUTTONDOWN);
            post(WM_LBUTTONUP);
            if icon.version >= 3 {
                post(NIN_SELECT);
            }
        }
    }
    Ok(())
}
