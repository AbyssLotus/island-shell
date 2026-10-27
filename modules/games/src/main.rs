//! Installed Steam games, an Island Shell module backend (docs/design/modules.md); its UI is
//! ui/games.js (the Games tab, its Command Center sheet, search results and Win + G). Read straight
//! from Steam's own files: the library folders, each game's app manifest (name, install state, last
//! played), and portrait cover art: the custom covers the user set in Steam
//! (userdata/<id>/config/grid/<appid>p.png) first, then the default art Steam keeps in its library
//! cache. Games launch through `steam://rungameid/`.
//!
//! For the Command Center's Games sheet there's more: each game's wide hero banner and logo (custom
//! ones first, with Steam's own logo placement), its size on disk, and how long you've played it
//! (all time and the last two weeks), read from your account's localconfig.vdf.
//!
//! Art is either a small copy in the module's cache folder (`cache:<path>`) or a file in one of
//! Steam's art folders, which the module shares with its UI (`host.images`). Nothing goes on the
//! network.
//!
//! RPC methods: `library`, `action { appid, what }`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use island_module::{CallResult, Host, Init, Module, RpcError, Value};
use island_win::steam::{decode, libraries, tokens};
use island_win::{image, reg, shell, steam};
use serde::Serialize;

#[derive(Serialize, Clone)]
struct Game {
    appid: u32,
    name: String,
    /// Unix seconds; 0 = never.
    last_played: u64,
    /// Portrait cover on disk: the user's custom one from Steam, else Steam's; a small cached copy once
    /// one has been made (animated covers stay as they are, so they keep playing).
    cover: Option<String>,
    /// The cover is the user's own custom artwork.
    custom: bool,
    /// Landscape header, used when there's no portrait cover.
    header: Option<String>,
    /// Install folder, for "Open install folder".
    dir: String,
    /// Installed but waiting on an update.
    needs_update: bool,
    /// The wide banner for the game's page: your custom one, else Steam's (a 2560-wide copy once made).
    hero: Option<String>,
    /// The game's logo, to lay over the hero.
    logo: Option<String>,
    /// Where Steam puts that logo on the hero (from your custom artwork settings).
    logo_pos: Option<LogoPos>,
    /// Bytes on disk.
    size: u64,
    /// Minutes played, all time and in the last two weeks (Steam's own count).
    playtime: u32,
    playtime_2wks: u32,
}

#[derive(Serialize, Clone)]
struct LogoPos {
    /// BottomLeft, UpperLeft, CenterCenter, UpperCenter or BottomCenter.
    pin: String,
    /// Largest size as a share of the hero's width and height, in percent.
    w: f64,
    h: f64,
}

#[derive(Serialize, Clone, Default)]
struct Library {
    installed: bool,
    games: Vec<Game>,
    /// The game Steam says is running right now.
    running: u32,
    /// Folders the page may load cover images from (custom artwork and Steam's cache).
    #[serde(skip)]
    art_dirs: Vec<PathBuf>,
}

/// Steam's own tools and things that aren't games (Wallpaper Engine gets its own integration).
const NOT_GAMES: [u32; 9] = [228980, 431960, 250820, 1070560, 1391110, 1628350, 1493710, 1887720, 2180100];
const NOT_GAME_WORDS: [&str; 6] = ["redistributable", "dedicated server", "sdk", "soundtrack", "proton", "steam linux runtime"];

/// The newest file with one of `names` in the game's cache folder (Steam keeps old versions in
/// hashed subfolders).
fn cached_art(cache: &Path, appid: u32, names: &[&str]) -> Option<String> {
    let dir = cache.join(appid.to_string());
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut consider = |p: PathBuf| {
        if let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().map_or(true, |(t, _)| m > *t) {
                best = Some((m, p));
            }
        }
    };
    for name in names {
        consider(dir.join(name));
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                for name in names {
                    consider(e.path().join(name));
                }
            }
        }
    }
    best.map(|(_, p)| p.to_string_lossy().into_owned())
}

