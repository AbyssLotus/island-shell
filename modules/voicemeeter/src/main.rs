//! Voicemeeter (and VoiceDeck), an Island Shell module backend (docs/design/modules.md); its UI is
//! ui/voicemeeter.js (a view of the Audio tab, and a mic switch on the top bar).
//!
//! The mixer is driven by VoiceDeck's own engine crate (`vd-vm-core`), the same code the VoiceDeck
//! app uses: it logs in through the Voicemeeter Remote DLL, re-reads parameters only when Voicemeeter
//! says something changed, reconnects when Voicemeeter restarts, and reads meters only when asked.
//! Voicemeeter accepts several remote clients at once, so the island and VoiceDeck can both be open;
//! a change made in either shows up in the other.
//!
//! The module runs while Voicemeeter does (and briefly when the UI asks for something without it,
//! like opening VoiceDeck). Its UI gets `state` (the whole mixer, or null when Voicemeeter isn't
//! running), `delta` (only what changed) and `levels` (meters, only while the mixer is on screen).
//!
//! RPC methods: `state`, `apps`, `set { name, value }`, `levels { on }`, `restart`, `mic`,
//! `voicedeck_open`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use island_module::{json, CallResult, Host, Init, Module, RpcError, Value};
use vd_vm_core::{Command, Engine, Event, LevelScope, VmDelta, VmState};

#[derive(Default)]
struct Voicemeeter {
    engine: OnceLock<Engine>,
    state: Arc<Mutex<Option<VmState>>>,
    /// Where VoiceDeck ran from last (data/voicedeck_path.txt).
    seen: OnceLock<PathBuf>,
}

fn installed() -> bool {
    #[cfg(windows)]
    return vd_vm_core::dll::locate_dll().is_some();
    #[cfg(not(windows))]
    false
}

impl Voicemeeter {
    /// Starts the engine (it waits quietly while Voicemeeter isn't running) and forwards its events.
    fn start(&self, host: &Host) {
        #[cfg(windows)]
        {
            let engine = self.engine.get_or_init(vd_vm_core::spawn_real);
            let events = engine.events();
            let (state, host) = (self.state.clone(), host.clone());
            std::thread::spawn(move || {
                while let Ok(ev) = events.recv() {
                    match ev {
                        Event::Connected(s) | Event::Snapshot(s) => {
                            if state.lock().unwrap().is_none() {
                                host.info(format!("connected ({:?}, {} strips, {} buses)", s.kind, s.strips.len(), s.buses.len()));
                            }
                            host.emit("state", Some(&*s));
                            *state.lock().unwrap() = Some(*s);
                        }
                        Event::Delta(d) => {
                            if let Some(s) = state.lock().unwrap().as_mut() {
                                apply_delta(s, &d);
                            }
                            host.emit("delta", &d);
                        }
                        Event::Levels(l) => host.emit("levels", &l),
                        Event::Disconnected { reason } => {
                            if state.lock().unwrap().take().is_some() {
                                host.info(format!("disconnected ({reason})"));
                            }
                            host.emit("state", None::<VmState>);
                        }
                    }
                }
            });
        }
        #[cfg(not(windows))]
        let _ = host;
    }

    fn send(&self, cmd: Command) -> Result<(), RpcError> {
        self.engine.get().ok_or_else(|| RpcError::failed("Voicemeeter isn't installed"))?.send(cmd);
        Ok(())
    }

    fn state(&self) -> Option<VmState> {
        self.state.lock().unwrap().clone()
    }

    /// Mutes or unmutes strip 1, the mic, the same strip VoiceDeck's mic-mute key and tray icon use.
    fn toggle_mic(&self) -> Result<bool, RpcError> {
        let muted = self.state().and_then(|s| s.strips.first().map(|s| s.mute)).ok_or_else(|| RpcError::failed("Voicemeeter isn't running"))?;
        self.send(Command::SetFloat { name: "Strip[0].Mute".into(), value: if muted { 0.0 } else { 1.0 } })?;
        Ok(!muted)
    }

