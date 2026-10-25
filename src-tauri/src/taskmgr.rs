//! Task Manager for the Command Center (cc-taskmgr.js): processes grouped the way Windows' Task
//! Manager groups them, live performance numbers, and startup apps.
//!
//! - Processes come from one NtQuerySystemInformation call per sample: CPU time, private working
//!   set (Task Manager's Memory column), I/O byte counts, threads and handles, and each thread's
//!   state (a process whose threads are all suspended is Suspended). It needs no admin rights and
//!   sees every process, protected ones included. GPU per process is the "GPU Engine" performance
//!   counter, the one Task Manager reads. Network per process needs an administrator's event trace,
//!   so there's no such column. Who runs a process and its command line are read once per process
//!   (Windows refuses both for some system processes without admin rights).
//! - Each logical processor's kernel and user time come from the same call (processor performance
//!   information), for the kernel line in the graphs.
//! - A process joins its parent's group when it runs the same program (Firefox's content
//!   processes) or is a helper (WebView2, console hosts). A group with a taskbar window is an App;
//!   one that's a service or lives in the Windows folder is a Windows process; the rest are
//!   Background processes. Groups of one program are then merged (all the Service Hosts are one row).
//! - Sampling runs once a second, and only while the window keeps asking (tm_snapshot within 4 s).
//! - Ending a process re-checks that each PID still belongs to the process that was shown (PIDs get
//!   reused) and refuses Island Shell itself, services and the processes Windows can't run without.
//! - App history: Windows keeps its own in a database only administrators can read, so the island
//!   keeps one: every 30 seconds (one process list, a few milliseconds) each program's CPU time,
//!   reads, writes and launches are added up, and saved to app_history.json every 10 minutes.
//! - Startup apps: the Run keys (yours, the machine's, the 32-bit one) and both Startup folders, with
//!   the on/off state Windows keeps under Explorer\StartupApproved. Only your own entries can be
//!   switched without administrator rights; the machine-wide ones are shown read-only.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation, SYSTEM_INFORMATION_CLASS};
use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, FILETIME, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, LookupAccountSidW, TokenUser, PSID, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegEnumValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
    KEY_SET_VALUE, REG_BINARY, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, SetPriorityClass, SetProcessInformation, TerminateProcess, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE, PROCESS_SET_INFORMATION,
    PROCESS_TERMINATE, ProcessPowerThrottling,
};

use crate::stats::{array, Query};
use crate::{places, running, util};

/// Programs that run inside another app's group (their parent's) instead of one of their own.
const HELPERS: [&str; 2] = ["msedgewebview2.exe", "conhost.exe"];
/// Ending these would crash or sign out Windows (or Windows won't allow it anyway).
const CRITICAL: [&str; 17] = [
    "system", "registry", "secure system", "memory compression", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "lsaiso.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "sgrmbroker.exe", "msmpeng.exe", "lsm.exe",
];
/// Windows' own processes that live in the user's session.
const WINDOWS_NAMES: [&str; 6] = ["system", "registry", "secure system", "memory compression", "csrss.exe", "winlogon.exe"];
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

#[derive(Serialize, Clone)]
pub struct ProcRow {
    pub pid: u32,
    /// Creation time in ms (FILETIME / 10 000, small enough for JavaScript): with the PID, names
    /// this exact process.
    pub created: i64,
    pub image: String,
    pub cpu: f64,
    pub mem: u64,
    pub io: f64,
    /// Bytes read and written per second (to disks, devices and the network stack, as Windows counts I/O).
    pub read: f64,
    pub write: f64,
    pub gpu: f64,
    pub threads: u32,
    pub handles: u32,
    /// Running at idle priority (Efficiency mode).
    pub eco: bool,
    pub ppid: u32,
    /// The Windows session it runs in (0 for services).
    pub session: u32,
    /// Every thread is suspended (a Store app Windows has paused, or a debugger's doing).
    pub suspended: bool,
    /// "DOMAIN\\user" without the domain when it's this PC; empty when Windows won't say.
    pub user: String,
    /// The command line it was started with (cut at 300 characters); empty when Windows won't say.
    pub cmd: String,
}

#[derive(Serialize, Clone)]
pub struct Group {
    /// "kind:program", the same from one sample to the next.
    pub id: String,
    pub name: String,
    pub image: String,
    /// Full path of the program, empty when Windows won't say (services, protected processes).
    pub exe: String,
    pub icon: Option<String>,
    /// "app", "background" or "windows".
    pub kind: &'static str,
    pub cpu: f64,
    pub mem: u64,
    pub io: f64,
    pub read: f64,
    pub write: f64,
    pub gpu: f64,
    pub threads: u32,
    pub handles: u32,
    /// Whoever runs its first process.
    pub user: String,
    /// Every process in it is suspended.
    pub suspended: bool,
    /// Every process in it is in Efficiency mode.
    pub eco: bool,
    /// At least one is (browsers put their background tabs there themselves).
    pub eco_some: bool,
    /// Why it can't be ended or changed from here.
    pub locked: Option<&'static str>,
    pub procs: Vec<ProcRow>,
}

