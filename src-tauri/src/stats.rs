//! Live system stats from Windows performance counters (PDH), memory, disks, DXGI and the registry.
//!
//! A light query (CPU, network) runs all the time so the CPU graph has history. The heavy query
//! (per-process CPU, GPU engines, VRAM) only runs while the Stats tab is asking for data.
//! Temperatures, fans and clocks come from sensors.rs (GPUs every tick for the bar's CPU temperature;
//! drives every 15 s, or every 6 s while the tab is open).

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Performance::*;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::Shared;

const HISTORY: usize = 50;
const DRIVE_FIXED: u32 = 3;

#[derive(Serialize, Clone, Default)]
pub struct Disk {
    pub letter: String,
    pub label: String,
    pub used: f64,
    pub total: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct Proc {
    pub name: String,
    pub cpu: f64,
}

#[derive(Serialize, Clone, Default)]
pub struct Stats {
    pub cpu: f64,
    pub cpu_name: String,
    pub cores: usize,
    pub cpu_hist: Vec<f64>,
    pub gpu: Option<f64>,
    pub gpu_name: String,
    pub vram_used: f64,
    pub vram_total: f64,
    pub ram_used: f64,
    pub ram_total: f64,
    pub net_down: f64,
    pub net_up: f64,
    pub uptime: u64,
    pub disks: Vec<Disk>,
    pub top: Vec<Proc>,
    pub sensors: crate::sensors::Sensors,
}

pub(crate) struct Query(PDH_HQUERY);

impl Query {
    pub(crate) fn new() -> Option<Self> {
        let mut q = PDH_HQUERY::default();
        (unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut q) } == 0).then_some(Query(q))
    }
    pub(crate) fn add(&self, path: &str) -> Option<PDH_HCOUNTER> {
        let mut c = PDH_HCOUNTER::default();
        (unsafe { PdhAddEnglishCounterW(self.0, &HSTRING::from(path), 0, &mut c) } == 0).then_some(c)
    }
    pub(crate) fn collect(&self) {
        unsafe {
            PdhCollectQueryData(self.0);
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.0);
        }
    }
}

const FMT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000); // PDH_FMT_NOCAP100

fn value(c: Option<PDH_HCOUNTER>) -> Option<f64> {
    let c = c?;
    let mut v = PDH_FMT_COUNTERVALUE::default();
    let r = unsafe { PdhGetFormattedCounterValue(c, FMT, None, &mut v) };
    (r == 0 && v.CStatus <= 1).then(|| unsafe { v.Anonymous.doubleValue })
}

pub(crate) fn array(c: Option<PDH_HCOUNTER>) -> Vec<(String, f64)> {
    let Some(c) = c else { return vec![] };
    let (mut size, mut count) = (0u32, 0u32);
    if unsafe { PdhGetFormattedCounterArrayW(c, FMT, &mut size, &mut count, None) } != PDH_MORE_DATA {
        return vec![];
    }
    let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8) + 1];
    let ptr = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
    if unsafe { PdhGetFormattedCounterArrayW(c, FMT, &mut size, &mut count, Some(ptr)) } != 0 {
        return vec![];
    }
    let items = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
    items
        .iter()
        .filter(|i| i.FmtValue.CStatus <= 1)
        .map(|i| (unsafe { i.szName.to_string().unwrap_or_default() }, unsafe { i.FmtValue.Anonymous.doubleValue }))
        .collect()
}

pub fn cpu_name() -> String {
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"),
            &HSTRING::from("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if r.is_err() {
        return "Processor".into();
    }
    let s = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
    let s = s.replace("(R)", "").replace("(TM)", "").replace(" CPU", "");
    let s = s.split(" @ ").next().unwrap_or(&s).to_string();
    // "AMD Ryzen 7 5800X 8-Core Processor" -> "AMD Ryzen 7 5800X"
    let words: Vec<&str> = s.split_whitespace().filter(|w| !w.ends_with("-Core") && *w != "Processor").collect();
    words.join(" ")
}

fn gpu_info() -> (String, f64) {
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return (String::new(), 0.0) };
    let (mut best_name, mut best_mem) = (String::new(), 0usize);
    let mut i = 0;
    while let Ok(a) = unsafe { f.EnumAdapters1(i) } {
        i += 1;
        let Ok(d) = (unsafe { a.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        if d.DedicatedVideoMemory > best_mem {
            best_mem = d.DedicatedVideoMemory;
            let end = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
            best_name = String::from_utf16_lossy(&d.Description[..end]).replace("(R)", "").replace("(TM)", "");
        }
    }
    (best_name.trim().to_string(), best_mem as f64 / 1_073_741_824.0)
}

fn disks() -> Vec<Disk> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = vec![];
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = HSTRING::from(format!("{}:\\", (b'A' + i as u8) as char));
        if unsafe { GetDriveTypeW(&root) } != DRIVE_FIXED {
            continue;
        }
        let (mut free, mut total) = (0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(&root, None, Some(&mut total), Some(&mut free)) }.is_err() || total == 0 {
            continue;
        }
        let mut name = [0u16; 64];
        let _ = unsafe { GetVolumeInformationW(&root, Some(&mut name), None, None, None, None) };
        let label = String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(0)]);
        out.push(Disk {
            letter: format!("{}:", (b'A' + i as u8) as char),
            label,
            used: (total - free) as f64 / 1_073_741_824.0,
            total: total as f64 / 1_073_741_824.0,
        });
    }
    out
}

