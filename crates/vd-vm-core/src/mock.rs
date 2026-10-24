//! An in-memory stand-in for the Voicemeeter DLL.
//!
//! Lets the engine, state reader and diffing logic be tested on a machine with
//! no Voicemeeter installed, and lets tests drive parameter changes on demand.

use crate::api::{LevelKind, LoginStatus, Result, VmError, VoicemeeterApi, VoicemeeterKind};
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default)]
struct MockInner {
    floats: HashMap<String, f32>,
    strings: HashMap<String, String>,
    levels: HashMap<(i32, usize), f32>,
    dirty: bool,
    logged_in: bool,
    engine_running: bool,
    /// Every script handed to `set_parameters`, for asserting on batching.
    scripts: Vec<String>,
}

pub struct MockApi {
    kind: VoicemeeterKind,
    inner: Mutex<MockInner>,
}

impl MockApi {
    pub fn new(kind: VoicemeeterKind) -> Self {
        Self {
            kind,
            inner: Mutex::new(MockInner { engine_running: true, ..Default::default() }),
        }
    }

    /// Start with the engine down, to exercise the reconnect path.
    pub fn offline(kind: VoicemeeterKind) -> Self {
        Self {
            kind,
            inner: Mutex::new(MockInner::default()),
        }
    }

    pub fn set_engine_running(&self, running: bool) {
        self.inner.lock().engine_running = running;
    }

    /// Change a parameter as if the user moved it in Voicemeeter itself,
    /// which also raises the dirty flag.
    pub fn poke(&self, name: &str, value: f32) {
        let mut inner = self.inner.lock();
        inner.floats.insert(name.to_string(), value);
        inner.dirty = true;
    }

    pub fn poke_string(&self, name: &str, value: &str) {
        let mut inner = self.inner.lock();
        inner.strings.insert(name.to_string(), value.to_string());
        inner.dirty = true;
    }

    pub fn set_level(&self, kind: LevelKind, channel: usize, value: f32) {
        self.inner.lock().levels.insert((kind as i32, channel), value);
    }

    pub fn scripts(&self) -> Vec<String> {
        self.inner.lock().scripts.clone()
    }

    pub fn is_logged_in(&self) -> bool {
        self.inner.lock().logged_in
    }
}

impl VoicemeeterApi for MockApi {
    fn login(&self) -> Result<LoginStatus> {
        let mut inner = self.inner.lock();
        inner.logged_in = true;
        Ok(if inner.engine_running {
            LoginStatus::Ok
        } else {
            LoginStatus::EngineNotRunning
        })
    }

    fn logout(&self) -> Result<()> {
        self.inner.lock().logged_in = false;
        Ok(())
    }

    fn run_voicemeeter(&self, _kind: VoicemeeterKind) -> Result<()> {
        self.inner.lock().engine_running = true;
        Ok(())
    }

    fn kind(&self) -> Result<VoicemeeterKind> {
        if self.inner.lock().engine_running {
            Ok(self.kind)
        } else {
            Err(VmError::EngineDown)
        }
    }

    fn is_dirty(&self) -> Result<bool> {
        let mut inner = self.inner.lock();
        if !inner.engine_running {
            return Err(VmError::EngineDown);
        }
        // Like the real API, reading clears the flag.
        Ok(std::mem::take(&mut inner.dirty))
    }

    fn get_float(&self, name: &str) -> Result<f32> {
        let inner = self.inner.lock();
        if !inner.engine_running {
            return Err(VmError::EngineDown);
        }
        // Unknown parameters read as 0, matching a fresh Voicemeeter.
        Ok(inner.floats.get(name).copied().unwrap_or(0.0))
    }

    fn set_float(&self, name: &str, value: f32) -> Result<()> {
        let mut inner = self.inner.lock();
        if !inner.engine_running {
            return Err(VmError::EngineDown);
        }
        inner.floats.insert(name.to_string(), value);
        Ok(())
    }

    fn get_string(&self, name: &str) -> Result<String> {
        let inner = self.inner.lock();
        if !inner.engine_running {
            return Err(VmError::EngineDown);
        }
        Ok(inner.strings.get(name).cloned().unwrap_or_default())
    }

    fn set_string(&self, name: &str, value: &str) -> Result<()> {
        let mut inner = self.inner.lock();
        inner.strings.insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn set_parameters(&self, script: &str) -> Result<()> {
        let mut inner = self.inner.lock();
        if !inner.engine_running {
            return Err(VmError::EngineDown);
        }
        inner.scripts.push(script.to_string());
        // Apply the simple `name=value;` form so tests can read results back.
        for stmt in script.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            if let Some((name, value)) = stmt.split_once('=') {
                if let Ok(v) = value.trim().parse::<f32>() {
                    inner.floats.insert(name.trim().to_string(), v);
                }
            }
        }
        Ok(())
    }

    fn get_level(&self, kind: LevelKind, channel: usize) -> Result<f32> {
        let inner = self.inner.lock();
        Ok(inner.levels.get(&(kind as i32, channel)).copied().unwrap_or(0.0))
    }
}