    /// VoiceDeck's exe: the running one (remembered for next time), else the installed Electron app,
    /// else wherever it ran from last.
    fn voicedeck_exe(&self) -> Option<PathBuf> {
        let seen = self.seen.get();
        let running = island_win::process::path("VoiceDeck.exe");
        if let (Some(p), Some(seen)) = (running.as_ref(), seen) {
            let _ = std::fs::write(seen, p.to_string_lossy().as_bytes());
        }
        let local = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("voicedeck").join("VoiceDeck.exe"));
        let last = seen.and_then(|s| std::fs::read_to_string(s).ok()).map(|s| PathBuf::from(s.trim()));
        running.into_iter().chain(local).chain(last).find(|p| p.is_file())
    }

    /// Opens VoiceDeck. When it's already running (it lives in the tray), launching it again hands
    /// over to the running copy, which brings its window back.
    fn open_voicedeck(&self) -> Result<(), String> {
        let exe = self.voicedeck_exe().ok_or("VoiceDeck isn't installed")?;
        island_win::shell::command(exe).spawn().map(|_| ()).map_err(|e| format!("Couldn't open VoiceDeck: {e}"))
    }
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

/// Only Strip[n] / Bus[n] gain, mute, solo and routing parameters are accepted from the page.
fn allowed(name: &str) -> bool {
    let Some((head, field)) = name.split_once("].") else { return false };
    let index_ok = |prefix: &str| head.strip_prefix(prefix).is_some_and(|n| n.parse::<usize>().is_ok_and(|n| n < 16));
    let strip_fields = ["Gain", "Mute", "Solo", "A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3"];
    (index_ok("Strip[") && strip_fields.contains(&field)) || (index_ok("Bus[") && ["Gain", "Mute"].contains(&field))
}

impl Module for Voicemeeter {
    fn init(&self, host: &Host, _init: &Init) -> Result<(), RpcError> {
        let seen = host.data_dir().join("voicedeck_path.txt");
        if let Err(e) = host.take_legacy("voicedeck_path.txt", &seen) {
            host.warn(format!("couldn't take over voicedeck_path.txt: {e}"));
        }
        let _ = self.seen.set(seen);
        if installed() {
            self.start(host);
        } else {
            host.info("Voicemeeter isn't installed");
        }
        Ok(())
    }

    fn call(&self, _host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "state" => Ok(serde_json::to_value(self.state()).unwrap_or(Value::Null)),
            "apps" => Ok(json!({ "voicemeeter": installed(), "voicedeck": self.voicedeck_exe().is_some() })),
            "set" => {
                let name = params["name"].as_str().unwrap_or("");
                if !allowed(name) {
                    return Err(RpcError::bad_params(format!("Not a mixer control: {name}")));
                }
                let value = params["value"].as_f64().ok_or_else(|| RpcError::bad_params("value must be a number"))? as f32;
                self.send(Command::SetFloat { name: name.to_string(), value }).map(|_| Value::Null)
            }
            // Meters stream only while the mixer is on screen.
            "levels" => self.send(Command::SetLevelScope(if params["on"].as_bool().unwrap_or(false) { LevelScope::all() } else { LevelScope::none() })).map(|_| Value::Null),
            "restart" => self.send(Command::RestartEngine).map(|_| Value::Null),
            "mic" => self.toggle_mic().map(Value::Bool),
            "voicedeck_open" => self.open_voicedeck().map(|_| Value::Null).map_err(RpcError::failed),
            _ => Err(RpcError::unknown_method(method)),
        }
    }

    fn shutdown(&self, _host: &Host) {
        // Logs out of the Remote API before the process goes.
        let _ = self.send(Command::Shutdown);
    }
}

fn main() {
    island_module::run(Voicemeeter::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mixer_controls_are_writable() {
        assert!(allowed("Strip[0].Mute"));
        assert!(allowed("Strip[7].B3"));
        assert!(allowed("Bus[5].Gain"));
        assert!(!allowed("Bus[0].A1"));
        assert!(!allowed("Command.Shutdown"));
        assert!(!allowed("Strip[0].device.wdm"));
        assert!(!allowed("Strip[99].Gain"));
        assert!(!allowed("Strip[x].Gain"));
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "voicemeeter");
    }

    #[test]
    fn nothing_to_drive_without_voicemeeter() {
        let vm = Voicemeeter::default();
        assert!(vm.toggle_mic().unwrap_err().message.contains("isn't running"));
        assert!(vm.send(Command::RestartEngine).unwrap_err().message.contains("isn't installed"));
    }
}
