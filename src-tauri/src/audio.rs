//! Sound through the Core Audio APIs: the default output's volume, every output and input device
//! (and switching the default), the microphone, and each app's own volume and output level
//! through its audio sessions.

use serde::Serialize;
use windows::core::Interface;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, AudioSessionStateActive, EDataFlow, IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

#[derive(Serialize, Clone, Default)]
pub struct AudioInfo {
    pub level: f32,
    pub muted: bool,
    /// Short device name, e.g. "Speakers".
    pub device: String,
    /// Driver part of the name, e.g. "Realtek(R) Audio".
    pub detail: String,
}

fn endpoint() -> windows::core::Result<(IMMDevice, IAudioEndpointVolume)> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
        Ok((dev, vol))
    }
}

fn friendly_name(dev: &IMMDevice) -> Option<String> {
    unsafe {
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let pv = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let p = PropVariantToStringAlloc(&pv).ok()?;
        Some(crate::util::take_pwstr(p))
    }
}

pub fn info() -> Option<AudioInfo> {
    let (dev, vol) = endpoint().ok()?;
    let (level, muted) = unsafe {
        (vol.GetMasterVolumeLevelScalar().ok()?, vol.GetMute().map(|b| b.as_bool()).unwrap_or(false))
    };
    let full = friendly_name(&dev).unwrap_or_default();
    let (device, detail) = match full.find(" (") {
        Some(i) => (full[..i].to_string(), full[i + 2..].trim_end_matches(')').to_string()),
        None => (full.clone(), String::new()),
    };
    Some(AudioInfo { level, muted, device, detail })
}

pub fn set_level(level: f32) -> windows::core::Result<()> {
    let (_, vol) = endpoint()?;
    unsafe {
        vol.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null())?;
        if level > 0.0 {
            vol.SetMute(false, std::ptr::null())?;
        }
    }
    Ok(())
}

pub fn set_mute(muted: bool) -> windows::core::Result<()> {
    let (_, vol) = endpoint()?;
    unsafe { vol.SetMute(muted, std::ptr::null()) }
}

/// An app's audio sessions on every active output device, playing ones first. Apps don't always
/// play to the default device, or from their main process: Apple Music's sound comes from
/// AMPLibraryAgent.exe. `exe_names` is one or more `|`-separated image names.
fn app_sessions(exe_names: &str) -> Vec<IAudioSessionControl> {
    let mut playing = vec![];
    let mut idle = vec![];
    let want: Vec<String> = exe_names.split('|').filter(|n| !n.is_empty()).map(|n| format!("\\{n}")).collect();
    if want.is_empty() {
        return playing;
    }
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return playing };
        let Ok(devices) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return playing };
        for d in 0..devices.GetCount().unwrap_or(0) {
            let Ok(dev) = devices.Item(d) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(c) = list.GetSession(i) else { continue };
                let Ok(c2) = c.cast::<IAudioSessionControl2>() else { continue };
                let pid = c2.GetProcessId().unwrap_or(0);
                let exe = if pid == 0 { String::new() } else { crate::running::exe_of(pid) };
                if exe.is_empty() || !want.iter().any(|w| exe.ends_with(w.as_str())) {
                    continue;
                }
                if c.GetState().is_ok_and(|s| s == AudioSessionStateActive) {
                    playing.push(c);
                } else {
                    idle.push(c);
                }
            }
        }
    }
    playing.extend(idle);
    playing
}

/// The app's volume (0–1), taken from its playing session.
pub fn app_volume(exe_name: &str) -> Option<f32> {
    let s = app_sessions(exe_name).into_iter().next()?;
    unsafe { s.cast::<ISimpleAudioVolume>().ok()?.GetMasterVolume().ok() }
}

/// Sets the app's volume on all of its sessions, like the Volume mixer does.
pub fn set_app_volume(exe_name: &str, level: f32) -> Result<(), String> {
    let sessions = app_sessions(exe_name);
    if sessions.is_empty() {
        return Err("That app isn't playing any sound".into());
    }
    for s in sessions {
        unsafe {
            if let Ok(v) = s.cast::<ISimpleAudioVolume>() {
                let _ = v.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
            }
        }
    }
    Ok(())
}