#[derive(Serialize, Clone, Default)]
pub struct DiskPerf {
    /// "0 C: D:" (the disk number, then its drive letters).
    pub name: String,
    pub active: f64,
    pub read: f64,
    pub write: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct NetPerf {
    pub name: String,
    pub rx: f64,
    pub tx: f64,
    /// Link speed, bits per second.
    pub bandwidth: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct Perf {
    pub cpu: f64,
    pub cores: Vec<f64>,
    /// Time spent in the kernel, the whole processor and each logical processor (percent).
    pub kernel: f64,
    pub cores_kernel: Vec<f64>,
    /// Effective clock (MHz), on average and for each logical processor.
    pub mhz: f64,
    pub cores_mhz: Vec<f64>,
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub cached: u64,
    pub paged: u64,
    pub nonpaged: u64,
    pub gpu: f64,
    /// Busiest engine types: ("3D", 41.0), ("VideoDecode", 3.0)…
    pub gpu_engines: Vec<(String, f64)>,
    pub disks: Vec<DiskPerf>,
    pub nets: Vec<NetPerf>,
}

#[derive(Serialize, Clone, Default)]
pub struct Snapshot {
    pub at: u64,
    /// How many samples have been taken since sampling started.
    pub gen: u64,
    pub groups: Vec<Group>,
    pub perf: Perf,
}

/* ---------- reading every process at once ---------- */

#[repr(C)]
struct UnicodeString {
    len: u16,
    max: u16,
    buf: *const u16,
}

/// SYSTEM_PROCESS_INFORMATION, in full (the SDK's copy hides most fields).
#[repr(C)]
struct Spi {
    next: u32,
    threads: u32,
    ws_private: i64,
    hard_faults: u32,
    threads_peak: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image: UnicodeString,
    base_priority: i32,
    pid: usize,
    ppid: usize,
    handles: u32,
    session: u32,
    key: usize,
    peak_virtual: usize,
    virtual_size: usize,
    page_faults: u32,
    peak_ws: usize,
    ws: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_nonpaged: usize,
    quota_nonpaged: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private_pages: usize,
    read_ops: i64,
    write_ops: i64,
    other_ops: i64,
    read_bytes: i64,
    write_bytes: i64,
    other_bytes: i64,
}

/// SYSTEM_THREAD_INFORMATION, which follows each process entry once per thread.
#[repr(C)]
struct Sti {
    kernel_time: i64,
    user_time: i64,
    create_time: i64,
    wait_time: u32,
    start: usize,
    client_pid: usize,
    client_tid: usize,
    priority: i32,
    base_priority: i32,
    switches: u32,
    state: u32,
    wait_reason: u32,
}
const THREAD_WAITING: u32 = 5;
const WAIT_SUSPENDED: u32 = 5;

struct Raw {
    pid: u32,
    ppid: u32,
    created: i64,
    /// User + kernel time, 100 ns units.
    cpu: i64,
    ws_private: u64,
    read: u64,
    write: u64,
    suspended: bool,
    threads: u32,
    handles: u32,
    session: u32,
    priority: i32,
    image: String,
}

fn raw_processes() -> Vec<Raw> {
    let mut buf: Vec<u64> = vec![0; 1 << 16];
    for _ in 0..4 {
        let mut need = 0u32;
        let st = unsafe { NtQuerySystemInformation(SystemProcessInformation, buf.as_mut_ptr() as *mut _, (buf.len() * 8) as u32, &mut need) };
        if st.0 == STATUS_INFO_LENGTH_MISMATCH {
            buf = vec![0; need as usize / 8 + 16384];
            continue;
        }
        if st.0 < 0 {
            return vec![];
        }
        let base = buf.as_ptr() as *const u8;
        let (mut off, mut out) = (0usize, Vec::with_capacity(400));
        loop {
            // SAFETY: Windows filled `buf` with a chain of entries; `next` is the offset to the next one.
            let p = unsafe { &*(base.add(off) as *const Spi) };
            let image = if p.image.buf.is_null() || p.image.len == 0 {
                String::new()
            } else {
                String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p.image.buf, p.image.len as usize / 2) })
            };
            // SAFETY: `threads` thread entries follow the process entry.
            let ths = unsafe { std::slice::from_raw_parts(base.add(off + std::mem::size_of::<Spi>()) as *const Sti, p.threads as usize) };
            out.push(Raw {
                pid: p.pid as u32,
                ppid: p.ppid as u32,
                created: p.create_time,
                cpu: p.user_time + p.kernel_time,
                ws_private: p.ws_private.max(0) as u64,
                read: p.read_bytes.max(0) as u64,
                write: p.write_bytes.max(0) as u64,
                suspended: !ths.is_empty() && ths.iter().all(|t| t.state == THREAD_WAITING && t.wait_reason == WAIT_SUSPENDED),
                threads: p.threads,
                handles: p.handles,
                session: p.session,
                priority: p.base_priority,
                image,
            });
            if p.next == 0 {
                break;
            }
            off += p.next as usize;
        }
        return out;
    }
    vec![]
}

/// `root` and every process started under it (its WebView2 browser, GPU, renderers…), with each
/// one's private working set in bytes. PIDs are matched with creation times so a reused PID isn't
/// mistaken for a child.
pub(crate) fn tree(root: u32) -> Vec<(u32, u64)> {
    let all = raw_processes();
    let created: HashMap<u32, i64> = all.iter().map(|r| (r.pid, r.created)).collect();
    let mut inside: HashMap<u32, bool> = HashMap::new();
    inside.insert(root, true);
    // Parents usually come before children in the list, but not always: repeat until nothing changes.
    for _ in 0..6 {
        let mut grew = false;
        for r in &all {
            if inside.contains_key(&r.pid) {
                continue;
            }
            if inside.contains_key(&r.ppid) && created.get(&r.ppid).is_some_and(|&c| c <= r.created) {
                inside.insert(r.pid, true);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    all.iter().filter(|r| inside.contains_key(&r.pid)).map(|r| (r.pid, r.ws_private)).collect()
}

/// Every running process's image name, by PID.
pub(crate) fn images() -> HashMap<u32, String> {
    raw_processes().into_iter().map(|r| (r.pid, r.image)).collect()
}

/* ---------- who runs a process, and its command line ---------- */

static ACCOUNTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// (user, command line) of a process, empty where Windows won't say.
fn owner_and_cmd(pid: u32) -> (String, String) {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return Default::default() };
        let user = token_user(h).unwrap_or_default();
        let cmd = command_line(h).unwrap_or_default();
        let _ = CloseHandle(h);
        (user, cmd)
    }
}

unsafe fn token_user(h: HANDLE) -> Option<String> {
    let mut tok = HANDLE::default();
    OpenProcessToken(h, TOKEN_QUERY, &mut tok).ok()?;
    let mut buf = vec![0u8; 256];
    let mut len = 0u32;
    let ok = GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, &mut len).is_ok();
    let _ = CloseHandle(tok);
    if !ok {
        return None;
    }
    let sid = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
    let mut s = PWSTR::null();
    ConvertSidToStringSidW(sid, &mut s).ok()?;
    let key = s.to_string().unwrap_or_default();
    let _ = LocalFree(Some(HLOCAL(s.0 as _)));
    if let Some(n) = ACCOUNTS.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return Some(n.clone());
    }
    let name = account_name(sid).unwrap_or_else(|| key.clone());
    ACCOUNTS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, name.clone());
    Some(name)
}

unsafe fn account_name(sid: PSID) -> Option<String> {
    let (mut name, mut dom) = ([0u16; 256], [0u16; 256]);
    let (mut nl, mut dl) = (name.len() as u32, dom.len() as u32);
    let mut use_ = SID_NAME_USE::default();
    LookupAccountSidW(PCWSTR::null(), sid, Some(PWSTR(name.as_mut_ptr())), &mut nl, Some(PWSTR(dom.as_mut_ptr())), &mut dl, &mut use_).ok()?;
    let n = String::from_utf16_lossy(&name[..nl as usize]);
    let d = String::from_utf16_lossy(&dom[..dl as usize]);
    let here = std::env::var("COMPUTERNAME").unwrap_or_default();
    Some(if d.is_empty() || d.eq_ignore_ascii_case(&here) || d.eq_ignore_ascii_case("NT AUTHORITY") { n } else { format!("{d}\\{n}") })
}

unsafe fn command_line(h: HANDLE) -> Option<String> {
    // ProcessCommandLineInformation: a UNICODE_STRING followed by its text.
    let mut buf: Vec<u64> = vec![0; 1024];
    let mut len = 0u32;
    let st = NtQueryInformationProcess(h, PROCESSINFOCLASS(60), buf.as_mut_ptr() as *mut _, (buf.len() * 8) as u32, &mut len);
    if st.0 < 0 {
        if len == 0 || len > 1 << 20 {
            return None;
        }
        buf = vec![0; len as usize / 8 + 8];
        if NtQueryInformationProcess(h, PROCESSINFOCLASS(60), buf.as_mut_ptr() as *mut _, (buf.len() * 8) as u32, &mut len).0 < 0 {
            return None;
        }
    }
    let us = &*(buf.as_ptr() as *const UnicodeString);
    if us.buf.is_null() || us.len == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(us.buf, us.len as usize / 2));
    Some(if s.chars().count() > 300 { s.chars().take(300).collect::<String>() + "…" } else { s })
}

/* ---------- names: the program's own description ---------- */

static VERSIONS: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);

/// (FileDescription, CompanyName) from a program's version resource.
pub(crate) fn version_info(path: &str) -> (String, String) {
    if path.is_empty() {
        return Default::default();
    }
    if let Some(v) = VERSIONS.lock().unwrap().get_or_insert_with(HashMap::new).get(path) {
        return v.clone();
    }
    let v = read_version(path).unwrap_or_default();
    VERSIONS.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.to_string(), v.clone());
    v
}

