//! HICON → PNG conversion, shared by the running-windows list and the tray host.

use windows::core::HSTRING;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetClassLongPtrW, GetIconInfo, SendMessageTimeoutW, GCLP_HICON, GCLP_HICONSM, HICON, ICONINFO, ICON_BIG, ICON_SMALL2,
    SMTO_ABORTIFHUNG, WM_GETICON,
};

use crate::util;

/// Reads a bitmap as top-down 32-bit BGRA.
unsafe fn dib32(hbm: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    if GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if w <= 0 || h <= 0 || w > 512 || h > 1024 {
        return None;
    }
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let hdc = GetDC(None);
    let lines = GetDIBits(hdc, hbm, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
    ReleaseDC(None, hdc);
    (lines != 0).then_some((w as u32, h as u32, buf))
}

/// Converts an icon to PNG bytes. The icon itself is not destroyed.
pub fn hicon_to_png(hicon: HICON) -> Option<Vec<u8>> {
    if hicon.is_invalid() {
        return None;
    }
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(hicon, &mut ii).ok()?;
        let out = (|| {
            if ii.hbmColor.is_invalid() {
                return None;
            }
            let (w, h, mut px) = dib32(ii.hbmColor)?;
            if !px.chunks(4).any(|p| p[3] != 0) {
                // Old-style icon: transparency comes from the AND mask (black = opaque).
                let (_, _, mask) = dib32(ii.hbmMask)?;
                for (p, m) in px.chunks_mut(4).zip(mask.chunks(4)) {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            }
            for p in px.chunks_mut(4) {
                p.swap(0, 2);
            }
            util::encode_png(w, h, &px)
        })();
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
        out
    }
}

/// The icon a window shows in the taskbar, falling back to its class icon.
pub fn window_icon_png(h: HWND) -> Option<Vec<u8>> {
    unsafe {
        for kind in [ICON_BIG, ICON_SMALL2] {
            let mut res = 0usize;
            let ok = SendMessageTimeoutW(h, WM_GETICON, WPARAM(kind as usize), LPARAM(0), SMTO_ABORTIFHUNG, 120, Some(&mut res));
            if ok.0 != 0 && res != 0 {
                if let Some(png) = hicon_to_png(HICON(res as _)) {
                    return Some(png);
                }
            }
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let v = GetClassLongPtrW(h, idx);
            if v != 0 {
                if let Some(png) = hicon_to_png(HICON(v as _)) {
                    return Some(png);
                }
            }
        }
    }
    None
}

/// The icon Explorer shows for an executable.
pub fn exe_icon_png(path: &str) -> Option<Vec<u8>> {
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        let png = hicon_to_png(sfi.hIcon);
        let _ = DestroyIcon(sfi.hIcon);
        png
    }
}
