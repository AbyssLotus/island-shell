//! What `http://mod.localhost/<id>/<path>` serves (§4.10), as a pure function of the request path,
//! so the rules are tested here and the base only wraps it in Tauri's protocol handler:
//!
//! - only modules that are installed and on;
//! - only files their manifest lists, after their sha256 matches (checked once per session);
//!   developer-mode folders are unsigned, so any file in them with a known type is served;
//! - `~cache/<path>` serves images from the module's cache folder, with no hash check;
//! - `~img/<absolute path>` serves images from folders the module's backend shared (`host.images`,
//!   like Steam's artwork for Games), and only from inside them;
//! - nothing else outside the module's folders, ever.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use island_module_proto::manifest::safe_path;
use sha2::{Digest, Sha256};

use crate::ModuleInfo;

#[derive(Debug, PartialEq)]
pub enum Served {
    Ok { body: Vec<u8>, mime: &'static str },
    NotFound,
    /// The file is there but may not be served (unlisted, or its hash doesn't match).
    Forbidden(String),
}

/// MIME types by extension; anything else isn't served.
pub fn mime_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain",
        _ => return None,
    })
}

/// `sha256-<base64>` of some bytes, as manifests list them.
pub fn sri(bytes: &[u8]) -> String {
    format!("sha256-{}", B64.encode(Sha256::digest(bytes)))
}

#[derive(Default)]
pub struct UiFiles {
    /// (module, version, path) → whether its hash matched, so each file is hashed once per session.
    checked: Mutex<HashMap<(String, String, String), bool>>,
}

impl UiFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// `path` is the URL path, like `/destiny/ui/destiny.js` (percent-encoded).
    pub fn serve(&self, modules: &[ModuleInfo], cache_root: &Path, path: &str) -> Served {
        let path = decode(path.trim_start_matches('/'));
        let Some((id, rel)) = path.split_once('/') else { return Served::NotFound };
        let Some(m) = modules.iter().find(|m| m.id == id && m.enabled) else { return Served::NotFound };
        if let Some(rest) = rel.strip_prefix("~cache/") {
            let Some(mime) = mime_of(rest).filter(|m| m.starts_with("image/")) else { return Served::Forbidden("only images come from the cache".into()) };
            if safe_path(rest).is_err() {
                return Served::Forbidden("bad path".into());
            }
            return read(&cache_root.join(id).join(rest), mime);
        }
        if let Some(file) = rel.strip_prefix("~img/") {
            let Some(mime) = mime_of(file).filter(|m| m.starts_with("image/")) else { return Served::Forbidden("only images can be shared".into()) };
            let Ok(real) = Path::new(file).canonicalize() else { return Served::NotFound };
            if !real.is_absolute() || !m.image_dirs.iter().any(|d| real.starts_with(d)) {
                return Served::Forbidden("that folder isn't shared".into());
            }
            return read(&real, mime);
        }
        if safe_path(rel).is_err() || rel == "module.json" {
            return Served::Forbidden("bad path".into());
        }
        let Some(mime) = mime_of(rel) else { return Served::Forbidden("unknown file type".into()) };
        let file = PathBuf::from(&m.dir).join(rel);
        if m.dev {
            return read(&file, mime);
        }
        let Some(want) = m.manifest.files.get(rel) else { return Served::Forbidden(format!("{rel} isn't in the manifest")) };
        let key = (m.id.clone(), m.manifest.version.clone(), rel.to_string());
        let known = self.checked.lock().unwrap().get(&key).copied();
        let body = match std::fs::read(&file) {
            Ok(b) => b,
            Err(_) => return Served::NotFound,
        };
        let ok = known.unwrap_or_else(|| {
            let ok = sri(&body) == *want;
            self.checked.lock().unwrap().insert(key, ok);
            ok
        });
        if ok {
            Served::Ok { body, mime }
        } else {
            Served::Forbidden(format!("{rel} doesn't match its hash"))
        }
    }
}

fn read(file: &Path, mime: &'static str) -> Served {
    match std::fs::read(file) {
        Ok(body) => Served::Ok { body, mime },
        Err(_) => Served::NotFound,
    }
}

