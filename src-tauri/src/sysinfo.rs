//! What this PC is, for the Task Manager's System Info page and the CPU details on Performance:
//! Windows (edition, version, build, install date, Secure Boot, TPM), the board and BIOS, the
//! processor (cores, caches, base speed, virtualization), each memory module (from the firmware's
//! SMBIOS tables: size, type, speed, maker, part number), graphics adapters with driver versions,
//! displays, disks and volumes, and network adapters. Everything is read without admin rights.

use std::sync::Mutex;

use serde::Serialize;
use windows::core::{Interface, GUID, HSTRING, PWSTR};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{CloseHandle, HLOCAL, LocalFree, ERROR_SUCCESS};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKET_ADDRESS};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Ioctl::*;
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerReadFriendlyName};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{
    GetComputerNameExW, GetFirmwareType, GetLogicalProcessorInformationEx, GetPhysicallyInstalledSystemMemory, GetSystemFirmwareTable, GetTickCount64,
    ComputerNamePhysicalDnsHostname, FirmwareTypeBios, FirmwareTypeUefi, RelationAll, FIRMWARE_TABLE_PROVIDER, FIRMWARE_TYPE,
};
use windows::Win32::System::Threading::{IsProcessorFeaturePresent, PF_VIRT_FIRMWARE_ENABLED};
use windows::Win32::System::Time::{GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION};
use windows::Win32::System::TpmBaseServices::{Tbsi_GetDeviceInfo, TPM_DEVICE_INFO};
use windows::Win32::System::IO::DeviceIoControl;

use crate::sensors;

/* ---------- registry ---------- */

pub(crate) fn reg_str(root: HKEY, path: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (len as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub(crate) fn reg_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut len)) };
    (r == ERROR_SUCCESS).then_some(v)
}

const CPU_KEY: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";
const BIOS_KEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
const NT_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

/* ---------- the processor ---------- */

#[derive(Serialize, Clone, Default)]
pub struct CpuInfo {
    pub name: String,
    pub vendor: String,
    /// Nominal (base) clock, MHz.
    pub base_mhz: u32,
    pub sockets: u32,
    pub cores: u32,
    pub logical: u32,
    /// Total cache at each level, bytes.
    pub l1: u64,
    pub l2: u64,
    pub l3: u64,
    /// Hardware virtualization switched on in the firmware.
    pub virtualization: bool,
    /// Windows itself runs as a guest of another machine.
    pub vm: bool,
    /// The active power plan's name.
    pub power_plan: String,
}

static CPU: Mutex<Option<CpuInfo>> = Mutex::new(None);

pub fn cpu() -> CpuInfo {
    if let Some(c) = CPU.lock().unwrap().clone() {
        let mut c = c;
        c.power_plan = power_plan(); // can change at any time
        return c;
    }
    let mut c = CpuInfo {
        name: reg_str(HKEY_LOCAL_MACHINE, CPU_KEY, "ProcessorNameString").unwrap_or_default(),
        vendor: reg_str(HKEY_LOCAL_MACHINE, CPU_KEY, "VendorIdentifier").unwrap_or_default(),
        base_mhz: reg_dword(HKEY_LOCAL_MACHINE, CPU_KEY, "~MHz").unwrap_or(0),
        logical: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        virtualization: unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool(),
        vm: is_guest(),
        ..Default::default()
    };
    // Cores, packages and caches, one variable-length record each.
    let mut len = 0u32;
    unsafe {
        let _ = GetLogicalProcessorInformationEx(RelationAll, None, &mut len);
    }
    if len > 0 {
        let mut buf = vec![0u8; len as usize];
        if unsafe { GetLogicalProcessorInformationEx(RelationAll, Some(buf.as_mut_ptr() as *mut _), &mut len) }.is_ok() {
            let mut off = 0usize;
            while off + 8 <= len as usize {
                let rel = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
                let size = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap()) as usize;
                if size == 0 {
                    break;
                }
                match rel {
                    0 => c.cores += 1,
                    3 => c.sockets += 1,
                    2 if off + 20 <= buf.len() => {
                        // CACHE_RELATIONSHIP: Level (u8), Associativity, LineSize (u16), CacheSize (u32), Type.
                        let level = buf[off + 8];
                        let bytes = u32::from_le_bytes(buf[off + 12..off + 16].try_into().unwrap()) as u64;
                        match level {
                            1 => c.l1 += bytes,
                            2 => c.l2 += bytes,
                            3 => c.l3 += bytes,
                            _ => {}
                        }
                    }
                    _ => {}
                }
                off += size;
            }
        }
    }
    *CPU.lock().unwrap() = Some(c.clone());
    c.power_plan = power_plan();
    c
}

