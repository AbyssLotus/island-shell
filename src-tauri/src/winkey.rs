//! The Windows key belongs to the island.
//!
//! - Tapping Win on its own opens the island (Apps) instead of the Start menu.
//! - A few Win shortcuts whose Windows panels the island replaces open the island's version
//!   instead: Win+A (Quick Settings) → Audio, Win+G (Xbox Game Bar) → Games, Win+V (clipboard
//!   history) → Clipboard, Win+N (notifications and calendar) and Win+W (Widgets) → Today, and
//!   Win+Space (switch keyboard language) → the Command Center.
//! - Every other Win shortcut (Win+E, Win+L, Win+D, Win+Shift+S…) still reaches Windows.
//!
//! How: a low-level keyboard hook that holds Win back. A Win press goes no further than the hook.
//! Windows only hears about it once another key joins it, and then Win is sent right in front of
//! that key (one SendInput call, so they stay in order), so every Windows shortcut still works. A Win
//! tap on its own never reaches Windows, so there's nothing for Start to open on. The island's own
//! shortcuts above never reach Windows at all: the letter (its press, repeats and release) is
//! swallowed along with Win. This is AutoHotkey's prefix-key behaviour (`LWin::` plus a `LWin & …::`
//! combination). Windows doesn't know Win is down until a key joins it, so Win with only the mouse
//! doesn't count as Win held.
//!
//! Why not a mask key: the usual trick lets Win through and sends an unassigned key (vkE8) so the
//! press doesn't count as a solo tap. On this PC Start still opened whether the mask went with the
//! press or the release: input sent from inside the hook can land ahead of the key being decided, a
//! held Win's key repeats come after it, and the island taking focus at the same moment cut the
//! replayed release short. Not sending Win at all leaves nothing to get out of order.
//!
//! Keeping the hook alive: Windows silently removes a low-level hook the first time it answers
//! slower than LowLevelHooksTimeout (a busy PC, or the island paged out after a long idle), and a
//! hook installed later by another program (PowerToys, Game Bar helpers) is asked before ours. Both
//! let Win+G and Win+Space reach Windows again. So the hook thread runs at high priority and puts in
//! a fresh hook every few seconds (new one first, then the old one out), which is always alive and
//! always first in line.
//!
//! A held-back Win is the one piece of key state the hook keeps itself (Windows can't know, it never
//! saw the press). A release the hook misses (the lock screen, or while Windows had dropped the
//! hook) must not leave it stuck, or every key would turn into a Win shortcut, the way a remembered
//! "Win is down" once made plain A, G, V, N and W untypeable. So it only counts while Win keeps
//! arriving: a held key repeats every few dozen ms (after the repeat delay, a second at most), and
//! after HELD_BACK_MS with neither it's treated as released. Once Win has been sent, Windows' own key
//! state decides, and Windows resets that after the lock screen.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, VIRTUAL_KEY,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

/// Win + key → island tab ("cc" is the full-screen Command Center, on Win + Space).
const SHORTCUTS: [(u8, &str); 6] = [(b'A', "audio"), (b'G', "games"), (b'V', "clipboard"), (b'N', "today"), (b'W', "today"), (b' ', "cc")];

const VK_LWIN_U32: u32 = 0x5B;
const VK_RWIN_U32: u32 = 0x5C;
/// How long a held-back Win still counts as down with no new press or key repeat (see the module
/// notes).
const HELD_BACK_MS: u32 = 1500;
/// How often a fresh hook replaces the old one (see the module notes).
const REHOOK_MS: u32 = 3000;

/// None = a solo Win tap; Some(tab) = one of the shortcuts.
static EVENTS: OnceLock<Sender<Option<&'static str>>> = OnceLock::new();
static STATE: Mutex<Keys> = Mutex::new(Keys { held_back: None, last_win: 0, combo: false, swallowing: 0 });

/// A key exactly as the keyboard sent it, so it can be sent on to Windows later.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    vk: u32,
    scan: u32,
    extended: bool,
}

/// One key event as the hook saw it.
#[derive(Debug, Clone, Copy)]
struct Ev {
    key: Key,
    down: bool,
    /// Windows' event time in ms (wraps every 49.7 days).
    time: u32,
}

/// What the hook remembers between keys. Only ever about the press in progress.
#[derive(Debug, Default)]
struct Keys {
    /// The Win key that's down but hasn't been sent to Windows (a possible solo tap).
    held_back: Option<Key>,
    /// When that Win last went down (the press or a key repeat).
    last_win: u32,
    /// One of the island's shortcuts was used during this Win press, so it isn't a tap.
    combo: bool,
    /// The letter of a shortcut in progress, so its repeats and release are swallowed too.
    swallowing: u32,
}

