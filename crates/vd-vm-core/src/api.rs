//! The Voicemeeter Remote API as a trait, so the engine can be tested against
//! a mock instead of the real DLL.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    #[error("Voicemeeter is not installed (VoicemeeterRemote64.dll not found)")]
    NotInstalled,
    #[error("loading VoicemeeterRemote64.dll: {0}")]
    Load(String),
    #[error("{func} returned {code}")]
    Call { func: &'static str, code: i32 },
    #[error("the Voicemeeter audio engine is not running")]
    EngineDown,
    #[error("unknown Voicemeeter edition: {0}")]
    UnknownKind(i32),
}

pub type Result<T> = std::result::Result<T, VmError>;

/// Which edition is installed. The number of strips and buses depends on it,
/// so everything downstream is sized from here rather than hardcoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoicemeeterKind {
    Standard,
    Banana,
    Potato,
}

impl VoicemeeterKind {
    pub fn from_raw(raw: i32) -> Result<Self> {
        match raw {
            1 => Ok(Self::Standard),
            2 => Ok(Self::Banana),
            // 4 is "Potato x64" in some builds; treat it as Potato.
            3 | 4 => Ok(Self::Potato),
            other => Err(VmError::UnknownKind(other)),
        }
    }

    /// The value `VBVMR_RunVoicemeeter` wants when launching this edition.
    pub fn run_arg(self) -> i32 {
        match self {
            Self::Standard => 1,
            Self::Banana => 2,
            Self::Potato => 3,
        }
    }

    pub fn layout(self) -> Layout {
        match self {
            Self::Standard => Layout { phys_in: 2, virt_in: 1, bus_a: 1, bus_b: 1 },
            Self::Banana => Layout { phys_in: 3, virt_in: 2, bus_a: 3, bus_b: 2 },
            Self::Potato => Layout { phys_in: 5, virt_in: 3, bus_a: 5, bus_b: 3 },
        }
    }
}

/// How many strips and buses this edition has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Hardware inputs.
    pub phys_in: usize,
    /// Virtual inputs (the ones apps can be routed into).
    pub virt_in: usize,
    /// Physical output buses (A1..).
    pub bus_a: usize,
    /// Virtual output buses (B1..).
    pub bus_b: usize,
}

impl Layout {
    pub fn strip_count(&self) -> usize {
        self.phys_in + self.virt_in
    }

    pub fn bus_count(&self) -> usize {
        self.bus_a + self.bus_b
    }

    /// Display names for the buses: `A1..A5` then `B1..B3`. Editions with a
    /// single bus of a kind call it just `A` / `B`, matching Voicemeeter.
    pub fn bus_names(&self) -> Vec<String> {
        let mut names = Vec::with_capacity(self.bus_count());
        for i in 0..self.bus_a {
            names.push(if self.bus_a > 1 { format!("A{}", i + 1) } else { "A".to_string() });
        }
        for i in 0..self.bus_b {
            names.push(if self.bus_b > 1 { format!("B{}", i + 1) } else { "B".to_string() });
        }
        names
    }

    /// Index of the first (left) level channel for a strip.
    ///
    /// Physical strips report 2 channels each; virtual strips report 8. This
    /// mirrors how Voicemeeter lays out its level array.
    pub fn strip_level_base(&self, strip: usize) -> usize {
        if strip < self.phys_in {
            strip * 2
        } else {
            self.phys_in * 2 + (strip - self.phys_in) * 8
        }
    }

    /// Index of the first level channel for a bus (8 channels each).
    pub fn bus_level_base(&self, bus: usize) -> usize {
        bus * 8
    }

    pub fn is_physical_strip(&self, strip: usize) -> bool {
        strip < self.phys_in
    }
}

/// What `VBVMR_Login` reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStatus {
    /// Logged in and the audio engine is running.
    Ok,
    /// Logged in, but Voicemeeter itself is not launched yet.
    EngineNotRunning,
    /// Already logged in on this handle.
    AlreadyLoggedIn,
}

/// Which meter to read. Matches the `nType` argument of `VBVMR_GetLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelKind {
    PreFaderInput = 0,
    PostFaderInput = 1,
    PostMuteInput = 2,
    BusOutput = 3,
}

/// The raw Voicemeeter Remote surface.
///
/// Implemented once for the real DLL and once for tests. Everything above this
/// line is pure data, so the engine logic can be exercised without Voicemeeter
/// installed at all.
pub trait VoicemeeterApi: Send {
    fn login(&self) -> Result<LoginStatus>;
    fn logout(&self) -> Result<()>;
    /// Launch the Voicemeeter GUI/engine if it is not already up.
    fn run_voicemeeter(&self, kind: VoicemeeterKind) -> Result<()>;
    fn kind(&self) -> Result<VoicemeeterKind>;
    /// True when a parameter changed since the last call. Must be polled or
    /// Voicemeeter never reports anything as dirty.
    fn is_dirty(&self) -> Result<bool>;
    fn get_float(&self, name: &str) -> Result<f32>;
    fn set_float(&self, name: &str, value: f32) -> Result<()>;
    fn get_string(&self, name: &str) -> Result<String>;
    fn set_string(&self, name: &str, value: &str) -> Result<()>;
    /// Apply many parameters in one call, e.g. `Strip[0].Gain=-6;Strip[1].Mute=1`.
    fn set_parameters(&self, script: &str) -> Result<()>;
    fn get_level(&self, kind: LevelKind, channel: usize) -> Result<f32>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potato_layout_matches_the_electron_build() {
        let l = VoicemeeterKind::Potato.layout();
        assert_eq!(l.strip_count(), 8);
        assert_eq!(l.bus_count(), 8);
        assert_eq!(l.bus_names(), vec!["A1", "A2", "A3", "A4", "A5", "B1", "B2", "B3"]);
    }

    #[test]
    fn standard_edition_uses_bare_bus_names() {
        let l = VoicemeeterKind::Standard.layout();
        assert_eq!(l.strip_count(), 3);
        assert_eq!(l.bus_names(), vec!["A", "B"]);
    }

    #[test]
    fn banana_sits_between_the_two() {
        let l = VoicemeeterKind::Banana.layout();
        assert_eq!(l.strip_count(), 5);
        assert_eq!(l.bus_names(), vec!["A1", "A2", "A3", "B1", "B2"]);
    }

    #[test]
    fn level_bases_account_for_wide_virtual_strips() {
        let l = VoicemeeterKind::Potato.layout();
        // 5 physical strips take channels 0..9 (two each).
        assert_eq!(l.strip_level_base(0), 0);
        assert_eq!(l.strip_level_base(4), 8);
        // Virtual strips are 8 channels wide and start after them.
        assert_eq!(l.strip_level_base(5), 10);
        assert_eq!(l.strip_level_base(6), 18);
        assert!(l.is_physical_strip(4));
        assert!(!l.is_physical_strip(5));
    }

    #[test]
    fn potato_x64_reports_as_potato() {
        assert_eq!(VoicemeeterKind::from_raw(4).unwrap(), VoicemeeterKind::Potato);
        assert!(VoicemeeterKind::from_raw(99).is_err());
    }
}