/// A hypervisor is present and Windows isn't its root partition (so this is a virtual machine).
fn is_guest() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        if __cpuid(1).ecx & (1 << 31) == 0 {
            return false;
        }
        // Hyper-V's feature leaf: CreatePartitions is only granted to the root (the real PC).
        let vendor = __cpuid(0x4000_0000);
        let hv = vendor.ebx == 0x7263_694d && vendor.ecx == 0x666f_736f && vendor.edx == 0x7648_2074; // "Microsoft Hv"
        if hv {
            return __cpuid(0x4000_0003).ebx & 1 == 0;
        }
        true
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

fn power_plan() -> String {
    unsafe {
        let mut guid: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut guid) != ERROR_SUCCESS || guid.is_null() {
            return String::new();
        }
        let mut buf = [0u8; 512];
        let mut len = buf.len() as u32;
        let ok = PowerReadFriendlyName(None, Some(guid), None, None, Some(buf.as_mut_ptr()), &mut len) == ERROR_SUCCESS;
        let _ = LocalFree(Some(HLOCAL(guid as _)));
        if !ok {
            return String::new();
        }
        let w: Vec<u16> = buf[..len as usize].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&w).trim_end_matches('\0').to_string()
    }
}

/* ---------- memory modules (SMBIOS type 17) ---------- */

#[derive(Serialize, Clone, Default, Debug)]
pub struct MemModule {
    pub slot: String,
    pub size_mb: u64,
    /// "DDR5", "DDR4"…
    pub kind: String,
    /// Rated and configured speed, MT/s.
    pub speed: u32,
    pub configured: u32,
    pub maker: String,
    pub part: String,
}

#[derive(Default)]
struct Smbios {
    modules: Vec<MemModule>,
    slots: u32,
}

fn smbios() -> Smbios {
    let sig = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    let need = unsafe { GetSystemFirmwareTable(sig, 0, None) };
    let mut out = Smbios::default();
    if need == 0 {
        return out;
    }
    let mut buf = vec![0u8; need as usize];
    if unsafe { GetSystemFirmwareTable(sig, 0, Some(&mut buf)) } == 0 || buf.len() < 8 {
        return out;
    }
    // RawSMBIOSData: four version bytes, the table length, then the structures.
    let table_len = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let t = &buf[8..(8 + table_len).min(buf.len())];
    let mut off = 0usize;
    while off + 4 <= t.len() {
        let (kind, len) = (t[off], t[off + 1] as usize);
        if len < 4 || off + len > t.len() {
            break;
        }
        let f = &t[off..off + len];
        // The strings follow the formatted area, ending with a double NUL.
        let mut strings = vec![];
        let mut p = off + len;
        loop {
            let end = t[p..].iter().position(|&b| b == 0).map_or(t.len(), |e| p + e);
            if end == p {
                p += 1;
                break;
            }
            strings.push(String::from_utf8_lossy(&t[p..end]).trim().to_string());
            p = end + 1;
            if p >= t.len() {
                break;
            }
        }
        if strings.is_empty() {
            p += 1; // an empty string set is two NULs
        }
        let s = |i: usize| -> String { f.get(i).and_then(|&n| (n as usize).checked_sub(1)).and_then(|n| strings.get(n)).cloned().unwrap_or_default() };
        let u16_at = |i: usize| -> u16 { f.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0) };
        let u32_at = |i: usize| -> u32 { f.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0) };
        match kind {
            16 => out.slots += u16_at(0x0D) as u32,
            17 => {
                let size = u16_at(0x0C);
                let size_mb = match size {
                    0 | 0xFFFF => 0,
                    0x7FFF => u32_at(0x1C) as u64,
                    s if s & 0x8000 != 0 => ((s & 0x7FFF) as u64) / 1024,
                    s => s as u64,
                };
                if size_mb > 0 {
                    let kind = match f.get(0x12).copied().unwrap_or(0) {
                        0x12 => "DDR",
                        0x13 => "DDR2",
                        0x18 => "DDR3",
                        0x1A => "DDR4",
                        0x1B => "LPDDR",
                        0x1C => "LPDDR2",
                        0x1D => "LPDDR3",
                        0x1E => "LPDDR4",
                        0x22 => "DDR5",
                        0x23 => "LPDDR5",
                        _ => "",
                    };
                    let mut speed = u16_at(0x15) as u32;
                    if speed == 0xFFFF {
                        speed = u32_at(0x54);
                    }
                    let mut configured = u16_at(0x20) as u32;
                    if configured == 0xFFFF {
                        configured = u32_at(0x58);
                    }
                    let loc = s(0x10);
                    let bank = s(0x11);
                    out.modules.push(MemModule {
                        // Boards often name every slot "DIMM 1" and tell them apart by channel.
                        slot: match (loc.is_empty(), bank.is_empty()) {
                            (true, _) => bank,
                            (false, true) => loc,
                            (false, false) => format!("{loc} · {bank}"),
                        },
                        size_mb,
                        kind: kind.into(),
                        speed,
                        configured,
                        maker: s(0x17),
                        part: s(0x1A),
                    });
                }
            }
            127 => break,
            _ => {}
        }
        off = p;
    }
    out
}