#[derive(Debug, PartialEq)]
enum Action {
    /// Let the key through to Windows.
    Pass,
    /// Keep the key from Windows.
    Swallow,
    /// Keep the key from Windows: it's this shortcut.
    Shortcut(&'static str),
    /// Keep the key from Windows: a solo Win tap just ended (Windows never saw any of it).
    Tap,
    /// Keep the key from Windows and send it again right behind the held-back Win press.
    WinThenKey(Key),
}

/// The whole decision, separate from Windows so it can be tested. `win_sent` and `plain` are
/// Windows' key state from before this key (a Win key down there; no Shift, Ctrl or Alt).
fn decide(st: &mut Keys, ev: Ev, win_sent: bool, plain: bool) -> Action {
    let vk = ev.key.vk;
    let is_win = vk == VK_LWIN_U32 || vk == VK_RWIN_U32;
    // A held-back Win that stopped arriving was let go where the hook couldn't see it. (Its own
    // release still ends it normally, however long it was held.)
    if st.held_back.is_some() && (ev.down || !is_win) && ev.time.wrapping_sub(st.last_win) > HELD_BACK_MS {
        st.held_back = None;
    }
    if is_win {
        if let Some(win) = st.held_back {
            if vk != win.vk {
                return Action::Swallow; // the other Win key alongside: Windows hasn't seen either
            }
            if ev.down {
                st.last_win = ev.time; // key repeat
                return Action::Swallow;
            }
            st.held_back = None;
            return if st.combo { Action::Swallow } else { Action::Tap };
        }
        if ev.down && !win_sent {
            *st = Keys { held_back: Some(ev.key), last_win: ev.time, combo: false, swallowing: st.swallowing };
            return Action::Swallow;
        }
        // Windows already has Win down (it's in a shortcut): its repeats and release are its own.
        return Action::Pass;
    }
    if st.swallowing == vk && vk != 0 {
        if !ev.down {
            st.swallowing = 0;
            return Action::Swallow;
        }
        if st.held_back.is_some() {
            return Action::Swallow; // repeat while Win is still down
        }
        // Win is up, so that letter's release was missed: it's an ordinary key again.
        st.swallowing = 0;
    }
    if let (true, Some(win)) = (ev.down, st.held_back) {
        if plain {
            if let Some((_, tab)) = SHORTCUTS.iter().find(|(l, _)| *l as u32 == vk) {
                st.combo = true;
                st.swallowing = vk;
                return Action::Shortcut(tab);
            }
        }
        // Any other key: Windows gets Win now, right in front of it.
        st.held_back = None;
        return Action::WinThenKey(win);
    }
    Action::Pass
}

fn key_down(k: Key) -> INPUT {
    let flags = if k.extended { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(k.vk as u16), wScan: k.scan as u16, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

fn send(event: Option<&'static str>) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.send(event);
    }
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let msg = wp.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        // Our own Win + key (and anything else synthetic) is left alone.
        if kb.flags.0 & LLKHF_INJECTED.0 == 0 && (down || up) {
            let key = Key { vk: kb.vkCode, scan: kb.scanCode, extended: kb.flags.0 & LLKHF_EXTENDED.0 != 0 };
            let win_sent = held(VK_LWIN) || held(VK_RWIN);
            let plain = !held(VK_SHIFT) && !held(VK_CONTROL) && !held(VK_MENU);
            let action = match STATE.lock() {
                Ok(mut st) => decide(&mut st, Ev { key, down, time: kb.time }, win_sent, plain),
                Err(_) => Action::Pass,
            };
            match action {
                Action::Pass => {}
                Action::Swallow => return LRESULT(1),
                Action::Shortcut(tab) => {
                    send(Some(tab));
                    return LRESULT(1);
                }
                Action::Tap => {
                    send(None);
                    return LRESULT(1);
                }
                Action::WinThenKey(win) => {
                    // If they can't be sent, the key still goes through on its own rather than
                    // being lost.
                    let inputs = [key_down(win), key_down(key)];
                    if SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == inputs.len() as u32 {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Installs the hook on its own thread; `on_key` runs on another thread with None for a solo Win
/// tap or the tab of a shortcut.
pub fn spawn(on_key: impl Fn(Option<&'static str>) + Send + 'static) {
    let (tx, rx) = channel::<Option<&'static str>>();
    if EVENTS.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        while let Ok(ev) = rx.recv() {
            // Logged here, never in the hook itself (file writes there could make it time out).
            if let Some(tab) = ev {
                crate::util::log_line(&format!("Win shortcut kept from Windows → {tab}"));
            }
            on_key(ev);
        }
    });
    std::thread::spawn(|| unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let module = GetModuleHandleW(None).ok().map(|m| m.into());
        let install = || SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0);
        let mut current = match install() {
            Ok(h) => {
                crate::util::log_line("Windows key hook installed (Win, Win+A/G/V/N/W/Space), renewed every 3 s");
                h
            }
            Err(e) => {
                crate::util::log_line(&format!("Windows key hook failed: {e}"));
                return;
            }
        };
        SetTimer(None, 0, REHOOK_MS, None);
        let mut failed = false;
        // Low-level hooks are called on this thread, so it has to pump messages. Hook calls only
        // arrive while it's waiting here, so the swap below never runs both hooks for one key.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                match install() {
                    Ok(h) => {
                        let _ = UnhookWindowsHookEx(current);
                        current = h;
                        failed = false;
                    }
                    Err(e) if !failed => {
                        failed = true;
                        crate::util::log_line(&format!("Windows key hook renewal failed (keeping the old one): {e}"));
                    }
                    Err(_) => {}
                }
                continue;
            }
            DispatchMessageW(&msg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: u32 = VK_LWIN_U32;
    const A: u32 = b'A' as u32;
    const E: u32 = b'E' as u32;
    const L: u32 = b'L' as u32;
    const SPACE: u32 = b' ' as u32;
    const SHIFT: u32 = 0xA0;
    const CTRL: u32 = 0xA2;

    fn key(vk: u32) -> Key {
        Key { vk, scan: 0, extended: vk == WIN || vk == VK_RWIN_U32 }
    }

    /// One key with no Shift, Ctrl or Alt held. `win_sent`: Windows has a Win key down.
    fn d(st: &mut Keys, vk: u32, down: bool, time: u32, win_sent: bool) -> Action {
        decide(st, Ev { key: key(vk), down, time }, win_sent, true)
    }

    /// Plain typing with Win up never touches the shortcut letters.
    #[test]
    fn letters_type_normally() {
        let mut st = Keys::default();
        for vk in [A, b'G' as u32, b'V' as u32, b'N' as u32, b'W' as u32, SPACE] {
            assert_eq!(d(&mut st, vk, true, 0, false), Action::Pass);
            assert_eq!(d(&mut st, vk, false, 10, false), Action::Pass);
        }
    }

    /// A Win tap on its own, quick or held with key repeats, never reaches Windows: no Start.
    #[test]
    fn solo_tap_is_kept_from_windows() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, WIN, false, 60, false), Action::Tap);
        assert_eq!(d(&mut st, WIN, true, 1000, false), Action::Swallow);
        for t in (1500..3000).step_by(33) {
            assert_eq!(d(&mut st, WIN, true, t, false), Action::Swallow); // key repeats
        }
        assert_eq!(d(&mut st, WIN, false, 3000, false), Action::Tap);
    }

    /// Win + any other key: Windows gets Win right in front of that key, then the rest is its own.
    #[test]
    fn windows_shortcuts_get_win_first() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, E, true, 80, false), Action::WinThenKey(key(WIN)));
        assert_eq!(d(&mut st, E, false, 150, true), Action::Pass);
        assert_eq!(d(&mut st, WIN, true, 600, true), Action::Pass); // key repeat, Windows has Win down
        assert_eq!(d(&mut st, WIN, false, 700, true), Action::Pass);
        // Win held a while (repeating) before the other key.
        assert_eq!(d(&mut st, WIN, true, 1000, false), Action::Swallow);
        for t in (1500..2500).step_by(33) {
            d(&mut st, WIN, true, t, false);
        }
        assert_eq!(d(&mut st, E, true, 2510, false), Action::WinThenKey(key(WIN)));
    }

    /// Win+Shift+…, Ctrl+Win+D and Shift+Win+A stay Windows' own.
    #[test]
    fn modified_combos_are_windows_own() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, SHIFT, true, 50, false), Action::WinThenKey(key(WIN)));
        assert_eq!(decide(&mut st, Ev { key: key(A), down: true, time: 90 }, true, false), Action::Pass);

        let mut st = Keys::default();
        assert_eq!(d(&mut st, CTRL, true, 0, false), Action::Pass);
        assert_eq!(decide(&mut st, Ev { key: key(WIN), down: true, time: 40 }, false, false), Action::Swallow);
        assert_eq!(decide(&mut st, Ev { key: key(b'D' as u32), down: true, time: 90 }, false, false), Action::WinThenKey(key(WIN)));

        let mut st = Keys::default();
        assert_eq!(decide(&mut st, Ev { key: key(WIN), down: true, time: 0 }, false, false), Action::Swallow);
        assert_eq!(decide(&mut st, Ev { key: key(A), down: true, time: 50 }, false, false), Action::WinThenKey(key(WIN)));
    }

    #[test]
    fn win_a_opens_audio_and_windows_sees_none_of_it() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, A, true, 50, false), Action::Shortcut("audio"));
        assert_eq!(d(&mut st, A, true, 600, false), Action::Swallow); // key repeat
        assert_eq!(d(&mut st, A, false, 650, false), Action::Swallow); // release
        assert_eq!(d(&mut st, WIN, false, 700, false), Action::Swallow); // not a solo tap
        assert_eq!(d(&mut st, A, true, 900, false), Action::Pass); // and typing works after
    }

    #[test]
    fn win_space_opens_the_command_center_and_space_still_types() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, SPACE, true, 40, false), Action::Shortcut("cc"));
        assert_eq!(d(&mut st, SPACE, false, 90, false), Action::Swallow);
        assert_eq!(d(&mut st, WIN, false, 120, false), Action::Swallow);
        assert_eq!(d(&mut st, SPACE, true, 300, false), Action::Pass);
        assert_eq!(d(&mut st, SPACE, false, 350, false), Action::Pass);
    }

    /// Win+A, then Win+E without letting go of Win: the second one is Windows'.
    #[test]
    fn island_shortcut_then_windows_shortcut() {
        let mut st = Keys::default();
        d(&mut st, WIN, true, 0, false);
        assert_eq!(d(&mut st, A, true, 50, false), Action::Shortcut("audio"));
        assert_eq!(d(&mut st, A, false, 100, false), Action::Swallow);
        assert_eq!(d(&mut st, E, true, 200, false), Action::WinThenKey(key(WIN)));
        assert_eq!(d(&mut st, WIN, false, 300, true), Action::Pass);
    }

    #[test]
    fn letter_release_missed_then_typed_normally() {
        let mut st = Keys::default();
        d(&mut st, WIN, true, 0, false);
        assert_eq!(d(&mut st, A, true, 50, false), Action::Shortcut("audio"));
        d(&mut st, WIN, false, 100, false);
        // A's release never arrived; the next plain A must still type.
        assert_eq!(d(&mut st, A, true, 400, false), Action::Pass);
        assert_eq!(d(&mut st, A, false, 450, false), Action::Pass);
    }

    /// A held-back Win whose release the hook never saw (lock screen, dropped hook) must not turn
    /// later typing into Win shortcuts: with no presses or repeats it stops counting.
    #[test]
    fn missed_win_release_does_not_eat_letters() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, A, true, 5000, false), Action::Pass);
        assert_eq!(d(&mut st, E, true, 5100, false), Action::Pass);
        // The next real Win press starts fresh.
        assert_eq!(d(&mut st, WIN, true, 9000, false), Action::Swallow);
        assert_eq!(d(&mut st, WIN, false, 9050, false), Action::Tap);
    }

    /// Win+L: Win goes to Windows with the L, and the lock screen eats both releases. Windows resets
    /// its key state, so letters type normally after unlocking.
    #[test]
    fn win_l_then_typing_after_unlock() {
        let mut st = Keys::default();
        d(&mut st, WIN, true, 0, false);
        assert_eq!(d(&mut st, L, true, 60, false), Action::WinThenKey(key(WIN)));
        assert_eq!(d(&mut st, A, true, 20_000, false), Action::Pass);
        assert_eq!(d(&mut st, A, false, 20_050, false), Action::Pass);
    }

    /// Both Win keys at once: the second is held back with the first, and it's still one tap.
    #[test]
    fn both_win_keys() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, 0, false), Action::Swallow);
        assert_eq!(d(&mut st, VK_RWIN_U32, true, 30, false), Action::Swallow);
        assert_eq!(d(&mut st, VK_RWIN_U32, false, 60, false), Action::Swallow);
        assert_eq!(d(&mut st, WIN, false, 90, false), Action::Tap);
    }

    /// Windows' event clock wraps around every 49.7 days.
    #[test]
    fn time_wraps() {
        let mut st = Keys::default();
        assert_eq!(d(&mut st, WIN, true, u32::MAX - 20, false), Action::Swallow);
        assert_eq!(d(&mut st, E, true, 30, false), Action::WinThenKey(key(WIN)));
    }
}
