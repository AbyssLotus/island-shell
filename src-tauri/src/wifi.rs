//! Wi-Fi networks: what's around, joining one (with its password the first time), leaving it and
//! forgetting it, through Windows' Native Wifi API (wlanapi). The radio's on/off switch is
//! radios.rs.
//!
//! Joining a new network writes a Windows profile for it, as Windows' own Wi-Fi list does: the
//! password goes in the profile, which Windows encrypts, and nowhere else. Then it connects with the
//! profile and waits up to 20 s for the connection. If it doesn't come up, a profile it had just
//! written is removed again, so a mistyped password isn't kept, and a saved network whose password
//! was being changed gets its old profile back. A saved network joins with its own profile, whatever
//! Windows named it. Networks that sign in with an account (WPA2/WPA3-Enterprise), WEP and Enhanced
//! Open are joined from Windows' own list the first time.
//!
//! On Windows 11 24H2 and later, desktop apps need location access to list networks; without it
//! Windows answers "access denied", and the page explains where to turn it on.

use std::time::{Duration, Instant};

use serde::Serialize;
use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::*;

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Network {
    pub ssid: String,
    /// 0–100.
    pub signal: u8,
    /// 0–4.
    pub bars: u8,
    /// "open", "wpa", "wpa2", "wpa3", or what the island leaves to Windows: "enterprise", "wep", "owe".
    pub security: String,
    /// Windows has a profile for it (it's been joined before).
    pub saved: bool,
    pub connected: bool,
    /// The name of Windows' profile for it (usually the SSID, not always). The page doesn't need it.
    #[serde(skip)]
    pub profile: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Wifi {
    /// A Wi-Fi adapter Windows can use.
    pub adapter: bool,
    pub networks: Vec<Network>,
}

const ACCESS_DENIED: u32 = 5;
const NOT_FOUND: u32 = 1168;
const SERVICE_NOT_ACTIVE: u32 = 1062;

/// What a Native Wifi error code means for the user.
fn problem(code: u32) -> String {
    match code {
        ACCESS_DENIED => "Windows needs location access to show Wi-Fi networks: Settings → Privacy & security → Location, turn on Let desktop apps access your location.".into(),
        SERVICE_NOT_ACTIVE => "Windows' Wi-Fi service (WLAN AutoConfig) isn't running.".into(),
        c => format!("Windows' Wi-Fi said no (error {c})."),
    }
}

/// A Native Wifi session, closed when dropped.
struct Client(HANDLE);

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, None);
        }
    }
}

fn open() -> Result<Client, String> {
    let (mut version, mut h) = (0u32, HANDLE::default());
    match unsafe { WlanOpenHandle(2, None, &mut version, &mut h) } {
        0 => Ok(Client(h)),
        c => Err(problem(c)),
    }
}

/// The Wi-Fi adapter to use: the connected one, otherwise the first. None without one.
fn interface(c: &Client) -> Result<Option<GUID>, String> {
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let code = unsafe { WlanEnumInterfaces(c.0, None, &mut list) };
    if code != 0 {
        return Err(problem(code));
    }
    let out = unsafe {
        let n = (*list).dwNumberOfItems as usize;
        let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), n);
        items.iter().find(|i| i.isState == wlan_interface_state_connected).or(items.first()).map(|i| i.InterfaceGuid)
    };
    unsafe { WlanFreeMemory(list as _) };
    Ok(out)
}

fn ssid_text(s: &DOT11_SSID) -> String {
    let n = (s.uSSIDLength as usize).min(32);
    String::from_utf8_lossy(&s.ucSSID[..n]).into_owned()
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

/// What a network asks for, from its default authentication and whether it's secured.
fn security(auth: i32, secured: bool) -> &'static str {
    match DOT11_AUTH_ALGORITHM(auth) {
        DOT11_AUTH_ALGO_80211_OPEN if !secured => "open",
        DOT11_AUTH_ALGO_80211_OPEN | DOT11_AUTH_ALGO_80211_SHARED_KEY => "wep",
        DOT11_AUTH_ALGO_WPA_PSK => "wpa",
        DOT11_AUTH_ALGO_RSNA_PSK => "wpa2",
        DOT11_AUTH_ALGO_WPA3_SAE => "wpa3",
        DOT11_AUTH_ALGO_OWE => "owe",
        _ => "enterprise",
    }
}