/// Percent-decoding for URL paths (invalid escapes stay as they are).
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::State;
    use island_module_proto::Manifest;

    fn setup(name: &str) -> (PathBuf, Vec<ModuleInfo>) {
        let root = std::env::temp_dir().join(format!("island-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("modules/destiny");
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/destiny.js"), "export function activate() {}").unwrap();
        std::fs::write(dir.join("ui/destiny.css"), ".d2 {}").unwrap();
        std::fs::write(dir.join("ui/extra.js"), "alert(1)").unwrap();
        std::fs::write(dir.join("module.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("cache/destiny/icons")).unwrap();
        std::fs::write(root.join("cache/destiny/icons/a.png"), [137, 80, 78, 71]).unwrap();
        std::fs::write(root.join("cache/destiny/data.json"), "{}").unwrap();
        std::fs::write(root.join("secret.txt"), "no").unwrap();
        let mut manifest = Manifest { id: "destiny".into(), name: "Destiny 2".into(), version: "1.0.0".into(), api: 1, description: "x".into(), icon: "ui/icon.svg".into(), ..Default::default() };
        manifest.files.insert("ui/destiny.js".into(), sri(b"export function activate() {}"));
        manifest.files.insert("ui/destiny.css".into(), sri(b"something else"));
        let info = ModuleInfo { id: "destiny".into(), manifest, dir: dir.to_string_lossy().into(), dev: false, enabled: true, state: State::Stopped, pid: None, error: String::new(), crashes: 0, image_dirs: vec![] };
        (root, vec![info])
    }

    fn ok(s: Served) -> (Vec<u8>, &'static str) {
        match s {
            Served::Ok { body, mime } => (body, mime),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn listed_files_with_matching_hashes() {
        let (root, mods) = setup("listed");
        let ui = UiFiles::new();
        let cache = root.join("cache");
        let (body, mime) = ok(ui.serve(&mods, &cache, "/destiny/ui/destiny.js"));
        assert_eq!((body.as_slice(), mime), (&b"export function activate() {}"[..], "text/javascript"));
        // Listed, but its hash is wrong.
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/ui/destiny.css"), Served::Forbidden(w) if w.contains("hash")));
        // In the folder but not in the manifest.
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/ui/extra.js"), Served::Forbidden(w) if w.contains("manifest")));
        // The manifest itself, other modules and missing files.
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/module.json"), Served::Forbidden(_)));
        assert_eq!(ui.serve(&mods, &cache, "/music/ui/music.js"), Served::NotFound);
        assert_eq!(ui.serve(&mods, &cache, "/destiny"), Served::NotFound);
        // The hash is checked once a session: a file changed afterwards keeps its verdict.
        std::fs::write(PathBuf::from(&mods[0].dir).join("ui/destiny.js"), "tampered").unwrap();
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/ui/destiny.js"), Served::Ok { .. }));
        let fresh = UiFiles::new();
        assert!(matches!(fresh.serve(&mods, &cache, "/destiny/ui/destiny.js"), Served::Forbidden(_)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_way_out_of_the_folder() {
        let (root, mods) = setup("traversal");
        let ui = UiFiles::new();
        let cache = root.join("cache");
        for p in ["/destiny/../secret.txt", "/destiny/ui/../../../secret.txt", "/destiny/%2e%2e/secret.txt", "/destiny/ui%2f..%2f..%2fsecret.txt", "/destiny/C:/x.js", "/destiny//ui/destiny.js", "/destiny/~cache/../../secret.txt", "/destiny/~cache/%2e%2e/%2e%2e/secret.txt"] {
            assert!(!matches!(ui.serve(&mods, &cache, p), Served::Ok { .. }), "{p}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cache_serves_images_only() {
        let (root, mods) = setup("cache");
        let ui = UiFiles::new();
        let cache = root.join("cache");
        let (_, mime) = ok(ui.serve(&mods, &cache, "/destiny/~cache/icons/a.png"));
        assert_eq!(mime, "image/png");
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/~cache/data.json"), Served::Forbidden(_)));
        assert_eq!(ui.serve(&mods, &cache, "/destiny/~cache/icons/missing.png"), Served::NotFound);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shared_image_folders_only() {
        let (root, mut mods) = setup("img");
        let ui = UiFiles::new();
        let cache = root.join("cache");
        std::fs::create_dir_all(root.join("steam/art")).unwrap();
        std::fs::write(root.join("steam/art/hero.jpg"), [0xff, 0xd8]).unwrap();
        std::fs::write(root.join("steam/art/notes.txt"), "no").unwrap();
        std::fs::write(root.join("steam/other.png"), [137, 80]).unwrap();
        let url = |p: PathBuf| format!("/destiny/~img/{}", p.to_string_lossy().bytes().map(|b| format!("%{b:02X}")).collect::<String>());
        let hero = root.join("steam/art/hero.jpg");
        // Nothing until the backend shares the folder.
        assert!(matches!(ui.serve(&mods, &cache, &url(hero.clone())), Served::Forbidden(_)));
        mods[0].image_dirs = vec![root.join("steam/art").canonicalize().unwrap()];
        assert_eq!(ok(ui.serve(&mods, &cache, &url(hero))).1, "image/jpeg");
        assert!(matches!(ui.serve(&mods, &cache, &url(root.join("steam/art/notes.txt"))), Served::Forbidden(_)));
        assert!(matches!(ui.serve(&mods, &cache, &url(root.join("steam/other.png"))), Served::Forbidden(_)));
        assert!(matches!(ui.serve(&mods, &cache, &url(root.join("steam/art/../other.png"))), Served::Forbidden(_)));
        assert_eq!(ui.serve(&mods, &cache, &url(root.join("steam/art/missing.png"))), Served::NotFound);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn developer_mode_and_disabled_modules() {
        let (root, mut mods) = setup("dev");
        let ui = UiFiles::new();
        let cache = root.join("cache");
        mods[0].dev = true;
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/ui/extra.js"), Served::Ok { .. }));
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/module.json"), Served::Forbidden(_)));
        assert!(matches!(ui.serve(&mods, &cache, "/destiny/../secret.txt"), Served::Forbidden(_)));
        mods[0].enabled = false;
        assert_eq!(ui.serve(&mods, &cache, "/destiny/ui/destiny.js"), Served::NotFound);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn types_and_decoding() {
        assert_eq!(mime_of("a/b.JS"), Some("text/javascript"));
        assert_eq!(mime_of("skin.css"), Some("text/css"));
        assert_eq!(mime_of("x.exe"), None);
        assert_eq!(mime_of("noext"), None);
        assert_eq!(decode("a%20b%2Fc%zz%4"), "a b/c%zz%4");
        assert_eq!(sri(b""), "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
    }
}
