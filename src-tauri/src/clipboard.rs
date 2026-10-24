//! Clipboard history: a message-only window listens for WM_CLIPBOARDUPDATE, a worker reads the
//! new content with arboard and keeps a deduplicated, persisted history.
//!
//! Content that password managers mark as private (ExcludeClipboardContentFromMonitorProcessing,
//! CanIncludeInClipboardHistory = 0, Clipboard Viewer Ignore) is never recorded.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::core::w;
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WNDCLASSW,
};

use crate::{util, Shared};

const MAX_ITEMS: usize = 150;
const MAX_IMAGES: usize = 40;
const MAX_TEXT: usize = 20_000;

static TX: OnceLock<Sender<()>> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone)]
pub struct Clip {
    pub id: u64,
    /// "text", "link", "color" or "image".
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub image: Option<String>,
    /// Small copy of the image for the list (the full one is only loaded for the preview).
    #[serde(default)]
    pub thumb: Option<String>,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    pub ts: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub hash: u64,
}

pub struct History {
    pub items: Vec<Clip>,
    next_id: u64,
    path: PathBuf,
    img_dir: PathBuf,
}

impl History {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("clipboard.json");
        let img_dir = dir.join("clips");
        let _ = std::fs::create_dir_all(&img_dir);
        let items: Vec<Clip> = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                util::log_line(&format!("clipboard history unreadable, starting empty: {e}"));
                vec![]
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => {
                util::log_line(&format!("couldn't open clipboard history {}: {e}", path.display()));
                vec![]
            }
        };
        let next_id = items.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        History { items, next_id, path, img_dir }
    }

    pub fn save(&self) {
        if let Ok(b) = serde_json::to_vec(&self.items) {
            let tmp = self.path.with_extension("json.tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, &self.path);
            }
        }
    }

    fn bump(&mut self, idx: usize) {
        let mut c = self.items.remove(idx);
        c.ts = util::now_ms();
        self.items.insert(0, c);
    }

    fn add_text(&mut self, text: String) {
        let text: String = text.chars().take(MAX_TEXT).collect();
        if let Some(i) = self.items.iter().position(|c| c.kind != "image" && c.text == text) {
            self.bump(i);
        } else {
            let id = self.next_id;
            self.next_id += 1;
            let kind = classify(&text);
            self.items.insert(0, Clip { id, kind, text, image: None, thumb: None, w: 0, h: 0, ts: util::now_ms(), pinned: false, hash: 0 });
        }
        self.trim();
        self.save();
    }

    fn add_image(&mut self, w: u32, h: u32, rgba: &[u8]) {
        let hash = fnv(rgba) ^ ((w as u64) << 32 | h as u64);
        if let Some(i) = self.items.iter().position(|c| c.kind == "image" && c.hash == hash) {
            self.bump(i);
            self.save();
            return;
        }
        let Some(png) = util::encode_png(w, h, rgba) else { return };
        let id = self.next_id;
        self.next_id += 1;
        let file = self.img_dir.join(format!("{id}.png"));
        if std::fs::write(&file, png).is_err() {
            return;
        }
        let thumb = write_thumb(&self.img_dir, id, w, h, rgba);
        self.items.insert(0, Clip {
            id,
            kind: "image".into(),
            text: format!("Image {w}×{h}"),
            image: Some(file.to_string_lossy().into_owned()),
            thumb,
            w,
            h,
            ts: util::now_ms(),
            pinned: false,
            hash,
        });
        self.trim();
        self.save();
    }

    fn trim(&mut self) {
        let mut images = 0;
        let mut keep = Vec::with_capacity(self.items.len());
        for (n, c) in std::mem::take(&mut self.items).into_iter().enumerate() {
            let is_img = c.kind == "image";
            if is_img && !c.pinned {
                images += 1;
            }
            if c.pinned || (n < MAX_ITEMS && !(is_img && images > MAX_IMAGES)) {
                keep.push(c);
            } else {
                remove_files(&c);
            }
        }
        self.items = keep;
    }

    pub fn pin(&mut self, id: u64) {
        if let Some(c) = self.items.iter_mut().find(|c| c.id == id) {
            c.pinned = !c.pinned;
        }
        self.save();
    }

    pub fn delete(&mut self, id: u64) {
        if let Some(i) = self.items.iter().position(|c| c.id == id) {
            remove_files(&self.items.remove(i));
        }
        self.save();
    }

    pub fn clear_unpinned(&mut self) {
        for c in self.items.iter().filter(|c| !c.pinned) {
            remove_files(c);
        }
        self.items.retain(|c| c.pinned);
        self.save();
    }
}

