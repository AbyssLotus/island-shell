//! Tailscale and Radmin VPN, an Island Shell module backend (docs/design/modules.md); its UI is
//! ui/vpn.js (a section of the Network tab, Controls tiles and a shield on the bar's network chip).
//!
//! Tailscale: its own command-line tool (`tailscale status --json`, `up`, `down`, `set`).
//! Radmin VPN has no command line or API. Its service writes a log of everything that matters
//! (service and power state, the connection to Radmin's server, our IP, every peer connecting
//! and dropping and over what kind of link), so the module reads that, plus the network names
//! the Radmin window remembers in the registry. Its power switch only exists in its own window,
//! which the module can bring forward.
//!
//! Nothing here goes on the network itself; Tailscale and Radmin do their own talking.
//!
//! RPC methods: `status`, `tailscale { action, arg }`, `radmin_open`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use island_module::{CallResult, Host, Init, Module, RpcError, Value};
use island_win::{process, shell};
use serde::Serialize;

const TS_DIR: &str = r"C:\Program Files\Tailscale";
#[cfg_attr(not(windows), allow(dead_code))]
const RV_DIR: &str = r"C:\Program Files (x86)\Radmin VPN";
const RV_LOG: &str = r"C:\ProgramData\Famatech\Radmin VPN\service.log";

#[derive(Serialize, Clone, Default)]
struct Status {
    tailscale: Option<Tailscale>,
    radmin: Option<Radmin>,
}

#[derive(Serialize, Clone, Default)]
struct Tailscale {
    /// "Running", "Stopped", "NeedsLogin", "Starting", or "NoState" when the Tailscale app isn't open.
    state: String,
    tailnet: String,
    name: String,
    ip: String,
    exit_node: String,
    health: Vec<String>,
    peers: Vec<TsPeer>,
}

#[derive(Serialize, Clone, Default)]
struct TsPeer {
    name: String,
    dns: String,
    os: String,
    ip: String,
    online: bool,
    active: bool,
    exit_option: bool,
    exit_node: bool,
    last_seen: String,
    /// "direct" or the relay (DERP) region code.
    link: String,
}

#[derive(Serialize, Clone, Default)]
struct Radmin {
    /// The Radmin VPN service is running.
    running: bool,
    /// Its power switch (in the Radmin window) is on.
    on: bool,
    /// Connected to Radmin's coordination server.
    server: bool,
    ip: String,
    networks: Vec<String>,
    peers: Vec<RvPeer>,
}

#[derive(Serialize, Clone, Default)]
struct RvPeer {
    id: String,
    name: String,
    online: bool,
    /// "Direct" (TCP/UDP) or "Relay".
    link: String,
    /// Local time the current connection started, or when the peer was last seen ("2026-10-13T15:35:25").
    since: String,
}

fn status() -> Status {
    Status { tailscale: tailscale(), radmin: radmin() }
}

// ---------- Tailscale ----------

fn ts_exe(name: &str) -> PathBuf {
    Path::new(TS_DIR).join(name)
}

fn ts(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(ts_exe("tailscale.exe"));
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run Tailscale: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { text.trim().to_string() } else { err })
    }
}

