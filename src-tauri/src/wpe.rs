//! Wallpaper Engine in the Wallpaper tab: your installed wallpapers (Workshop subscriptions, your
//! own projects and the bundled ones), which one is on, recently used ones, and applying / pausing /
//! muting through Wallpaper Engine's own command line (`wallpaper64.exe -control …`).
//!
//! Wallpaper previews are ~1 MB GIFs or JPGs (about 400 MB across a big library), far too much to
//! show at once, so each gets a small still thumbnail made once with Windows' image decoder (WIC)
//! and cached; the page only plays the real animated preview of the tile under the pointer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
    WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::util::log_line;

const APP_ID: &str = "431960";
/// Thumbnail width; tiles are ~150 px wide, so this stays sharp at 150% scaling.
const THUMB_W: u32 = 192;

#[derive(Serialize, Clone)]
pub struct Wallpaper {
    /// The wallpaper's folder (its identity everywhere below).
    pub dir: String,
    pub title: String,
    /// "scene", "video", "web", "application" or "".
    pub kind: String,
    pub tags: Vec<String>,
    /// "Everyone", "Questionable", "Mature" or "".
    pub rating: String,
    /// The original preview (often an animated GIF), for hover.
    pub preview: Option<String>,
    /// Cached still thumbnail; None until it has been made.
    pub thumb: Option<String>,
    /// When it was added (folder time, Unix seconds), for "newest first".
    pub added: u64,
    pub workshop: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct Library {
    pub installed: bool,
    pub wallpapers: Vec<Wallpaper>,
    /// Folder of the wallpaper on the first monitor.
    pub current: Option<String>,
    /// Folders of recently used wallpapers, newest first.
    pub recent: Vec<String>,
    /// Folders the page may load previews and thumbnails from.
    #[serde(skip)]
    pub dirs: Vec<PathBuf>,
}

/// Wallpaper Engine's install folder (it's a Steam app), found through Steam's libraries.
fn install_dir() -> Option<PathBuf> {
    let steam = crate::games::steam_dir()?;
    crate::games::libraries(&steam)
        .into_iter()
        .map(|l| l.join("steamapps").join("common").join("wallpaper_engine"))
        .find(|d| d.join("wallpaper64.exe").is_file() || d.join("wallpaper32.exe").is_file())
}

fn workshop_dirs() -> Vec<PathBuf> {
    let Some(steam) = crate::games::steam_dir() else { return vec![] };
    crate::games::libraries(&steam)
        .into_iter()
        .map(|l| l.join("steamapps").join("workshop").join("content").join(APP_ID))
        .filter(|d| d.is_dir())
        .collect()
}

/// Folder of a wallpaper file as Wallpaper Engine records it ("C:/…/2276970166/scene.pkg").
fn folder_of(file: &str) -> String {
    Path::new(&file.replace('/', "\\")).parent().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

pub fn library(data_dir: &Path) -> Library {
    let Some(install) = install_dir() else { return Library::default() };
    let mut roots: Vec<(PathBuf, bool)> = workshop_dirs().into_iter().map(|d| (d, true)).collect();
    roots.push((install.join("projects").join("myprojects"), false));
    roots.push((install.join("projects").join("defaultprojects"), false));
    let thumbs = data_dir.join("wethumbs");
    let mut out = vec![];
    for (root, workshop) in &roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let dir = e.path();
            let Some(j) = std::fs::read(dir.join("project.json")).ok().and_then(|b| serde_json::from_slice::<Value>(strip_bom(&b)).ok()) else { continue };
            let s = |k: &str| j[k].as_str().unwrap_or("").trim().to_string();
            let kind = s("type").to_lowercase();
            if kind == "preset" {
                continue; // settings presets of other wallpapers, not wallpapers
            }
            let preview = Some(s("preview")).filter(|p| !p.is_empty()).map(|p| dir.join(p)).filter(|p| p.is_file());
            let thumb = preview.as_ref().map(|p| thumb_path(&thumbs, p)).filter(|t| t.is_file());
            out.push(Wallpaper {
                title: Some(s("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()),
                kind,
                tags: j["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
                rating: s("contentrating"),
                preview: preview.map(|p| p.to_string_lossy().into_owned()),
                thumb: thumb.map(|t| t.to_string_lossy().into_owned()),
                added: e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                workshop: *workshop,
                dir: key(&dir),
            });
        }
    }
    out.sort_by(|a, b| b.added.cmp(&a.added));
    let (current, recent) = selection(&install);
    let mut dirs: Vec<PathBuf> = roots.into_iter().map(|(d, _)| d).collect();
    dirs.push(thumbs);
    Library { installed: true, wallpapers: out, current, recent, dirs }
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(b"\xef\xbb\xbf").unwrap_or(b)
}

/// The wallpaper on the first monitor and the recently used ones, from Wallpaper Engine's config.
fn selection(install: &Path) -> (Option<String>, Vec<String>) {
    let Some(cfg) = std::fs::read(install.join("config.json")).ok().and_then(|b| serde_json::from_slice::<Value>(strip_bom(&b)).ok()) else {
        return (None, vec![]);
    };
    let user = std::env::var("USERNAME").unwrap_or_default();
    // Settings live under the Windows user name; fall back to any entry that has them.
    let general = cfg
        .get(&user)
        .map(|u| &u["general"])
        .filter(|g| g.get("wallpaperconfig").is_some())
        .or_else(|| cfg.as_object()?.values().map(|u| &u["general"]).find(|g| g.get("wallpaperconfig").is_some()));
    let Some(g) = general else { return (None, vec![]) };
    let first_file = |c: &Value| -> Option<String> {
        let sel = c["selectedwallpapers"].as_object()?;
        let m = sel.get("Monitor0").or_else(|| sel.values().next())?;
        m["file"].as_str().map(folder_of)
    };
    let current = first_file(&g["wallpaperconfig"]);
    let mut recent: Vec<String> = vec![];
    for r in g["wallpaperconfigrecent"].as_array().into_iter().flatten() {
        if let Some(d) = first_file(&r["config"]) {
            if !recent.contains(&d) {
                recent.push(d);
            }
        }
    }
    (current, recent)
}

fn thumb_path(thumbs: &Path, preview: &Path) -> PathBuf {
    let stamp = std::fs::metadata(preview).map(|m| m.len()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in preview.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    thumbs.join(format!("{h:016x}-{stamp}.png"))
}

static MAKING: AtomicBool = AtomicBool::new(false);

/// Makes the missing thumbnails in the background, telling the page about each one (`we:thumb`).
pub fn make_thumbs(app: AppHandle, lib: &Library, data_dir: PathBuf) {
    let todo: Vec<(String, PathBuf)> = lib
        .wallpapers
        .iter()
        .filter(|w| w.thumb.is_none())
        .filter_map(|w| Some((w.dir.clone(), PathBuf::from(w.preview.as_ref()?))))
        .collect();
    if todo.is_empty() || MAKING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        crate::util::com_mta();
        let thumbs = data_dir.join("wethumbs");
        let _ = std::fs::create_dir_all(&thumbs);
        let started = std::time::Instant::now();
        let mut made = 0;
        for (dir, preview) in todo {
            let dst = thumb_path(&thumbs, &preview);
            if dst.is_file() || make_thumb(&preview, &dst).is_some() {
                made += 1;
                let _ = app.emit("we:thumb", serde_json::json!({ "dir": dir, "thumb": dst.to_string_lossy() }));
            }
        }
        log_line(&format!("wallpaper engine: made {made} thumbnails in {:.1} s", started.elapsed().as_secs_f32()));
        MAKING.store(false, Ordering::SeqCst);
    });
}

/// First frame of the preview, scaled to THUMB_W wide, saved as PNG.
fn make_thumb(src: &Path, dst: &Path) -> Option<()> {
    scale_image(src, dst, THUMB_W)
}

/// The first frame of any image Windows can decode, scaled down to at most `max_w` wide, saved as PNG.
pub(crate) fn scale_image(src: &Path, dst: &Path, max_w: u32) -> Option<()> {
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
        std::fs::write(dst, crate::util::encode_png(tw, th, &buf)?).ok()
    }
}

fn exe() -> Option<PathBuf> {
    let running = crate::vpn::process_path("wallpaper64.exe").or_else(|| crate::vpn::process_path("wallpaper32.exe"));
    running.or_else(|| {
        let d = install_dir()?;
        [d.join("wallpaper64.exe"), d.join("wallpaper32.exe")].into_iter().find(|p| p.is_file())
    })
}

fn run(args: &str) -> Result<(), String> {
    let exe = exe().ok_or("Wallpaper Engine isn't installed")?;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args), PCWSTR::null(), SW_HIDE) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err("Wallpaper Engine didn't accept that".into())
    }
}