/// Each Steam account's custom artwork folder, most recently changed first.
fn grid_dirs(steam: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(steam.join("userdata"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path().join("config").join("grid"))
                .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().map(|(_, p)| p).collect()
}

/// The portrait cover the user set for this game in Steam (Manage → Set custom artwork). These can
/// be animated (WebP or APNG, often saved with a .png name), which the page shows as-is. A huge
/// still image is shrunk once into the island's own cache; one that can't be is skipped.
fn custom_cover(grids: &[PathBuf], appid: u32) -> Option<String> {
    let src = grids.iter().find_map(|g| ["png", "jpg", "jpeg", "webp"].iter().map(|ext| g.join(format!("{appid}p.{ext}"))).find(|p| p.is_file()))?;
    let (kind, w, h) = image_info(&src)?;
    if w <= MAX_W && h <= MAX_H {
        return Some(src.to_string_lossy().into_owned());
    }
    if kind != Kind::Png {
        eprintln!("custom cover {} is {w}x{h}, too big to show; using Steam's", src.display());
        return None;
    }
    let stamp = std::fs::metadata(&src).and_then(|m| m.modified()).ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let dst = cache().join("covers").join(format!("{appid}-{stamp}.png"));
    if !dst.is_file() {
        let _ = std::fs::create_dir_all(dst.parent()?);
        let t = std::time::Instant::now();
        match shrink_png(&src, &dst, 600) {
            Some(()) => eprintln!("shrank the {w}x{h} custom cover for {appid} in {:.1} s", t.elapsed().as_secs_f32()),
            None => {
                eprintln!("couldn't shrink the {w}x{h} custom cover for {appid}; using Steam's");
                return None;
            }
        }
    }
    Some(art(&dst))
}

/// A custom hero or logo from Steam (`<appid>_hero.png`, `<appid>_logo.png`).
fn custom_art(grids: &[PathBuf], appid: u32, suffix: &str) -> Option<String> {
    grids.iter().find_map(|g| ["png", "jpg", "jpeg", "webp"].iter().map(|ext| g.join(format!("{appid}{suffix}.{ext}"))).find(|p| p.is_file())).map(|p| p.to_string_lossy().into_owned())
}

/// The logo placement Steam saved next to a custom logo (`<appid>.json`).
fn logo_pos(grids: &[PathBuf], appid: u32) -> Option<LogoPos> {
    let text = grids.iter().find_map(|g| std::fs::read_to_string(g.join(format!("{appid}.json"))).ok())?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let p = &v["logoPosition"];
    Some(LogoPos { pin: p["pinnedPosition"].as_str()?.to_string(), w: p["nWidthPct"].as_f64()?, h: p["nHeightPct"].as_f64()? })
}

/* ---------- small copies of the art ----------
   What an image costs the page is its decoded size, not its file size: a 600×900 cover is 2 MB of
   pixels and a 3840-wide hero 19 MB, so a shelf of 36 covers plus a hero ran to ~250 MB. Covers are
   shown at most ~160 px wide and heroes at the screen's width, so smaller copies (made once, in the
   background, with Windows' image decoder) look the same at a fraction of the memory. */

const COVER_W: u32 = 300;
const HERO_W: u32 = 2560;

/// An animated PNG, WebP or GIF (custom covers can be), which is left as it is so it keeps playing.
fn animated(path: &Path) -> bool {
    use std::io::Read;
    let mut d = vec![0u8; 64 * 1024];
    let Ok(n) = std::fs::File::open(path).and_then(|mut f| f.read(&mut d)) else { return false };
    let d = &d[..n];
    if d.starts_with(b"\x89PNG") {
        let idat = d.windows(4).position(|w| w == b"IDAT").unwrap_or(d.len());
        return d[..idat].windows(4).any(|w| w == b"acTL");
    }
    if d.len() > 21 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" && &d[12..16] == b"VP8X" {
        return d[20] & 0x02 != 0;
    }
    d.starts_with(b"GIF8")
}

fn small_path(src: &Path, w: u32) -> PathBuf {
    let stamp = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in src.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    cache().join("thumbs").join(format!("{h:016x}-{stamp}-{w}.png"))
}

/// The small copy of `src` when it exists; otherwise `src`, noting the copy to make.
fn small_or(src: Option<String>, w: u32, todo: &mut Vec<(PathBuf, PathBuf, u32)>) -> Option<String> {
    let s = src?;
    if s.starts_with("cache:") {
        return Some(s); // already a small copy of ours
    }
    let p = Path::new(&s);
    if animated(p) || image_info(p).is_some_and(|(_, iw, _)| iw <= w) {
        return Some(s);
    }
    let small = small_path(p, w);
    if small.is_file() {
        return Some(art(&small));
    }
    todo.push((p.to_path_buf(), small, w));
    Some(s)
}

static MAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes the missing small copies on a background thread (the next refresh picks them up).
fn make_small(todo: Vec<(PathBuf, PathBuf, u32)>) {
    if todo.is_empty() || MAKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        image::com();
        let t = std::time::Instant::now();
        let mut made = 0;
        for (src, dst, w) in &todo {
            let _ = std::fs::create_dir_all(dst.parent().unwrap_or(Path::new(".")));
            if island_win::image::scale_to_png(src, dst, *w).is_some() {
                made += 1;
            }
        }
        eprintln!("made {made} small copies of covers and heroes in {:.1} s", t.elapsed().as_secs_f32());
        MAKING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/* ---------- playtime, from localconfig.vdf ---------- */

/// A Valve KeyValues value: a string, or a block of named values.
#[derive(Debug, PartialEq)]
enum Kv {
    S(String),
    B(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::B(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::S(_) => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Kv::S(s) => Some(s),
            Kv::B(_) => None,
        }
    }
}

/// Parses KeyValues text into nested blocks (quoted keys and values, braces, // comments).
fn parse_kv(text: &str) -> Kv {
    fn quoted(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut s = String::new();
        while let Some(c) = it.next() {
            match c {
                '\\' => {
                    if let Some(n) = it.next() {
                        s.push(n);
                    }
                }
                '"' => break,
                _ => s.push(c),
            }
        }
        s
    }
    fn block(it: &mut std::iter::Peekable<std::str::Chars>, depth: usize) -> Vec<(String, Kv)> {
        let mut items = vec![];
        let mut key: Option<String> = None;
        while let Some(c) = it.next() {
            match c {
                '"' => {
                    let s = quoted(it);
                    match key.take() {
                        None => key = Some(s),
                        Some(k) => items.push((k, Kv::S(s))),
                    }
                }
                '{' => {
                    let inner = if depth < 64 { block(it, depth + 1) } else { vec![] };
                    if let Some(k) = key.take() {
                        items.push((k, Kv::B(inner)));
                    }
                }
                '}' => return items,
                '/' if it.peek() == Some(&'/') => {
                    for c in it.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        items
    }
    Kv::B(block(&mut text.chars().peekable(), 0))
}

/// The newest localconfig.vdf (the account that played most recently).
fn local_config(steam: &Path) -> Option<PathBuf> {
    std::fs::read_dir(steam.join("userdata"))
        .ok()?
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"))
        .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

static PLAYTIME: Mutex<Option<(PathBuf, SystemTime, HashMap<u32, (u32, u32)>)>> = Mutex::new(None);

/// appid → (minutes played, minutes in the last two weeks), re-read only when the file changes.
fn playtimes(steam: &Path) -> HashMap<u32, (u32, u32)> {
    let Some(path) = local_config(steam) else { return HashMap::new() };
    let Ok(stamp) = std::fs::metadata(&path).and_then(|m| m.modified()) else { return HashMap::new() };
    let mut cache = PLAYTIME.lock().unwrap();
    if let Some((p, t, map)) = cache.as_ref() {
        if *p == path && *t == stamp {
            return map.clone();
        }
    }
    let mut map = HashMap::new();
    if let Ok(bytes) = std::fs::read(&path) {
        let kv = parse_kv(&decode(&bytes));
        let apps = kv.get("UserLocalConfigStore").and_then(|v| v.get("Software")).and_then(|v| v.get("Valve")).and_then(|v| v.get("Steam")).and_then(|v| v.get("apps"));
        if let Some(Kv::B(items)) = apps {
            for (id, v) in items {
                let Ok(appid) = id.parse::<u32>() else { continue };
                let n = |k: &str| v.get(k).and_then(Kv::str).and_then(|s| s.parse().ok()).unwrap_or(0);
                map.insert(appid, (n("Playtime"), n("Playtime2wks")));
            }
        }
    }
    *cache = Some((path, stamp, map.clone()));
    map
}

/// The module's cache folder (small copies of covers and heroes), set at init.
static CACHE: Mutex<Option<PathBuf>> = Mutex::new(None);

fn cache() -> PathBuf {
    CACHE.lock().unwrap().clone().unwrap_or_else(std::env::temp_dir)
}

/// How the page finds a picture: a file in the cache as `cache:<path>`, anything else as it is.
fn art(p: &Path) -> String {
    match p.strip_prefix(cache()) {
        Ok(rel) => format!("cache:{}", rel.to_string_lossy().replace('\\', "/")),
        Err(_) => p.to_string_lossy().into_owned(),
    }
}

/// Bigger than this (3× a 600×900 cover) gets shrunk.
const MAX_W: u32 = 1800;
const MAX_H: u32 = 2700;

#[derive(PartialEq, Debug)]
enum Kind {
    Png,
    Jpeg,
    Webp,
}

/// Format and pixel size from the file header, whatever the extension says.
fn image_info(path: &Path) -> Option<(Kind, u32, u32)> {
    use std::io::Read;
    let mut d = vec![0u8; 256 * 1024];
    let n = std::fs::File::open(path).ok()?.read(&mut d).ok()?;
    let d = &d[..n];
    let be16 = |i: usize| u16::from_be_bytes([d[i], d[i + 1]]) as u32;
    let be32 = |i: usize| u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
    let le24 = |i: usize| d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    if d.len() > 24 && d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((Kind::Png, be32(16), be32(20)));
    }
    if d.len() > 30 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        return match &d[12..16] {
            b"VP8X" => Some((Kind::Webp, 1 + le24(24), 1 + le24(27))),
            b"VP8 " => Some((Kind::Webp, (d[26] as u32 | (d[27] as u32) << 8) & 0x3fff, (d[28] as u32 | (d[29] as u32) << 8) & 0x3fff)),
            b"VP8L" => {
                let b = u32::from_le_bytes([d[21], d[22], d[23], d[24]]);
                Some((Kind::Webp, (b & 0x3fff) + 1, ((b >> 14) & 0x3fff) + 1))
            }
            _ => None,
        };
    }
    if d.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < d.len() {
            if d[i] != 0xff {
                i += 1;
                continue;
            }
            if matches!(d[i + 1], 0xc0..=0xc3) {
                return Some((Kind::Jpeg, be16(i + 7), be16(i + 5)));
            }
            i += 2 + be16(i + 2) as usize;
        }
    }
    None
}

/// Shrinks a (possibly enormous) still PNG to `tw` pixels wide by averaging blocks of source
/// pixels, reading one row at a time so the full image never has to fit in memory.
fn shrink_png(src: &Path, dst: &Path, tw: u32) -> Option<()> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(src).ok()?));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let info = reader.info();
    if info.interlaced || info.animation_control.is_some() {
        return None; // rows don't arrive in order / it's animated: leave those alone
    }
    let (w, h) = (info.width, info.height);
    let (color, _) = reader.output_color_type();
    let ch = color.samples();
    let th = ((h as u64 * tw as u64) / w as u64).max(1) as u32;
    let mut acc = vec![0u64; tw as usize * 4];
    let mut cnt = vec![0u64; tw as usize];
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    let mut cur = 0u32;
    let flush = |acc: &mut [u64], cnt: &mut [u64], out: &mut Vec<u8>| {
        for x in 0..tw as usize {
            let n = cnt[x].max(1);
            out.extend((0..4).map(|c| (acc[x * 4 + c] / n) as u8));
        }
        acc.iter_mut().for_each(|v| *v = 0);
        cnt.iter_mut().for_each(|v| *v = 0);
    };
    let mut y = 0u32;
    while let Ok(Some(row)) = reader.next_row() {
        let oy = ((y as u64 * th as u64) / h as u64) as u32;
        if oy != cur {
            flush(&mut acc, &mut cnt, &mut out);
            cur = oy;
        }
        let data = row.data();
        for x in 0..w as usize {
            let ox = x * tw as usize / w as usize;
            let p = &data[x * ch..x * ch + ch];
            let rgba = match ch {
                1 => [p[0], p[0], p[0], 255],
                2 => [p[0], p[0], p[0], p[1]],
                3 => [p[0], p[1], p[2], 255],
                _ => [p[0], p[1], p[2], p[3]],
            };
            for c in 0..4 {
                acc[ox * 4 + c] += rgba[c] as u64;
            }
            cnt[ox] += 1;
        }
        y += 1;
    }
    flush(&mut acc, &mut cnt, &mut out);
    let rows = (out.len() / (tw as usize * 4)) as u32;
    let bytes = image::encode_png(tw, rows, &out)?;
    std::fs::write(dst, bytes).ok()
}