/// 0–100 signal quality as 0–4 bars, like Windows' own.
pub fn bars(signal: u8) -> u8 {
    match signal {
        0..=19 => 0,
        20..=39 => 1,
        40..=59 => 2,
        60..=79 => 3,
        _ => 4,
    }
}

/// Windows lists a network once per profile and once without one: one entry per name, with the
/// strongest signal, saved if any entry is, connected if any is. The connected network first, then
/// the saved ones, then by signal.
pub fn merge(mut all: Vec<Network>) -> Vec<Network> {
    let mut out: Vec<Network> = vec![];
    all.retain(|n| !n.ssid.trim().is_empty() && !n.ssid.chars().all(|c| c == '\0'));
    for n in all {
        match out.iter_mut().find(|o| o.ssid == n.ssid) {
            Some(o) => {
                if !n.profile.is_empty() && (o.profile.is_empty() || n.connected) {
                    o.profile = n.profile.clone();
                }
                o.saved |= n.saved;
                o.connected |= n.connected;
                if n.signal > o.signal {
                    o.signal = n.signal;
                    o.bars = n.bars;
                }
            }
            None => out.push(n),
        }
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.saved.cmp(&a.saved)).then(b.signal.cmp(&a.signal)).then(a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase())));
    out
}

/// The networks Windows saw on its last scan.
pub fn list() -> Result<Wifi, String> {
    let c = open()?;
    let Some(guid) = interface(&c)? else { return Ok(Wifi::default()) };
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    let code = unsafe { WlanGetAvailableNetworkList(c.0, &guid, 0, None, &mut list) };
    if code != 0 {
        return Err(problem(code));
    }
    let nets = unsafe {
        let n = (*list).dwNumberOfItems as usize;
        std::slice::from_raw_parts((*list).Network.as_ptr(), n)
            .iter()
            .map(|a| {
                let signal = a.wlanSignalQuality.min(100) as u8;
                let profile = if a.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0 { wide(&a.strProfileName) } else { String::new() };
                Network {
                    ssid: ssid_text(&a.dot11Ssid),
                    signal,
                    bars: bars(signal),
                    security: security(a.dot11DefaultAuthAlgorithm.0, a.bSecurityEnabled.as_bool()).into(),
                    saved: !profile.is_empty(),
                    connected: a.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                    profile,
                }
            })
            .collect::<Vec<_>>()
    };
    unsafe { WlanFreeMemory(list as _) };
    Ok(Wifi { adapter: true, networks: merge(nets) })
}

