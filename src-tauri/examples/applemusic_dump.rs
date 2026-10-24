//! Diagnostic: dumps every media session Windows knows about, and Apple Music's UI Automation tree
//! (what applemusic.rs reads). Run it when Apple Music updates break the bridge: `applemusic_dump.exe > dump.txt`.

use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GetClassNameW};

fn main() {
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    sessions();
    uia();
}

fn sessions() {
    let mgr = Manager::RequestAsync().unwrap().get().unwrap();
    let cur = mgr.GetCurrentSession().ok().and_then(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string());
    println!("current session: {cur:?}");
    for s in mgr.GetSessions().unwrap() {
        println!("\n== session {}", s.SourceAppUserModelId().unwrap());
        if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|a| a.get()) {
            println!("  Title        {:?}", p.Title().map(|h| h.to_string()));
            println!("  Subtitle     {:?}", p.Subtitle().map(|h| h.to_string()));
            println!("  Artist       {:?}", p.Artist().map(|h| h.to_string()));
            println!("  AlbumTitle   {:?}", p.AlbumTitle().map(|h| h.to_string()));
            println!("  AlbumArtist  {:?}", p.AlbumArtist().map(|h| h.to_string()));
            println!("  TrackNumber  {:?} of {:?}", p.TrackNumber(), p.AlbumTrackCount());
            println!("  Genres       {:?}", p.Genres().map(|g| g.into_iter().map(|h| h.to_string()).collect::<Vec<_>>()));
            println!("  PlaybackType {:?}", p.PlaybackType().and_then(|v| v.Value()).map(|v| v.0));
            if let Ok(t) = p.Thumbnail() {
                if let Ok(st) = t.OpenReadAsync().and_then(|a| a.get()) {
                    println!("  Thumbnail    {} bytes, {:?}", st.Size().unwrap_or(0), st.ContentType().map(|h| h.to_string()));
                }
            } else {
                println!("  Thumbnail    none");
            }
        }
        if let Ok(i) = s.GetPlaybackInfo() {
            println!("  Status       {:?}", i.PlaybackStatus().map(|v| v.0));
            println!("  Shuffle      {:?}", i.IsShuffleActive().and_then(|v| v.Value()));
            println!("  Repeat       {:?}", i.AutoRepeatMode().and_then(|v| v.Value()).map(|v| v.0));
            println!("  Rate         {:?}", i.PlaybackRate().and_then(|v| v.Value()));
            if let Ok(c) = i.Controls() {
                println!(
                    "  Controls     play={:?} pause={:?} toggle={:?} next={:?} prev={:?} position={:?} shuffle={:?} repeat={:?} rate={:?} stop={:?}",
                    c.IsPlayEnabled(), c.IsPauseEnabled(), c.IsPlayPauseToggleEnabled(), c.IsNextEnabled(), c.IsPreviousEnabled(),
                    c.IsPlaybackPositionEnabled(), c.IsShuffleEnabled(), c.IsRepeatEnabled(), c.IsPlaybackRateEnabled(), c.IsStopEnabled()
                );
            }
        }
        if let Ok(t) = s.GetTimelineProperties() {
            let d = |r: windows::core::Result<windows::Foundation::TimeSpan>| r.map(|t| t.Duration as f64 / 1e7).unwrap_or(-1.0);
            println!(
                "  Timeline     start={} end={} pos={} minSeek={} maxSeek={} updated={:?}",
                d(t.StartTime()), d(t.EndTime()), d(t.Position()), d(t.MinSeekTime()), d(t.MaxSeekTime()),
                t.LastUpdatedTime().map(|u| u.UniversalTime)
            );
        }
    }
}

fn exe_of(h: HWND) -> String {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let _ = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(p);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

unsafe extern "system" fn enum_cb(h: HWND, l: LPARAM) -> BOOL {
    let v = &mut *(l.0 as *mut Vec<HWND>);
    if IsWindowVisible(h).as_bool() && exe_of(h).to_ascii_lowercase().ends_with("applemusic.exe") {
        v.push(h);
    }
    BOOL(1)
}

fn uia() {
    let mut wins: Vec<HWND> = vec![];
    unsafe { let _ = EnumWindows(Some(enum_cb), LPARAM(&mut wins as *mut _ as isize)); }
    println!("\n== Apple Music windows: {}", wins.len());
    let auto: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap() };
    let walker = unsafe { auto.ControlViewWalker().unwrap() };
    for h in wins {
        let mut t = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut t) };
        let mut c = [0u16; 256];
        let cn = unsafe { GetClassNameW(h, &mut c) };
        println!("-- window {:?} title {:?} class {:?}", h.0, String::from_utf16_lossy(&t[..n as usize]), String::from_utf16_lossy(&c[..cn as usize]));
        if let Ok(root) = unsafe { auto.ElementFromHandle(h) } {
            dump(&walker, &root, 0);
        }
    }
}

fn dump(w: &IUIAutomationTreeWalker, e: &IUIAutomationElement, depth: usize) {
    if depth > 40 { return; }
    unsafe {
        let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
        let id = e.CurrentAutomationId().map(|b| b.to_string()).unwrap_or_default();
        let ct = e.CurrentControlType().map(|c| c.0).unwrap_or(0);
        let class = e.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
        let mut extra = String::new();
        if let Ok(v) = e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            extra += &format!(" value={:?}", v.CurrentValue().map(|b| b.to_string()).unwrap_or_default());
        }
        if let Ok(r) = e.GetCurrentPatternAs::<IUIAutomationRangeValuePattern>(UIA_RangeValuePatternId) {
            extra += &format!(" range={:?}/{:?}..{:?}", r.CurrentValue(), r.CurrentMinimum(), r.CurrentMaximum());
        }
        if let Ok(t) = e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            extra += &format!(" toggle={:?}", t.CurrentToggleState().map(|s| s.0));
        }
        if e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId).is_ok() {
            extra += " [invoke]";
        }
        let off = e.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(false);
        println!("{}{} name={:?} id={:?} class={:?}{}{}", "  ".repeat(depth), ct, name, id, class, extra, if off { " (offscreen)" } else { "" });
        let mut child = w.GetFirstChildElement(e).ok();
        while let Some(c) = child {
            dump(w, &c, depth + 1);
            child = w.GetNextSiblingElement(&c).ok();
        }
    }
}
