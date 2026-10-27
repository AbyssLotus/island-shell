//! Windows helpers shared by module backends (Wallpaper Engine, Games, VPN, Voicemeeter): finding
//! Steam and its libraries, looking up running programs, making image thumbnails with Windows' own
//! decoder, and starting programs without a window (rule 2: no console ever flashes).
//!
//! Off Windows everything compiles and answers "nothing found", so modules' own tests run anywhere.

use std::path::{Path, PathBuf};

// ---------- Steam ----------

pub mod steam {
    use super::*;

    /// Steam's folder (HKCU\Software\Valve\Steam\SteamPath), if it's installed.
    pub fn dir() -> Option<PathBuf> {
        let p = PathBuf::from(crate::reg::user_str("Software\\Valve\\Steam", "SteamPath")?.replace('/', "\\"));
        p.join("steam.exe").is_file().then_some(p)
    }

    /// Every Steam library folder: Steam's own, then the ones in libraryfolders.vdf.
    pub fn libraries(steam: &Path) -> Vec<PathBuf> {
        let mut libs = vec![steam.to_path_buf()];
        if let Ok(b) = std::fs::read(steam.join("steamapps").join("libraryfolders.vdf")) {
            let t = tokens(&decode(&b));
            for w in t.windows(2) {
                if w[0] == "path" {
                    let p = PathBuf::from(&w[1]);
                    if !libs.iter().any(|l| l.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy())) {
                        libs.push(p);
                    }
                }
            }
        }
        libs
    }

    /// Quoted strings of a Valve KeyValues (.vdf/.acf) file, in order. Braces are skipped; that's
    /// enough to pick out the flat keys we need.
    pub fn tokens(text: &str) -> Vec<String> {
        let mut out = vec![];
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '"' {
                continue;
            }
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            s.push(n);
                        }
                    }
                    '"' => break,
                    _ => s.push(c),
                }
            }
            out.push(s);
        }
        out
    }

    /// Steam writes names as UTF-8, but some older manifests hold Windows-1252 bytes (e.g. "™").
    pub fn decode(bytes: &[u8]) -> String {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes
                .iter()
                .map(|&b| match b {
                    0x99 => '\u{2122}',
                    0x80 => '\u{20ac}',
                    0x92 => '\u{2019}',
                    0x93 => '\u{201c}',
                    0x94 => '\u{201d}',
                    0x96 => '\u{2013}',
                    0x97 => '\u{2014}',
                    _ => b as char,
                })
                .collect(),
        }
    }
}

// ---------- the registry ----------

pub mod reg {
    /// A string value under HKEY_CURRENT_USER.
    #[cfg(windows)]
    pub fn user_str(sub: &str, value: &str) -> Option<String> {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        unsafe {
            RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(sub), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).ok().ok()?;
        }
        let n = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    /// A DWORD value under HKEY_CURRENT_USER.
    #[cfg(windows)]
    pub fn user_dword(sub: &str, value: &str) -> Option<u32> {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut v = 0u32;
        let mut size = 4u32;
        unsafe {
            RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(sub), &HSTRING::from(value), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut size)).ok().ok()?;
        }
        Some(v)
    }

    #[cfg(not(windows))]
    pub fn user_str(_sub: &str, _value: &str) -> Option<String> {
        None
    }

    #[cfg(not(windows))]
    pub fn user_dword(_sub: &str, _value: &str) -> Option<u32> {
        None
    }
}

// ---------- processes ----------

pub mod process {
    use super::*;

    /// The id of a running program, by image name ("Discord.exe"; any case).
    #[cfg(windows)]
    pub fn find(exe_name: &str) -> Option<u32> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut found = None;
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(exe_name) {
                    found = Some(e.th32ProcessID);
                    break;
                }
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
            found
        }
    }

    /// Full path of a process image as Windows spells it, or empty when it can't be read.
    #[cfg(windows)]
    pub fn path_of(pid: u32) -> String {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            if ok {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                String::new()
            }
        }
    }

    #[cfg(not(windows))]
    pub fn find(_exe_name: &str) -> Option<u32> {
        None
    }

    #[cfg(not(windows))]
    pub fn path_of(_pid: u32) -> String {
        String::new()
    }

    pub fn running(exe_name: &str) -> bool {
        find(exe_name).is_some()
    }

    /// Full path of a running program, by image name.
    pub fn path(exe_name: &str) -> Option<PathBuf> {
        let path = path_of(find(exe_name)?);
        (!path.is_empty()).then(|| PathBuf::from(path))
    }
}

