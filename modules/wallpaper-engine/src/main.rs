//! Wallpaper Engine in the Wallpaper tab, an Island Shell module backend (docs/design/modules.md);
//! its UI is ui/wallpaper-engine.js. Your installed wallpapers (Workshop subscriptions, your own
//! projects and the bundled ones), which one is on, recently used ones, and applying / pausing /
//! muting through Wallpaper Engine's own command line (`wallpaper64.exe -control …`).
//!
//! Wallpaper previews are ~1 MB GIFs or JPGs (about 400 MB across a big library), far too much to
//! show at once, so each gets a small still thumbnail made once with Windows' image decoder (WIC)
//! and kept in the module's cache folder (the page loads them from there); the page only plays the
//! real animated preview of the tile under the pointer, which it asks for one at a time.
//!
//! Nothing here goes on the network.
//!
//! RPC methods: `library`, `apply { dir }`, `control { cmd }`, `preview { dir }`. Events: `thumb`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine;
use island_module::{json, CallResult, Host, Init, Module, RpcError, Value};
use island_win::{image, process, shell, steam};
use serde::Serialize;

const APP_ID: &str = "431960";
/// Thumbnail width; tiles are ~150 px wide, so this stays sharp at 150% scaling.
const THUMB_W: u32 = 192;
/// Hover previews bigger than this aren't sent (the protocol's messages stop at 4 MiB).
const PREVIEW_MAX: u64 = 2_800_000;

#[derive(Serialize, Clone)]
struct Wallpaper {
    /// The wallpaper's folder (its identity everywhere below).
    dir: String,
    title: String,
    /// "scene", "video", "web", "application" or "".
    kind: String,
    tags: Vec<String>,
    /// "Everyone", "Questionable", "Mature" or "".
    rating: String,
    /// Whether it has an animated (GIF) preview to play on hover.
    animated: bool,
    /// Cached still thumbnail, relative to the module's cache folder; None until it has been made.
    thumb: Option<String>,
    /// When it was added (folder time, Unix seconds), for "newest first".
    added: u64,
    workshop: bool,
    #[serde(skip)]
    preview: Option<PathBuf>,
}

#[derive(Serialize, Clone, Default)]
struct Library {
    installed: bool,
    wallpapers: Vec<Wallpaper>,
    /// Folder of the wallpaper on the first monitor.
    current: Option<String>,
    /// Folders of recently used wallpapers, newest first.
    recent: Vec<String>,
}

/// Wallpaper Engine's install folder (it's a Steam app), found through Steam's libraries.
fn install_dir() -> Option<PathBuf> {
    let steam = steam::dir()?;
    steam::libraries(&steam)
        .into_iter()
        .map(|l| l.join("steamapps").join("common").join("wallpaper_engine"))
        .find(|d| d.join("wallpaper64.exe").is_file() || d.join("wallpaper32.exe").is_file())
}

fn workshop_dirs() -> Vec<PathBuf> {
    let Some(steam) = steam::dir() else { return vec![] };
    steam::libraries(&steam)
        .into_iter()
        .map(|l| l.join("steamapps").join("workshop").join("content").join(APP_ID))
        .filter(|d| d.is_dir())
        .collect()
}