/// Asks the adapter to look again; the results come into list() over the next few seconds.
pub fn scan() -> Result<(), String> {
    let c = open()?;
    let Some(guid) = interface(&c)? else { return Ok(()) };
    match unsafe { WlanScan(c.0, &guid, None, None, None) } {
        0 => Ok(()),
        code => Err(problem(code)),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// A WPA passphrase is 8–63 printable characters, or the 64-hex-digit key itself.
pub fn key_kind(password: &str) -> Option<&'static str> {
    if password.len() == 64 && password.chars().all(|c| c.is_ascii_hexdigit()) {
        Some("networkKey")
    } else if (8..=63).contains(&password.chars().count()) && password.chars().all(|c| (' '..='~').contains(&c)) {
        Some("passPhrase")
    } else {
        None
    }
}

/// The Windows profile for a network: open, or WPA/WPA2/WPA3 personal with its password.
pub fn profile_xml(ssid: &str, security: &str, password: &str, auto: bool) -> Result<String, String> {
    let (auth, enc) = match security {
        "open" => ("open", "none"),
        "wpa" => ("WPAPSK", "TKIP"),
        "wpa2" => ("WPA2PSK", "AES"),
        "wpa3" => ("WPA3SAE", "AES"),
        _ => return Err("This network has to be joined from Windows' own Wi-Fi list.".into()),
    };
    let key = if security == "open" {
        String::new()
    } else {
        let kind = key_kind(password).ok_or("A Wi-Fi password is 8 to 63 characters.")?;
        format!("<sharedKey><keyType>{kind}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>", xml_escape(password))
    };
    let name = xml_escape(ssid);
    let hex: String = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    Ok(format!(
        r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>{name}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>{mode}</connectionMode><MSM><security><authEncryption><authentication>{auth}</authentication><encryption>{enc}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM></WLANProfile>"#,
        mode = if auto { "auto" } else { "manual" }
    ))
}

/// The SSID the adapter is connected to right now, if any.
fn connected_to(c: &Client, guid: &GUID) -> Option<String> {
    let (mut size, mut data) = (0u32, std::ptr::null_mut());
    let code = unsafe { WlanQueryInterface(c.0, guid, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None) };
    if code != 0 {
        return None; // ERROR_INVALID_STATE: not connected
    }
    let out = unsafe {
        let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
        (a.isState == wlan_interface_state_connected).then(|| ssid_text(&a.wlanAssociationAttributes.dot11Ssid))
    };
    unsafe { WlanFreeMemory(data) };
    out
}

/// Windows' profile named `name` on this adapter, with its key still encrypted (it can only be set
/// again on this PC), and its flags (this account's only, or set by policy).
fn get_profile(c: &Client, guid: &GUID, name: &str) -> Option<(String, u32)> {
    let (mut xml, mut flags) = (windows::core::PWSTR::null(), 0u32);
    let code = unsafe { WlanGetProfile(c.0, guid, &HSTRING::from(name), None, &mut xml, Some(&mut flags as *mut u32), None) };
    if code != 0 {
        return None;
    }
    let out = unsafe { xml.to_string() }.unwrap_or_default();
    unsafe { WlanFreeMemory(xml.0 as _) };
    Some((out, flags))
}

/// Saves a profile, replacing one with the same name: for everyone on this PC when allowed,
/// otherwise for this account only.
fn set_profile(c: &Client, guid: &GUID, xml: &str, flags: u32) -> Result<(), String> {
    let xml = HSTRING::from(xml);
    let mut reason = 0u32;
    let mut code = unsafe { WlanSetProfile(c.0, guid, flags, &xml, PCWSTR::null(), true, None, &mut reason) };
    if code == ACCESS_DENIED && flags & WLAN_PROFILE_USER == 0 {
        code = unsafe { WlanSetProfile(c.0, guid, WLAN_PROFILE_USER, &xml, PCWSTR::null(), true, None, &mut reason) };
    }
    match code {
        0 => Ok(()),
        _ => Err(format!("Windows wouldn't save the network ({}).", if reason != 0 { format!("reason {reason}") } else { format!("error {code}") })),
    }
}

/// Joins `ssid`: with its saved profile when there's no password, otherwise with a new profile for
/// it (open, or the password for WPA/WPA2/WPA3 personal). Waits until it's connected.
pub fn connect(ssid: &str, password: Option<&str>, auto: bool) -> Result<(), String> {
    let c = open()?;
    let guid = interface(&c)?.ok_or("There's no Wi-Fi adapter.")?;
    let net = list()?.networks.into_iter().find(|n| n.ssid == ssid).ok_or_else(|| format!("{ssid} isn't in range anymore."))?;
    // Without a password a saved network joins with its own profile. Otherwise the island writes
    // one named after the network, keeping any it replaces to put back if the new one fails.
    let (name, before) = if password.is_none() && !net.profile.is_empty() {
        (net.profile.clone(), None)
    } else {
        let xml = profile_xml(ssid, &net.security, password.unwrap_or(""), auto)?;
        let before = get_profile(&c, &guid, ssid);
        set_profile(&c, &guid, &xml, 0)?;
        (ssid.to_string(), Some(before))
    };
    let name = HSTRING::from(name);
    let params = WLAN_CONNECTION_PARAMETERS { wlanConnectionMode: wlan_connection_mode_profile, strProfile: PCWSTR(name.as_ptr()), pDot11Ssid: std::ptr::null_mut(), pDesiredBssidList: std::ptr::null_mut(), dot11BssType: dot11_BSS_type_any, dwFlags: 0 };
    let code = unsafe { WlanConnect(c.0, &guid, &params, None) };
    let ok = code == 0 && {
        let start = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if connected_to(&c, &guid).as_deref() == Some(ssid) {
                break true;
            }
            if start.elapsed() > Duration::from_secs(20) {
                break false;
            }
        }
    };
    if ok {
        return Ok(());
    }
    // A wrong password isn't kept: a profile it just wrote goes again, or the one it replaced comes back.
    match before {
        Some(Some((xml, flags))) => {
            let _ = set_profile(&c, &guid, &xml, flags & WLAN_PROFILE_USER);
        }
        Some(None) => unsafe {
            WlanDeleteProfile(c.0, &guid, &name, None);
        },
        None => {}
    }
    Err(if code != 0 { problem(code) } else if password.is_some() { format!("Couldn't connect to {ssid}. Check the password and try again.") } else { format!("Couldn't connect to {ssid}.") })
}