// ---------- images ----------

pub mod image {
    use super::*;

    pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().ok()?;
            wr.write_image_data(rgba).ok()?;
        }
        Some(out)
    }

    /// The first frame of any image Windows can decode (GIF, JPG, PNG…), scaled down to at most
    /// `max_w` wide, saved as PNG. Needs COM on the calling thread.
    #[cfg(windows)]
    pub fn scale_to_png(src: &Path, dst: &Path, max_w: u32) -> Option<()> {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::GENERIC_READ;
        use windows::Win32::Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
            WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        unsafe {
            let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let dec = f.CreateDecoderFromFilename(&HSTRING::from(src.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
            let frame = dec.GetFrame(0).ok()?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h).ok()?;
            if w == 0 || h == 0 {
                return None;
            }
            let tw = w.min(max_w);
            let th = ((h as u64 * tw as u64) / w as u64).max(1) as u32;
            let scaler = f.CreateBitmapScaler().ok()?;
            scaler.Initialize(&frame, tw, th, WICBitmapInterpolationModeFant).ok()?;
            let conv = f.CreateFormatConverter().ok()?;
            conv.Initialize(&scaler, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            let mut buf = vec![0u8; (tw * th * 4) as usize];
            conv.CopyPixels(std::ptr::null(), tw * 4, &mut buf).ok()?;
            std::fs::write(dst, encode_png(tw, th, &buf)?).ok()
        }
    }

    #[cfg(not(windows))]
    pub fn scale_to_png(_src: &Path, _dst: &Path, _max_w: u32) -> Option<()> {
        None
    }

    /// COM for this thread (WIC needs it; a no-op when it's already set up).
    pub fn com() {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
    }
}

// ---------- starting programs ----------

pub mod shell {
    use super::*;

    /// Runs `exe` with `args` through the shell, hidden (for command-line control of an app that's
    /// already running, like Wallpaper Engine's `-control`).
    #[cfg(windows)]
    pub fn run_hidden(exe: &Path, args: &str) -> Result<(), String> {
        open_with(exe.as_os_str(), args, windows::Win32::UI::WindowsAndMessaging::SW_HIDE)
    }

    /// Opens a file, folder, app or `steam://` link the way Explorer would (its own window, if it
    /// has one, shows normally: the user asked for it).
    #[cfg(windows)]
    pub fn open(target: &str) -> Result<(), String> {
        open_with(std::ffi::OsStr::new(target), "", windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL)
    }

    #[cfg(windows)]
    fn open_with(file: &std::ffi::OsStr, args: &str, show: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD) -> Result<(), String> {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        let args = HSTRING::from(args);
        let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(file), if args.is_empty() { PCWSTR::null() } else { PCWSTR(args.as_ptr()) }, PCWSTR::null(), show) };
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!("Windows couldn't open {}", file.to_string_lossy()))
        }
    }

    #[cfg(not(windows))]
    pub fn run_hidden(_exe: &Path, _args: &str) -> Result<(), String> {
        Err("needs Windows".into())
    }

    #[cfg(not(windows))]
    pub fn open(_target: &str) -> Result<(), String> {
        Err("needs Windows".into())
    }

    /// A `Command` that starts without a window (CREATE_NO_WINDOW), with no console handles of ours.
    pub fn command(exe: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
        let mut c = std::process::Command::new(exe);
        c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::steam::*;

    #[test]
    fn vdf_tokens_and_libraries() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\Games\\\\Steam\"\n\t}\n}";
        let t = tokens(vdf);
        assert_eq!(t[0], "libraryfolders");
        assert!(t.contains(&"D:\\Games\\Steam".to_string()));
        let dir = std::env::temp_dir().join(format!("island-win-steam-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("steamapps")).unwrap();
        std::fs::write(dir.join("steamapps").join("libraryfolders.vdf"), vdf).unwrap();
        let libs = libraries(&dir);
        assert_eq!(libs.len(), 3);
        assert_eq!(libs[2], std::path::PathBuf::from("D:\\Games\\Steam"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_manifests_decode() {
        assert_eq!(decode(b"Game\x99 \x96 Edition"), "Game\u{2122} \u{2013} Edition");
        assert_eq!(decode("Café".as_bytes()), "Café");
    }

    #[test]
    fn png_encodes() {
        let png = super::image::encode_png(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
