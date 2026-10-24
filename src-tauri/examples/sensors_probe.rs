//! Diagnostics: which hardware sensors Windows itself exposes without admin rights or a helper app
//! (no HWiNFO, no kernel driver). Read-only.
//!   - GPUs through the graphics kernel (D3DKMT perf data: what Task Manager shows), one per adapter
//!   - NVMe/SSD temperatures through IOCTL_STORAGE_QUERY_PROPERTY on a zero-access drive handle
//!   cargo run --example sensors_probe
use windows::core::Interface;
use windows::Wdk::Graphics::Direct3D::*;
use windows::Win32::Foundation::{CloseHandle, LUID};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Ioctl::*;
use windows::Win32::System::IO::DeviceIoControl;

fn adapter_name(luid: LUID) -> String {
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else { return "?".into() };
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            if let Ok(d) = a.GetDesc1() {
                if d.AdapterLuid.LowPart == luid.LowPart && d.AdapterLuid.HighPart == luid.HighPart {
                    let n = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                    return String::from_utf16_lossy(&d.Description[..n]);
                }
            }
            i += 1;
        }
        let _ = f.cast::<IDXGIFactory1>();
        "(not a DXGI adapter)".into()
    }
}

unsafe fn query<T: Default>(h: u32, ty: KMTQUERYADAPTERINFOTYPE, mut v: T) -> Option<T> {
    let mut q = D3DKMT_QUERYADAPTERINFO { hAdapter: h, Type: ty, pPrivateDriverData: &mut v as *mut T as *mut _, PrivateDriverDataSize: std::mem::size_of::<T>() as u32 };
    (D3DKMTQueryAdapterInfo(&mut q).0 >= 0).then_some(v)
}

fn gpus() {
    unsafe {
        let mut e = D3DKMT_ENUMADAPTERS2::default();
        let _ = D3DKMTEnumAdapters2(&mut e);
        let mut list = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
        e.pAdapters = list.as_mut_ptr();
        let st = D3DKMTEnumAdapters2(&mut e);
        println!("D3DKMTEnumAdapters2: {:#x}, {} adapters", st.0, e.NumAdapters);
        for a in &list[..e.NumAdapters as usize] {
            println!("\n== {} ==", adapter_name(a.AdapterLuid));
            match query(a.hAdapter, KMTQAITYPE_ADAPTERPERFDATA, D3DKMT_ADAPTER_PERFDATA::default()) {
                Some(p) => println!(
                    "  temperature {:.1} C | fan {} rpm | power {:.1}% | memory clock {} MHz (max {}) | mem bandwidth {} | PCIe bandwidth {}",
                    p.Temperature as f64 / 10.0, p.FanRPM, p.Power as f64 / 10.0, p.MemoryFrequency / 1_000_000, p.MaxMemoryFrequency / 1_000_000, p.MemoryBandwidth, p.PCIEBandwidth
                ),
                None => println!("  no adapter perf data"),
            }
            if let Some(c) = query(a.hAdapter, KMTQAITYPE_ADAPTERPERFDATA_CAPS, D3DKMT_ADAPTER_PERFDATACAPS::default()) {
                println!("  caps: max fan {} rpm, temp max {:.1} C, warning {:.1} C", c.MaxFanRPM, c.TemperatureMax as f64 / 10.0, c.TemperatureWarning as f64 / 10.0);
            }
            for node in 0..6 {
                let v = D3DKMT_NODE_PERFDATA { NodeOrdinal: node, ..Default::default() };
                if let Some(n) = query(a.hAdapter, KMTQAITYPE_NODEPERFDATA, v) {
                    if n.Frequency > 0 || n.Voltage > 0 {
                        println!("  engine {node}: {} MHz (max {}), {} mV", n.Frequency / 1_000_000, n.MaxFrequency / 1_000_000, n.Voltage);
                    }
                }
            }
            let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: a.hAdapter });
        }
    }
}

fn drives() {
    println!("\n== drives ==");
    for i in 0..6 {
        let path = windows::core::HSTRING::from(format!(r"\\.\PhysicalDrive{i}"));
        unsafe {
            // Zero access rights: no admin needed, and enough for property queries.
            let Ok(h) = CreateFileW(&path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) else { continue };
            let q = STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceTemperatureProperty, QueryType: PropertyStandardQuery, ..Default::default() };
            let mut buf = [0u8; 512];
            let mut got = 0u32;
            let ok = DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, Some(&q as *const _ as *const _), std::mem::size_of_val(&q) as u32, Some(buf.as_mut_ptr() as *mut _), buf.len() as u32, Some(&mut got), None);
            match ok {
                Ok(()) => {
                    let d = &*(buf.as_ptr() as *const STORAGE_TEMPERATURE_DATA_DESCRIPTOR);
                    let infos = std::slice::from_raw_parts(d.TemperatureInfo.as_ptr(), d.InfoCount as usize);
                    let temps: Vec<String> = infos.iter().map(|t| format!("sensor {}: {} C", t.Index, t.Temperature)).collect();
                    println!("  PhysicalDrive{i}: {} (warning {} C, critical {} C)", temps.join(", "), d.WarningTemperature, d.CriticalTemperature);
                }
                Err(e) => println!("  PhysicalDrive{i}: no temperature ({})", e.message()),
            }
            let _ = CloseHandle(h);
        }
    }
}

fn main() {
    gpus();
    drives();
}
