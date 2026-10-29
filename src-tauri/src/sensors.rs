//! Temperatures, fan speeds, clocks and power that Windows itself exposes: no admin rights, no
//! helper app (HWiNFO) and no kernel driver.
//!
//! - GPUs, through the graphics kernel's performance data (D3DKMT, the same source as Task
//!   Manager): temperature, fan speed, power as a share of the card's limit, engine and memory
//!   clocks, voltage.
//! - The CPU: a processor with built-in graphics (the Ryzen 7 8700G's Radeon 780M) reports through
//!   that GPU, which sits on the same chip, so its temperature is the CPU chip's. It follows CPU load
//!   (37 °C idle → 49 °C under an all-core load, checked with examples/sensors_probe.rs). A CPU
//!   without built-in graphics has no temperature this way.
//! - SSDs: their own sensors, through IOCTL_STORAGE_QUERY_PROPERTY on a handle opened with no
//!   access rights (which needs no admin).
//!
//! Not available like this: motherboard fans and CPU power, which need a driver.

use std::collections::HashMap;

use serde::Serialize;
use windows::core::HSTRING;
use windows::Wdk::Graphics::Direct3D::*;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetLogicalDrives, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::*;
use windows::Win32::System::IO::DeviceIoControl;

const DRIVE_FIXED: u32 = 3;

#[derive(Serialize, Clone, Default, Debug)]
pub struct Gpu {
    pub name: String,
    /// Built into the CPU (its temperature is the CPU chip's).
    pub integrated: bool,
    pub temp: Option<f64>,
    pub fan_rpm: Option<u32>,
    pub fan_max: Option<u32>,
    /// Power draw as a percentage of the card's limit (Windows doesn't report watts).
    pub power_pct: Option<f64>,
    pub core_mhz: Option<f64>,
    pub core_max_mhz: Option<f64>,
    pub mem_mhz: Option<f64>,
    pub voltage_mv: Option<u32>,
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct Drive {
    pub name: String,
    /// Drive letters on this disk.
    pub letters: Vec<String>,
    /// The drive's main ("composite") temperature.
    pub temp: f64,
    /// Its hottest sensor (some SSDs report the controller separately).
    pub hottest: f64,
    pub warning: f64,
    pub critical: f64,
}

#[derive(Serialize, Clone, Default, Debug)]
pub struct Sensors {
    pub cpu_temp: Option<f64>,
    /// Which sensor the CPU temperature comes from (e.g. "Radeon 780M on the same chip").
    pub cpu_temp_from: String,
    /// Effective CPU clock right now, from Windows' performance counters.
    pub cpu_mhz: Option<f64>,
    pub gpus: Vec<Gpu>,
    pub drives: Vec<Drive>,
}

struct Adapter {
    handle: u32,
    name: String,
    integrated: bool,
}

/// Keeps the GPUs' kernel handles open between reads (re-opened if a read fails).
pub struct Gpus {
    cpu_name: String,
    adapters: Vec<Adapter>,
}

impl Drop for Gpus {
    fn drop(&mut self) {
        self.close();
    }
}

unsafe fn query<T>(h: u32, ty: KMTQUERYADAPTERINFOTYPE, mut v: T) -> Option<T> {
    let mut q = D3DKMT_QUERYADAPTERINFO { hAdapter: h, Type: ty, pPrivateDriverData: &mut v as *mut T as *mut _, PrivateDriverDataSize: std::mem::size_of::<T>() as u32 };
    (D3DKMTQueryAdapterInfo(&mut q).0 >= 0).then_some(v)
}

/// A hardware graphics adapter as DXGI lists it.
pub struct DxgiAdapter {
    pub adapter: IDXGIAdapter1,
    pub luid: LUID,
    pub name: String,
    pub vendor: u32,
}

/// Every graphics adapter, in DXGI's order; software adapters left out.
pub fn dxgi_adapters() -> Vec<DxgiAdapter> {
    let mut out = vec![];
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return out };
    let mut i = 0;
    while let Ok(a) = unsafe { f.EnumAdapters1(i) } {
        i += 1;
        let Ok(d) = (unsafe { a.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let end = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        let name = String::from_utf16_lossy(&d.Description[..end]).replace("(R)", "").replace("(TM)", "").trim().to_string();
        out.push(DxgiAdapter { adapter: a, luid: d.AdapterLuid, name, vendor: d.VendorId });
    }
    out
}

/// DXGI names (and vendor ids) by adapter LUID.
fn dxgi_names() -> HashMap<(u32, i32), (String, u32)> {
    dxgi_adapters().into_iter().map(|a| ((a.luid.LowPart, a.luid.HighPart), (a.name, a.vendor))).collect()
}

/// Whether a GPU is the one built into this CPU: AMD lists it in the CPU's name ("… w/ Radeon 780M
/// Graphics", "… with Radeon Graphics"); Intel's integrated graphics are any Intel GPU but Arc.
pub fn on_cpu(gpu: &str, vendor: u32, cpu: &str) -> bool {
    let core = gpu.replace("AMD ", "").replace("Intel ", "");
    let core = core.trim();
    let amd_igpu = cpu.contains("AMD") && (cpu.contains(core) || core == "Radeon Graphics");
    let intel_igpu = vendor == 0x8086 && cpu.contains("Intel") && !gpu.contains("Arc");
    amd_igpu || intel_igpu
}

impl Gpus {
    pub fn new(cpu_name: &str) -> Self {
        let mut g = Gpus { cpu_name: cpu_name.to_string(), adapters: vec![] };
        g.open();
        g
    }

    fn open(&mut self) {
        self.close();
        let names = dxgi_names();
        unsafe {
            let mut e = D3DKMT_ENUMADAPTERS2::default();
            if D3DKMTEnumAdapters2(&mut e).0 < 0 {
                return;
            }
            let mut list = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
            e.pAdapters = list.as_mut_ptr();
            if D3DKMTEnumAdapters2(&mut e).0 < 0 {
                return;
            }
            for a in &list[..e.NumAdapters as usize] {
                let LUID { LowPart, HighPart } = a.AdapterLuid;
                match names.get(&(LowPart, HighPart)) {
                    Some((name, vendor)) => {
                        let integrated = on_cpu(name, *vendor, &self.cpu_name);
                        self.adapters.push(Adapter { handle: a.hAdapter, name: name.clone(), integrated });
                    }
                    None => {
                        let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: a.hAdapter });
                    }
                }
            }
        }
    }

