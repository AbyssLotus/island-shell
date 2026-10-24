use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::PWSTR;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED};

pub const IDENTIFIER: &str = "league.seaweed.islandshell";

/// Same folder Tauri resolves for `$APPDATA`, available before the app is built.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join(IDENTIFIER)
}

/// Appends a line to `island.log` in the data folder (the release build has no console).
pub fn log_line(msg: &str) {
    use std::io::Write;
    let path = data_dir().join("island.log");
    if std::fs::metadata(&path).map(|m| m.len() > 512_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {msg}", now_ms());
    }
    eprintln!("[island] {msg}");
}

/// Blocks until the process `pid` exits, or `timeout_ms` passes.
pub fn wait_for_exit(pid: u32, timeout_ms: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, timeout_ms);
            let _ = CloseHandle(h);
        }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn com_mta() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn com_sta() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// Reads a COM-allocated wide string and frees it.
pub fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

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

/// Averages blocks of pixels so the longest side is at most `max` (small images come back as-is).
pub fn downscale_rgba(w: u32, h: u32, rgba: &[u8], max: u32) -> (u32, u32, Vec<u8>) {
    if w <= max && h <= max {
        return (w, h, rgba.to_vec());
    }
    let scale = max as f64 / w.max(h) as f64;
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    let mut acc = vec![0u64; (tw * th * 4) as usize];
    let mut cnt = vec![0u64; (tw * th) as usize];
    for y in 0..h as usize {
        let oy = y * th as usize / h as usize;
        for x in 0..w as usize {
            let o = oy * tw as usize + x * tw as usize / w as usize;
            let i = (y * w as usize + x) * 4;
            for c in 0..4 {
                acc[o * 4 + c] += rgba[i + c] as u64;
            }
            cnt[o] += 1;
        }
    }
    let out = acc.iter().enumerate().map(|(i, v)| (v / cnt[i / 4].max(1)) as u8).collect();
    (tw, th, out)
}

pub fn decode_png_rgba(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}

/// Opens a Windows Settings page or the notification center. Only those URI schemes are accepted.
pub fn open_settings(uri: &str) -> Result<(), String> {
    let allowed = uri.starts_with("ms-settings:") || uri == "ms-actioncenter:";
    if !allowed || uri.contains(['"', '&', '|', ' ']) {
        return Err("Only Settings pages and the notification center can be opened".into());
    }
    std::process::Command::new("explorer.exe").arg(uri).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Encrypts a secret (API key, token) for this Windows account only (DPAPI), as base64.
/// Used by discord.rs and destiny.rs for what they store in the data folder.
pub fn protect(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        if CryptProtectData(&input, None, None, None, None, 0, &mut out).is_err() {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        B64.encode(bytes)
    }
}

pub fn unprotect(enc: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let Ok(bytes) = B64.decode(enc) else { return String::new() };
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        if CryptUnprotectData(&input, None, None, None, None, 0, &mut out).is_err() {
            return String::new();
        }
        let plain = String::from_utf8_lossy(std::slice::from_raw_parts(out.pbData, out.cbData as usize)).into_owned();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        plain
    }
}

/// Opens a target (URL, protocol link) with whatever Windows has registered for it.
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as isize > 32 { Ok(()) } else { Err(format!("Windows couldn't open {target}")) }
}

/// Opens a web page in the default browser, but never in Edge (the user doesn't want it): if Edge
/// is the default, the link is copied to the clipboard instead.
pub fn open_web(url: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let edge = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice", "/v", "ProgId"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("MSEdge"))
        .unwrap_or(false);
    if edge {
        let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(url));
        return Err("Edge is your default browser, so the link was copied instead".into());
    }
    shell_open(url)
}

/// Percent-encodes text for a URL query.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