/* ---------- graphics and displays ---------- */

#[derive(Serialize, Clone, Default)]
pub struct GpuInfo {
    pub name: String,
    pub vram: u64,
    pub shared: u64,
    pub driver: String,
    pub luid: i64,
}

pub fn gpus() -> Vec<GpuInfo> {
    let mut out = vec![];
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
        for i in 0..16 {
            let Ok(a) = f.EnumAdapters1(i) else { break };
            let Ok(d) = a.GetDesc1() else { continue };
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let name = String::from_utf16_lossy(&d.Description[..d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len())]);
            // The user-mode driver's version, a.b.c.d packed in 64 bits.
            let driver = a.CheckInterfaceSupport(&IDXGIDevice::IID).map(|v| format!("{}.{}.{}.{}", v >> 48, (v >> 32) & 0xFFFF, (v >> 16) & 0xFFFF, v & 0xFFFF)).unwrap_or_default();
            out.push(GpuInfo {
                name,
                vram: d.DedicatedVideoMemory as u64,
                shared: d.SharedSystemMemory as u64,
                driver,
                luid: ((d.AdapterLuid.HighPart as i64) << 32) | d.AdapterLuid.LowPart as i64,
            });
        }
    }
    out
}

#[derive(Serialize, Clone, Default)]
pub struct DisplayInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub hz: f64,
    /// The graphics adapter it's plugged into.
    pub gpu: String,
}

pub fn displays(gpus: &[GpuInfo]) -> Vec<DisplayInfo> {
    let mut out = vec![];
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) != ERROR_SUCCESS {
            return out;
        }
        for p in &paths[..np as usize] {
            let mut tn = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tn.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tn.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tn.header.adapterId = p.targetInfo.adapterId;
            tn.header.id = p.targetInfo.id;
            let name = if DisplayConfigGetDeviceInfo(&mut tn.header) == 0 {
                let w = &tn.monitorFriendlyDeviceName;
                String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
            } else {
                String::new()
            };
            let rr = p.targetInfo.refreshRate;
            let hz = if rr.Denominator > 0 { rr.Numerator as f64 / rr.Denominator as f64 } else { 0.0 };
            let idx = p.sourceInfo.Anonymous.modeInfoIdx as usize;
            let (mut width, mut height) = (0, 0);
            if let Some(m) = modes.get(idx).filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) {
                width = m.Anonymous.sourceMode.width;
                height = m.Anonymous.sourceMode.height;
            }
            let luid = ((p.targetInfo.adapterId.HighPart as i64) << 32) | p.targetInfo.adapterId.LowPart as i64;
            out.push(DisplayInfo {
                name: if name.is_empty() { "Display".into() } else { name },
                width,
                height,
                hz,
                gpu: gpus.iter().find(|g| g.luid == luid).map(|g| g.name.clone()).unwrap_or_default(),
            });
        }
    }
    out
}

/* ---------- disks and volumes ---------- */

#[derive(Serialize, Clone, Default)]
pub struct DiskInfo {
    pub number: u32,
    pub model: String,
    pub size: u64,
    /// "NVMe", "SATA", "USB"…
    pub bus: String,
    pub ssd: Option<bool>,
    pub letters: Vec<String>,
}