fn library() -> Library {
    let Some(steam) = steam::dir() else { return Library::default() };
    let cache = steam.join("appcache").join("librarycache");
    let grids = grid_dirs(&steam);
    let played = playtimes(&steam);
    let mut games = vec![];
    let mut todo = vec![];
    for lib in libraries(&steam) {
        let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            let t = tokens(&decode(&bytes));
            let get = |k: &str| t.windows(2).find(|w| w[0] == k).map(|w| w[1].clone()).unwrap_or_default();
            let Ok(appid) = get("appid").parse::<u32>() else { continue };
            let flags: u32 = get("StateFlags").parse().unwrap_or(0);
            let title = get("name").trim().to_string();
            let lower = title.to_lowercase();
            if flags & 4 == 0 || NOT_GAMES.contains(&appid) || NOT_GAME_WORDS.iter().any(|w| lower.contains(w)) {
                continue;
            }
            games.push(Game {
                appid,
                last_played: get("LastPlayed").parse().unwrap_or(0),
                custom: custom_cover(&grids, appid).is_some(),
                cover: small_or(custom_cover(&grids, appid).or_else(|| cached_art(&cache, appid, &["library_600x900.jpg", "library_capsule.jpg"])), COVER_W, &mut todo),
                header: cached_art(&cache, appid, &["library_header.jpg", "header.jpg"]),
                dir: lib.join("steamapps").join("common").join(get("installdir")).to_string_lossy().into_owned(),
                needs_update: flags & 2 != 0,
                hero: small_or(custom_art(&grids, appid, "_hero").or_else(|| cached_art(&cache, appid, &["library_hero.jpg"])), HERO_W, &mut todo),
                logo: custom_art(&grids, appid, "_logo").or_else(|| cached_art(&cache, appid, &["logo.png", "library_logo.png"])),
                logo_pos: logo_pos(&grids, appid),
                size: get("SizeOnDisk").parse().unwrap_or(0),
                playtime: played.get(&appid).map_or(0, |p| p.0),
                playtime_2wks: played.get(&appid).map_or(0, |p| p.1),
                name: title,
            });
        }
    }
    make_small(todo);
    // Most recently played first; never-played games after, alphabetically.
    games.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let mut art_dirs = grids;
    art_dirs.push(cache);
    Library { installed: true, games, running: reg::user_dword("Software\\Valve\\Steam", "RunningAppID").unwrap_or(0), art_dirs }
}