/// Longest side of the list thumbnails, in pixels.
const THUMB: u32 = 320;

fn write_thumb(dir: &Path, id: u64, w: u32, h: u32, rgba: &[u8]) -> Option<String> {
    let (tw, th, small) = util::downscale_rgba(w, h, rgba, THUMB);
    let file = dir.join(format!("{id}.thumb.png"));
    std::fs::write(&file, util::encode_png(tw, th, &small)?).ok()?;
    Some(file.to_string_lossy().into_owned())
}

fn remove_files(c: &Clip) {
    for p in [&c.image, &c.thumb].into_iter().flatten() {
        let _ = std::fs::remove_file(p);
    }
}

/// Makes thumbnails for images saved before thumbnails existed (one image in memory at a time).
fn backfill_thumbs(shared: &Shared) {
    let todo: Vec<(u64, String)> = {
        let h = shared.clips.lock().unwrap();
        h.items.iter().filter(|c| c.thumb.is_none()).filter_map(|c| Some((c.id, c.image.clone()?))).collect()
    };
    if todo.is_empty() {
        return;
    }
    let mut made = 0;
    for (id, path) in todo {
        let Some((w, h, rgba)) = util::decode_png_rgba(Path::new(&path)) else { continue };
        let dir = Path::new(&path).parent().map(Path::to_path_buf).unwrap_or_default();
        if let Some(t) = write_thumb(&dir, id, w, h, &rgba) {
            if let Some(c) = shared.clips.lock().unwrap().items.iter_mut().find(|c| c.id == id) {
                c.thumb = Some(t);
                made += 1;
            }
        }
    }
    shared.clips.lock().unwrap().save();
    util::log_line(&format!("clipboard: made {made} image thumbnails"));
}

fn classify(t: &str) -> String {
    let s = t.trim();
    let is_link = (s.starts_with("http://") || s.starts_with("https://")) && !s.contains(char::is_whitespace);
    let hex = s.strip_prefix('#').unwrap_or("");
    let is_color = (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit());
    if is_link { "link" } else if is_color { "color" } else { "text" }.into()
}

fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b.iter().step_by(7) {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// True when the source app asked monitors not to record this clipboard content.
fn is_private() -> bool {
    unsafe {
        let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if IsClipboardFormatAvailable(exclude).is_ok() || IsClipboardFormatAvailable(ignore).is_ok() {
            return true;
        }
        let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(history).is_ok() && OpenClipboard(None).is_ok() {
            let mut allowed = 1u32;
            if let Ok(h) = GetClipboardData(history) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g);
                if !p.is_null() {
                    allowed = *(p as *const u32);
                    let _ = GlobalUnlock(g);
                }
            }
            let _ = CloseClipboard();
            return allowed == 0;
        }
        false
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if let Some(tx) = TX.get() {
            let _ = tx.send(());
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn listen() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = w!("IslandShellClipboardListener");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: module.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(module.into()), None)
        else {
            return;
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
}

pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    let (tx, rx) = channel::<()>();
    let _ = TX.set(tx);
    std::thread::spawn(listen);
    std::thread::spawn(move || {
        backfill_thumbs(&shared);
        while rx.recv().is_ok() {
            // Let the source app finish writing every format, then coalesce bursts.
            std::thread::sleep(Duration::from_millis(120));
            while rx.try_recv().is_ok() {}
            if is_private() {
                continue;
            }
            let Ok(mut cb) = arboard::Clipboard::new() else { continue };
            let text = cb.get_text().ok().filter(|t| !t.trim().is_empty());
            let changed = if let Some(t) = text {
                shared.clips.lock().unwrap().add_text(t);
                true
            } else if let Ok(img) = cb.get_image() {
                if img.width > 0 && img.height > 0 && img.width * img.height <= 8192 * 8192 {
                    shared.clips.lock().unwrap().add_image(img.width as u32, img.height as u32, &img.bytes);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if changed {
                let _ = app.emit("clip:changed", ());
            }
        }
    });
}

/// Puts a history item back on the clipboard. The listener then moves it to the top.
pub fn copy(item: &Clip) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    if let Some(path) = &item.image {
        let (w, h, rgba) = util::decode_png_rgba(Path::new(path)).ok_or("Couldn't read the saved image")?;
        cb.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: Cow::Owned(rgba) }).map_err(|e| e.to_string())
    } else {
        cb.set_text(item.text.clone()).map_err(|e| e.to_string())
    }
}
