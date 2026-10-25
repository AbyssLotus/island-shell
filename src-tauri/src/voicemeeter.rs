//! Voicemeeter (and VoiceDeck) in the Audio tab.
//!
//! The mixer is driven by VoiceDeck's own engine crate (`vd-vm-core` in audio-app/crates), the same
//! code the VoiceDeck app uses: it logs in through the Voicemeeter Remote DLL, re-reads parameters
//! only when Voicemeeter says something changed, reconnects when Voicemeeter restarts, and reads
//! meters only when asked. Voicemeeter accepts several remote clients at once, so the island and
//! VoiceDeck can both be open; a change made in either shows up in the other.
//!
//! The page gets `vm:state` (the whole mixer, or null when Voicemeeter isn't running), `vm:delta`
//! (only what changed) and `vm:levels` (meters, only while the Voicemeeter view is showing).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use vd_vm_core::{Command, Engine, Event, LevelScope, VmDelta, VmState};

use crate::util::log_line;

struct Vm {
    engine: Engine,
    state: Mutex<Option<VmState>>,
}

static VM: OnceLock<Vm> = OnceLock::new();

pub fn installed() -> bool {
    vd_vm_core::dll::locate_dll().is_some()
}

/// Starts the engine if Voicemeeter is installed (it waits quietly while Voicemeeter isn't running).
pub fn spawn(app: AppHandle) {
    if !installed() {
        log_line("voicemeeter: not installed");
        return;
    }
    let vm = VM.get_or_init(|| Vm { engine: vd_vm_core::spawn_real(), state: Mutex::new(None) });
    let events = vm.engine.events();
    std::thread::spawn(move || {
        while let Ok(ev) = events.recv() {
            let Some(vm) = VM.get() else { return };
            match ev {
                Event::Connected(s) | Event::Snapshot(s) => {
                    if vm.state.lock().unwrap().is_none() {
                        log_line(&format!("voicemeeter: connected ({:?}, {} strips, {} buses)", s.kind, s.strips.len(), s.buses.len()));
                    }
                    let _ = app.emit("vm:state", Some(&*s));
                    *vm.state.lock().unwrap() = Some(*s);
                }
                Event::Delta(d) => {
                    if let Some(s) = vm.state.lock().unwrap().as_mut() {
                        apply_delta(s, &d);
                    }
                    let _ = app.emit("vm:delta", &d);
                }
                Event::Levels(l) => {
                    let _ = app.emit("vm:levels", &l);
                }
                Event::Disconnected { reason } => {
                    if vm.state.lock().unwrap().take().is_some() {
                        log_line(&format!("voicemeeter: disconnected ({reason})"));
                    }
                    let _ = app.emit("vm:state", None::<VmState>);
                }
            }
        }
    });
}

/// Same as VoiceDeck's bridge::apply_delta: fold a change into the cached mixer.
fn apply_delta(state: &mut VmState, delta: &VmDelta) {
    for d in &delta.strips {
        let Some(s) = state.strips.get_mut(d.index) else { continue };
        if let Some(v) = &d.label {
            s.label = v.clone();
        }
        if let Some(v) = &d.device {
            s.device = v.clone();
        }
        if let Some(v) = d.gain {
            s.gain = v;
        }
        if let Some(v) = d.mute {
            s.mute = v;
        }
        if let Some(v) = d.solo {
            s.solo = v;
        }
        if let Some(v) = &d.routing {
            s.routing = v.clone();
        }
        if let Some(v) = d.fx {
            s.fx = v;
        }
    }
    for d in &delta.buses {
        let Some(b) = state.buses.get_mut(d.index) else { continue };
        if let Some(v) = &d.name {
            b.name = v.clone();
        }
        if let Some(v) = &d.device {
            b.device = v.clone();
        }
        if let Some(v) = d.gain {
            b.gain = v;
        }
        if let Some(v) = d.mute {
            b.mute = v;
        }
    }
    if let Some(r) = &delta.recorder {
        state.recorder = r.clone();
    }
}

fn send(cmd: Command) -> Result<(), String> {
    VM.get().ok_or("Voicemeeter isn't installed")?.engine.send(cmd);
    Ok(())
}

pub fn state() -> Option<VmState> {
    VM.get().and_then(|vm| vm.state.lock().unwrap().clone())
}

/// Only Strip[n] / Bus[n] gain, mute, solo and routing parameters are accepted from the page.
fn allowed(name: &str) -> bool {
    let Some((head, field)) = name.split_once("].") else { return false };
    let index_ok = |prefix: &str| head.strip_prefix(prefix).is_some_and(|n| n.parse::<usize>().is_ok_and(|n| n < 16));
    let strip_fields = ["Gain", "Mute", "Solo", "A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3"];
    (index_ok("Strip[") && strip_fields.contains(&field)) || (index_ok("Bus[") && ["Gain", "Mute"].contains(&field))
}

pub fn set(name: &str, value: f32) -> Result<(), String> {
    if !allowed(name) {
        return Err(format!("Not a mixer control: {name}"));
    }
    send(Command::SetFloat { name: name.to_string(), value })
}

/// Meters stream only while the Voicemeeter view is on screen.
pub fn levels(on: bool) -> Result<(), String> {
    send(Command::SetLevelScope(if on { LevelScope::all() } else { LevelScope::none() }))
}

pub fn restart_engine() -> Result<(), String> {
    send(Command::RestartEngine)
}

/// Mutes or unmutes strip 1, the mic, the same strip VoiceDeck's mic-mute key and tray icon use.
pub fn toggle_mic() -> Result<bool, String> {
    let muted = state().and_then(|s| s.strips.first().map(|s| s.mute)).ok_or("Voicemeeter isn't running")?;
    send(Command::SetFloat { name: "Strip[0].Mute".into(), value: if muted { 0.0 } else { 1.0 } })?;
    Ok(!muted)
}

/// VoiceDeck's exe: the running one (remembered for next time), else the installed Electron app,
/// else wherever it ran from last.
fn voicedeck_exe() -> Option<PathBuf> {
    let seen = crate::util::data_dir().join("voicedeck_path.txt");
    let running = island_win::process::path("VoiceDeck.exe");
    if let Some(p) = running.as_ref() {
        let _ = std::fs::write(&seen, p.to_string_lossy().as_bytes());
    }
    let local = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("voicedeck").join("VoiceDeck.exe"));
    let last = std::fs::read_to_string(&seen).ok().map(|s| PathBuf::from(s.trim()));
    running.into_iter().chain(local).chain(last).find(|p| p.is_file())
}

pub fn voicedeck_installed() -> bool {
    voicedeck_exe().is_some()
}

/// Opens VoiceDeck. When it's already running (it lives in the tray), launching it again hands
/// over to the running copy, which brings its window back.
pub fn open_voicedeck() -> Result<(), String> {
    let exe = voicedeck_exe().ok_or("VoiceDeck isn't installed")?;
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| format!("Couldn't open VoiceDeck: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_mixer_controls_are_writable() {
        use super::allowed;
        assert!(allowed("Strip[0].Mute"));
        assert!(allowed("Strip[7].B3"));
        assert!(allowed("Bus[5].Gain"));
        assert!(!allowed("Bus[0].A1"));
        assert!(!allowed("Command.Shutdown"));
        assert!(!allowed("Strip[0].device.wdm"));
        assert!(!allowed("Strip[99].Gain"));
        assert!(!allowed("Strip[x].Gain"));
    }
}