/// Folder of a wallpaper file as Wallpaper Engine records it ("C:/…/2276970166/scene.pkg").
fn folder_of(file: &str) -> String {
    file.replace('/', "\\").rsplit_once('\\').map(|(d, _)| d.to_lowercase()).unwrap_or_default()
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

fn library(cache: &Path) -> Library {
    let Some(install) = install_dir() else { return Library::default() };
    let mut roots: Vec<(PathBuf, bool)> = workshop_dirs().into_iter().map(|d| (d, true)).collect();
    roots.push((install.join("projects").join("myprojects"), false));
    roots.push((install.join("projects").join("defaultprojects"), false));
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
            let thumb = preview.as_ref().map(|p| thumb_name(p)).filter(|t| cache.join(t).is_file());
            out.push(Wallpaper {
                title: Some(s("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| e.file_name().to_string_lossy().into_owned()),
                kind,
                tags: j["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default(),
                rating: s("contentrating"),
                animated: preview.as_ref().is_some_and(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gif"))),
                thumb,
                added: e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                workshop: *workshop,
                dir: key(&dir),
                preview,
            });
        }
    }
    out.sort_by(|a, b| b.added.cmp(&a.added));
    let (current, recent) = selection(&install);
    Library { installed: true, wallpapers: out, current, recent }
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

/// A preview's thumbnail, relative to the cache folder: named by its path and size, so a changed
/// preview gets a new one.
fn thumb_name(preview: &Path) -> String {
    let stamp = std::fs::metadata(preview).map(|m| m.len()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in preview.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("thumbs/{h:016x}-{stamp}.png")
}

fn exe() -> Option<PathBuf> {
    let running = process::path("wallpaper64.exe").or_else(|| process::path("wallpaper32.exe"));
    running.or_else(|| {
        let d = install_dir()?;
        [d.join("wallpaper64.exe"), d.join("wallpaper32.exe")].into_iter().find(|p| p.is_file())
    })
}

fn run(args: &str) -> Result<(), String> {
    let exe = exe().ok_or("Wallpaper Engine isn't installed")?;
    shell::run_hidden(&exe, args).map_err(|_| "Wallpaper Engine didn't accept that".into())
}

/// Whether `dir` is a wallpaper folder in one of Wallpaper Engine's own libraries.
fn in_library(dir: &str) -> bool {
    !dir.contains('"')
        && Path::new(dir).join("project.json").is_file()
        && workshop_dirs().into_iter().chain(install_dir().map(|d| d.join("projects"))).any(|root| key(Path::new(dir)).starts_with(&key(&root)))
}

/// Puts a wallpaper (its folder) on the first monitor.
fn apply(dir: &str) -> Result<(), String> {
    if !in_library(dir) {
        return Err("That isn't a Wallpaper Engine wallpaper".into());
    }
    run(&format!("-control openWallpaper -file \"{}\" -monitor 0", Path::new(dir).join("project.json").display()))
}

/// pause | play | mute | unmute | stop | next
fn control(cmd: &str) -> Result<(), String> {
    let arg = match cmd {
        "pause" | "play" | "mute" | "unmute" | "stop" => cmd,
        "next" => "nextWallpaper",
        _ => return Err(format!("Unknown Wallpaper Engine command {cmd}")),
    };
    run(&format!("-control {arg}"))
}

#[derive(Default)]
struct WallpaperEngine {
    /// The thumbnail maker is at work.
    making: Arc<AtomicBool>,
}

impl WallpaperEngine {
    /// Makes the missing thumbnails in the background, telling the page about each one (`thumb`).
    fn make_thumbs(&self, host: &Host, lib: &Library) {
        let todo: Vec<(String, PathBuf)> = lib.wallpapers.iter().filter(|w| w.thumb.is_none()).filter_map(|w| Some((w.dir.clone(), w.preview.clone()?))).collect();
        if todo.is_empty() || self.making.swap(true, Ordering::SeqCst) {
            return;
        }
        let (host, making) = (host.clone(), self.making.clone());
        std::thread::spawn(move || {
            image::com();
            let cache = host.cache_dir();
            let _ = std::fs::create_dir_all(cache.join("thumbs"));
            let started = std::time::Instant::now();
            let mut made = 0;
            for (dir, preview) in todo {
                let name = thumb_name(&preview);
                let dst = cache.join(&name);
                if dst.is_file() || image::scale_to_png(&preview, &dst, THUMB_W).is_some() {
                    made += 1;
                    host.emit("thumb", json!({ "dir": dir, "thumb": name }));
                }
            }
            host.info(format!("made {made} thumbnails in {:.1} s", started.elapsed().as_secs_f32()));
            making.store(false, Ordering::SeqCst);
        });
    }
}

/// A wallpaper's animated preview as a data: URL, for the tile under the pointer.
fn preview(dir: &str) -> Result<Option<String>, String> {
    if !in_library(dir) {
        return Err("That isn't a Wallpaper Engine wallpaper".into());
    }
    let j: Value = std::fs::read(Path::new(dir).join("project.json")).ok().and_then(|b| serde_json::from_slice(strip_bom(&b)).ok()).unwrap_or(Value::Null);
    let name = j["preview"].as_str().unwrap_or("");
    if !name.to_ascii_lowercase().ends_with(".gif") || name.contains("..") {
        return Ok(None);
    }
    let p = Path::new(dir).join(name);
    if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(u64::MAX) > PREVIEW_MAX {
        return Ok(None);
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    Ok(Some(format!("data:image/gif;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

impl Module for WallpaperEngine {
    fn init(&self, _host: &Host, _init: &Init) -> Result<(), RpcError> {
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        let failed = |e: String| RpcError::failed(e);
        let dir = params["dir"].as_str().unwrap_or("");
        match method {
            "library" => {
                let lib = library(&host.cache_dir());
                self.make_thumbs(host, &lib);
                Ok(serde_json::to_value(&lib).unwrap_or(Value::Null))
            }
            "apply" => apply(dir).map(|_| Value::Null).map_err(failed),
            "control" => control(params["cmd"].as_str().unwrap_or("")).map(|_| Value::Null).map_err(failed),
            "preview" => preview(dir).map(|p| json!(p)).map_err(failed),
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}

fn main() {
    island_module::run(WallpaperEngine::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_thumbnail_names() {
        assert_eq!(folder_of("C:/Steam/steamapps/workshop/content/431960/2276970166/scene.pkg"), "c:\\steam\\steamapps\\workshop\\content\\431960\\2276970166");
        let a = thumb_name(Path::new("C:\\nowhere\\preview.gif"));
        assert!(a.starts_with("thumbs/") && a.ends_with("-0.png"));
        assert_eq!(a, thumb_name(Path::new("c:\\NOWHERE\\preview.gif")));
    }

    #[test]
    fn control_commands() {
        assert!(control("rm -rf").unwrap_err().contains("Unknown"));
        assert!(apply("C:\\Windows").is_err());
        assert!(preview("C:\\Windows").is_err());
    }

    #[test]
    fn selection_reads_the_config() {
        let dir = std::env::temp_dir().join(format!("wpe-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = json!({ "someone": { "general": {
            "wallpaperconfig": { "selectedwallpapers": { "Monitor0": { "file": "C:/W/1/scene.pkg" } } },
            "wallpaperconfigrecent": [{ "config": { "selectedwallpapers": { "Monitor0": { "file": "C:/W/2/v.mp4" } } } }, { "config": { "selectedwallpapers": { "Monitor0": { "file": "C:/W/2/v.mp4" } } } }]
        } } });
        std::fs::write(dir.join("config.json"), format!("\u{feff}{cfg}")).unwrap();
        let (cur, recent) = selection(&dir);
        assert_eq!(cur.as_deref(), Some("c:\\w\\1"));
        assert_eq!(recent, ["c:\\w\\2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "wallpaper-engine");
    }
}
