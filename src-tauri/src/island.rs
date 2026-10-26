//! The island window: placement at the top center of the primary monitor, click-through
//! everywhere except over the island itself, and stepping aside while a fullscreen app runs.
//!
//! The window is shown once through Tauri and never hidden again. Showing it with a raw
//! ShowWindow call leaves tao believing it's invisible, and tao then strips WS_VISIBLE every time
//! it rewrites the window style (which `set_ignore_cursor_events` does), so the pill vanished as
//! soon as the mouse crossed it. Fullscreen apps fade the island out in the page instead.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, LogicalSize, PhysicalPosition, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetTopWindow, GetWindow, GetWindowRect, IsWindowVisible, IsZoomed,
    SetForegroundWindow, SetWindowPos, GW_HWNDLAST, GW_HWNDPREV, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, WS_EX_TOPMOST};

use crate::Shared;

pub const WIDTH: f64 = 820.0;
pub const HEIGHT: f64 = 660.0;

/// The island's rectangle in CSS pixels inside the window, as reported by the page.
#[derive(Clone, Copy, Default)]
pub struct Geom {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub expanded: bool,
    /// Height of the top bar strip that always takes clicks (0 in island style).
    pub bar_h: f64,
}

#[derive(Clone, Copy)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    /// The primary monitor in physical pixels.
    pub monitor: RECT,
}

impl Default for Placement {
    fn default() -> Self {
        Placement { x: 0, y: 0, scale: 1.0, monitor: RECT::default() }
    }
}

/// Island style: an 820px window centered at the top. Bar style: the full monitor width.
pub fn place(win: &WebviewWindow, bar: bool) -> Placement {
    let mon = win.primary_monitor().ok().flatten().or_else(|| win.current_monitor().ok().flatten());
    let Some(mon) = mon else { return Placement::default() };
    let scale = mon.scale_factor();
    let (mx, my) = (mon.position().x, mon.position().y);
    let (mw, mh) = (mon.size().width as i32, mon.size().height as i32);
    let width = if bar { mw as f64 / scale } else { WIDTH };
    let _ = win.set_size(LogicalSize::new(width, HEIGHT));
    let w = (width * scale).round() as i32;
    let x = mx + (mw - w) / 2;
    let _ = win.set_position(PhysicalPosition::new(x, my));
    Placement { x, y: my, scale, monitor: RECT { left: mx, top: my, right: mx + mw, bottom: my + mh } }
}

/// Tauri may use a different `windows` crate version, so convert its HWND by raw pointer.
pub fn hwnd_of(win: &WebviewWindow) -> Option<HWND> {
    win.hwnd().ok().map(|h| HWND(h.0 as _))
}

/// Shows the window through Tauri (so tao knows it's visible) and hands focus straight back
/// to whatever the user was doing.
pub fn show_quietly(win: &WebviewWindow) {
    let prev = unsafe { GetForegroundWindow() };
    let _ = win.show();
    if !prev.is_invalid() && Some(prev) != hwnd_of(win) {
        unsafe {
            let _ = SetForegroundWindow(prev);
        }
    }
}

/// Gives focus back to the app the user was in before opening the panel.
pub fn release_focus(shared: &Shared) {
    let prev = shared.prev_fg.load(Ordering::Relaxed);
    if prev != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(prev as _));
        }
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Live-wallpaper programs whose windows count as part of the desktop.
const WALLPAPER_APPS: [&str; 5] = ["wallpaper32.exe", "wallpaper64.exe", "webwallpaper32.exe", "lively.exe", "livelywpf.exe"];

fn is_desktop_layer(h: HWND) -> bool {
    let class = class_of(h);
    if class == "Progman" || class == "WorkerW" {
        return true;
    }
    let (_, exe) = crate::running::exe_of_window(h);
    WALLPAPER_APPS.iter().any(|w| exe.ends_with(&format!("\\{w}")))
}

/// The highest window of the desktop layer: Progman, the WorkerW windows that hold the icons and
/// Wallpaper Engine's wallpaper, and any top-level live-wallpaper windows. Found by walking up
/// from the bottom of the z-order until the first visible ordinary window.
fn desktop_layer_top(own: HWND) -> Option<HWND> {
    unsafe {
        let mut h = GetWindow(GetTopWindow(None).ok()?, GW_HWNDLAST).ok()?;
        let mut top = None;
        loop {
            if h != own && IsWindowVisible(h).as_bool() {
                if is_desktop_layer(h) {
                    top = Some(h);
                } else {
                    break;
                }
            }
            match GetWindow(h, GW_HWNDPREV) {
                Ok(p) if !p.is_invalid() => h = p,
                _ => break,
            }
        }
        top
    }
}