fn read_version(path: &str) -> Option<(String, String)> {
    let h = HSTRING::from(path);
    let size = unsafe { GetFileVersionInfoSizeW(&h, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(&h, None, size, buf.as_mut_ptr() as *mut _) }.ok()?;
    let query = |sub: &str| -> Option<(*const u16, u32)> {
        let (mut ptr, mut len) = (std::ptr::null_mut(), 0u32);
        let ok = unsafe { VerQueryValueW(buf.as_ptr() as *const _, &HSTRING::from(sub), &mut ptr, &mut len) }.as_bool();
        (ok && !ptr.is_null() && len > 0).then_some((ptr as *const u16, len))
    };
    let mut langs: Vec<(u16, u16)> = vec![];
    if let Some((p, len)) = query("\\VarFileInfo\\Translation") {
        let pairs = unsafe { std::slice::from_raw_parts(p, len as usize / 2) };
        langs.extend(pairs.chunks_exact(2).map(|c| (c[0], c[1])));
    }
    langs.extend([(0x0409, 0x04b0), (0x0409, 0x04e4), (0x0000, 0x04b0)]);
    let get = |name: &str| -> String {
        for (l, c) in &langs {
            if let Some((p, len)) = query(&format!("\\StringFileInfo\\{l:04x}{c:04x}\\{name}")) {
                let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len as usize) });
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
        String::new()
    };
    Some((get("FileDescription"), get("CompanyName")))
}

fn title_case(stem: &str) -> String {
    let mut c = stem.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn display_name(image: &str, exe: &str) -> String {
    match image.to_ascii_lowercase().as_str() {
        "" => return "System Idle Process".into(),
        "system" => return "System".into(),
        "registry" => return "Registry".into(),
        "memory compression" => return "Memory Compression".into(),
        "secure system" => return "Secure System".into(),
        "svchost.exe" => return "Service Host".into(),
        _ => {}
    }
    // Protected processes don't let their path be read, so no version resource either.
    let known = match image.to_ascii_lowercase().as_str() {
        "csrss.exe" => "Client Server Runtime Process",
        "smss.exe" => "Windows Session Manager",
        "wininit.exe" => "Windows Start-Up Application",
        "winlogon.exe" => "Windows Logon Application",
        "services.exe" => "Services and Controller app",
        "lsass.exe" => "Local Security Authority Process",
        "lsaiso.exe" => "Credential Guard",
        "msmpeng.exe" => "Antimalware Service Executable",
        "nissrv.exe" => "Microsoft Network Realtime Inspection Service",
        "sgrmbroker.exe" => "System Guard Runtime Monitor Broker",
        "fontdrvhost.exe" => "Usermode Font Driver Host",
        "dwm.exe" => "Desktop Window Manager",
        "searchindexer.exe" => "Microsoft Windows Search Indexer",
        "spoolsv.exe" => "Spooler SubSystem App",
        "audiodg.exe" => "Windows Audio Device Graph Isolation",
        _ => "",
    };
    if exe.is_empty() && !known.is_empty() {
        return known.into();
    }
    let (desc, _) = version_info(exe);
    if !desc.is_empty() && desc.len() <= 60 {
        return desc;
    }
    title_case(image.strip_suffix(".exe").or_else(|| image.strip_suffix(".EXE")).unwrap_or(image))
}

/* ---------- the sampler ---------- */

static SNAP: Mutex<Option<Snapshot>> = Mutex::new(None);
static WANTED: AtomicU64 = AtomicU64::new(0);
static START: Once = Once::new();

/// The latest sample, and keeps sampling going for another few seconds.
pub fn snapshot() -> Option<Snapshot> {
    WANTED.store(util::now_ms(), Ordering::Relaxed);
    START.call_once(|| {
        std::thread::spawn(sampler);
    });
    SNAP.lock().unwrap().clone()
}

struct Counters {
    q: Query,
    gpu: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    cores: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    freq: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    perf_pct: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    disk_idle: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    disk_r: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    disk_w: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    // Per volume, for PCs whose PhysicalDisk counters have no instances (it happens: the counters
    // can be switched off or lost while LogicalDisk keeps working).
    vol_idle: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    vol_r: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    vol_w: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    /// Drive letter ("c:") → disk number.
    disk_of: HashMap<String, u32>,
    net_rx: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    net_tx: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
    net_bw: Option<windows::Win32::System::Performance::PDH_HCOUNTER>,
}

#[derive(Default)]
struct State {
    /// (cpu time, read bytes, write bytes) at the last sample.
    prev: HashMap<(u32, i64), (i64, u64, u64)>,
    prev_at: Option<Instant>,
    exes: HashMap<(u32, i64), String>,
    owners: HashMap<(u32, i64), (String, String)>,
    /// Each logical processor's (idle, kernel, user) time at the last sample.
    prev_cpu: Vec<(i64, i64, i64)>,
    gen: u64,
}

/// SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION, one per logical processor.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Sppi {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    interrupts: u32,
}

/// Each logical processor's (idle, kernel including idle, user) time, 100 ns units.
fn processor_times() -> Vec<(i64, i64, i64)> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let mut buf = vec![Sppi::default(); n + 8];
    let mut len = 0u32;
    let st = unsafe { NtQuerySystemInformation(SYSTEM_INFORMATION_CLASS(8), buf.as_mut_ptr() as *mut _, (buf.len() * std::mem::size_of::<Sppi>()) as u32, &mut len) };
    if st.0 < 0 {
        return vec![];
    }
    buf.truncate(len as usize / std::mem::size_of::<Sppi>());
    buf.iter().map(|p| (p.idle, p.kernel, p.user)).collect()
}

impl Counters {
    fn new() -> Option<Self> {
        let q = Query::new()?;
        Some(Counters {
            gpu: q.add("\\GPU Engine(*)\\Utilization Percentage"),
            cores: q.add("\\Processor Information(*)\\% Processor Utility").or_else(|| q.add("\\Processor Information(*)\\% Processor Time")),
            freq: q.add("\\Processor Information(*)\\Processor Frequency"),
            perf_pct: q.add("\\Processor Information(*)\\% Processor Performance"),
            disk_idle: q.add("\\PhysicalDisk(*)\\% Idle Time"),
            disk_r: q.add("\\PhysicalDisk(*)\\Disk Read Bytes/sec"),
            disk_w: q.add("\\PhysicalDisk(*)\\Disk Write Bytes/sec"),
            vol_idle: q.add("\\LogicalDisk(*)\\% Idle Time"),
            vol_r: q.add("\\LogicalDisk(*)\\Disk Read Bytes/sec"),
            vol_w: q.add("\\LogicalDisk(*)\\Disk Write Bytes/sec"),
            disk_of: crate::sensors::letters_by_disk().into_iter().flat_map(|(n, ls)| ls.into_iter().map(move |l| (l.to_ascii_lowercase(), n))).collect(),
            net_rx: q.add("\\Network Interface(*)\\Bytes Received/sec"),
            net_tx: q.add("\\Network Interface(*)\\Bytes Sent/sec"),
            net_bw: q.add("\\Network Interface(*)\\Current Bandwidth"),
            q,
        })
    }
}