pub fn disconnect() -> Result<(), String> {
    let c = open()?;
    let guid = interface(&c)?.ok_or("There's no Wi-Fi adapter.")?;
    match unsafe { WlanDisconnect(c.0, &guid, None) } {
        0 => Ok(()),
        code => Err(problem(code)),
    }
}

/// Removes Windows' saved profile for `ssid` (its password goes with it).
pub fn forget(ssid: &str) -> Result<(), String> {
    let c = open()?;
    let guid = interface(&c)?.ok_or("There's no Wi-Fi adapter.")?;
    // Windows' name for the profile, which isn't always the network's.
    let name = list().ok().and_then(|w| w.networks.into_iter().find(|n| n.ssid == ssid)).map(|n| n.profile).filter(|p| !p.is_empty()).unwrap_or_else(|| ssid.to_string());
    match unsafe { WlanDeleteProfile(c.0, &guid, &HSTRING::from(name), None) } {
        0 | NOT_FOUND => Ok(()),
        code => Err(problem(code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(ssid: &str, signal: u8, saved: bool, connected: bool) -> Network {
        Network { ssid: ssid.into(), signal, bars: bars(signal), security: "wpa2".into(), saved, connected, profile: if saved { ssid.into() } else { String::new() } }
    }

    #[test]
    fn one_entry_per_network_connected_and_saved_first() {
        let m = merge(vec![n("Cafe", 40, false, false), n("Home", 90, false, false), n("Home", 85, true, true), n("", 99, false, false), n("Library", 70, false, false), n("Office", 30, true, false)]);
        assert_eq!(m.iter().map(|x| x.ssid.as_str()).collect::<Vec<_>>(), ["Home", "Office", "Library", "Cafe"]);
        assert!(m[0].saved && m[0].connected && m[0].signal == 90 && m[0].bars == 4);
        // The profile Windows joins it with stays with the entry, but isn't sent to the page.
        assert_eq!(m[0].profile, "Home");
        assert!(!serde_json::to_string(&m[0]).unwrap().contains("profile"));
        let named = merge(vec![n("Campus", 60, false, false), Network { profile: "Campus (school setup)".into(), ..n("Campus", 50, true, false) }]);
        assert_eq!((named.len(), named[0].saved, named[0].profile.as_str()), (1, true, "Campus (school setup)"));
    }

    #[test]
    fn what_a_network_asks_for() {
        assert_eq!(security(1, false), "open");
        assert_eq!(security(1, true), "wep");
        assert_eq!(security(4, true), "wpa");
        assert_eq!(security(7, true), "wpa2");
        assert_eq!(security(9, true), "wpa3");
        assert_eq!(security(6, true), "enterprise");
        assert_eq!(security(10, false), "owe");
        assert_eq!((bars(0), bars(19), bars(20), bars(59), bars(60), bars(100)), (0, 0, 1, 2, 3, 4));
    }

    /// Live, read-only: what Windows lists on this PC (counts, not the neighbours' names).
    /// `cargo test -p island-shell live_wifi -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a Wi-Fi adapter"]
    fn live_wifi() {
        match list() {
            Ok(w) => {
                let kinds = w.networks.iter().fold(std::collections::BTreeMap::new(), |mut m, n| { *m.entry(n.security.as_str()).or_insert(0) += 1; m });
                println!("adapter {}, {} networks, connected to {:?}, saved {}, by security {:?}", w.adapter, w.networks.len(), w.networks.iter().find(|n| n.connected).map(|n| (&n.ssid, n.bars)), w.networks.iter().filter(|n| n.saved).count(), kinds);
                // The connected network's profile: its name, and that it reads back (size and flags only).
                if let Some(n) = w.networks.iter().find(|n| n.connected) {
                    let c = open().unwrap();
                    let guid = interface(&c).unwrap().unwrap();
                    let back = get_profile(&c, &guid, &n.profile);
                    println!("profile named like its SSID: {}, reads back: {:?}", n.profile == n.ssid, back.map(|(x, f)| (x.len(), f, x.contains("<protected>true</protected>"))));
                }
            }
            Err(e) => println!("list failed: {e}"),
        }
        println!("scan: {:?}", scan());
        std::thread::sleep(Duration::from_secs(5));
        // The raw entries after the scan: authentication and cipher numbers, per network (names hashed).
        let c = open().unwrap();
        let guid = interface(&c).unwrap().unwrap();
        let mut l: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        let code = unsafe { WlanGetAvailableNetworkList(c.0, &guid, 0, None, &mut l) };
        println!("after scan: code {code}");
        if code == 0 {
            let items = unsafe { std::slice::from_raw_parts((*l).Network.as_ptr(), (*l).dwNumberOfItems as usize) };
            for a in items {
                let name = ssid_text(&a.dot11Ssid);
                let shown = if a.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0 { name.clone() } else { format!("#{}", name.len()) };
                println!("  {shown:16} auth {} cipher {} secured {} signal {} flags {} connectable {}", a.dot11DefaultAuthAlgorithm.0, a.dot11DefaultCipherAlgorithm.0, a.bSecurityEnabled.as_bool(), a.wlanSignalQuality, a.dwFlags, a.bNetworkConnectable.as_bool());
            }
            unsafe { WlanFreeMemory(l as _) };
        }
    }

    #[test]
    fn profiles() {
        let x = profile_xml("Tom & Jerry's", "wpa2", "p<a>ss\"word", true).unwrap();
        assert!(x.contains("<name>Tom &amp; Jerry&apos;s</name>"));
        assert!(x.contains("<hex>546F6D2026204A657272792773</hex>"));
        assert!(x.contains("<authentication>WPA2PSK</authentication><encryption>AES</encryption>"));
        assert!(x.contains("<keyType>passPhrase</keyType><protected>false</protected><keyMaterial>p&lt;a&gt;ss&quot;word</keyMaterial>"));
        assert!(x.contains("<connectionMode>auto</connectionMode>"));
        let open = profile_xml("Free", "open", "", false).unwrap();
        assert!(open.contains("<authentication>open</authentication><encryption>none</encryption>") && !open.contains("sharedKey") && open.contains(">manual<"));
        assert!(profile_xml("Lab", "wpa3", "correct horse", true).unwrap().contains("WPA3SAE"));
        assert!(profile_xml("Corp", "enterprise", "x", true).unwrap_err().contains("Windows' own"));
        assert!(profile_xml("Home", "wpa2", "short", true).unwrap_err().contains("8 to 63"));
        assert_eq!(key_kind(&"ab".repeat(32)), Some("networkKey"));
        assert_eq!(key_kind("emoji 🙂 pass"), None);
    }
}
