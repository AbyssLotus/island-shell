//! Wi-Fi and Bluetooth radios, plus the current network connection.

use serde::Serialize;
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Networking::Connectivity::NetworkInformation;

#[derive(Serialize, Clone, Default)]
pub struct RadioInfo {
    pub present: bool,
    pub on: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct NetInfo {
    /// "wifi", "ethernet" or "none".
    pub kind: String,
    pub name: Option<String>,
    /// Signal strength in bars (0–5) for wireless connections.
    pub bars: Option<u8>,
}

pub fn radios() -> (RadioInfo, RadioInfo) {
    let (mut wifi, mut bt) = (RadioInfo::default(), RadioInfo::default());
    let Ok(list) = Radio::GetRadiosAsync().and_then(|op| op.get()) else { return (wifi, bt) };
    for i in 0..list.Size().unwrap_or(0) {
        let Ok(r) = list.GetAt(i) else { continue };
        let on = r.State().map(|s| s == RadioState::On).unwrap_or(false);
        match r.Kind() {
            Ok(RadioKind::WiFi) => {
                wifi.present = true;
                wifi.on |= on;
            }
            Ok(RadioKind::Bluetooth) => {
                bt.present = true;
                bt.on |= on;
            }
            _ => {}
        }
    }
    (wifi, bt)
}

pub fn set_radio(kind: &str, on: bool) -> Result<(), String> {
    let target = match kind {
        "wifi" => RadioKind::WiFi,
        "bluetooth" => RadioKind::Bluetooth,
        _ => return Err("Unknown radio".into()),
    };
    let e = |e: windows::core::Error| e.message().to_string();
    let access = Radio::RequestAccessAsync().and_then(|op| op.get()).map_err(e)?;
    if access != RadioAccessStatus::Allowed {
        return Err("Windows didn't allow changing radios. Check Settings > Privacy > Radios.".into());
    }
    let list = Radio::GetRadiosAsync().and_then(|op| op.get()).map_err(e)?;
    let state = if on { RadioState::On } else { RadioState::Off };
    let mut found = false;
    for i in 0..list.Size().unwrap_or(0) {
        let Ok(r) = list.GetAt(i) else { continue };
        if r.Kind().ok() == Some(target) {
            found = true;
            r.SetStateAsync(state).and_then(|op| op.get()).map_err(e)?;
        }
    }
    if found { Ok(()) } else { Err("No radio of that kind on this PC".into()) }
}

pub fn network() -> NetInfo {
    match NetworkInformation::GetInternetConnectionProfile() {
        Ok(p) => {
            let wlan = p.IsWlanConnectionProfile().unwrap_or(false);
            NetInfo {
                kind: if wlan { "wifi".into() } else { "ethernet".into() },
                name: p.ProfileName().ok().map(|h| h.to_string()).filter(|s| !s.is_empty()),
                bars: if wlan { p.GetSignalBars().ok().and_then(|r| r.Value().ok()) } else { None },
            }
        }
        Err(_) => NetInfo { kind: "none".into(), name: None, bars: None },
    }
}