fn open(target: &str) -> Result<(), String> {
    shell::open(target)
}

/// play | library (show in Steam) | folder (install folder) | screenshots | store
fn action(appid: u32, what: &str) -> Result<(), String> {
    match what {
        "play" => open(&format!("steam://rungameid/{appid}")),
        "library" => open(&format!("steam://nav/games/details/{appid}")),
        "screenshots" => open(&format!("steam://open/screenshots/{appid}")),
        "store" => open(&format!("steam://store/{appid}")),
        "folder" => {
            let g = library().games.into_iter().find(|g| g.appid == appid).ok_or("That game isn't installed")?;
            if !Path::new(&g.dir).is_dir() {
                return Err("Its install folder is missing".into());
            }
            open(&g.dir)
        }
        _ => Err(format!("Unknown game action {what}")),
    }
}

// ---------- the module ----------

struct Games;

impl Module for Games {
    fn init(&self, host: &Host, _init: &Init) -> Result<(), RpcError> {
        *CACHE.lock().unwrap() = Some(host.cache_dir());
        Ok(())
    }

    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "library" => {
                let lib = library();
                // Steam's art folders, for the covers, heroes and logos that aren't copies of ours.
                host.share_images(&lib.art_dirs);
                Ok(serde_json::to_value(&lib).unwrap_or(Value::Null))
            }
            "action" => {
                let appid = params["appid"].as_u64().and_then(|a| u32::try_from(a).ok()).ok_or_else(|| RpcError::bad_params("appid"))?;
                action(appid, params["what"].as_str().unwrap_or("")).map(|_| Value::Null).map_err(RpcError::failed)
            }
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}