    fn close(&mut self) {
        for a in self.adapters.drain(..) {
            unsafe {
                let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: a.handle });
            }
        }
    }

    /// Current readings for every GPU that reports any (re-opens the adapters if one stops answering).
    pub fn read(&mut self) -> Vec<Gpu> {
        let mut out = vec![];
        let mut failed = false;
        for a in &self.adapters {
            unsafe {
                let Some(p) = query(a.handle, KMTQAITYPE_ADAPTERPERFDATA, D3DKMT_ADAPTER_PERFDATA::default()) else {
                    failed = true;
                    continue;
                };
                let caps = query(a.handle, KMTQAITYPE_ADAPTERPERFDATA_CAPS, D3DKMT_ADAPTER_PERFDATACAPS::default());
                // Engine clocks: the busiest engine's (3D, compute, copy, video… each report one).
                let (mut core, mut core_max, mut volt) = (0u64, 0u64, 0u32);
                for node in 0..12 {
                    let Some(n) = query(a.handle, KMTQAITYPE_NODEPERFDATA, D3DKMT_NODE_PERFDATA { NodeOrdinal: node, ..Default::default() }) else { break };
                    core = core.max(n.Frequency);
                    core_max = core_max.max(n.MaxFrequency);
                    volt = volt.max(n.Voltage);
                }
                let max_fan = caps.map(|c| c.MaxFanRPM).unwrap_or(0);
                let mhz = |hz: u64| (hz > 0).then_some(hz as f64 / 1e6);
                if p.Temperature == 0 && core == 0 {
                    continue;
                }
                out.push(Gpu {
                    name: a.name.clone(),
                    integrated: a.integrated,
                    temp: (p.Temperature > 0).then_some(p.Temperature as f64 / 10.0),
                    // A card that can report fans says how fast they can go; 0 rpm then means stopped.
                    fan_rpm: (max_fan > 0).then_some(p.FanRPM),
                    fan_max: (max_fan > 0).then_some(max_fan),
                    power_pct: (!a.integrated && p.Power > 0).then_some(p.Power as f64 / 10.0),
                    core_mhz: mhz(core),
                    core_max_mhz: mhz(core_max),
                    mem_mhz: mhz(p.MemoryFrequency),
                    voltage_mv: (volt > 0).then_some(volt),
                });
            }
        }
        if failed {
            self.open();
        }
        out
    }
}

/// The CPU chip's temperature, from the GPU built into it (if there is one).
pub fn cpu_temp(gpus: &[Gpu]) -> (Option<f64>, String) {
    match gpus.iter().find(|g| g.integrated && g.temp.is_some()) {
        Some(g) => (g.temp, format!("{} on the same chip", g.name.replace("AMD ", "").replace(" Graphics", ""))),
        None => (None, String::new()),
    }
}

pub(crate) fn open_device(path: &str) -> Option<HANDLE> {
    // No access rights asked for: enough for these queries, and no admin needed.
    unsafe { CreateFileW(&HSTRING::from(path), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) }.ok()
}

pub(crate) fn property(h: HANDLE, id: STORAGE_PROPERTY_ID, buf: &mut [u8]) -> bool {
    let q = STORAGE_PROPERTY_QUERY { PropertyId: id, QueryType: PropertyStandardQuery, ..Default::default() };
    let mut got = 0u32;
    unsafe {
        DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, Some(&q as *const _ as *const _), std::mem::size_of_val(&q) as u32, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, Some(&mut got), None)
            .is_ok()
            && got >= 8
    }
}