fn sampler() {
    util::com_sta();
    let Some(c) = Counters::new() else { return };
    let mut st = State::default();
    let mut idle_since: Option<Instant> = None;
    loop {
        let wanted = util::now_ms().saturating_sub(WANTED.load(Ordering::Relaxed)) < 4000;
        if !wanted {
            // Nobody's looking: stop, and forget the numbers after half a minute.
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(30) {
                *SNAP.lock().unwrap() = None;
                st = State::default();
            }
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        idle_since = None;
        let started = Instant::now();
        let first = st.prev_at.is_none();
        let snap = sample(&mut st, &c);
        if !first {
            *SNAP.lock().unwrap() = Some(snap);
        }
        // The very first sample is only a baseline for the CPU and I/O rates.
        let wait = if first { Duration::from_millis(400) } else { Duration::from_millis(1000).saturating_sub(started.elapsed()) };
        std::thread::sleep(wait);
    }
}

fn parse_gpu_instance(name: &str) -> Option<(u32, String)> {
    let pid = name.strip_prefix("pid_")?.split('_').next()?.parse().ok()?;
    let eng = name.rsplit("engtype_").next()?.to_string();
    Some((pid, eng))
}

fn sample(st: &mut State, c: &Counters) -> Snapshot {
    let now = Instant::now();
    let secs = st.prev_at.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(1.0).max(0.05);
    st.prev_at = Some(now);
    let cores_n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let ticks = secs * 10_000_000.0 * cores_n;

    let raws = raw_processes();
    c.q.collect();

    // GPU: per process the busiest engine type (summed over that type's engines), like Task Manager.
    let mut gpu_pid: HashMap<u32, HashMap<String, f64>> = HashMap::new();
    let mut gpu_type: HashMap<String, f64> = HashMap::new();
    for (inst, v) in array(c.gpu) {
        if let Some((pid, eng)) = parse_gpu_instance(&inst) {
            *gpu_pid.entry(pid).or_default().entry(eng.clone()).or_default() += v;
            *gpu_type.entry(eng).or_default() += v;
        }
    }
    let gpu_of = |pid: u32| gpu_pid.get(&pid).map(|m| m.values().cloned().fold(0.0, f64::max)).unwrap_or(0.0).min(100.0);

    let app_pids = running::task_window_pids();
    let own = std::process::id();
    let mut next_prev = HashMap::with_capacity(raws.len());
    let mut rows: Vec<(ProcRow, u32, u32)> = Vec::with_capacity(raws.len()); // row, ppid, session
    for r in &raws {
        if r.pid == 0 {
            continue;
        }
        let key = (r.pid, r.created);
        let (cpu, read, write) = match st.prev.get(&key) {
            Some(&(pc, pr, pw)) => (((r.cpu - pc).max(0) as f64 / ticks * 100.0).min(100.0), r.read.saturating_sub(pr) as f64 / secs, r.write.saturating_sub(pw) as f64 / secs),
            None => (0.0, 0.0, 0.0),
        };
        next_prev.insert(key, (r.cpu, r.read, r.write));
        let (user, cmd) = st.owners.entry(key).or_insert_with(|| owner_and_cmd(r.pid)).clone();
        rows.push((
            ProcRow {
                pid: r.pid,
                created: r.created / 10_000,
                image: r.image.clone(),
                cpu,
                mem: r.ws_private,
                io: read + write,
                read,
                write,
                gpu: gpu_of(r.pid),
                threads: r.threads,
                handles: r.handles,
                eco: r.priority == 4,
                ppid: r.ppid,
                session: r.session,
                suspended: r.suspended,
                user,
                cmd,
            },
            r.ppid,
            r.session,
        ));
    }
    st.prev = next_prev;
    st.exes.retain(|k, _| st.prev.contains_key(k));
    st.owners.retain(|k, _| st.prev.contains_key(k));

    // Trees: climb to the parent while it's the same program, or while this is a helper.
    let idx: HashMap<u32, usize> = rows.iter().enumerate().map(|(i, r)| (r.0.pid, i)).collect();
    let parent = |i: usize| -> Option<usize> {
        let (row, ppid, _) = &rows[i];
        let j = *idx.get(ppid)?;
        (j != i && rows[j].0.created <= row.created).then_some(j)
    };
    let lower: Vec<String> = rows.iter().map(|r| r.0.image.to_ascii_lowercase()).collect();
    let root_of = |i: usize| -> usize {
        let mut r = i;
        for _ in 0..64 {
            match parent(r) {
                Some(p) if lower[p] == lower[r] => r = p,
                Some(p) if HELPERS.contains(&lower[r].as_str()) && rows[p].2 != 0 => r = p,
                _ => break,
            }
        }
        r
    };
    let mut trees: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..rows.len() {
        trees.entry(root_of(i)).or_default().push(i);
    }

    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_ascii_lowercase() + "\\";
    let mut groups: HashMap<String, Group> = HashMap::new();
    for (root, members) in trees {
        let (rrow, _, session) = &rows[root];
        let key = (rrow.pid, rrow.created);
        let exe = st.exes.entry(key).or_insert_with(|| running::exe_path_of(rrow.pid)).clone();
        let exe_l = exe.to_lowercase();
        let image = lower[root].clone();
        let kind = if members.iter().any(|&m| app_pids.contains(&rows[m].0.pid)) {
            "app"
        } else if *session == 0 || WINDOWS_NAMES.contains(&image.as_str()) || exe_l.starts_with(&windir) {
            "windows"
        } else {
            "background"
        };
        let locked = if members.iter().any(|&m| rows[m].0.pid == own) {
            Some("This is Island Shell; quit it from Power")
        } else if CRITICAL.contains(&image.as_str()) {
            Some("Windows needs this to keep running")
        } else if *session == 0 {
            Some("A system service; only an administrator can end it")
        } else {
            None
        };
        let id = format!("{kind}:{image}");
        let g = groups.entry(id.clone()).or_insert_with(|| Group {
            id,
            name: display_name(&rrow.image, &exe),
            image: rrow.image.clone(),
            icon: if exe.is_empty() { None } else { running::exe_icon(&exe_l) },
            exe: exe.clone(),
            kind,
            cpu: 0.0,
            mem: 0,
            io: 0.0,
            read: 0.0,
            write: 0.0,
            gpu: 0.0,
            threads: 0,
            handles: 0,
            user: rrow.user.clone(),
            suspended: true,
            eco: true,
            eco_some: false,
            locked,
            procs: vec![],
        });
        if g.locked.is_none() {
            g.locked = locked;
        }
        for &m in &members {
            let (row, _, _) = &rows[m];
            g.cpu += row.cpu;
            g.mem += row.mem;
            g.io += row.io;
            g.read += row.read;
            g.write += row.write;
            g.gpu += row.gpu;
            g.threads += row.threads;
            g.handles += row.handles;
            g.suspended &= row.suspended;
            if g.user.is_empty() {
                g.user = row.user.clone();
            }
            g.eco &= row.eco;
            g.eco_some |= row.eco;
            g.procs.push(row.clone());
        }
    }
    let mut groups: Vec<Group> = groups.into_values().collect();
    for g in &mut groups {
        g.cpu = g.cpu.min(100.0);
        g.gpu = g.gpu.min(100.0);
        g.procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.mem.cmp(&a.mem)));
    }

    // Whole-system numbers.
    let mut perf = Perf::default();
    let mut cores: Vec<(u32, u32, f64)> = vec![];
    for (inst, v) in array(c.cores) {
        if inst == "_Total" {
            perf.cpu = v.clamp(0.0, 100.0);
        } else if let Some((a, b)) = inst.split_once(',') {
            if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                cores.push((a, b, v.clamp(0.0, 100.0)));
            }
        }
    }
    cores.sort_by_key(|x| (x.0, x.1));
    perf.cores = cores.into_iter().map(|x| x.2).collect();
    // Kernel time per logical processor, from the processor performance information.
    let times = processor_times();
    if times.len() == st.prev_cpu.len() && !times.is_empty() {
        let (mut k_all, mut t_all) = (0i64, 0i64);
        perf.cores_kernel = times
            .iter()
            .zip(&st.prev_cpu)
            .map(|(&(i, k, u), &(pi, pk, pu))| {
                let (di, dk, du) = ((i - pi).max(0), (k - pk).max(0), (u - pu).max(0));
                let total = dk + du;
                k_all += (dk - di).max(0);
                t_all += total;
                if total > 0 { ((dk - di).max(0) as f64 / total as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 }
            })
            .collect();
        perf.kernel = if t_all > 0 { (k_all as f64 / t_all as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 };
    }
    st.prev_cpu = times;
    // Effective clocks: the nominal frequency times how hard each processor is running.
    let pct: HashMap<String, f64> = array(c.perf_pct).into_iter().collect();
    let mut mhz: Vec<(u32, u32, f64)> = vec![];
    for (inst, f) in array(c.freq) {
        let eff = f * pct.get(&inst).copied().unwrap_or(100.0) / 100.0;
        if inst == "_Total" {
            perf.mhz = eff;
        } else if let Some((a, b)) = inst.split_once(',') {
            if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                mhz.push((a, b, eff));
            }
        }
    }
    mhz.sort_by_key(|x| (x.0, x.1));
    perf.cores_mhz = mhz.into_iter().map(|x| x.2).collect();
    if perf.mhz == 0.0 && !perf.cores_mhz.is_empty() {
        perf.mhz = perf.cores_mhz.iter().sum::<f64>() / perf.cores_mhz.len() as f64;
    }
    let mut pi = PERFORMANCE_INFORMATION { cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    if unsafe { GetPerformanceInfo(&mut pi, pi.cb) }.is_ok() {
        let pg = pi.PageSize as u64;
        perf.mem_total = pi.PhysicalTotal as u64 * pg;
        perf.mem_used = (pi.PhysicalTotal - pi.PhysicalAvailable) as u64 * pg;
        perf.committed = pi.CommitTotal as u64 * pg;
        perf.commit_limit = pi.CommitLimit as u64 * pg;
        perf.cached = pi.SystemCache as u64 * pg;
        perf.paged = pi.KernelPaged as u64 * pg;
        perf.nonpaged = pi.KernelNonpaged as u64 * pg;
        perf.processes = pi.ProcessCount;
        perf.threads = pi.ThreadCount;
        perf.handles = pi.HandleCount;
    }
    let mut engines: Vec<(String, f64)> = gpu_type.into_iter().map(|(k, v)| (k, v.min(100.0))).collect();
    engines.sort_by(|a, b| b.1.total_cmp(&a.1));
    perf.gpu = engines.first().map(|e| e.1).unwrap_or(0.0);
    engines.truncate(4);
    perf.gpu_engines = engines;

    let reads: HashMap<String, f64> = array(c.disk_r).into_iter().collect();
    let writes: HashMap<String, f64> = array(c.disk_w).into_iter().collect();
    let mut disks: Vec<DiskPerf> = array(c.disk_idle)
        .into_iter()
        .filter(|(n, _)| n != "_Total")
        .map(|(n, idle)| DiskPerf { active: (100.0 - idle).clamp(0.0, 100.0), read: reads.get(&n).copied().unwrap_or(0.0), write: writes.get(&n).copied().unwrap_or(0.0), name: n })
        .collect();
    if disks.is_empty() {
        // No PhysicalDisk instances: add up each disk's volumes instead (busiest volume's active
        // time, summed speeds), named the same way ("0 C: D:").
        let reads: HashMap<String, f64> = array(c.vol_r).into_iter().collect();
        let writes: HashMap<String, f64> = array(c.vol_w).into_iter().collect();
        let mut by: std::collections::BTreeMap<u32, (Vec<String>, f64, f64, f64)> = Default::default();
        for (inst, idle) in array(c.vol_idle) {
            let Some(&n) = c.disk_of.get(&inst.to_ascii_lowercase()) else { continue };
            let e = by.entry(n).or_default();
            e.0.push(inst.to_ascii_uppercase());
            e.1 = e.1.max((100.0 - idle).clamp(0.0, 100.0));
            e.2 += reads.get(&inst).copied().unwrap_or(0.0);
            e.3 += writes.get(&inst).copied().unwrap_or(0.0);
        }
        disks = by.into_iter().map(|(n, (mut ls, active, read, write))| { ls.sort(); DiskPerf { name: format!("{n} {}", ls.join(" ")), active, read, write } }).collect();
    }
    disks.sort_by(|a, b| a.name.cmp(&b.name));
    perf.disks = disks;
    let txs: HashMap<String, f64> = array(c.net_tx).into_iter().collect();
    let bws: HashMap<String, f64> = array(c.net_bw).into_iter().collect();
    let mut nets: Vec<NetPerf> = array(c.net_rx)
        .into_iter()
        .filter(|(n, _)| {
            let l = n.to_ascii_lowercase();
            bws.get(n).copied().unwrap_or(0.0) > 0.0 && !["isatap", "teredo", "loopback", "kernel debug", "6to4"].iter().any(|x| l.contains(x))
        })
        .map(|(n, rx)| NetPerf { rx, tx: txs.get(&n).copied().unwrap_or(0.0), bandwidth: bws.get(&n).copied().unwrap_or(0.0), name: n })
        .collect();
    nets.sort_by(|a, b| (b.rx + b.tx).total_cmp(&(a.rx + a.tx)).then(a.name.cmp(&b.name)));
    perf.nets = nets;

    st.gen += 1;
    Snapshot { at: util::now_ms(), gen: st.gen, groups, perf }
}

/* ---------- acting on processes ---------- */

/// Checks every (pid, created) still names the same process and may be touched; returns them.
fn checked(procs: &[(u32, i64)]) -> Result<Vec<(u32, String)>, String> {
    let live: HashMap<u32, Raw> = raw_processes().into_iter().map(|r| (r.pid, r)).collect();
    let own = std::process::id();
    let mut out = vec![];
    for &(pid, created) in procs {
        let Some(r) = live.get(&pid).filter(|r| r.created / 10_000 == created) else { continue }; // already gone
        let image = r.image.to_ascii_lowercase();
        if pid == own || pid <= 4 || r.session == 0 || CRITICAL.contains(&image.as_str()) {
            return Err(format!("{} can't be changed from here", r.image));
        }
        out.push((pid, image));
    }
    Ok(out)
}

/// Ends the given processes. Returns how many ended.
pub fn end(procs: &[(u32, i64)]) -> Result<usize, String> {
    let targets = checked(procs)?;
    let (mut ended, mut denied) = (0, 0);
    let mut explorer = false;
    for (pid, image) in &targets {
        unsafe {
            match OpenProcess(PROCESS_TERMINATE, false, *pid) {
                Ok(h) => {
                    if TerminateProcess(h, 1).is_ok() {
                        ended += 1;
                        explorer |= image == "explorer.exe";
                    } else {
                        denied += 1;
                    }
                    let _ = CloseHandle(h);
                }
                Err(_) => denied += 1,
            }
        }
    }
    if explorer {
        // Windows usually starts Explorer again by itself; make sure it does.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(2500));
            if !raw_processes().iter().any(|r| r.image.eq_ignore_ascii_case("explorer.exe")) {
                let _ = std::process::Command::new("explorer.exe").spawn();
            }
        });
    }
    util::log_line(&format!("Task Manager: ended {ended} process(es){}", if denied > 0 { format!(", {denied} refused") } else { String::new() }));
    if ended == 0 && denied > 0 {
        return Err("Windows didn't allow it (it may need administrator rights)".into());
    }
    Ok(ended)
}