fn main() {
    island_module::run(Games);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vdf_tokens() {
        let t = tokens("\"AppState\"\n{\n\t\"appid\"\t\t\"220\"\n\t\"name\"\t\t\"Half-Life 2\"\n\t\"path\"\t\t\"C:\\\\Steam\"\n}");
        assert_eq!(t, ["AppState", "appid", "220", "name", "Half-Life 2", "path", "C:\\Steam"]);
        assert_eq!(decode(b"HELLDIVERS\x99 2"), "HELLDIVERS\u{2122} 2");
    }

    #[test]
    fn shrinks_huge_covers() {
        let dir = std::env::temp_dir().join("island-shrink-test");
        let _ = std::fs::create_dir_all(&dir);
        let (w, h) = (3000u32, 4500u32);
        // Left half red, right half blue.
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let src = dir.join("big.png");
        std::fs::write(&src, image::encode_png(w, h, &rgba).unwrap()).unwrap();
        assert_eq!(image_info(&src), Some((Kind::Png, w, h)));
        let dst = dir.join("small.png");
        shrink_png(&src, &dst, 600).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&dst).unwrap())).read_info().unwrap();
        let mut px = vec![0u8; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut px).unwrap();
        let (sw, sh) = (info.width, info.height);
        assert_eq!((sw, sh), (600, 900));
        assert_eq!(&px[..4], &[255, 0, 0, 255]);
        assert_eq!(&px[(599 * 4) as usize..(600 * 4) as usize], &[0, 0, 255, 255]);
    }

    #[test]
    fn manifest_is_valid() {
        let m: island_module::proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.contributes.hotkeys[0].key, "G");
    }

    #[test]
    fn cached_art_is_marked() {
        *CACHE.lock().unwrap() = Some(std::env::temp_dir().join("games-cache-test"));
        assert_eq!(art(&cache().join("thumbs").join("a.png")), "cache:thumbs/a.png");
        assert!(!art(Path::new("/elsewhere/a.png")).starts_with("cache:"));
    }

    #[test]
    fn reads_nested_keyvalues() {
        let kv = parse_kv("\"Root\" { // a comment\n \"apps\" { \"440\" { \"Playtime\" \"50166\" \"LastPlayed\" \"1\" } \"7\" { \"cloud\" { \"x\" \"y\" } } } \"name\" \"a\\\"b\" }");
        let apps = kv.get("root").and_then(|r| r.get("Apps")).unwrap();
        assert_eq!(apps.get("440").and_then(|a| a.get("playtime")).and_then(Kv::str), Some("50166"));
        assert_eq!(kv.get("Root").and_then(|r| r.get("name")).and_then(Kv::str), Some("a\"b"));
        assert!(apps.get("7").and_then(|a| a.get("cloud")).is_some());
    }

    /// `cargo test small_copy_live -- --ignored --nocapture`: every cover or hero still shown at full
    /// size that decodes to 4 MB or more (the small copies are made in the background).
    #[test]
    #[ignore = "needs Steam with games installed"]
    fn small_copy_live() {
        image::com();
        for g in library().games {
            for (what, p) in [("cover", &g.cover), ("hero", &g.hero)] {
                let Some(p) = p else { continue };
                if p.contains("gamethumbs") { continue }
                let path = Path::new(p);
                let decoded = image_info(path).map(|(_, w, h)| w as u64 * h as u64 * 4 / 1_048_576).unwrap_or(0);
                if decoded >= 4 { println!("{what:5} {:>4} MB animated {} {:?} {}", decoded, animated(path), image_info(path), g.name); }
            }
        }
    }

    /// GAMES_DUMP=<file>: writes the live library as JSON (for page tests).
    #[test]
    #[ignore = "needs Steam and GAMES_DUMP=<file>"]
    fn dump_json() {
        let Ok(out) = std::env::var("GAMES_DUMP") else { return };
        std::fs::write(out, serde_json::to_string(&library()).unwrap()).unwrap();
    }

    /// `cargo test live_library -- --ignored --nocapture`
    #[test]
    #[ignore = "needs Steam with games installed"]
    fn live_library() {
        let l = library();
        for g in &l.games {
            println!("{:>8} {:<40} cover {:<7} hero {} logo {} pos {:?} {:>6.1} GB {:>6.1} h ({} min lately)", g.appid, g.name, if g.custom { "custom" } else if g.cover.is_some() { "steam" } else { "none" }, g.hero.is_some(), g.logo.is_some(), g.logo_pos.as_ref().map(|p| (p.pin.clone(), p.w.round(), p.h.round())), g.size as f64 / 1e9, g.playtime as f64 / 60.0, g.playtime_2wks);
        }
        println!("{} games, running {}", l.games.len(), l.running);
    }
}