#[derive(Serialize, Clone, Default)]
pub struct VolumeInfo {
    pub letter: String,
    pub label: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    /// "Fixed", "Removable", "Network", "Optical".
    pub kind: String,
}

pub fn disks() -> Vec<DiskInfo> {
    let letters = sensors::letters_by_disk();
    let mut out = vec![];
    for n in 0..16u32 {
        let Some(h) = sensors::open_device(&format!(r"\\.\PhysicalDrive{n}")) else { continue };
        let mut d = DiskInfo { number: n, letters: letters.get(&n).cloned().unwrap_or_default(), ..Default::default() };
        let mut buf = [0u8; 1024];
        if sensors::property(h, StorageDeviceProperty, &mut buf) {
            let sd = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
            let vendor = sensors::ascii_at(&buf, sd.VendorIdOffset);
            let product = sensors::ascii_at(&buf, sd.ProductIdOffset);
            d.model = if vendor.is_empty() || product.starts_with(&vendor) { product } else { format!("{vendor} {product}") };
            d.bus = match sd.BusType.0 {
                1 => "SCSI",
                2 => "ATAPI",
                3 => "ATA",
                4 => "FireWire",
                6 => "Fibre Channel",
                7 => "USB",
                8 => "RAID",
                10 => "SAS",
                11 => "SATA",
                12 => "SD card",
                13 => "MMC",
                14 => "Virtual",
                15 => "Virtual",
                17 => "NVMe",
                _ => "",
            }
            .into();
        }
        let mut sp = [0u8; 64];
        if sensors::property(h, StorageDeviceSeekPenaltyProperty, &mut sp) {
            let s = unsafe { &*(sp.as_ptr() as *const DEVICE_SEEK_PENALTY_DESCRIPTOR) };
            d.ssd = Some(!s.IncursSeekPenalty);
        }
        let mut geo = [0u8; 256];
        let mut got = 0u32;
        if unsafe { DeviceIoControl(h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None, 0, Some(geo.as_mut_ptr() as *mut _), geo.len() as u32, Some(&mut got), None) }.is_ok() && got >= 32 {
            d.size = i64::from_le_bytes(geo[24..32].try_into().unwrap()).max(0) as u64;
        }
        unsafe {
            let _ = CloseHandle(h);
        }
        out.push(d);
    }
    out
}

pub fn volumes() -> Vec<VolumeInfo> {
    let mut out = vec![];
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i as u8) as char);
        let root = HSTRING::from(format!("{letter}\\"));
        let kind = match unsafe { GetDriveTypeW(&root) } {
            2 => "Removable",
            3 => "Fixed",
            4 => "Network",
            5 => "Optical",
            6 => "RAM disk",
            _ => continue,
        };
        let (mut label, mut fs) = ([0u16; 261], [0u16; 64]);
        let named = unsafe { GetVolumeInformationW(&root, Some(&mut label), None, None, None, Some(&mut fs)) }.is_ok();
        let (mut free, mut total) = (0u64, 0u64);
        let sized = unsafe { GetDiskFreeSpaceExW(&root, None, Some(&mut total), Some(&mut free)) }.is_ok();
        if !named && !sized {
            continue; // an empty card reader or disc drive
        }
        let z = |w: &[u16]| String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]);
        out.push(VolumeInfo { letter, label: z(&label), fs: z(&fs), total, free, kind: kind.into() });
    }
    out
}

/* ---------- network adapters ---------- */

#[derive(Serialize, Clone, Default)]
pub struct NetAdapter {
    pub name: String,
    pub description: String,
    /// "Ethernet", "Wi-Fi", "VPN"…
    pub kind: String,
    pub mac: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
    /// Bits per second.
    pub speed: u64,
}

pub(crate) fn sockaddr_ip(a: &SOCKET_ADDRESS) -> Option<std::net::IpAddr> {
    if a.lpSockaddr.is_null() {
        return None;
    }
    unsafe {
        let sa = &*(a.lpSockaddr as *const SOCKADDR);
        let raw = std::slice::from_raw_parts(a.lpSockaddr as *const u8, a.iSockaddrLength.max(0) as usize);
        if sa.sa_family == AF_INET && raw.len() >= 8 {
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7])))
        } else if sa.sa_family == AF_INET6 && raw.len() >= 24 {
            let b: [u8; 16] = raw[8..24].try_into().ok()?;
            Some(std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)))
        } else {
            None
        }
    }
}