/// Puts the island directly on top of the desktop layer: above the wallpaper, below every app.
/// (Windows' own "bottom" position would put it under the desktop, where it can't be seen.)
pub fn sink(own: HWND) {
    let Some(desk) = desktop_layer_top(own) else { return };
    unsafe {
        // Already sitting right above the desktop (ignoring invisible windows in between)?
        let mut a = GetWindow(desk, GW_HWNDPREV);
        while let Ok(x) = a {
            if x.is_invalid() {
                break;
            }
            if x == own {
                return;
            }
            if IsWindowVisible(x).as_bool() {
                break;
            }
            a = GetWindow(x, GW_HWNDPREV);
        }
        let after = GetWindow(desk, GW_HWNDPREV).ok().filter(|h| !h.is_invalid()).unwrap_or(HWND_TOP);
        let _ = SetWindowPos(own, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
}

/// Tells WebView2 how hard to hold on to memory. While the island rests as a pill (or the bar has
/// nothing dropped down) "Low" lets it drop decoded images, raster tiles and other caches it
/// would otherwise keep around because the PC isn't short of memory; "Normal" comes back the
/// moment the island is used, so opening it stays smooth.
pub fn set_memory_low(win: &WebviewWindow, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core_wv::Interface;
    let _ = win.with_webview(move |w| unsafe {
        let Ok(core) = w.controller().CoreWebView2() else { return };
        // ICoreWebView2_19 needs WebView2 Runtime 114+; older runtimes simply keep their default.
        if let Ok(c) = core.cast::<ICoreWebView2_19>() {
            let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            let _ = c.SetMemoryUsageTargetLevel(level);
        }
    });
}

/// When the island last handed its memory back (ms since start), to do it once per rest and then
/// only every 10 minutes.
static TRIMMED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// While the island rests, hands the memory its WebView2 processes aren't using back to Windows
/// (EmptyWorkingSet: the same trim Windows makes when it's short of memory). The page itself has
/// already dropped its caches (set_memory_low); this returns what Chromium keeps around after use
/// (decoded pictures, raster scratch, the GPU process's pools) instead of holding it for minutes.
/// Anything needed again comes straight back from memory the moment the island is opened.
pub fn trim_at_rest(now_ms: u64, idle_since_ms: u64) {
    use std::sync::atomic::Ordering;
    let last = TRIMMED_AT.load(Ordering::Relaxed);
    let resting = now_ms.saturating_sub(idle_since_ms);
    if resting < 30_000 || (last > idle_since_ms && now_ms.saturating_sub(last) < 600_000) {
        return;
    }
    TRIMMED_AT.store(now_ms, Ordering::Relaxed);
    std::thread::spawn(|| {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA};
        let before = crate::taskmgr::tree(std::process::id());
        for &(pid, _) in &before {
            unsafe {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SET_QUOTA, false, pid) {
                    let _ = EmptyWorkingSet(h);
                    let _ = CloseHandle(h);
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let mb = |t: &[(u32, u64)]| t.iter().map(|x| x.1).sum::<u64>() / 1_048_576;
        let after = crate::taskmgr::tree(std::process::id());
        crate::util::log_line(&format!("memory: at rest, {} processes {} MB -> {} MB", after.len(), mb(&before), mb(&after)));
    });
}

/// Keeps the island above every window, fullscreen apps included, while a notification that must be
/// seen is showing (a Discord @mention); without activating it, so a game keeps the keyboard and mouse.
///
/// Windows ignores HWND_TOPMOST for a window sitting directly on the desktop (where the island rests),
/// and HWND_TOP from a background process stops just under the foreground window. Lifting it with
/// HWND_TOP first and then asking for HWND_TOPMOST works (checked with examples/topmost_probe.rs).
pub fn pin(own: HWND, on: bool) {
    let f = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    unsafe {
        if on {
            let _ = SetWindowPos(own, Some(HWND_TOP), 0, 0, 0, 0, f);
        }
        let _ = SetWindowPos(own, Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, f);
        let topmost = is_topmost(own);
        if topmost != on {
            crate::util::log_line(&format!("island: couldn't {} always-on-top for a notification", if on { "turn on" } else { "turn off" }));
        }
    }
}

pub fn is_topmost(own: HWND) -> bool {
    unsafe { GetWindowLongPtrW(own, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}

/// Brings the island above app windows (panel open, or a notification showing).
pub fn raise(own: HWND) {
    unsafe {
        let _ = SetWindowPos(own, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Returns the foreground window's class when a fullscreen app (game, video, F11 browser) covers
/// the island's monitor. Maximized windows don't count, even though they cover the whole screen
/// while the taskbar is hidden; neither does the desktop or a live wallpaper behind it.
pub fn fullscreen_app(own: Option<HWND>) -> Option<String> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || Some(fg) == own {
            return None;
        }
        let class = class_of(fg);
        const DESKTOP: [&str; 6] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Windows.UI.Core.CoreWindow", "XamlExplorerHostIslandWindow"];
        if DESKTOP.contains(&class.as_str()) || IsZoomed(fg).as_bool() {
            return None;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        if let Some(own) = own {
            if mon != MonitorFromWindow(own, MONITOR_DEFAULTTONEAREST) {
                return None;
            }
        }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let mut r = RECT::default();
        if !GetMonitorInfoW(mon, &mut mi).as_bool() || GetWindowRect(fg, &mut r).is_err() {
            return None;
        }
        let m = mi.rcMonitor;
        (r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom).then_some(class)
    }
}

/// Polls the cursor and lets mouse input reach the page only while it is over the island
/// (or while the panel is open, so clicks outside it can close it).
/// `island.geometry` (§4.7): the island's rectangle in physical screen pixels, its monitor, the
/// scale, and whether it shows (not in bar style without its dropdown, not faded for fullscreen).
fn geometry(g: &Geom, p: &Placement, faded: bool, own: Option<HWND>) -> serde_json::Value {
    let mut wr = RECT { left: p.x, top: p.y, ..Default::default() };
    if let Some(h) = own {
        let _ = unsafe { GetWindowRect(h, &mut wr) };
    }
    let s = p.scale;
    let m = p.monitor;
    serde_json::json!({
        "rect": { "x": (wr.left as f64 + g.x * s).round(), "y": (wr.top as f64 + g.y * s).round(), "w": (g.w * s).round(), "h": (g.h * s).round() },
        "monitor": { "x": m.left, "y": m.top, "w": m.right - m.left, "h": m.bottom - m.top },
        "scale": s,
        "visible": g.w > 0.0 && !faded,
    })
}

pub fn spawn_cursor(win: WebviewWindow, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut last: Option<bool> = None;
        let own = hwnd_of(&win);
        let mut last_geo: Option<serde_json::Value> = None;
        let mut tick: u64 = 0;
        crate::util::com_mta();
        let vdm = crate::bar::desktop_manager();
        loop {
            std::thread::sleep(Duration::from_millis(25));
            tick += 1;
            // Follow the user across virtual desktops (checked every 150ms).
            if tick % 6 == 0 {
                if let (Some(v), Some(h)) = (&vdm, own) {
                    crate::bar::follow_current_desktop(v, h);
                }
            }
            // Every 300ms, settle back onto the desktop layer unless the panel or a notification is up;
            // keep an important notification on top if something knocked it down.
            if tick % 12 == 0 && !shared.bar_mode.load(Ordering::Relaxed) {
                if let Some(h) = own {
                    if shared.urgent.load(Ordering::Relaxed) {
                        if !is_topmost(h) {
                            pin(h, true);
                        }
                    } else if !shared.raised.load(Ordering::Relaxed) {
                        sink(h);
                    }
                }
            }
            // Remember the last app the user was in, so closing the panel can hand focus back.
            let fg = unsafe { GetForegroundWindow() };
            if !fg.is_invalid() && Some(fg) != own {
                shared.prev_fg.store(fg.0 as isize, Ordering::Relaxed);
            }
            let g = *shared.geom.lock().unwrap();
            let p = *shared.place.lock().unwrap();
            let faded = shared.fullscreen_hidden.load(Ordering::Relaxed);
            // Where the island is on screen, for modules (`island.geometry`: the eye tracker), when it changes.
            if tick % 4 == 0 {
                let geo = geometry(&g, &p, faded, own);
                if last_geo.as_ref() != Some(&geo) {
                    crate::modules::publish(island_host::proto::topic::ISLAND_GEOMETRY, geo.clone());
                    last_geo = Some(geo);
                }
            }
            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_err() {
                continue;
            }
            // Map the cursor through where the window really is right now, not where we put it:
            // Windows can nudge it (e.g. out of the reserved bar strip).
            let mut wr = RECT { left: p.x, top: p.y, right: p.monitor.right, bottom: p.monitor.bottom };
            if let Some(h) = own {
                let _ = unsafe { GetWindowRect(h, &mut wr) };
            }
            let rx = (pt.x - wr.left) as f64 / p.scale;
            let ry = (pt.y - wr.top) as f64 / p.scale;
            let over = g.w > 0.0 && rx >= g.x - 2.0 && rx <= g.x + g.w + 2.0 && ry >= g.y - 2.0 && ry <= g.y + g.h + 2.0;
            let win_w = (wr.right - wr.left) as f64 / p.scale;
            let over_bar = g.bar_h > 0.0 && rx >= 0.0 && rx <= win_w && ry >= 0.0 && ry <= g.bar_h;
            let inside = g.expanded || (!faded && (over || over_bar));
            if last != Some(inside) {
                let _ = win.set_ignore_cursor_events(!inside);
                if !inside {
                    let _ = win.emit("island:leave", ());
                }
                last = Some(inside);
            }
        }
    });
}