/// Puts a wallpaper (its folder) on the first monitor.
pub fn apply(dir: &str) -> Result<(), String> {
    let project = Path::new(dir).join("project.json");
    let lib_ok = workshop_dirs()
        .into_iter()
        .chain(install_dir().map(|d| d.join("projects")))
        .any(|root| key(Path::new(dir)).starts_with(&key(&root)));
    if !lib_ok || !project.is_file() || dir.contains('"') {
        return Err("That isn't a Wallpaper Engine wallpaper".into());
    }
    run(&format!("-control openWallpaper -file \"{}\" -monitor 0", project.display()))
}

/// pause | play | mute | unmute | stop | next
pub fn control(cmd: &str) -> Result<(), String> {
    let arg = match cmd {
        "pause" | "play" | "mute" | "unmute" | "stop" => cmd,
        "next" => "nextWallpaper",
        _ => return Err(format!("Unknown Wallpaper Engine command {cmd}")),
    };
    run(&format!("-control {arg}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test live_wpe -- --ignored --nocapture`: reads the real library and thumbnails a few
    /// previews (a GIF and a JPG at least) into a temp folder.
    #[test]
    #[ignore = "needs Wallpaper Engine installed"]
    fn live_wpe() {
        crate::util::com_mta();
        let tmp = std::env::temp_dir().join("island-wpe-test");
        let lib = library(&tmp);
        println!("{} wallpapers, current {:?}, {} recent", lib.wallpapers.len(), lib.current, lib.recent.len());
        let cur = lib.wallpapers.iter().find(|w| Some(&w.dir) == lib.current.as_ref());
        println!("current title: {:?}", cur.map(|w| &w.title));
        let _ = std::fs::create_dir_all(tmp.join("wethumbs"));
        let mut picked: Vec<&Wallpaper> = vec![];
        for ext in [".gif", ".jpg", ".png"] {
            picked.extend(lib.wallpapers.iter().filter(|w| w.preview.as_deref().is_some_and(|p| p.to_lowercase().ends_with(ext))).take(2));
        }
        for w in picked {
            let src = PathBuf::from(w.preview.as_ref().unwrap());
            let dst = thumb_path(&tmp.join("wethumbs"), &src);
            let t = std::time::Instant::now();
            let ok = make_thumb(&src, &dst).is_some();
            let size = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
            let dims = crate::util::decode_png_rgba(&dst).map(|(w, h, _)| (w, h));
            println!("{ok} {:>5} ms {:>7} B {:?} {}", t.elapsed().as_millis(), size, dims, src.file_name().unwrap().to_string_lossy());
        }
    }
}