fn tailscale() -> Option<Tailscale> {
    if !ts_exe("tailscale.exe").is_file() {
        return None;
    }
    let v: Value = match ts(&["status", "--json"]) {
        Ok(s) => serde_json::from_str(&s).ok()?,
        Err(_) => return Some(Tailscale { state: "NoState".into(), ..Default::default() }),
    };
    let strs = |v: &Value| -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
    let ipv4 = |v: &Value| strs(v).into_iter().find(|ip| ip.contains('.')).unwrap_or_default();
    let mut peers: Vec<TsPeer> = v["Peer"]
        .as_object()
        .map(|m| {
            m.values()
                .map(|p| TsPeer {
                    name: p["HostName"].as_str().unwrap_or("").to_string(),
                    dns: p["DNSName"].as_str().unwrap_or("").trim_end_matches('.').to_string(),
                    os: p["OS"].as_str().unwrap_or("").to_string(),
                    ip: ipv4(&p["TailscaleIPs"]),
                    online: p["Online"].as_bool().unwrap_or(false),
                    active: p["Active"].as_bool().unwrap_or(false),
                    exit_option: p["ExitNodeOption"].as_bool().unwrap_or(false),
                    exit_node: p["ExitNode"].as_bool().unwrap_or(false),
                    last_seen: p["LastSeen"].as_str().unwrap_or("").to_string(),
                    link: if !p["CurAddr"].as_str().unwrap_or("").is_empty() {
                        "direct".into()
                    } else {
                        p["Relay"].as_str().unwrap_or("").to_string()
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    peers.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let exit_node = peers.iter().find(|p| p.exit_node).map(|p| p.name.clone()).unwrap_or_default();
    Some(Tailscale {
        state: v["BackendState"].as_str().unwrap_or("NoState").to_string(),
        tailnet: v["CurrentTailnet"]["Name"].as_str().unwrap_or("").to_string(),
        name: v["Self"]["HostName"].as_str().unwrap_or("").to_string(),
        ip: ipv4(&v["TailscaleIPs"]),
        exit_node,
        health: strs(&v["Health"]),
        peers,
    })
}

fn ts_state() -> String {
    tailscale().map(|t| t.state).unwrap_or_default()
}

/// up | down | exit (`arg` = exit node IP, or "" for none) | open
fn tailscale_cmd(action: &str, arg: &str) -> Result<(), String> {
    match action {
        "up" => {
            // The Tailscale tray app starts the backend; without it the state stays "NoState".
            if !process::running("tailscale-ipn.exe") {
                shell::command(ts_exe("tailscale-ipn.exe")).spawn().map_err(|e| format!("Couldn't start Tailscale: {e}"))?;
                let t = Instant::now();
                while t.elapsed() < Duration::from_secs(8) && matches!(ts_state().as_str(), "NoState" | "") {
                    std::thread::sleep(Duration::from_millis(400));
                }
                // It reconnects by itself when it was left connected.
                std::thread::sleep(Duration::from_millis(1200));
            }
            match ts_state().as_str() {
                "Running" | "Starting" => Ok(()),
                "NeedsLogin" => {
                    let _ = shell::command(ts_exe("tailscale-ipn.exe")).spawn();
                    Err("Tailscale needs you to sign in. Its window is opening.".into())
                }
                _ => ts(&["up", "--timeout=15s"]).map(|_| ()).map_err(|e| {
                    if e.contains("non-default") {
                        "Tailscale wants its settings confirmed. Connect from the Tailscale app this once.".to_string()
                    } else {
                        e
                    }
                }),
            }
        }
        "down" => ts(&["down"]).map(|_| ()),
        "exit" => {
            if arg.chars().any(|c| !(c.is_ascii_alphanumeric() || ".:-_".contains(c))) {
                return Err("That isn't a Tailscale device".into());
            }
            ts(&["set", &format!("--exit-node={arg}")]).map(|_| ())
        }
        "open" => shell::command(ts_exe("tailscale-ipn.exe")).spawn().map(|_| ()).map_err(|e| e.to_string()),
        _ => Err(format!("Unknown Tailscale action {action}")),
    }
}

// ---------- Radmin VPN ----------

/// Parsed log, reused until the file changes.
static RV_CACHE: Mutex<Option<(u64, SystemTime, Radmin)>> = Mutex::new(None);

fn radmin() -> Option<Radmin> {
    let meta = std::fs::metadata(RV_LOG).ok()?;
    let (len, modified) = (meta.len(), meta.modified().ok()?);
    let mut rv = {
        let mut cache = RV_CACHE.lock().unwrap();
        match cache.as_ref() {
            Some((l, m, r)) if *l == len && *m == modified => r.clone(),
            _ => {
                let r = parse_log(&std::fs::read(RV_LOG).ok()?);
                *cache = Some((len, modified, r.clone()));
                r
            }
        }
    };
    rv.networks = radmin_networks();
    Some(rv)
}

/// "2026.10.13 15:27:58.271" → "2026-10-13T15:27:58" (local time, which JS Date reads as local).
fn iso(ts: &str) -> String {
    let t = ts.split('.').take(3).collect::<Vec<_>>().join("-");
    t.split_once(' ').map(|(d, h)| format!("{d}T{}", &h[..h.len().min(8)])).unwrap_or_default()
}

/// "160529030/'THUNDERKIWI' via TCP/incoming" → ("160529030", "THUNDERKIWI", rest)
fn node(s: &str) -> Option<(String, String, &str)> {
    let (id, rest) = s.split_once("/'")?;
    let (name, rest) = rest.split_once('\'')?;
    Some((id.trim().to_string(), name.to_string(), rest))
}

fn link_kind(rest: &str) -> String {
    let l = rest.to_lowercase();
    if l.contains("relay") { "Relay".into() } else if l.contains("udp") || l.contains("tcp") { "Direct".into() } else { String::new() }
}

fn parse_log(bytes: &[u8]) -> Radmin {
    let words: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let text = String::from_utf16_lossy(&words);
    let mut rv = Radmin { on: true, ..Default::default() };
    let mut rid = String::new();
    let mut peers: Vec<RvPeer> = vec![];
    for line in text.lines() {
        let mut f = line.trim_start_matches('\u{feff}').splitn(3, '\t');
        let (Some(ts), Some(_level), Some(msg)) = (f.next(), f.next(), f.next()) else { continue };
        let msg = msg.trim();
        let when = iso(ts.trim());
        let set_offline = |peers: &mut Vec<RvPeer>| {
            for p in peers.iter_mut().filter(|p| p.online) {
                p.online = false;
                p.since = when.clone();
            }
        };
        if msg.starts_with("Service starting") {
            rv.running = true;
            rv.server = false;
            set_offline(&mut peers);
        } else if msg.starts_with("Service stopped") {
            rv.running = false;
            rv.server = false;
            set_offline(&mut peers);
        } else if msg == "Switched On" {
            rv.on = true;
        } else if msg == "Switched Off" {
            rv.on = false;
            rv.server = false;
            set_offline(&mut peers);
        } else if msg.starts_with("Connected to server") {
            rv.server = true;
        } else if msg.starts_with("Disconnected from server") {
            rv.server = false;
        } else if let Some(ip) = msg.strip_prefix("IP: ") {
            rv.ip = ip.trim().to_string();
        } else if let Some(r) = msg.strip_prefix("RID: ") {
            rid = r.trim().to_string();
        } else {
            let (prefix, online) = if let Some(r) = msg.strip_prefix("Connected to ") {
                (r, Some(true))
            } else if let Some(r) = msg.strip_prefix("Channel to ") {
                (r, None)
            } else if let Some(r) = msg.strip_prefix("Node ") {
                (r, if r.contains("' disconnected") { Some(false) } else { None })
            } else if let Some(r) = msg.strip_prefix("Failed to connect to ") {
                (r, Some(false))
            } else {
                continue;
            };
            let Some((id, name, rest)) = node(prefix) else { continue };
            let i = match peers.iter().position(|p| p.id == id) {
                Some(i) => i,
                None => {
                    peers.push(RvPeer { id: id.clone(), name: name.clone(), since: when.clone(), ..Default::default() });
                    peers.len() - 1
                }
            };
            let p = &mut peers[i];
            p.name = name;
            match online {
                Some(true) => {
                    if !p.online {
                        p.since = when.clone();
                    }
                    p.online = true;
                    p.link = link_kind(rest);
                }
                Some(false) => {
                    if p.online {
                        p.since = when.clone();
                    }
                    p.online = false;
                }
                None if msg.starts_with("Channel to ") => p.link = link_kind(rest),
                None => {}
            }
        }
    }
    peers.retain(|p| p.id != rid);
    peers.sort_by(|a, b| b.online.cmp(&a.online).then_with(|| b.since.cmp(&a.since)));
    rv.peers = peers;
    rv
}

/// Networks the Radmin window lists (it remembers each one's expanded state under this key).
#[cfg(windows)]
fn radmin_networks() -> Vec<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    let mut out = vec![];
    unsafe {
        let mut key = HKEY::default();
        let sub: Vec<u16> = "Software\\Famatech\\Radmin VPN\\ui\\networks\0".encode_utf16().collect();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), Some(0), KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..64 {
            let mut buf = [0u16; 256];
            let mut n = buf.len() as u32;
            if RegEnumKeyExW(key, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut n, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..n as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

#[cfg(not(windows))]
fn radmin_networks() -> Vec<String> {
    vec![]
}

/// Shows the Radmin VPN window (it normally hides in the tray), or starts it.
#[cfg(windows)]
fn radmin_open() -> Result<(), String> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Option<HWND>);
        let mut buf = [0u16; 128];
        let n = GetClassNameW(h, &mut buf);
        let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let n = GetWindowTextW(h, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        if class.starts_with("Qt") && class.ends_with("QWindowIcon") && title == "Radmin VPN" {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if process::path_of(pid).to_lowercase().ends_with("\\rvrvpngui.exe") {
                *out = Some(h);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
        if let Some(h) = found {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = ShowWindow(h, SW_RESTORE);
            let _ = SetForegroundWindow(h);
            return Ok(());
        }
    }
    shell::command(Path::new(RV_DIR).join("RvRvpnGui.exe")).spawn().map(|_| ()).map_err(|e| format!("Couldn't start Radmin VPN: {e}"))
}

#[cfg(not(windows))]
fn radmin_open() -> Result<(), String> {
    Err("needs Windows".into())
}

// ---------- the module ----------

struct Vpn;

impl Module for Vpn {
    fn init(&self, _host: &Host, _init: &Init) -> Result<(), RpcError> {
        Ok(())
    }

    fn call(&self, _host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "status" => Ok(serde_json::to_value(status()).unwrap_or(Value::Null)),
            "tailscale" => tailscale_cmd(params["action"].as_str().unwrap_or(""), params["arg"].as_str().unwrap_or("")).map(|_| Value::Null).map_err(RpcError::failed),
            "radmin_open" => radmin_open().map(|_| Value::Null).map_err(RpcError::failed),
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}

fn main() {
    island_module::run(Vpn);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn reads_radmin_log() {
        let log = "\u{feff}2026.10.12 10:00:00.000\tINFO\tService starting...\r\n\
            2026.10.12 10:00:00.001\tINFO\tRID: 111\r\n\
            2026.10.12 10:00:00.002\tINFO\tIP: 26.1.2.3\r\n\
            2026.10.12 10:00:05.000\tINFO\tConnected to server (16)\r\n\
            2026.10.12 10:01:00.000\tINFO\tConnected to 222/'KIWI' via TcpRelay/incoming\r\n\
            2026.10.12 10:01:01.000\tINFO\tChannel to 222/'KIWI' is replaced by TCP/incoming\r\n\
            2026.10.12 10:02:00.000\tINFO\tConnected to 333/'OWL' via TCP/outgoing\r\n\
            2026.10.12 10:03:00.000\tINFO\tNode 333/'OWL' disconnected\r\n\
            2026.10.12 10:04:00.000\tINFO\tFailed to connect to 444/'FERN' (node offline)\r\n";
        let rv = parse_log(&utf16(log));
        assert!(rv.running && rv.on && rv.server);
        assert_eq!(rv.ip, "26.1.2.3");
        let names: Vec<_> = rv.peers.iter().map(|p| (p.name.as_str(), p.online, p.link.as_str())).collect();
        assert_eq!(names[0], ("KIWI", true, "Direct"));
        assert!(names.contains(&("OWL", false, "Direct")));
        assert!(names.contains(&("FERN", false, "")));
        assert_eq!(rv.peers[0].since, "2026-10-12T10:01:00");

        let stopped = parse_log(&utf16(&format!("{log}2026.10.12 11:00:00.000\tINFO\tService stopped\r\n")));
        assert!(!stopped.running && !stopped.server && stopped.peers.iter().all(|p| !p.online));
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "vpn");
    }

    #[test]
    fn exit_nodes_are_checked() {
        assert!(tailscale_cmd("exit", "1.2.3.4; calc").unwrap_err().contains("isn't a Tailscale device"));
        assert!(tailscale_cmd("reboot", "").unwrap_err().contains("Unknown"));
    }

    /// `cargo test live_vpn_status -- --ignored --nocapture` prints what the Network tab would show.
    #[test]
    #[ignore = "needs Tailscale or Radmin VPN: prints what the Network tab would show"]
    fn live_vpn_status() {
        println!("{}", serde_json::to_string_pretty(&status()).unwrap());
    }
}
