//! Voicemeeter Remote DLL wrapper and mixer state cache.
//!
//! Layered so the interesting logic is testable without Voicemeeter installed:
//!
//! - [`api`] — the DLL surface as a trait, plus edition/layout maths
//! - [`dll`] — the real `VoicemeeterRemote64.dll` behind that trait
//! - [`mock`] — an in-memory stand-in for tests
//! - [`state`] — the cached mixer state and sparse diffing
//! - [`engine`] — the single owner thread everyone talks to over channels
//!
//! Nothing here depends on Tauri.

pub mod api;
pub mod engine;
pub mod mock;
pub mod state;

#[cfg(windows)]
pub mod dll;

pub use api::{Layout, LevelKind, LoginStatus, Result, VmError, VoicemeeterApi, VoicemeeterKind};
pub use engine::{Command, Engine, Event, LevelScope, Levels};
pub use state::{
    diff, read_state, BusDelta, BusState, FxState, RecorderState, StripDelta, StripState, VmDelta,
    VmState,
};

/// Start an engine backed by the real DLL, discovering it on each connect so a
/// Voicemeeter installed *after* VoiceDeck started is still picked up.
#[cfg(windows)]
pub fn spawn_real() -> Engine {
    Engine::spawn(|| {
        let dll = dll::RemoteDll::load()?;
        Ok(Box::new(dll) as Box<dyn VoicemeeterApi>)
    })
}
