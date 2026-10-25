//! Windows services and signed-in users, for the Task Manager's Services and Users pages.
//!
//! - Services: every Win32 service with its state, process, start type, group, account and
//!   description (from the Service Control Manager; configurations are cached, states read fresh).
//!   Starting or stopping most services needs administrator rights: when Windows refuses, the same
//!   change is made through PowerShell started as administrator, so Windows asks first.
//! - Users: the sessions on this PC (Remote Desktop Services API), with who's signed in, whether the
//!   session is active, and when they signed in. Their CPU and memory come from the process sampler.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::System::RemoteDesktop::{WTSEnumerateSessionsW, WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfo, WTS_SESSION_INFOW, WTSINFOW};
use windows::Win32::System::Services::*;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::util;

#[derive(Serialize, Clone, Default)]
pub struct Service {
    pub name: String,
    pub display: String,
    pub description: String,
    /// "Running", "Stopped", "Starting", "Stopping", "Paused"…
    pub state: &'static str,
    pub pid: u32,
    /// "Automatic", "Automatic (delayed start)", "Manual", "Disabled".
    pub start: &'static str,
    /// The Service Host group it shares (svchost -k …), else its load-order group.
    pub group: String,
    pub account: String,
    pub path: String,
    /// It accepts a stop request right now.
    pub stoppable: bool,
}

#[derive(Clone, Default)]
struct Config {
    description: String,
    start: &'static str,
    group: String,
    account: String,
    path: String,
}

static CONFIGS: Mutex<Option<HashMap<String, Config>>> = Mutex::new(None);

struct Sc(SC_HANDLE);
impl Drop for Sc {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn manager(access: u32) -> Result<Sc, String> {
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access) }.map(Sc).map_err(|e| e.message())
}

fn state_name(s: SERVICE_STATUS_CURRENT_STATE) -> &'static str {
    match s {
        SERVICE_RUNNING => "Running",
        SERVICE_STOPPED => "Stopped",
        SERVICE_START_PENDING => "Starting",
        SERVICE_STOP_PENDING => "Stopping",
        SERVICE_PAUSED => "Paused",
        SERVICE_PAUSE_PENDING => "Pausing",
        SERVICE_CONTINUE_PENDING => "Resuming",
        _ => "Unknown",
    }
}

fn pw(p: PWSTR) -> String {
    if p.is_null() { String::new() } else { unsafe { p.to_string().unwrap_or_default() } }
}

fn config(scm: &Sc, name: &str) -> Config {
    let mut c = Config::default();
    unsafe {
        let Ok(h) = OpenServiceW(scm.0, &HSTRING::from(name), SERVICE_QUERY_CONFIG) else { return c };
        let h = Sc(h);
        let mut need = 0u32;
        let _ = QueryServiceConfigW(h.0, None, 0, &mut need);
        if need > 0 {
            let mut buf: Vec<u64> = vec![0; need as usize / 8 + 1];
            if QueryServiceConfigW(h.0, Some(buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW), (buf.len() * 8) as u32, &mut need).is_ok() {
                let q = &*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
                c.start = match q.dwStartType {
                    SERVICE_AUTO_START => "Automatic",
                    SERVICE_DEMAND_START => "Manual",
                    SERVICE_DISABLED => "Disabled",
                    SERVICE_BOOT_START => "Boot",
                    SERVICE_SYSTEM_START => "System",
                    _ => "",
                };
                c.path = pw(q.lpBinaryPathName);
                c.account = pw(q.lpServiceStartName);
                let l = c.path.to_ascii_lowercase();
                c.group = match l.find(" -k ") {
                    Some(i) => c.path[i + 4..].split_whitespace().next().unwrap_or("").to_string(),
                    None => pw(q.lpLoadOrderGroup),
                };
            }
        }
        if c.start == "Automatic" {
            let mut buf = [0u8; 16];
            if QueryServiceConfig2W(h.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(&mut buf), &mut need).is_ok() && buf[0] != 0 {
                c.start = "Automatic (delayed start)";
            }
        }
        need = 0;
        let _ = QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, None, &mut need);
        if need > 0 && need < 1 << 16 {
            let mut buf = vec![0u8; need as usize + 8];
            if QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, Some(&mut buf), &mut need).is_ok() {
                c.description = pw((*(buf.as_ptr() as *const SERVICE_DESCRIPTIONW)).lpDescription);
            }
        }
    }
    c
}

