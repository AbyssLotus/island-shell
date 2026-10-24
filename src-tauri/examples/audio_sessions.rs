//! Diagnostic: every audio session on every output device, with owner process id and live peak
//! levels. Run outside the sandbox: `audio_sessions.exe > sessions.txt`.

use windows::core::Interface;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
        let devices = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).unwrap();
        for d in 0..devices.GetCount().unwrap() {
            let dev = devices.Item(d).unwrap();
            let id = dev.GetId().map(|p| p.to_string().unwrap_or_default()).unwrap_or_default();
            let dmeter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None).unwrap();
            println!("device {d} {id} device-peak {:.3}", dmeter.GetPeakValue().unwrap_or(-1.0));
            let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None).unwrap();
            let list = mgr.GetSessionEnumerator().unwrap();
            for i in 0..list.GetCount().unwrap() {
                let c = list.GetSession(i).unwrap();
                let c2: IAudioSessionControl2 = c.cast().unwrap();
                let pid = c2.GetProcessId().unwrap_or(0);
                let meter: IAudioMeterInformation = c.cast().unwrap();
                let mut peaks = vec![];
                for _ in 0..8 {
                    peaks.push((meter.GetPeakValue().unwrap_or(-1.0) * 100.0).round());
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                println!("   session pid {pid} state {} peaks {peaks:?}", c.GetState().map(|s| s.0).unwrap_or(-1));
            }
        }
    }
}