/// Efficiency mode on or off: Windows' "EcoQoS" throttling plus idle priority, as Task Manager does.
pub fn eco(procs: &[(u32, i64)], on: bool) -> Result<(), String> {
    let targets = checked(procs)?;
    let mut done = 0;
    for (pid, _) in &targets {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, *pid) else { continue };
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
            };
            let a = SetProcessInformation(h, ProcessPowerThrottling, &state as *const _ as *const _, std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32);
            let b = SetPriorityClass(h, if on { IDLE_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS });
            if a.is_ok() || b.is_ok() {
                done += 1;
            }
            let _ = CloseHandle(h);
        }
    }
    if done == 0 && !targets.is_empty() {
        return Err("Windows didn't allow it (it may need administrator rights)".into());
    }
    Ok(())
}

/* ---------- run a new task ---------- */

/// Runs a program, document, folder or command line the way the Run box does, optionally as
/// administrator (Windows asks first).
pub fn run(command: &str, admin: bool) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let c = command.trim();
    if c.is_empty() || c.contains(['\n', '\r', '\0']) {
        return Err("Type a program, folder, document or website".into());
    }
    // A quoted program, or the longest leading part that names a file, takes the rest as arguments.
    let (file, args) = if let Some(rest) = c.strip_prefix('"') {
        match rest.find('"') {
            Some(i) => (rest[..i].to_string(), rest[i + 1..].trim().to_string()),
            None => (rest.to_string(), String::new()),
        }
    } else if Path::new(c).exists() || c.contains("://") || c.starts_with("shell:") || c.starts_with("ms-") {
        (c.to_string(), String::new())
    } else {
        match c.split_once(' ') {
            Some((a, b)) => (a.to_string(), b.trim().to_string()),
            None => (c.to_string(), String::new()),
        }
    };
    let (f, a) = (HSTRING::from(file.as_str()), HSTRING::from(args.as_str()));
    let verb = HSTRING::from(if admin { "runas" } else { "open" });
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(f.as_ptr()),
        lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(a.as_ptr()) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    match unsafe { ShellExecuteExW(&mut sei) } {
        Ok(()) => {
            util::log_line(&format!("Task Manager: ran {file}{}", if admin { " as administrator" } else { "" }));
            Ok(())
        }
        Err(e) if e.code() == windows::Win32::Foundation::ERROR_CANCELLED.to_hresult() => Ok(()),
        Err(_) => Err(format!("Windows can't find “{file}”")),
    }
}