/// Live output level of one app, for the equalizer bars.
pub struct Meter(Vec<IAudioMeterInformation>);

impl Meter {
    pub fn find(exe_name: &str) -> Option<Meter> {
        let meters: Vec<_> = app_sessions(exe_name).iter().filter_map(|s| s.cast::<IAudioMeterInformation>().ok()).collect();
        (!meters.is_empty()).then_some(Meter(meters))
    }
    pub fn sessions(&self) -> usize {
        self.0.len()
    }
    /// Loudest current peak (0–1) across the app's sessions.
    pub fn peak(&self) -> f32 {
        self.0.iter().filter_map(|m| unsafe { m.GetPeakValue().ok() }).fold(0.0, f32::max)
    }
}

// ---------- the Audio section: devices, microphone, per-app mixer ----------

#[derive(Serialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub default: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct Devices {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    /// The default microphone's level (0–1) and mute, when there is one.
    pub mic: Option<(f32, bool)>,
}

fn split_name(full: &str) -> (String, String) {
    match full.find(" (") {
        Some(i) => (full[..i].to_string(), full[i + 2..].trim_end_matches(')').to_string()),
        None => (full.to_string(), String::new()),
    }
}

fn device_id(dev: &IMMDevice) -> String {
    unsafe { dev.GetId().map(crate::util::take_pwstr).unwrap_or_default() }
}

fn list(en: &IMMDeviceEnumerator, flow: EDataFlow) -> Vec<Device> {
    let default = unsafe { en.GetDefaultAudioEndpoint(flow, eConsole).ok().map(|d| device_id(&d)) };
    let mut out = vec![];
    unsafe {
        let Ok(c) = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..c.GetCount().unwrap_or(0) {
            let Ok(dev) = c.Item(i) else { continue };
            let id = device_id(&dev);
            let (name, detail) = split_name(&friendly_name(&dev).unwrap_or_default());
            out.push(Device { default: default.as_deref() == Some(id.as_str()), id, name, detail });
        }
    }
    out.sort_by(|a, b| b.default.cmp(&a.default).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn mic_endpoint() -> Option<IAudioEndpointVolume> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        en.GetDefaultAudioEndpoint(eCapture, eConsole).ok()?.Activate(CLSCTX_ALL, None).ok()
    }
}

pub fn devices() -> Devices {
    let Ok(en) = (unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) }) else { return Devices::default() };
    let mic = mic_endpoint().and_then(|v| unsafe { Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().map(|b| b.as_bool()).unwrap_or(false))) });
    Devices { outputs: list(&en, eRender), inputs: list(&en, eCapture), mic }
}

pub fn set_mic(level: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    let v = mic_endpoint().ok_or("No microphone")?;
    unsafe {
        if let Some(l) = level {
            v.SetMasterVolumeLevelScalar(l.clamp(0.0, 1.0), std::ptr::null()).map_err(|e| e.message().to_string())?;
        }
        if let Some(m) = muted {
            v.SetMute(m, std::ptr::null()).map_err(|e| e.message().to_string())?;
        }
    }
    Ok(())
}

/// The interface Windows itself uses to change the default device (undocumented but stable since
/// Windows 7; the Sound settings page, EarTrumpet and SoundSwitch all go through it). Only
/// SetDefaultEndpoint is called; the rest are declared to keep the vtable in order.
mod policy {
    // Method names follow the Windows interface.
    #![allow(non_snake_case)]
    use std::ffi::c_void;
    use windows::core::{GUID, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: windows::core::IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: i32) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// Makes a device the default for everything (games and media, and calls).
    pub fn set_default(id: &str) -> Result<(), String> {
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let pc: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL).map_err(|e| e.message().to_string())?;
            for role in [eConsole, eMultimedia, eCommunications] {
                pc.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role.0).ok().map_err(|e| e.message().to_string())?;
            }
        }
        Ok(())
    }
}
pub use policy::set_default;

#[derive(Serialize, Clone)]
pub struct AppAudio {
    pub pid: u32,
    /// Lower-case path of the program ("" for system sounds).
    pub exe: String,
    pub icon: Option<String>,
    pub volume: f32,
    pub muted: bool,
    /// Current output level, 0–1.
    pub peak: f32,
    /// Playing right now.
    pub active: bool,
}

