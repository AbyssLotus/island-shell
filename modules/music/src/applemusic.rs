//! Apple Music for Windows, beyond what it tells Windows' media controls.
//!
//! Apple Music's media session carries the title, the artwork, play/pause/next, and the artist and
//! album squeezed into one field ("Artist — Album"). It won't seek through Windows and doesn't
//! report shuffle or repeat. Its own player bar does all of that, so this module reads and presses
//! the bar through UI Automation (the accessibility API screen readers use): the Shuffle toggle, the
//! Repeat button, Favorite, the Lossless / Dolby Atmos badge and the seek slider. This works while
//! Apple Music is minimized or showing its mini player.
//!
//! One thread owns the UI Automation objects; everything else talks to it through `send`.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationRangeValuePattern,
    IUIAutomationTogglePattern, ToggleState_On, TreeScope_Descendants, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId,
    UIA_InvokePatternId, UIA_RangeValuePatternId, UIA_TogglePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, IsWindow};

use crate::log_line;

#[derive(Clone, Default)]
pub struct State {
    /// Apple Music's player bar was found and is being read.
    pub connected: bool,
    pub shuffle: Option<bool>,
    /// "off", "all" or "one".
    pub repeat: Option<String>,
    pub favorite: Option<bool>,
    /// "Lossless", "Hi-Res Lossless", "Dolby Atmos"…; empty when the song has no badge.
    pub badge: String,
    /// Apple Music's main window.
    pub hwnd: isize,
}

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    Shuffle,
    Repeat,
    Favorite,
    Prev,
    /// Jump to this many seconds into the song.
    Seek(f64),
}

type Reply = Sender<Result<(), String>>;

static STATE: Mutex<Option<State>> = Mutex::new(None);
static TX: OnceLock<Mutex<Sender<(Cmd, Reply)>>> = OnceLock::new();

pub fn state() -> State {
    STATE.lock().unwrap().clone().unwrap_or_default()
}

/// Presses something in Apple Music and waits (briefly) for the result.
pub fn send(cmd: Cmd) -> Result<(), String> {
    let tx = TX.get().ok_or("Apple Music control isn't running")?;
    let (rtx, rrx) = channel();
    tx.lock().unwrap().send((cmd, rtx)).map_err(|_| "Apple Music control stopped".to_string())?;
    rrx.recv_timeout(Duration::from_secs(4)).map_err(|_| "Apple Music didn't respond".to_string())?
}

pub fn spawn() {
    let (tx, rx) = channel();
    if TX.set(Mutex::new(tx)).is_ok() {
        std::thread::spawn(move || run(rx));
    }
}

/// The parts of Apple Music's player bar we use.
struct Bar {
    win: HWND,
    shuffle: Option<IUIAutomationElement>,
    repeat: Option<IUIAutomationElement>,
    prev: Option<IUIAutomationElement>,
    favorite: Option<IUIAutomationElement>,
    badge: Option<IUIAutomationElement>,
    scrubber: Option<IUIAutomationElement>,
}

