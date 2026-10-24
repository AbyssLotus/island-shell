//! The cached mixer state, how it is read, and how it is diffed.
//!
//! Reading every parameter costs one DLL call each, so the engine only does it
//! when `VBVMR_IsParametersDirty` says something moved. It then diffs against
//! the cache and emits **only the fields that changed**, which is what keeps
//! the IPC traffic near zero while the mixer sits idle.

use crate::api::{Layout, Result, VoicemeeterApi, VoicemeeterKind};
use serde::{Deserialize, Serialize};

/// Per-strip effects. `None` means this edition does not expose the knob.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FxState {
    pub comp: Option<f32>,
    pub gate: Option<f32>,
    pub denoiser: Option<f32>,
    pub eq_low: Option<f32>,
    pub eq_mid: Option<f32>,
    pub eq_high: Option<f32>,
}

/// The Voicemeeter parameter suffix for each FX knob, in `FxState` order.
const FX_PARAMS: [&str; 6] = ["Comp", "Gate", "Denoiser", "EQGain1", "EQGain2", "EQGain3"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripState {
    pub index: usize,
    /// Hardware input (a mic) rather than a virtual one apps can be routed to.
    pub physical: bool,
    pub label: String,
    /// Device name for physical strips; empty for virtual ones.
    pub device: String,
    pub gain: f32,
    pub mute: bool,
    pub solo: bool,
    /// One flag per bus, in `VmState::bus_names` order.
    pub routing: Vec<bool>,
    pub fx: FxState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusState {
    pub index: usize,
    pub name: String,
    pub device: String,
    pub gain: f32,
    pub mute: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderState {
    pub record: bool,
    pub play: bool,
    /// True when recording the bus outputs rather than the pre-fader inputs.
    pub rec_bus: bool,
    /// Which buses recorder playback is sent to.
    pub routing: Vec<bool>,
}

/// Everything the UI needs to draw the mixer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmState {
    pub kind: VoicemeeterKind,
    pub bus_names: Vec<String>,
    pub strips: Vec<StripState>,
    pub buses: Vec<BusState>,
    pub recorder: RecorderState,
}

// ---------------------------------------------------------------------------
// Sparse deltas
// ---------------------------------------------------------------------------

/// Only the strip fields that changed. `index` is always present so the
/// frontend knows which card to update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StripDelta {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing: Option<Vec<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fx: Option<FxState>,
}

impl StripDelta {
    fn has_changes(&self) -> bool {
        self.label.is_some()
            || self.device.is_some()
            || self.gain.is_some()
            || self.mute.is_some()
            || self.solo.is_some()
            || self.routing.is_some()
            || self.fx.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusDelta {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
}

impl BusDelta {
    fn has_changes(&self) -> bool {
        self.name.is_some() || self.device.is_some() || self.gain.is_some() || self.mute.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmDelta {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub strips: Vec<StripDelta>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buses: Vec<BusDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorder: Option<RecorderState>,
}

impl VmDelta {
    pub fn is_empty(&self) -> bool {
        self.strips.is_empty() && self.buses.is_empty() && self.recorder.is_none()
    }

    /// Fold a newer delta into this one, so several dirty ticks can be
    /// coalesced into a single message to the frontend.
    pub fn merge(&mut self, other: VmDelta) {
        for incoming in other.strips {
            match self.strips.iter_mut().find(|s| s.index == incoming.index) {
                Some(existing) => {
                    if incoming.label.is_some() { existing.label = incoming.label; }
                    if incoming.device.is_some() { existing.device = incoming.device; }
                    if incoming.gain.is_some() { existing.gain = incoming.gain; }
                    if incoming.mute.is_some() { existing.mute = incoming.mute; }
                    if incoming.solo.is_some() { existing.solo = incoming.solo; }
                    if incoming.routing.is_some() { existing.routing = incoming.routing; }
                    if incoming.fx.is_some() { existing.fx = incoming.fx; }
                }
                None => self.strips.push(incoming),
            }
        }
        for incoming in other.buses {
            match self.buses.iter_mut().find(|b| b.index == incoming.index) {
                Some(existing) => {
                    if incoming.name.is_some() { existing.name = incoming.name; }
                    if incoming.device.is_some() { existing.device = incoming.device; }
                    if incoming.gain.is_some() { existing.gain = incoming.gain; }
                    if incoming.mute.is_some() { existing.mute = incoming.mute; }
                }
                None => self.buses.push(incoming),
            }
        }
        if other.recorder.is_some() {
            self.recorder = other.recorder;
        }
    }
}

/// Gains are floats; ignore differences too small to see or hear.
const GAIN_EPSILON: f32 = 0.01;

fn changed(a: f32, b: f32) -> bool {
    (a - b).abs() > GAIN_EPSILON
}

fn fx_changed(a: &FxState, b: &FxState) -> bool {
    let pairs = [
        (a.comp, b.comp),
        (a.gate, b.gate),
        (a.denoiser, b.denoiser),
        (a.eq_low, b.eq_low),
        (a.eq_mid, b.eq_mid),
        (a.eq_high, b.eq_high),
    ];
    pairs.iter().any(|(x, y)| match (x, y) {
        (Some(x), Some(y)) => changed(*x, *y),
        (None, None) => false,
        _ => true,
    })
}

/// Compare two snapshots and describe only what moved.
pub fn diff(old: &VmState, new: &VmState) -> VmDelta {
    let mut delta = VmDelta::default();

    for (o, n) in old.strips.iter().zip(new.strips.iter()) {
        let mut d = StripDelta { index: n.index, ..Default::default() };
        if o.label != n.label { d.label = Some(n.label.clone()); }
        if o.device != n.device { d.device = Some(n.device.clone()); }
        if changed(o.gain, n.gain) { d.gain = Some(n.gain); }
        if o.mute != n.mute { d.mute = Some(n.mute); }
        if o.solo != n.solo { d.solo = Some(n.solo); }
        if o.routing != n.routing { d.routing = Some(n.routing.clone()); }
        if fx_changed(&o.fx, &n.fx) { d.fx = Some(n.fx); }
        if d.has_changes() { delta.strips.push(d); }
    }

    for (o, n) in old.buses.iter().zip(new.buses.iter()) {
        let mut d = BusDelta { index: n.index, ..Default::default() };
        if o.name != n.name { d.name = Some(n.name.clone()); }
        if o.device != n.device { d.device = Some(n.device.clone()); }
        if changed(o.gain, n.gain) { d.gain = Some(n.gain); }
        if o.mute != n.mute { d.mute = Some(n.mute); }
        if d.has_changes() { delta.buses.push(d); }
    }

    if old.recorder != new.recorder {
        delta.recorder = Some(new.recorder.clone());
    }

    delta
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read a float, treating "this parameter does not exist" as `None` rather
/// than an error — editions differ in which FX knobs they expose.
fn opt_float(api: &dyn VoicemeeterApi, name: &str) -> Option<f32> {
    api.get_float(name).ok()
}

fn float_or(api: &dyn VoicemeeterApi, name: &str, fallback: f32) -> f32 {
    api.get_float(name).unwrap_or(fallback)
}

fn flag(api: &dyn VoicemeeterApi, name: &str) -> bool {
    api.get_float(name).map(|v| v == 1.0).unwrap_or(false)
}

fn text(api: &dyn VoicemeeterApi, name: &str) -> String {
    api.get_string(name).unwrap_or_default()
}

/// Read the entire mixer. One DLL call per parameter, so this runs only when
/// the dirty flag is set (or on an explicit snapshot request).
pub fn read_state(api: &dyn VoicemeeterApi, kind: VoicemeeterKind) -> Result<VmState> {
    let layout: Layout = kind.layout();
    let bus_names = layout.bus_names();

    let mut strips = Vec::with_capacity(layout.strip_count());
    for i in 0..layout.strip_count() {
        let physical = layout.is_physical_strip(i);

        let routing = bus_names
            .iter()
            .map(|bus| flag(api, &format!("Strip[{i}].{bus}")))
            .collect();

        let mut fx = FxState::default();
        let values: Vec<Option<f32>> = FX_PARAMS
            .iter()
            .map(|p| opt_float(api, &format!("Strip[{i}].{p}")))
            .collect();
        fx.comp = values[0];
        fx.gate = values[1];
        fx.denoiser = values[2];
        fx.eq_low = values[3];
        fx.eq_mid = values[4];
        fx.eq_high = values[5];

        strips.push(StripState {
            index: i,
            physical,
            label: text(api, &format!("Strip[{i}].Label")),
            device: if physical { text(api, &format!("Strip[{i}].device.name")) } else { String::new() },
            gain: float_or(api, &format!("Strip[{i}].Gain"), 0.0),
            mute: flag(api, &format!("Strip[{i}].Mute")),
            solo: flag(api, &format!("Strip[{i}].Solo")),
            routing,
            fx,
        });
    }

    let mut buses = Vec::with_capacity(layout.bus_count());
    for (i, name) in bus_names.iter().enumerate() {
        buses.push(BusState {
            index: i,
            name: name.clone(),
            device: text(api, &format!("Bus[{i}].device.name")),
            gain: float_or(api, &format!("Bus[{i}].Gain"), 0.0),
            mute: flag(api, &format!("Bus[{i}].Mute")),
        });
    }

    let recorder = RecorderState {
        record: flag(api, "Recorder.record"),
        play: flag(api, "Recorder.play"),
        rec_bus: flag(api, "Recorder.mode.recbus"),
        routing: bus_names
            .iter()
            .map(|bus| flag(api, &format!("Recorder.{bus}")))
            .collect(),
    };

    Ok(VmState { kind, bus_names, strips, buses, recorder })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockApi;

    fn sample() -> VmState {
        let api = MockApi::new(VoicemeeterKind::Potato);
        read_state(&api, VoicemeeterKind::Potato).unwrap()
    }

    #[test]
    fn identical_snapshots_produce_no_delta() {
        let a = sample();
        let b = a.clone();
        assert!(diff(&a, &b).is_empty(), "an idle mixer must emit nothing");
    }

    #[test]
    fn only_the_changed_field_is_reported() {
        let a = sample();
        let mut b = a.clone();
        b.strips[2].gain = -6.0;

        let d = diff(&a, &b);
        assert_eq!(d.strips.len(), 1, "one strip changed");
        assert_eq!(d.strips[0].index, 2);
        assert_eq!(d.strips[0].gain, Some(-6.0));
        assert_eq!(d.strips[0].mute, None, "untouched fields stay absent");
        assert!(d.buses.is_empty());
        assert!(d.recorder.is_none());
    }

    #[test]
    fn tiny_gain_wobble_is_ignored() {
        let a = sample();
        let mut b = a.clone();
        b.strips[0].gain += 0.001; // float noise, not a real move
        assert!(diff(&a, &b).is_empty());

        b.strips[0].gain += 0.5; // a real move
        assert!(!diff(&a, &b).is_empty());
    }

    #[test]
    fn routing_and_recorder_changes_are_caught() {
        let a = sample();
        let mut b = a.clone();
        b.strips[5].routing[0] = !b.strips[5].routing[0];
        b.recorder.record = !b.recorder.record;

        let d = diff(&a, &b);
        assert_eq!(d.strips.len(), 1);
        assert!(d.strips[0].routing.is_some());
        assert!(d.recorder.is_some());
    }

    #[test]
    fn deltas_merge_without_losing_earlier_fields() {
        let mut first = VmDelta {
            strips: vec![StripDelta { index: 1, gain: Some(-3.0), ..Default::default() }],
            ..Default::default()
        };
        let second = VmDelta {
            strips: vec![
                StripDelta { index: 1, mute: Some(true), ..Default::default() },
                StripDelta { index: 4, gain: Some(0.0), ..Default::default() },
            ],
            ..Default::default()
        };
        first.merge(second);

        assert_eq!(first.strips.len(), 2);
        let s1 = first.strips.iter().find(|s| s.index == 1).unwrap();
        assert_eq!(s1.gain, Some(-3.0), "earlier field survives the merge");
        assert_eq!(s1.mute, Some(true), "later field is folded in");
    }

    #[test]
    fn sparse_delta_serializes_without_empty_keys() {
        let d = VmDelta {
            strips: vec![StripDelta { index: 3, mute: Some(true), ..Default::default() }],
            ..Default::default()
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"mute\":true"));
        assert!(!json.contains("gain"), "unchanged fields must not be sent: {json}");
        assert!(!json.contains("buses"), "empty collections are skipped: {json}");
    }

    #[test]
    fn reads_the_expected_shape_for_potato() {
        let s = sample();
        assert_eq!(s.strips.len(), 8);
        assert_eq!(s.buses.len(), 8);
        assert_eq!(s.strips[0].routing.len(), 8, "one flag per bus");
        assert!(s.strips[0].physical);
        assert!(!s.strips[7].physical);
        assert_eq!(s.recorder.routing.len(), 8);
    }
}