pub fn adapters() -> Vec<NetAdapter> {
    let mut out = vec![];
    unsafe {
        let mut len = 32 * 1024u32;
        let mut buf: Vec<u64> = vec![0; len as usize / 8];
        let mut r = GetAdaptersAddresses(AF_UNSPEC.0 as u32, GAA_FLAG_INCLUDE_GATEWAYS, None, Some(buf.as_mut_ptr() as *mut _), &mut len);
        if r == 111 {
            // ERROR_BUFFER_OVERFLOW
            buf = vec![0; len as usize / 8 + 1];
            r = GetAdaptersAddresses(AF_UNSPEC.0 as u32, GAA_FLAG_INCLUDE_GATEWAYS, None, Some(buf.as_mut_ptr() as *mut _), &mut len);
        }
        if r != 0 {
            return out;
        }
        let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !p.is_null() {
            let a = &*p;
            p = a.Next;
            if a.OperStatus != IfOperStatusUp || a.IfType == 24 {
                continue; // down, or the loopback
            }
            let name = a.FriendlyName.to_string().unwrap_or_default();
            let description = a.Description.to_string().unwrap_or_default();
            let l = format!("{name} {description}").to_lowercase();
            let kind = if a.IfType == 71 || l.contains("wi-fi") || l.contains("wireless") {
                "Wi-Fi"
            } else if ["vpn", "tunnel", "wintun", "tap-", "wireguard", "tailscale", "radmin", "zerotier"].iter().any(|k| l.contains(k)) {
                "VPN"
            } else if l.contains("bluetooth") {
                "Bluetooth"
            } else if l.contains("hyper-v") || l.contains("virtual") || l.contains("vethernet") {
                "Virtual"
            } else {
                "Ethernet"
            };
            let mac = a.PhysicalAddress[..(a.PhysicalAddressLength as usize).min(8)].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
            let mut ad = NetAdapter { name, description, kind: kind.into(), mac, speed: a.ReceiveLinkSpeed.max(a.TransmitLinkSpeed), ..Default::default() };
            if ad.speed == u64::MAX {
                ad.speed = 0;
            }
            let mut u = a.FirstUnicastAddress;
            while !u.is_null() {
                match sockaddr_ip(&(*u).Address) {
                    Some(std::net::IpAddr::V4(ip)) => ad.ipv4.push(ip.to_string()),
                    Some(std::net::IpAddr::V6(ip)) if !ip.is_unicast_link_local() => ad.ipv6.push(ip.to_string()),
                    _ => {}
                }
                u = (*u).Next;
            }
            let mut g = a.FirstGatewayAddress;
            while !g.is_null() {
                if let Some(ip) = sockaddr_ip(&(*g).Address) {
                    ad.gateway.push(ip.to_string());
                }
                g = (*g).Next;
            }
            let mut d = a.FirstDnsServerAddress;
            while !d.is_null() {
                if let Some(ip) = sockaddr_ip(&(*d).Address) {
                    if !matches!(ip, std::net::IpAddr::V6(v) if v.segments()[0] == 0xfec0) {
                        ad.dns.push(ip.to_string());
                    }
                }
                d = (*d).Next;
            }
            if ad.ipv4.is_empty() && ad.ipv6.is_empty() {
                continue;
            }
            out.push(ad);
        }
    }
    out
}

/* ---------- the whole picture ---------- */

#[derive(Serialize, Clone, Default)]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    /// Unix seconds.
    pub installed: u64,
    pub booted: u64,
    pub computer: String,
    pub user: String,
    pub owner: String,
    pub firmware: String,
    pub secure_boot: Option<bool>,
    /// "2.0", "1.2", or empty when there's no TPM.
    pub tpm: String,
    pub time_zone: String,
    pub maker: String,
    pub model: String,
    pub board: String,
    pub bios: String,
    pub bios_date: String,
    pub cpu: CpuInfo,
    pub mem_installed: u64,
    pub mem_usable: u64,
    pub mem_slots: u32,
    pub modules: Vec<MemModule>,
    pub gpus: Vec<GpuInfo>,
    pub displays: Vec<DisplayInfo>,
    pub disks: Vec<DiskInfo>,
    pub volumes: Vec<VolumeInfo>,
    pub adapters: Vec<NetAdapter>,
}