fn run(rx: Receiver<(Cmd, Reply)>) {
    crate::com_mta();
    let auto: IUIAutomation = match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
        Ok(a) => a,
        Err(e) => {
            log_line(&format!("Apple Music bridge: UI Automation unavailable ({e})"));
            return;
        }
    };
    let mut bar: Option<Bar> = None;
    let mut names = String::new();
    loop {
        // Read the bar often while Apple Music is open; otherwise just look for it now and then.
        let wait = if bar.is_some() { 700 } else { 3000 };
        match rx.recv_timeout(Duration::from_millis(wait)) {
            Ok((cmd, reply)) => {
                if bar.is_none() {
                    bar = find(&auto);
                }
                let mut r = bar.as_ref().map_or(Err("Apple Music isn't open".to_string()), |b| press(b, cmd));
                if r.is_err() {
                    // The bar may have been rebuilt (window resized, app restarted): find it again once.
                    bar = find(&auto);
                    if let Some(b) = &bar {
                        r = press(b, cmd);
                    }
                }
                if let Err(e) = &r {
                    log_line(&format!("Apple Music {cmd:?} failed: {e}"));
                }
                let _ = reply.send(r);
                // Give Apple Music a moment to update the buttons before reading them again.
                std::thread::sleep(Duration::from_millis(180));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if bar.as_ref().is_some_and(|b| !unsafe { IsWindow(Some(b.win)) }.as_bool()) {
            bar = None;
        }
        if bar.is_none() {
            bar = find(&auto);
            if let Some(b) = &bar {
                log_line(&format!(
                    "Apple Music bridge connected (shuffle {}, repeat {}, favorite {}, seek {}, badge {})",
                    b.shuffle.is_some(),
                    b.repeat.is_some(),
                    b.favorite.is_some(),
                    b.scrubber.is_some(),
                    b.badge.is_some()
                ));
            }
        }
        let read = bar.as_ref().and_then(read);
        if read.is_none() && bar.is_some() {
            bar = None; // stale elements; look again next round
        }
        let (st, raw) = read.unwrap_or_default();
        // Apple Music's button names are the only record of repeat/favorite state; log their wording
        // when it changes so the parsing below can be checked against the real app.
        if !raw.is_empty() && raw != names {
            log_line(&format!("Apple Music bar: {raw}"));
            names = raw;
        }
        *STATE.lock().unwrap() = Some(st);
    }
}

/// Apple Music's main window (the mini player has no shuffle, repeat or favorite buttons).
fn main_window() -> Option<HWND> {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Option<HWND>);
        let mut buf = [0u16; 64];
        let n = GetWindowTextW(h, &mut buf);
        if String::from_utf16_lossy(&buf[..n.max(0) as usize]) == "Apple Music" {
            let n = GetClassNameW(h, &mut buf);
            if String::from_utf16_lossy(&buf[..n.max(0) as usize]) == "WinUIDesktopWin32WindowClass"
                && crate::exe_of_window(h).ends_with("\\applemusic.exe")
            {
                *out = Some(h);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

fn find(auto: &IUIAutomation) -> Option<Bar> {
    let win = main_window()?;
    unsafe {
        let root = auto.ElementFromHandle(win).ok()?;
        let cond = auto.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("TransportBar")).ok()?;
        let tb = root.FindFirst(TreeScope_Descendants, &cond).ok()?;
        let all = tb.FindAll(TreeScope_Descendants, &auto.CreateTrueCondition().ok()?).ok()?;
        let mut bar = Bar { win, shuffle: None, repeat: None, prev: None, favorite: None, badge: None, scrubber: None };
        for i in 0..all.Length().unwrap_or(0) {
            let Ok(e) = all.GetElement(i) else { continue };
            let id = e.CurrentAutomationId().map(|b| b.to_string()).unwrap_or_default();
            match id.as_str() {
                "ShuffleButton" => bar.shuffle = Some(e),
                "RepeatButton" => bar.repeat = Some(e),
                "TransportControl_SkipBack" => bar.prev = Some(e),
                "LCDScrubber" => bar.scrubber = Some(e),
                "AudioBadgeButton" => bar.badge = Some(e),
                // The Favorite button has no id, only its name.
                "" if e.CurrentControlType().is_ok_and(|t| t == UIA_ButtonControlTypeId)
                    && name(&e).is_some_and(|n| n.to_lowercase().contains("favo")) =>
                {
                    bar.favorite = Some(e)
                }
                _ => {}
            }
        }
        (bar.shuffle.is_some() || bar.scrubber.is_some()).then_some(bar)
    }
}

fn name(e: &IUIAutomationElement) -> Option<String> {
    unsafe { e.CurrentName().ok().map(|b| b.to_string()) }
}

/// "Do Not Repeat" / "Repeat" / "Repeat One" (the button is named after the current mode).
fn repeat_mode(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if l.contains("not") || l.contains("off") {
        "off"
    } else if l.contains("one") || l.contains("song") || l.contains("track") {
        "one"
    } else {
        "all"
    }
}

fn is_favorited(name: &str) -> bool {
    let l = name.to_lowercase();
    ["unfavo", "undo", "remove", "favorited", "favourited"].iter().any(|w| l.contains(w))
}

/// Reads the bar. None means an element went stale and the bar has to be found again.
fn read(b: &Bar) -> Option<(State, String)> {
    let mut st = State { connected: true, hwnd: b.win.0 as isize, ..Default::default() };
    let mut raw = vec![];
    unsafe {
        if let Some(e) = &b.shuffle {
            let t: IUIAutomationTogglePattern = e.GetCurrentPatternAs(UIA_TogglePatternId).ok()?;
            st.shuffle = Some(t.CurrentToggleState().ok()? == ToggleState_On);
        }
        if let Some(e) = &b.repeat {
            let n = name(e)?;
            st.repeat = Some(repeat_mode(&n).to_string());
            raw.push(format!("repeat {n:?}"));
        }
        if let Some(e) = &b.favorite {
            let n = name(e)?;
            st.favorite = Some(is_favorited(&n));
            raw.push(format!("favorite {n:?}"));
        }
        if let Some(e) = &b.badge {
            if !e.CurrentIsOffscreen().map(|o| o.as_bool()).unwrap_or(true) {
                st.badge = name(e)?;
            }
        }
    }
    Some((st, raw.join(", ")))
}

fn press(b: &Bar, cmd: Cmd) -> Result<(), String> {
    let missing = || format!("Apple Music has no {cmd:?} control here");
    let invoke = |e: &Option<IUIAutomationElement>| -> Result<(), String> {
        let e = e.as_ref().ok_or_else(missing)?;
        unsafe {
            let p: IUIAutomationInvokePattern = e.GetCurrentPatternAs(UIA_InvokePatternId).map_err(err)?;
            p.Invoke().map_err(err)
        }
    };
    unsafe {
        match cmd {
            Cmd::Shuffle => {
                let e = b.shuffle.as_ref().ok_or_else(missing)?;
                let t: IUIAutomationTogglePattern = e.GetCurrentPatternAs(UIA_TogglePatternId).map_err(err)?;
                t.Toggle().map_err(err)
            }
            Cmd::Repeat => invoke(&b.repeat),
            Cmd::Favorite => invoke(&b.favorite),
            Cmd::Prev => invoke(&b.prev),
            Cmd::Seek(secs) => {
                let e = b.scrubber.as_ref().ok_or_else(missing)?;
                let r: IUIAutomationRangeValuePattern = e.GetCurrentPatternAs(UIA_RangeValuePatternId).map_err(err)?;
                let max = r.CurrentMaximum().unwrap_or(f64::MAX);
                r.SetValue(secs.clamp(0.0, max)).map_err(err)
            }
        }
    }
}

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}