/// Every Win32 service. `fresh` re-reads the configurations too (start types, descriptions…).
pub fn list(fresh: bool) -> Result<Vec<Service>, String> {
    let scm = manager(SC_MANAGER_ENUMERATE_SERVICE | SC_MANAGER_CONNECT)?;
    let mut need = 0u32;
    let mut count = 0u32;
    let mut resume = 0u32;
    let mut buf = vec![0u8; 256 * 1024];
    let mut out = vec![];
    let mut cache = CONFIGS.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if fresh {
        cache.clear();
    }
    loop {
        let r = unsafe { EnumServicesStatusExW(scm.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL, Some(&mut buf), &mut need, &mut count, Some(&mut resume), PCWSTR::null()) };
        let more = r.as_ref().err().is_some_and(|e| e.code() == windows::Win32::Foundation::ERROR_MORE_DATA.to_hresult());
        if r.is_err() && !more {
            return Err(r.unwrap_err().message());
        }
        let items = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize) };
        for it in items {
            let name = pw(it.lpServiceName);
            let st = it.ServiceStatusProcess;
            let c = cache.entry(name.clone()).or_insert_with(|| config(&scm, &name)).clone();
            out.push(Service {
                display: pw(it.lpDisplayName),
                description: c.description,
                state: state_name(st.dwCurrentState),
                pid: st.dwProcessId,
                start: c.start,
                group: c.group,
                account: c.account,
                path: c.path,
                stoppable: st.dwControlsAccepted & SERVICE_ACCEPT_STOP != 0,
                name,
            });
        }
        if !more {
            break;
        }
        if need as usize > buf.len() {
            buf = vec![0u8; need as usize];
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 256 && name.chars().all(|c| c.is_alphanumeric() || " _-.$@{}".contains(c))
}

/// start | stop | restart. Returns "done", or "asked" when Windows is asking for permission.
pub fn control(name: &str, action: &str) -> Result<&'static str, String> {
    if !valid_name(name) {
        return Err("That isn't a service name".into());
    }
    let scm = manager(SC_MANAGER_CONNECT)?;
    let direct = unsafe {
        OpenServiceW(scm.0, &HSTRING::from(name), SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS).map(Sc).and_then(|h| {
            let mut st = SERVICE_STATUS::default();
            match action {
                "start" => StartServiceW(h.0, None),
                "stop" => ControlService(h.0, SERVICE_CONTROL_STOP, &mut st),
                "restart" => {
                    let _ = ControlService(h.0, SERVICE_CONTROL_STOP, &mut st);
                    // Wait (up to 15 s) for it to stop before starting it again.
                    for _ in 0..60 {
                        let mut buf = [0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
                        let mut need = 0u32;
                        if QueryServiceStatusEx(h.0, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut need).is_ok()
                            && (*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS)).dwCurrentState == SERVICE_STOPPED
                        {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    StartServiceW(h.0, None)
                }
                _ => Err(windows::core::Error::from_hresult(windows::core::HRESULT(-1))),
            }
        })
    };
    match direct {
        Ok(()) => {
            util::log_line(&format!("Task Manager: service {name} {action}"));
            Ok("done")
        }
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => {
            // Ask Windows for permission and let PowerShell, as administrator, make the change.
            let cmd = match action {
                "start" => "Start-Service",
                "stop" => "Stop-Service -Force",
                _ => "Restart-Service -Force",
            };
            let params = HSTRING::from(format!("-NoProfile -NonInteractive -WindowStyle Hidden -Command \"{cmd} -Name '{name}'\""));
            let verb = HSTRING::from("runas");
            let file = HSTRING::from("powershell.exe");
            let mut sei = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(file.as_ptr()),
                lpParameters: PCWSTR(params.as_ptr()),
                nShow: SW_HIDE.0,
                ..Default::default()
            };
            match unsafe { ShellExecuteExW(&mut sei) } {
                Ok(()) => {
                    if !sei.hProcess.is_invalid() {
                        unsafe {
                            let _ = windows::Win32::Foundation::CloseHandle(sei.hProcess);
                        }
                    }
                    util::log_line(&format!("Task Manager: service {name} {action} (as administrator)"));
                    Ok("asked")
                }
                Err(e) if e.code() == windows::Win32::Foundation::ERROR_CANCELLED.to_hresult() => Err("Cancelled".into()),
                Err(e) => Err(e.message()),
            }
        }
        Err(e) => Err(e.message()),
    }
}

