//! Diagnostics: how the resting island window can be brought forward (or made always-on-top) by a
//! background process, as the island itself is when a notification arrives. Windowless (no console
//! pops up); writes its findings to %USERPROFILE%\island-topmost-probe.txt. The island sinks itself
//! back onto the desktop within 300 ms after each try. Run it outside Claude's sandbox.
#![windows_subsystem = "windows"]
use std::fmt::Write as _;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn topmost(h: HWND) -> bool {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}

/// Visible windows above `h` in z-order (0 = it's on top of everything).
fn above(h: HWND) -> usize {
    let mut n = 0;
    let mut x = h;
    while let Ok(p) = unsafe { GetWindow(x, GW_HWNDPREV) } {
        if p.is_invalid() {
            break;
        }
        if unsafe { IsWindowVisible(p) }.as_bool() {
            n += 1;
        }
        x = p;
    }
    n
}

fn main() {
    let mut out = String::new();
    let Ok(h) = (unsafe { FindWindowW(PCWSTR::null(), w!("Island Shell")) }) else { return };
    let fg = unsafe { GetForegroundWindow() };
    let _ = writeln!(out, "foreground: {} (visible above it: {})", class_of(fg), above(fg));
    let path = std::env::var("USERPROFILE").unwrap_or_default() + "\\island-topmost-probe.txt";
    // `topmost_probe read`: only report where the island is now (e.g. while a notification shows).
    if std::env::args().any(|a| a == "read") {
        let mut below = h;
        let mut next = String::new();
        while let Ok(n) = unsafe { GetWindow(below, GW_HWNDNEXT) } {
            if n.is_invalid() {
                break;
            }
            below = n;
            if unsafe { IsWindowVisible(n) }.as_bool() {
                next = class_of(n);
                break;
            }
        }
        let _ = writeln!(out, "island: visible above it {}, topmost {}, next visible below: {next}", above(h), topmost(h));
        let _ = std::fs::write(path, out);
        return;
    }
    let f = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    let mut tryit = |label: &str, go: &dyn Fn() -> windows::core::Result<()>| {
        std::thread::sleep(std::time::Duration::from_millis(800)); // let the island sink back first
        let before = above(h);
        let r = go();
        let now = (above(h), topmost(h));
        std::thread::sleep(std::time::Duration::from_millis(60));
        let _ = writeln!(out, "{label:<30} {:<4} above: {before:>3} -> {:>3} (topmost {}), 60ms later {:>3}", if r.is_ok() { "ok" } else { "ERR" }, now.0, now.1, above(h));
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_NOTOPMOST), 0, 0, 0, 0, f);
        }
    };
    unsafe {
        tryit("HWND_TOP", &|| SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, f));
        tryit("HWND_TOPMOST", &|| SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, f));
        tryit("HWND_TOPMOST (again)", &|| SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, f));
        tryit("NOTOPMOST, then TOPMOST", &|| SetWindowPos(h, Some(HWND_NOTOPMOST), 0, 0, 0, 0, f).and_then(|_| SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, f)));
        tryit("TOP, then TOPMOST", &|| SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, f).and_then(|_| SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, f)));
    }
    let _ = std::fs::write(path, out);
}