fn join_nonempty(parts: &[String]) -> String {
    const FILLER: [&str; 7] = ["to be filled by o.e.m.", "system manufacturer", "system product name", "system version", "default string", "not applicable", "none"];
    let mut v: Vec<&str> = parts.iter().map(|s| s.trim()).filter(|s| !s.is_empty() && !FILLER.contains(&s.to_ascii_lowercase().as_str())).collect();
    v.dedup();
    v.join(" ")
}

pub fn system() -> SystemInfo {
    let hk = HKEY_LOCAL_MACHINE;
    let nt = |n: &str| reg_str(hk, NT_KEY, n).unwrap_or_default();
    let build: u32 = nt("CurrentBuild").parse().unwrap_or(0);
    let ubr = reg_dword(hk, NT_KEY, "UBR").unwrap_or(0);
    // Windows 11 still calls itself "Windows 10" in ProductName.
    let mut os_name = nt("ProductName");
    if build >= 22000 {
        os_name = os_name.replace("Windows 10", "Windows 11");
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let uptime = unsafe { GetTickCount64() } / 1000;
    let computer = unsafe {
        let mut buf = [0u16; 256];
        let mut n = buf.len() as u32;
        if GetComputerNameExW(ComputerNamePhysicalDnsHostname, Some(PWSTR(buf.as_mut_ptr())), &mut n).is_ok() { String::from_utf16_lossy(&buf[..n as usize]) } else { std::env::var("COMPUTERNAME").unwrap_or_default() }
    };
    let firmware = unsafe {
        let mut f = FIRMWARE_TYPE::default();
        match GetFirmwareType(&mut f) {
            Ok(()) if f == FirmwareTypeUefi => "UEFI",
            Ok(()) if f == FirmwareTypeBios => "Legacy BIOS",
            _ => "",
        }
    };
    let tpm = unsafe {
        let mut info = TPM_DEVICE_INFO { structVersion: 1, ..Default::default() };
        if Tbsi_GetDeviceInfo(std::mem::size_of::<TPM_DEVICE_INFO>() as u32, &mut info as *mut _ as *mut _) == 0 {
            match info.tpmVersion {
                1 => "1.2",
                2 => "2.0",
                _ => "",
            }
        } else {
            ""
        }
    };
    let time_zone = unsafe {
        let mut tz = DYNAMIC_TIME_ZONE_INFORMATION::default();
        GetDynamicTimeZoneInformation(&mut tz);
        let k = &tz.TimeZoneKeyName;
        String::from_utf16_lossy(&k[..k.iter().position(|&c| c == 0).unwrap_or(k.len())])
    };
    let bios = |n: &str| reg_str(hk, BIOS_KEY, n).unwrap_or_default();
    let mut installed_kb = 0u64;
    unsafe {
        let _ = GetPhysicallyInstalledSystemMemory(&mut installed_kb);
    }
    let sm = smbios();
    let gpus = gpus();
    let mut perf = windows::Win32::System::ProcessStatus::PERFORMANCE_INFORMATION { cb: std::mem::size_of::<windows::Win32::System::ProcessStatus::PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    let usable = if unsafe { windows::Win32::System::ProcessStatus::GetPerformanceInfo(&mut perf, perf.cb) }.is_ok() { perf.PhysicalTotal as u64 * perf.PageSize as u64 } else { 0 };
    SystemInfo {
        os_version: nt("DisplayVersion"),
        os_build: if ubr > 0 { format!("{build}.{ubr}") } else { build.to_string() },
        os_name,
        installed: reg_dword(hk, NT_KEY, "InstallDate").unwrap_or(0) as u64,
        booted: now.saturating_sub(uptime),
        computer,
        user: std::env::var("USERNAME").unwrap_or_default(),
        owner: nt("RegisteredOwner"),
        firmware: firmware.into(),
        secure_boot: reg_dword(hk, r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled").map(|v| v != 0),
        tpm: tpm.into(),
        time_zone,
        maker: join_nonempty(&[bios("SystemManufacturer")]),
        model: join_nonempty(&[bios("SystemProductName"), bios("SystemVersion")]),
        board: join_nonempty(&[bios("BaseBoardManufacturer"), bios("BaseBoardProduct")]),
        bios: join_nonempty(&[bios("BIOSVendor"), bios("BIOSVersion")]),
        bios_date: bios("BIOSReleaseDate"),
        cpu: cpu(),
        mem_installed: installed_kb * 1024,
        mem_usable: usable,
        mem_slots: sm.slots,
        modules: sm.modules,
        displays: displays(&gpus),
        gpus,
        disks: disks(),
        volumes: volumes(),
        adapters: adapters(),
    }
}

/* ---------- NVMe health (the drive's own SMART log) ---------- */

#[derive(Serialize, Clone, Default, Debug)]
pub struct NvmeHealth {
    pub number: u32,
    pub critical_warning: u8,
    pub temp: f64,
    pub spare: u8,
    pub spare_threshold: u8,
    pub used: u8,
    /// Bytes.
    pub read: u64,
    pub written: u64,
    pub host_reads: u64,
    pub host_writes: u64,
    /// Minutes.
    pub busy_minutes: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log: u64,
    pub warning_temp_minutes: u32,
    pub critical_temp_minutes: u32,
}

/// The health log page of each NVMe drive Windows lets us ask.
pub fn nvme_health() -> Vec<NvmeHealth> {
    let mut out = vec![];
    for n in 0..16u32 {
        let Some(h) = sensors::open_device(&format!(r"\\.\PhysicalDrive{n}")) else { continue };
        // STORAGE_PROPERTY_QUERY (PropertyId 50: protocol specific, standard query) followed by
        // STORAGE_PROTOCOL_SPECIFIC_DATA asking for NVMe log page 2, then room for the 512-byte page.
        let mut buf = vec![0u8; 8 + 40 + 512];
        buf[0..4].copy_from_slice(&50u32.to_le_bytes());
        let psd = |o: usize, v: u32, b: &mut Vec<u8>| b[8 + o..8 + o + 4].copy_from_slice(&v.to_le_bytes());
        psd(0, 3, &mut buf); // ProtocolType: NVMe
        psd(4, 2, &mut buf); // DataType: log page
        psd(8, 2, &mut buf); // the SMART / health information log
        psd(12, 0, &mut buf);
        psd(16, 40, &mut buf); // ProtocolDataOffset (from the start of this structure)
        psd(20, 512, &mut buf); // ProtocolDataLength
        let mut got = 0u32;
        let ok = unsafe { DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, Some(buf.as_ptr() as *const _), buf.len() as u32, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, Some(&mut got), None) }.is_ok();
        unsafe {
            let _ = CloseHandle(h);
        }
        if !ok || got < 8 + 40 + 200 {
            continue;
        }
        // STORAGE_PROTOCOL_DATA_DESCRIPTOR: Version, Size, then the protocol data (offset relative to it).
        let off = u32::from_le_bytes(buf[8 + 16..8 + 20].try_into().unwrap()) as usize;
        let Some(p) = buf.get(8 + off..8 + off + 512) else { continue };
        let u128_lo = |i: usize| u64::from_le_bytes(p[i..i + 8].try_into().unwrap());
        let k = u16::from_le_bytes([p[1], p[2]]) as f64;
        out.push(NvmeHealth {
            number: n,
            critical_warning: p[0],
            temp: if k > 0.0 { k - 273.15 } else { 0.0 },
            spare: p[3],
            spare_threshold: p[4],
            used: p[5],
            read: u128_lo(32).saturating_mul(512_000),
            written: u128_lo(48).saturating_mul(512_000),
            host_reads: u128_lo(64),
            host_writes: u128_lo(80),
            busy_minutes: u128_lo(96),
            power_cycles: u128_lo(112),
            power_on_hours: u128_lo(128),
            unsafe_shutdowns: u128_lo(144),
            media_errors: u128_lo(160),
            error_log: u128_lo(176),
            warning_temp_minutes: u32::from_le_bytes(p[192..196].try_into().unwrap()),
            critical_temp_minutes: u32::from_le_bytes(p[196..200].try_into().unwrap()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test --release sysinfo::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live() {
        let s = system();
        println!("{}", serde_json::to_string_pretty(&s).unwrap());
        println!("{:#?}", nvme_health());
    }

    #[test]
    fn cpu_basics() {
        let c = cpu();
        assert!(c.logical >= 1 && c.cores >= 1 && c.cores <= c.logical, "{} cores / {} logical", c.cores, c.logical);
        assert!(c.l1 > 0 && c.l2 > 0, "caches {} {} {}", c.l1, c.l2, c.l3);
    }
}