/* ---------- app history ---------- */

#[derive(Serialize, serde::Deserialize, Clone, Default)]
pub struct AppUse {
    pub exe: String,
    pub name: String,
    /// CPU time, ms.
    pub cpu_ms: u64,
    pub read: u64,
    pub write: u64,
    pub launches: u64,
    /// Unix ms.
    pub first: u64,
    pub last: u64,
    #[serde(skip_deserializing)]
    pub icon: Option<String>,
    /// Lives in the Windows folder (or has no readable path).
    #[serde(default)]
    pub system: bool,
}

#[derive(Serialize, serde::Deserialize, Clone, Default)]
pub struct History {
    /// When the island started keeping it (Unix ms).
    pub since: u64,
    pub apps: Vec<AppUse>,
}

static HISTORY: Mutex<Option<(History, HashMap<String, usize>)>> = Mutex::new(None);

fn history_file() -> std::path::PathBuf {
    util::data_dir().join("app_history.json")
}

fn save_history() {
    let h = HISTORY.lock().unwrap().as_ref().map(|(h, _)| h.clone());
    if let Some(h) = h {
        if let Ok(s) = serde_json::to_string(&h) {
            let _ = std::fs::write(history_file(), s);
        }
    }
}

/// Starts the history sampler (once, at start-up).
pub fn history_spawn() {
    let loaded: History = std::fs::read_to_string(history_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| History { since: util::now_ms(), apps: vec![] });
    let idx = loaded.apps.iter().enumerate().map(|(i, a)| (a.exe.clone(), i)).collect();
    *HISTORY.lock().unwrap() = Some((loaded, idx));
    std::thread::spawn(|| {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_ascii_lowercase() + "\\";
        // Per process: (cpu, read, write) last time, and which program it is.
        let mut seen: HashMap<(u32, i64), (i64, u64, u64, String)> = HashMap::new();
        let mut first = true;
        let mut saved = Instant::now();
        loop {
            let raws = raw_processes();
            let now = util::now_ms();
            let mut next = HashMap::with_capacity(raws.len());
            let by_pid: HashMap<u32, &Raw> = raws.iter().map(|r| (r.pid, r)).collect();
            {
                let mut g = HISTORY.lock().unwrap();
                let Some((h, idx)) = g.as_mut() else { return };
                for r in &raws {
                    if r.pid <= 4 {
                        continue;
                    }
                    let key = (r.pid, r.created);
                    let (dc, dr, dw, exe, new) = match seen.remove(&key) {
                        Some((c, rd, wr, exe)) => ((r.cpu - c).max(0), r.read.saturating_sub(rd), r.write.saturating_sub(wr), exe, false),
                        None => {
                            let exe = running::exe_of(r.pid);
                            // Everything it did before we first saw it counts, unless it was already running at start-up.
                            if first { (0, 0, 0, exe, false) } else { (r.cpu, r.read, r.write, exe, true) }
                        }
                    };
                    next.insert(key, (r.cpu, r.read, r.write, exe.clone()));
                    let id = if exe.is_empty() { format!("?{}", r.image.to_ascii_lowercase()) } else { exe.clone() };
                    let i = *idx.entry(id.clone()).or_insert_with(|| {
                        h.apps.push(AppUse { exe: id.clone(), name: String::new(), first: now, system: exe.is_empty() || exe.starts_with(&windir), ..Default::default() });
                        h.apps.len() - 1
                    });
                    let a = &mut h.apps[i];
                    if a.name.is_empty() {
                        a.name = display_name(&r.image, &exe);
                    }
                    a.cpu_ms += (dc / 10_000) as u64;
                    a.read += dr;
                    a.write += dw;
                    a.last = now;
                    // A launch: a new process whose parent isn't the same program.
                    let parent_same = by_pid.get(&r.ppid).is_some_and(|p| p.image.eq_ignore_ascii_case(&r.image) && p.created <= r.created);
                    if new && !parent_same {
                        a.launches += 1;
                    }
                }
            }
            seen = next;
            first = false;
            if saved.elapsed() > Duration::from_secs(600) {
                save_history();
                saved = Instant::now();
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

pub fn history() -> History {
    let mut h = HISTORY.lock().unwrap().as_ref().map(|(h, _)| h.clone()).unwrap_or_default();
    h.apps.retain(|a| a.cpu_ms > 0 || a.launches > 0 || a.read + a.write > 0);
    for a in &mut h.apps {
        if !a.exe.starts_with('?') {
            a.icon = running::exe_icon(&a.exe);
        }
    }
    h.apps.sort_by(|a, b| b.cpu_ms.cmp(&a.cpu_ms));
    h
}

pub fn history_clear() {
    if let Some((h, idx)) = HISTORY.lock().unwrap().as_mut() {
        h.apps.clear();
        idx.clear();
        h.since = util::now_ms();
    }
    save_history();
}

/// Saves the history now (when the island quits).
pub fn history_flush() {
    save_history();
}

/* ---------- startup apps ---------- */

#[derive(Serialize, Clone)]
pub struct Startup {
    /// "u|run|Name", "m|run32|Name", "u|folder|File.lnk"…
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub command: String,
    pub path: String,
    pub icon: Option<String>,
    pub on: bool,
    /// Yours, so it can be switched without administrator rights.
    pub user: bool,
    pub source: &'static str,
}

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open_key(root: HKEY, path: &str) -> Option<Key> {
    let mut k = HKEY::default();
    (unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut k) } == ERROR_SUCCESS).then_some(Key(k))
}

/// Every value in a key: (name, type, data).
fn values(root: HKEY, path: &str) -> Vec<(String, u32, Vec<u8>)> {
    let Some(k) = open_key(root, path) else { return vec![] };
    let mut out = vec![];
    for i in 0..512 {
        let mut name = [0u16; 512];
        let mut name_len = name.len() as u32;
        let mut kind = 0u32;
        let mut data = vec![0u8; 8192];
        let mut data_len = data.len() as u32;
        let r = unsafe {
            RegEnumValueW(k.0, i, Some(PWSTR(name.as_mut_ptr())), &mut name_len, None, Some(&mut kind), Some(data.as_mut_ptr()), Some(&mut data_len))
        };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        if r != ERROR_SUCCESS {
            continue;
        }
        data.truncate(data_len as usize);
        out.push((String::from_utf16_lossy(&name[..name_len as usize]), kind, data));
    }
    out
}

fn reg_string(data: &[u8]) -> String {
    let w: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
}

/// %VARS% in a command, from this process's environment.
fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => {
                let var = &after[..b];
                match std::env::var(var) {
                    Ok(v) if !var.is_empty() => out.push_str(&v),
                    _ => out.push_str(&rest[a..a + b + 2]),
                }
                rest = &after[b + 1..];
            }
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The program a startup command runs.
fn exe_from_command(cmd: &str) -> String {
    let c = expand(cmd.trim());
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().unwrap_or("").to_string();
    }
    let l = c.to_ascii_lowercase();
    match l.find(".exe") {
        Some(i) => c[..i + 4].to_string(),
        None => c.split_whitespace().next().unwrap_or("").to_string(),
    }
}

/// Windows' own on/off state for startup entries (first byte even = on); missing = on.
fn approved(root: HKEY, sub: &str) -> HashMap<String, bool> {
    values(root, &format!("{APPROVED}\\{sub}")).into_iter().filter(|v| v.1 == REG_BINARY.0 && !v.2.is_empty()).map(|(n, _, d)| (n.to_lowercase(), d[0] & 1 == 0)).collect()
}

/// A startup entry's own name reads well ("Steam", "Voicemeeter (VB-Audio)"), unlike ids such as
/// "MicrosoftEdgeAutoLaunch_6482…" or "Mozilla-Zen-F0DC299D…", which give way to the description.
fn clean_label(label: &str) -> bool {
    let hex_run = label.split(|c: char| !c.is_ascii_hexdigit()).any(|run| run.len() >= 8);
    label.len() <= 28 && !hex_run && !label.contains('.') && !label.contains('_')
}

fn entry(id: String, label: &str, command: String, path: String, on: bool, user: bool, source: &'static str) -> Startup {
    let (desc, company) = version_info(&path);
    let exists = !path.is_empty() && Path::new(&path).exists();
    Startup {
        id,
        name: if !desc.is_empty() && desc.len() <= 48 && !clean_label(label) { desc } else { label.to_string() },
        publisher: company,
        command,
        icon: if exists { running::exe_icon(&path.to_lowercase()) } else { None },
        path,
        on,
        user,
        source,
    }
}

pub fn startup() -> Vec<Startup> {
    let mut out = vec![];
    for (root, scope, path, sub, user) in [
        (HKEY_CURRENT_USER, "u", RUN, "Run", true),
        (HKEY_LOCAL_MACHINE, "m", RUN, "Run", false),
        (HKEY_LOCAL_MACHINE, "m", RUN32, "Run32", false),
    ] {
        let state = approved(root, sub);
        for (name, kind, data) in values(root, path) {
            if kind != REG_SZ.0 && kind != REG_EXPAND_SZ.0 {
                continue;
            }
            let command = reg_string(&data);
            let on = state.get(&name.to_lowercase()).copied().unwrap_or(true);
            let exe = exe_from_command(&command);
            out.push(entry(format!("{scope}|{}|{name}", sub.to_lowercase()), &name, command, exe, on, user, "Registry"));
        }
    }
    for (folder, root, scope, user) in [
        (places::known(&windows::Win32::UI::Shell::FOLDERID_Startup), HKEY_CURRENT_USER, "u", true),
        (places::known(&windows::Win32::UI::Shell::FOLDERID_CommonStartup), HKEY_LOCAL_MACHINE, "m", false),
    ] {
        let Some(folder) = folder else { continue };
        let state = approved(root, "StartupFolder");
        let Ok(rd) = std::fs::read_dir(&folder) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let file = e.file_name().to_string_lossy().into_owned();
            if file.eq_ignore_ascii_case("desktop.ini") || p.is_dir() {
                continue;
            }
            let target = if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) { places::link_target(&p).unwrap_or_default() } else { p.to_string_lossy().into_owned() };
            let label = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file.clone());
            let on = state.get(&file.to_lowercase()).copied().unwrap_or(true);
            out.push(entry(format!("{scope}|folder|{file}"), &label, p.to_string_lossy().into_owned(), target, on, user, "Startup folder"));
        }
    }
    out.sort_by(|a, b| b.on.cmp(&a.on).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// Switches one of your own startup entries on or off, the way Task Manager records it.
pub fn set_startup(id: &str, on: bool) -> Result<(), String> {
    let item = startup().into_iter().find(|s| s.id == id).ok_or("That startup entry is gone")?;
    if !item.user {
        return Err("That one is set for every account on this PC; changing it needs administrator rights".into());
    }
    let mut parts = id.splitn(3, '|');
    let (_, source, name) = (parts.next(), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let sub = match source {
        "run" => "Run",
        "folder" => "StartupFolder",
        _ => return Err("Unknown startup entry".into()),
    };
    let mut data = vec![if on { 2u8 } else { 3u8 }, 0, 0, 0];
    let ft: FILETIME = if on { FILETIME::default() } else { unsafe { GetSystemTimeAsFileTime() } };
    data.extend_from_slice(&ft.dwLowDateTime.to_le_bytes());
    data.extend_from_slice(&ft.dwHighDateTime.to_le_bytes());
    let mut k = HKEY::default();
    let r = unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(format!("{APPROVED}\\{sub}")), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None)
    };
    if r != ERROR_SUCCESS {
        return Err(format!("Couldn't open the startup settings ({})", r.0));
    }
    let k = Key(k);
    let r = unsafe { RegSetValueExW(k.0, &HSTRING::from(name), None, REG_BINARY, Some(&data)) };
    if r != ERROR_SUCCESS {
        return Err(format!("Couldn't save it ({})", r.0));
    }
    util::log_line(&format!("Task Manager: startup {} {}", item.name, if on { "on" } else { "off" }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_to_programs() {
        assert_eq!(exe_from_command("\"C:\\Program Files\\App\\app.exe\" --tray"), "C:\\Program Files\\App\\app.exe");
        assert_eq!(exe_from_command("C:\\Tools\\thing.EXE -silent"), "C:\\Tools\\thing.EXE");
        assert_eq!(exe_from_command("rundll32.exe shell32.dll,Foo"), "rundll32.exe");
        let windir = std::env::var("SystemRoot").unwrap();
        assert_eq!(exe_from_command("%SystemRoot%\\system32\\x.exe /a"), format!("{windir}\\system32\\x.exe"));
        assert_eq!(expand("100% sure %NOPE_NOT_SET%"), "100% sure %NOPE_NOT_SET%");
        assert!(clean_label("Steam") && clean_label("Voicemeeter (VB-Audio)") && clean_label("SunJavaUpdateSched"));
        assert!(!clean_label("Mozilla-Zen-F0DC299D809B9700") && !clean_label("MicrosoftEdgeAutoLaunch_6482399B") && !clean_label("com.squirrel.Teams.Teams"));
    }

    #[test]
    fn gpu_instances() {
        assert_eq!(parse_gpu_instance("pid_1234_luid_0x00000000_0x0000D1E3_phys_0_eng_0_engtype_3D"), Some((1234, "3D".into())));
        assert_eq!(parse_gpu_instance("_Total"), None);
    }

    #[test]
    fn layouts() {
        // The process entry is 0x100 bytes and each thread entry 0x50 on 64-bit Windows.
        assert_eq!(std::mem::size_of::<Spi>(), 0x100);
        assert_eq!(std::mem::size_of::<Sti>(), 0x50);
        assert_eq!(std::mem::size_of::<Sppi>(), 48);
        assert_eq!(processor_times().len(), std::thread::available_parallelism().unwrap().get());
        let (user, cmd) = owner_and_cmd(std::process::id());
        assert!(!user.is_empty() && cmd.to_ascii_lowercase().contains("island_shell"), "{user} | {cmd}");
    }

    #[test]
    fn live_processes() {
        let all = raw_processes();
        assert!(all.len() > 20, "only {} processes", all.len());
        let me = all.iter().find(|r| r.pid == std::process::id()).expect("this test's own process");
        assert!(me.image.to_ascii_lowercase().ends_with(".exe"), "{}", me.image);
        assert!(me.ws_private > 0 && me.threads > 0);
        assert!(all.iter().any(|r| r.pid == 4 && r.image == "System"));
    }

    /// TM_DUMP=<folder>: writes a few live samples and the startup list as JSON (for page tests).
    #[test]
    #[ignore = "needs TM_DUMP=<folder>: writes live samples"]
    fn dump_json() {
        util::com_mta();
        let Ok(dir) = std::env::var("TM_DUMP") else { return };
        let mut st = State::default();
        let c = Counters::new().unwrap();
        sample(&mut st, &c);
        let mut snaps = vec![];
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(1000));
            snaps.push(sample(&mut st, &c));
        }
        std::fs::write(Path::new(&dir).join("tm_samples.json"), serde_json::to_string(&snaps).unwrap()).unwrap();
        std::fs::write(Path::new(&dir).join("tm_startup.json"), serde_json::to_string(&startup()).unwrap()).unwrap();
        let w = |name: &str, json: String| std::fs::write(Path::new(&dir).join(name), json).unwrap();
        w("tm_cpu.json", serde_json::to_string(&crate::sysinfo::cpu()).unwrap());
        w("tm_system.json", serde_json::to_string(&crate::sysinfo::system()).unwrap());
        w("tm_nvme.json", serde_json::to_string(&crate::sysinfo::nvme_health()).unwrap());
        w("tm_services.json", serde_json::to_string(&crate::services::list(false).unwrap()).unwrap());
        w("tm_sessions.json", serde_json::to_string(&crate::services::sessions()).unwrap());
        w("tm_installed.json", serde_json::to_string(&crate::installed::list()).unwrap());
        crate::conns::list();
        std::thread::sleep(Duration::from_secs(10));
        w("tm_conns.json", serde_json::to_string(&crate::conns::list()).unwrap());
        crate::diskscan::start("C:\\").unwrap();
        while crate::diskscan::status().running {
            std::thread::sleep(Duration::from_millis(300));
        }
        w("disk_status.json", serde_json::to_string(&crate::diskscan::status()).unwrap());
        let mut views = serde_json::Map::new();
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        for p in ["", r"C:\Program Files (x86)", r"C:\Program Files (x86)\Steam\steamapps\common", r"C:\Users", home.as_str()] {
            views.insert(p.to_string(), serde_json::to_value(crate::diskscan::view(p)).unwrap());
        }
        w("disk_views.json", serde_json::Value::Object(views).to_string());
        crate::diskscan::clear();
        // A short app history from three samples, as the page would see it.
        let raws = raw_processes();
        let apps: Vec<AppUse> = raws
            .iter()
            .filter(|r| r.pid > 4)
            .take(60)
            .map(|r| {
                let exe = running::exe_of(r.pid);
                AppUse { name: display_name(&r.image, &exe), icon: if exe.is_empty() { None } else { running::exe_icon(&exe) }, system: exe.is_empty(), exe, cpu_ms: (r.cpu / 10_000) as u64, read: r.read, write: r.write, launches: 1, first: util::now_ms() - 86_400_000, last: util::now_ms() }
            })
            .collect();
        w("tm_history.json", serde_json::to_string(&History { since: util::now_ms() - 3 * 86_400_000, apps }).unwrap());
    }

    #[test]
    #[ignore = "needs a real PC: prints a live sample"]
    fn print_sample() {
        util::com_mta();
        let mut st = State::default();
        let c = Counters::new().unwrap();
        sample(&mut st, &c);
        std::thread::sleep(Duration::from_millis(1000));
        let s = sample(&mut st, &c);
        let mut g = s.groups.clone();
        g.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        for x in g.iter().take(25) {
            println!("{:<10} {:<40} {:>5.1}% {:>7} MB  gpu {:>4.1}  io {:>8.0}  n={} {:?}", x.kind, x.name, x.cpu, x.mem / 1_048_576, x.gpu, x.io, x.procs.len(), x.locked);
        }
        println!("groups {}  apps {}  bg {}  win {}", g.len(), g.iter().filter(|x| x.kind == "app").count(), g.iter().filter(|x| x.kind == "background").count(), g.iter().filter(|x| x.kind == "windows").count());
        println!("cpu {:.1} cores {:?}", s.perf.cpu, s.perf.cores);
        println!("mem {} / {} GB, commit {} / {}", s.perf.mem_used >> 30, s.perf.mem_total >> 30, s.perf.committed >> 30, s.perf.commit_limit >> 30);
        println!("gpu {:.1} {:?}", s.perf.gpu, s.perf.gpu_engines);
        for d in &s.perf.disks {
            println!("disk {} active {:.1} r {:.0} w {:.0}", d.name, d.active, d.read, d.write);
        }
        for n in &s.perf.nets {
            println!("net {} rx {:.0} tx {:.0} bw {:.0}", n.name, n.rx, n.tx, n.bandwidth);
        }
        for s in startup() {
            println!("startup {:<5} {:<30} {:<24} user={} {} | {}", s.on, s.name, s.publisher, s.user, s.id, s.path);
        }
    }
}
