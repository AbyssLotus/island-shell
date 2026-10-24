//! Saves a rendered wallpaper PNG and sets it on every monitor with IDesktopWallpaper.

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper, DWPOS_FILL};

use crate::util;

fn set(path: PathBuf) -> Result<(), String> {
    // Shell COM objects want an STA, so use a fresh thread instead of a pooled one.
    std::thread::spawn(move || -> Result<(), String> {
        util::com_sta();
        unsafe {
            let dw: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).map_err(|e| e.message().to_string())?;
            let _ = dw.SetPosition(DWPOS_FILL);
            dw.SetWallpaper(PCWSTR::null(), &HSTRING::from(path.to_string_lossy().as_ref())).map_err(|e| e.message().to_string())
        }
    })
    .join()
    .map_err(|_| "Wallpaper thread crashed".to_string())?
}

pub fn save_and_set(dir: &Path, name: &str, png: &[u8]) -> Result<String, String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid wallpaper name".into());
    }
    if png.len() < 8 || &png[1..4] != b"PNG" {
        return Err("Wallpaper data isn't a PNG".into());
    }
    let folder = dir.join("wallpapers");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join(format!("{name}.png"));
    std::fs::write(&file, png).map_err(|e| e.to_string())?;
    set(file.clone())?;
    Ok(file.to_string_lossy().into_owned())
}
