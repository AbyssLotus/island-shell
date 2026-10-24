//! Diagnostic: dumps the UI Automation tree of every top-level window (shown or hidden) of the
//! processes whose image name contains the first argument, e.g. `uia_dump.exe rvrvpngui > out.txt`.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

fn exe_of(h: HWND) -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let _ = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(p);
        String::from_utf16_lossy(&buf[..n as usize]).to_lowercase()
    }
}

unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
    let (want, out) = &mut *(l.0 as *mut (String, Vec<HWND>));
    if exe_of(h).contains(want.as_str()) {
        out.push(h);
    }
    BOOL(1)
}

fn main() {
    let want = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    let mut ctx = (want, Vec::<HWND>::new());
    unsafe { let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut _ as isize)); }
    let auto: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap() };
    let walker = unsafe { auto.RawViewWalker().unwrap() };
    for h in ctx.1 {
        let mut t = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut t) };
        let mut c = [0u16; 256];
        let cn = unsafe { GetClassNameW(h, &mut c) };
        println!(
            "-- window {:?} visible {} title {:?} class {:?}",
            h.0,
            unsafe { IsWindowVisible(h).as_bool() },
            String::from_utf16_lossy(&t[..n as usize]),
            String::from_utf16_lossy(&c[..cn as usize])
        );
        if let Ok(root) = unsafe { auto.ElementFromHandle(h) } {
            dump(&walker, &root, 0);
        }
    }
}

fn dump(w: &IUIAutomationTreeWalker, e: &IUIAutomationElement, depth: usize) {
    if depth > 30 {
        return;
    }
    unsafe {
        let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
        let id = e.CurrentAutomationId().map(|b| b.to_string()).unwrap_or_default();
        let ct = e.CurrentControlType().map(|c| c.0).unwrap_or(0);
        let class = e.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
        let mut extra = String::new();
        if let Ok(v) = e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            extra += &format!(" value={:?}", v.CurrentValue().map(|b| b.to_string()).unwrap_or_default());
        }
        if let Ok(t) = e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            extra += &format!(" toggle={:?}", t.CurrentToggleState().map(|s| s.0));
        }
        if e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId).is_ok() {
            extra += " [invoke]";
        }
        println!("{}{} name={:?} id={:?} class={:?}{}", "  ".repeat(depth), ct, name, id, class, extra);
        let mut child = w.GetFirstChildElement(e).ok();
        while let Some(c) = child {
            dump(w, &c, depth + 1);
            child = w.GetNextSiblingElement(&c).ok();
        }
    }
}