/* ---------- users ---------- */

#[derive(Serialize, Clone, Default)]
pub struct Session {
    pub id: u32,
    pub user: String,
    pub domain: String,
    /// "Active", "Disconnected", "Connected"…
    pub state: &'static str,
    pub station: String,
    /// Unix ms.
    pub logon: u64,
    /// This app's own session.
    pub own: bool,
}

pub fn sessions() -> Vec<Session> {
    let mut out = vec![];
    let own = {
        let mut s = 0u32;
        unsafe {
            let _ = windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(std::process::id(), &mut s);
        }
        s
    };
    unsafe {
        let mut p: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
        let mut n = 0u32;
        if WTSEnumerateSessionsW(None, 0, 1, &mut p, &mut n).is_err() || p.is_null() {
            return out;
        }
        for s in std::slice::from_raw_parts(p, n as usize) {
            let mut b = PWSTR::null();
            let mut len = 0u32;
            if WTSQuerySessionInformationW(None, s.SessionId, WTSSessionInfo, &mut b, &mut len).is_err() || b.is_null() {
                continue;
            }
            let info = &*(b.0 as *const WTSINFOW);
            let z = |w: &[u16]| String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]);
            let user = z(&info.UserName);
            if !user.is_empty() {
                let state = match info.State.0 {
                    0 => "Active",
                    1 => "Connected",
                    4 => "Disconnected",
                    6 => "Listening",
                    _ => "Idle",
                };
                // FILETIME (100 ns since 1601) → Unix ms.
                let logon = if info.LogonTime > 0 { ((info.LogonTime - 116_444_736_000_000_000) / 10_000).max(0) as u64 } else { 0 };
                out.push(Session { id: s.SessionId, user, domain: z(&info.Domain), state, station: z(&info.WinStationName), logon, own: s.SessionId == own });
            }
            WTSFreeMemory(b.0 as *mut _);
        }
        WTSFreeMemory(p as *mut _);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a real PC: prints its services and sessions"]
    fn live() {
        let t = std::time::Instant::now();
        let s = list(false).unwrap();
        println!("{} services in {:?}", s.len(), t.elapsed());
        let t = std::time::Instant::now();
        let s2 = list(false).unwrap();
        println!("again (cached configs) {} in {:?}", s2.len(), t.elapsed());
        for x in s.iter().take(12) {
            println!("{:<28} {:<9} pid {:<6} {:<26} {:<24} {:<28} | {}", x.name, x.state, x.pid, x.start, x.group, x.account, x.description.chars().take(60).collect::<String>());
        }
        println!("{:#?}", sessions().iter().map(|s| (s.id, &s.user, s.state, s.logon, s.own)).collect::<Vec<_>>());
        assert!(s.len() > 50);
    }

    #[test]
    fn names() {
        assert!(valid_name("AudioSrv") && valid_name("CDPUserSvc_e3d07") && valid_name("MSSQL$SQLEXPRESS"));
        assert!(!valid_name("x'; rm") && !valid_name("a\"b") && !valid_name(""));
    }
}