fn friendly_proc(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "code" => "VS Code".into(),
        "msedge" => "Edge".into(),
        "msedgewebview2" => "WebView2".into(),
        "chrome" => "Chrome".into(),
        "firefox" => "Firefox".into(),
        "javaw" | "java" => "Java".into(),
        "dwm" => "Desktop Window Manager".into(),
        "explorer" => "Explorer".into(),
        "system" => "System".into(),
        "island-shell" => "Island Shell".into(),
        _ => {
            let mut c = name.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

pub fn spawn(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let (gpu_name, vram_total) = gpu_info();
        let cpu_name = cpu_name();
        let Some(light) = Query::new() else { return };
        let cpu = light.add("\\Processor Information(_Total)\\% Processor Utility").or_else(|| light.add("\\Processor(_Total)\\% Processor Time"));
        let rx = light.add("\\Network Interface(*)\\Bytes Received/sec");
        let tx = light.add("\\Network Interface(*)\\Bytes Sent/sec");
        let heavy = Query::new();
        let procs = heavy.as_ref().and_then(|q| q.add("\\Process(*)\\% Processor Time"));
        let gpu = heavy.as_ref().and_then(|q| q.add("\\GPU Engine(*engtype_3D)\\Utilization Percentage"));
        let vram = heavy.as_ref().and_then(|q| q.add("\\GPU Adapter Memory(*)\\Dedicated Usage"));
        // Effective clock = base frequency x "% Processor Performance" (it goes past 100% when boosting).
        let cpu_perf = heavy.as_ref().and_then(|q| q.add("\\Processor Information(_Total)\\% Processor Performance"));
        let cpu_freq = heavy.as_ref().and_then(|q| q.add("\\Processor Information(_Total)\\Processor Frequency"));
        let mut gpus = crate::sensors::Gpus::new(&cpu_name);
        let mut drives = crate::sensors::drives();
        light.collect();
        let mut hist: Vec<f64> = Vec::with_capacity(HISTORY);
        let mut tick: u64 = 0;
        let mut disk_cache: Vec<Disk> = disks();
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            tick += 1;
            light.collect();
            let cpu_v = value(cpu).unwrap_or(0.0).clamp(0.0, 100.0);
            hist.push(cpu_v);
            if hist.len() > HISTORY {
                hist.remove(0);
            }
            let wanted = crate::util::now_ms().saturating_sub(shared.stats_wanted.load(Ordering::Relaxed)) < 6000;
            let mut s = shared.stats.lock().unwrap().clone();
            s.cpu = cpu_v;
            s.cpu_hist = hist.clone();
            s.cpu_name = cpu_name.clone();
            s.cores = cores;
            s.gpu_name = gpu_name.clone();
            s.vram_total = vram_total;
            let g = gpus.read();
            (s.sensors.cpu_temp, s.sensors.cpu_temp_from) = crate::sensors::cpu_temp(&g);
            s.sensors.gpus = g;
            if tick % if wanted { 4 } else { 10 } == 0 {
                drives = crate::sensors::drives();
            }
            s.sensors.drives = drives.clone();
            if wanted {
                s.net_down = array(rx).iter().map(|x| x.1).sum();
                s.net_up = array(tx).iter().map(|x| x.1).sum();
                if let Some(q) = &heavy {
                    q.collect();
                    let mut by_name: HashMap<String, f64> = HashMap::new();
                    for (name, v) in array(procs) {
                        let base = name.split('#').next().unwrap_or(&name).to_string();
                        if base == "_Total" || base == "Idle" {
                            continue;
                        }
                        *by_name.entry(base).or_default() += v / cores as f64;
                    }
                    let mut top: Vec<Proc> = by_name.into_iter().filter(|(_, v)| *v > 0.05).map(|(n, cpu)| Proc { name: friendly_proc(&n), cpu }).collect();
                    top.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
                    top.truncate(5);
                    s.top = top;
                    let g = array(gpu);
                    s.gpu = if gpu.is_some() { Some(g.iter().map(|x| x.1).sum::<f64>().clamp(0.0, 100.0)) } else { None };
                    s.vram_used = array(vram).iter().map(|x| x.1).sum::<f64>() / 1_073_741_824.0;
                    s.sensors.cpu_mhz = value(cpu_freq).zip(value(cpu_perf)).map(|(f, p)| f * p / 100.0).filter(|m| *m > 0.0);
                }
                if tick % 20 == 0 {
                    disk_cache = disks();
                }
                s.disks = disk_cache.clone();
                let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
                if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
                    s.ram_total = m.ullTotalPhys as f64 / 1_073_741_824.0;
                    s.ram_used = (m.ullTotalPhys - m.ullAvailPhys) as f64 / 1_073_741_824.0;
                }
                s.uptime = unsafe { GetTickCount64() } / 1000;
            }
            *shared.stats.lock().unwrap() = s;
        }
    });
}
