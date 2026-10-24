//! Diagnostics: what Windows receives around the Windows key once the island's own hook has had its
//! say. Logs each Win, mask (vkE8/vkFF) and modifier key with whether it was injected, every other
//! key only as "other" (so typing isn't recorded), and foreground changes (Start opening shows up as
//! SearchHost / StartMenuExperienceHost). Prints to the console and %USERPROFILE%\island-winkey-log.txt
//! for 5 minutes. Run it from a terminal outside Claude's sandbox.
//!
//! `winkey_log block` also keeps every real Win press and release from Windows (its hook renews
//! itself every second to stay ahead of the island's), to test whether a low-level hook can stop
//! Start at all on this PC. The island doesn't see Win while it runs.
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_TIMER,
};

static START: OnceLock<Instant> = OnceLock::new();
static LINES: OnceLock<Sender<String>> = OnceLock::new();
static BLOCK: AtomicBool = AtomicBool::new(false);

fn ms() -> u128 {
    START.get().map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

fn emit(line: String) {
    if let Some(tx) = LINES.get() {
        let _ = tx.send(line);
    }
}

fn name(vk: u32) -> &'static str {
    match vk {
        0x5B => "LWin",
        0x5C => "RWin",
        0xE8 => "mask vkE8",
        0xFF => "mask vkFF",
        0xA0 | 0xA1 => "Shift",
        0xA2 | 0xA3 => "Ctrl",
        0xA4 | 0xA5 => "Alt",
        _ => "other",
    }
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let up = kb.flags.0 & LLKHF_UP.0 != 0;
        let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
        let win = kb.vkCode == 0x5B || kb.vkCode == 0x5C;
        let blocked = BLOCK.load(Ordering::Relaxed) && win && !injected;
        emit(format!(
            "{:>7} key   {:<9} {:<4} {}{}",
            ms(),
            name(kb.vkCode),
            if up { "up" } else { "down" },
            if injected { "INJECTED" } else { "hardware" },
            if blocked { "  (kept from Windows)" } else { "" }
        ));
        if blocked {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return format!("pid {pid}") };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return format!("pid {pid}");
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or(&path).to_string()
    }
}

fn main() {
    START.set(Instant::now()).ok();
    let block = std::env::args().any(|a| a == "block");
    BLOCK.store(block, Ordering::Relaxed);
    let (tx, rx) = channel::<String>();
    LINES.set(tx).ok();

    std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(None).ok().map(|m| m.into());
        let install = || SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0);
        let Ok(mut current) = install() else {
            emit("hook failed".into());
            return;
        };
        if block {
            SetTimer(None, 0, 1000, None);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                if let Ok(h) = install() {
                    let _ = UnhookWindowsHookEx(current);
                    current = h;
                }
                continue;
            }
            DispatchMessageW(&msg);
        }
    });

    std::thread::spawn(|| {
        let mut last = String::new();
        loop {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
            let now = process_name(pid);
            if now != last {
                emit(format!("{:>7} focus -> {now}", ms()));
                last = now;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    });

    let path = std::env::var("USERPROFILE").unwrap_or_default() + "\\island-winkey-log.txt";
    let mut file = std::fs::File::create(&path).ok();
    let head = format!("winkey_log{} — tap Win a few times, a couple of seconds apart. Runs 5 minutes.", if block { " block" } else { "" });
    println!("{head}");
    if let Some(f) = file.as_mut() {
        let _ = writeln!(f, "{head}");
    }
    let end = Instant::now() + Duration::from_secs(300);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let Ok(line) = rx.recv_timeout(left) else { break };
        if !line.contains("other") {
            println!("{line}");
        }
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
    println!("done");
}