/// A NUL-terminated ASCII string at `offset` in a storage descriptor.
pub(crate) fn ascii_at(buf: &[u8], offset: u32) -> String {
    let o = offset as usize;
    if o == 0 || o >= buf.len() {
        return String::new();
    }
    let end = buf[o..].iter().position(|&b| b == 0).map_or(buf.len(), |e| o + e);
    String::from_utf8_lossy(&buf[o..end]).trim().to_string()
}

/// Which disk each fixed drive letter lives on.
pub(crate) fn letters_by_disk() -> HashMap<u32, Vec<String>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i as u8) as char);
        if unsafe { GetDriveTypeW(&HSTRING::from(format!("{letter}\\"))) } != DRIVE_FIXED {
            continue;
        }
        let Some(h) = open_device(&format!(r"\\.\{letter}")) else { continue };
        let mut buf = [0u8; 256];
        let mut got = 0u32;
        let ok = unsafe { DeviceIoControl(h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, 0, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, Some(&mut got), None) };
        if ok.is_ok() {
            let v = unsafe { &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS) };
            let extents = unsafe { std::slice::from_raw_parts(v.Extents.as_ptr(), (v.NumberOfDiskExtents as usize).min(8)) };
            for e in extents {
                let l = out.entry(e.DiskNumber).or_default();
                if !l.contains(&letter) {
                    l.push(letter.clone());
                }
            }
        }
        unsafe {
            let _ = CloseHandle(h);
        }
    }
    out
}

/// Every drive that reports a temperature, with its name and drive letters.
pub fn drives() -> Vec<Drive> {
    let letters = letters_by_disk();
    let mut out = vec![];
    for n in 0..16u32 {
        let Some(h) = open_device(&format!(r"\\.\PhysicalDrive{n}")) else { continue };
        let mut buf = [0u8; 512];
        if property(h, StorageDeviceTemperatureProperty, &mut buf) {
            let d = unsafe { &*(buf.as_ptr() as *const STORAGE_TEMPERATURE_DATA_DESCRIPTOR) };
            let count = (d.InfoCount as usize).min((buf.len() - 24) / std::mem::size_of::<STORAGE_TEMPERATURE_INFO>());
            let infos = unsafe { std::slice::from_raw_parts(d.TemperatureInfo.as_ptr(), count) };
            if let Some(first) = infos.first().filter(|t| t.Temperature > 0) {
                let hottest = infos.iter().map(|t| t.Temperature).max().unwrap_or(first.Temperature);
                let (warning, critical) = (d.WarningTemperature, d.CriticalTemperature);
                let mut desc = [0u8; 1024];
                let name = if property(h, StorageDeviceProperty, &mut desc) {
                    let sd = unsafe { &*(desc.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
                    let vendor = ascii_at(&desc, sd.VendorIdOffset);
                    let product = ascii_at(&desc, sd.ProductIdOffset);
                    if vendor.is_empty() || product.starts_with(&vendor) { product } else { format!("{vendor} {product}") }
                } else {
                    String::new()
                };
                out.push(Drive {
                    name: if name.is_empty() { format!("Drive {n}") } else { name },
                    letters: letters.get(&n).cloned().unwrap_or_default(),
                    temp: first.Temperature as f64,
                    hottest: hottest as f64,
                    warning: if warning > 0 { warning as f64 } else { 70.0 },
                    critical: if critical > 0 { critical as f64 } else { 80.0 },
                });
            }
        }
        unsafe {
            let _ = CloseHandle(h);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_gpu_detection() {
        assert!(on_cpu("AMD Radeon 780M Graphics", 0x1002, "AMD Ryzen 7 8700G w/ Radeon 780M Graphics"));
        assert!(!on_cpu("AMD Radeon RX 6800 XT", 0x1002, "AMD Ryzen 7 8700G w/ Radeon 780M Graphics"));
        assert!(on_cpu("AMD Radeon Graphics", 0x1002, "AMD Ryzen 9 7950X 16-Core Processor"));
        assert!(on_cpu("Intel UHD Graphics 770", 0x8086, "13th Gen Intel Core i7-13700K"));
        assert!(!on_cpu("Intel Arc A770 Graphics", 0x8086, "13th Gen Intel Core i7-13700K"));
        assert!(!on_cpu("NVIDIA GeForce RTX 4070", 0x10de, "13th Gen Intel Core i7-13700K"));
    }

    /// Real readings on this machine (prints them).
    #[test]
    #[ignore = "needs a GPU: prints its sensor readings"]
    fn live_sensors() {
        let cpu = "AMD Ryzen 7 8700G w/ Radeon 780M Graphics";
        let gpus = Gpus::new(cpu).read();
        println!("{gpus:#?}\ncpu: {:?}\n{:#?}", cpu_temp(&gpus), drives());
        assert!(!gpus.is_empty());
    }
}