/// Every program with an audio session on any output device, merged per process (an app playing
/// to several devices shows once), the ones playing first.
pub fn apps() -> Vec<AppAudio> {
    let own = std::process::id();
    let mut out: Vec<AppAudio> = vec![];
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        let Ok(devices) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for d in 0..devices.GetCount().unwrap_or(0) {
            let Ok(dev) = devices.Item(d) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(c) = list.GetSession(i) else { continue };
                let Ok(c2) = c.cast::<IAudioSessionControl2>() else { continue };
                let pid = c2.GetProcessId().unwrap_or(0);
                if pid == own {
                    continue;
                }
                let active = c.GetState().is_ok_and(|s| s == AudioSessionStateActive);
                let (volume, muted) = c
                    .cast::<ISimpleAudioVolume>()
                    .ok()
                    .map(|v| (v.GetMasterVolume().unwrap_or(1.0), v.GetMute().map(|b| b.as_bool()).unwrap_or(false)))
                    .unwrap_or((1.0, false));
                let peak = c.cast::<IAudioMeterInformation>().ok().and_then(|m| m.GetPeakValue().ok()).unwrap_or(0.0);
                match out.iter_mut().find(|a| a.pid == pid) {
                    Some(a) => {
                        a.peak = a.peak.max(peak);
                        // The playing session speaks for the app.
                        if active && !a.active {
                            (a.volume, a.muted) = (volume, muted);
                        }
                        a.active |= active;
                    }
                    None => {
                        let exe = if pid == 0 { String::new() } else { crate::running::exe_of(pid) };
                        if pid != 0 && exe.is_empty() {
                            continue; // gone, or a process we can't look into
                        }
                        let icon = if exe.is_empty() { None } else { crate::running::exe_icon(&exe) };
                        out.push(AppAudio { pid, exe, icon, volume, muted, peak, active });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.active.cmp(&a.active).then_with(|| a.exe.cmp(&b.exe)));
    out
}

/// Sets one program's volume and/or mute on all of its sessions.
pub fn set_app(pid: u32, level: Option<f32>, muted: Option<bool>) -> Result<(), String> {
    let mut any = false;
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.message().to_string())?;
        let devices = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(|e| e.message().to_string())?;
        for d in 0..devices.GetCount().unwrap_or(0) {
            let Ok(dev) = devices.Item(d) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = mgr.GetSessionEnumerator() else { continue };
            for i in 0..list.GetCount().unwrap_or(0) {
                let Ok(c) = list.GetSession(i) else { continue };
                if c.cast::<IAudioSessionControl2>().ok().and_then(|c2| c2.GetProcessId().ok()) != Some(pid) {
                    continue;
                }
                if let Ok(v) = c.cast::<ISimpleAudioVolume>() {
                    if let Some(l) = level {
                        let _ = v.SetMasterVolume(l.clamp(0.0, 1.0), std::ptr::null());
                    }
                    if let Some(m) = muted {
                        let _ = v.SetMute(m, std::ptr::null());
                    }
                    any = true;
                }
            }
        }
    }
    if any { Ok(()) } else { Err("That app isn't playing sound anymore".into()) }
}

#[cfg(test)]
mod tests {
    /// Read-only: `cargo test live_audio -- --ignored --nocapture` lists devices and the mixer.
    #[test]
    #[ignore = "needs audio devices: prints them and the mixer"]
    fn live_audio() {
        crate::util::com_mta();
        let d = super::devices();
        for o in &d.outputs {
            println!("out {} {:<40} {}", if o.default { "*" } else { " " }, o.name, o.detail);
        }
        for i in &d.inputs {
            println!("in  {} {:<40} {}", if i.default { "*" } else { " " }, i.name, i.detail);
        }
        println!("mic {:?}", d.mic);
        for a in super::apps() {
            println!("app pid {:>6} vol {:.2} mute {} peak {:.2} active {} icon {} {}", a.pid, a.volume, a.muted, a.peak, a.active, a.icon.is_some(), a.exe);
        }
    }
}
