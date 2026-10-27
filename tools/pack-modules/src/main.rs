//! pack-modules: builds what a release carries for updates (handoff M3.1).
//!
//! ```text
//! pack-modules pack --out DIR --version X.Y.Z --release-url URL [--installer FILE] [--notes TEXT]
//!                   [--modules DIR] [--bins DIR] [--previous modules.json] [--published TIME] [--unsigned]
//! pack-modules verify DIR --key FILE
//! pack-modules stage --out DIR [--modules DIR] [--bins DIR]
//! ```
//!
//! `pack` makes, in DIR:
//! - `<id>-<version>.zip` for every module in `--modules` (default `modules`): its manifest with
//!   `files` filled in (the sha256 of every file), its UI and assets, and its backend exe from
//!   `--bins` (default `target/release`). The zips are deterministic: the same files make the same
//!   bytes. A module whose version is already in `--previous` (the last release's catalog) keeps that
//!   release's zip and link, and must not have changed.
//! - `IslandShellSetup.exe`, a copy of `--installer`, for the built-in updater.
//! - `modules.json`, the catalog (crates/island-catalog), and `modules.json.minisig`, its minisign
//!   signature made with the key in `ISLAND_SIGNING_KEY` (the text of the key file `cargo tauri
//!   signer generate` writes; `ISLAND_SIGNING_KEY_PASSWORD` if it has one). `--unsigned` skips it,
//!   for dry runs: nothing will accept that catalog.
//!
//! `verify` checks a folder made by `pack` against a public key: the signature, the catalog's
//! rules, and every zip and installer in the folder against what the catalog says.
//!
//! `stage` writes every module, unzipped, into DIR/<id>/ with its backend exe beside it (from
//! `--bins`, default `target/debug`): folders the island loads in developer mode (Modules → Load a
//! folder…, which takes DIR itself to load them all), for trying a build without a release.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use island_catalog::{sha256_hex, Base, Catalog, Module};
use island_host::package;
use island_module_proto::Manifest;

const INSTALLER: &str = "IslandShellSetup.exe";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("pack") => Opts::parse(&args[1..]).and_then(|o| pack(&o)).map(|s| println!("{s}")),
        Some("verify") => verify_cmd(&args[1..]).map(|s| println!("{s}")),
        Some("stage") => stage_cmd(&args[1..]).map(|s| println!("{s}")),
        _ => Err("usage: pack-modules pack --out DIR --version X.Y.Z --release-url URL [options] | pack-modules verify DIR --key FILE | pack-modules stage --out DIR".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pack-modules: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Default)]
struct Opts {
    out: PathBuf,
    version: String,
    release_url: String,
    installer: Option<PathBuf>,
    notes: String,
    modules: PathBuf,
    bins: PathBuf,
    previous: Option<PathBuf>,
    published: Option<String>,
    unsigned: bool,
    /// The signing key's text and password (from the environment, or given by tests).
    key: Option<(String, Option<String>)>,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Opts, String> {
        let mut o = Opts { modules: "modules".into(), bins: PathBuf::from("target").join("release"), ..Default::default() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
            match a.as_str() {
                "--out" => o.out = val()?.into(),
                "--version" => o.version = val()?,
                "--release-url" => o.release_url = val()?,
                "--installer" => o.installer = Some(val()?.into()),
                "--notes" => o.notes = val()?,
                "--modules" => o.modules = val()?.into(),
                "--bins" => o.bins = val()?.into(),
                "--previous" => o.previous = Some(val()?.into()),
                "--published" => o.published = Some(val()?),
                "--unsigned" => o.unsigned = true,
                other => return Err(format!("unknown option {other}")),
            }
        }
        if o.out.as_os_str().is_empty() || o.version.is_empty() || o.release_url.is_empty() {
            return Err("--out, --version and --release-url are needed".into());
        }
        if !o.unsigned {
            let key = std::env::var("ISLAND_SIGNING_KEY").unwrap_or_default();
            if key.trim().is_empty() {
                return Err("ISLAND_SIGNING_KEY isn't set (use --unsigned for a dry run)".into());
            }
            let pw = std::env::var("ISLAND_SIGNING_KEY_PASSWORD").ok().filter(|p| !p.is_empty());
            o.key = Some((key, pw));
        }
        Ok(o)
    }
}

/// Packs everything (see the crate notes); returns a summary.
fn pack(o: &Opts) -> Result<String, String> {
    island_catalog::version(&o.version).ok_or_else(|| format!("{} isn't a version", o.version))?;
    let url = o.release_url.trim_end_matches('/');
    let previous: Option<Catalog> = match &o.previous {
        Some(p) => Some(serde_json::from_slice(&std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?),
        None => None,
    };
    std::fs::create_dir_all(&o.out).map_err(|e| e.to_string())?;
    let mut summary = vec![];
    let mut modules = vec![];
    for dir in module_dirs(&o.modules)? {
        let (manifest, json, files) = collect(&dir, &o.bins)?;
        let zip = package::write(&json, &files)?;
        // What the installer will do with it, before it goes anywhere.
        package::read(&zip).map_err(|e| format!("{}: {e}", manifest.id))?;
        let sha = sha256_hex(&zip);
        let name = format!("{}-{}.zip", manifest.id, manifest.version);
        let kept = previous.as_ref().and_then(|p| p.modules.iter().find(|m| m.id == manifest.id && m.version == manifest.version));
        let entry = match kept {
            Some(m) if m.sha256 == sha => {
                summary.push(format!("{name}: unchanged, kept from the last release"));
                // The package is the same; what the guide and search say about it comes from the manifest.
                island_catalog::Module { keywords: manifest.keywords.clone(), preselect: preselect(&manifest), ..m.clone() }
            }
            Some(_) => return Err(format!("{} changed since the last release but is still version {}: bump its version", manifest.id, manifest.version)),
            None => {
                std::fs::write(o.out.join(&name), &zip).map_err(|e| e.to_string())?;
                summary.push(format!("{name}: {} bytes", zip.len()));
                Module {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    api: manifest.api,
                    description: manifest.description.clone(),
                    talks_to: manifest.talks_to.clone(),
                    size: zip.len() as u64,
                    url: format!("{url}/{name}"),
                    sha256: sha,
                    keywords: manifest.keywords.clone(),
                    preselect: preselect(&manifest),
                }
            }
        };
        modules.push(entry);
    }
    let base = match &o.installer {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            std::fs::write(o.out.join(INSTALLER), &bytes).map_err(|e| e.to_string())?;
            summary.push(format!("{INSTALLER}: {} bytes", bytes.len()));
            Some(Base { version: o.version.clone(), url: format!("{url}/{INSTALLER}"), sha256: sha256_hex(&bytes), size: bytes.len() as u64, notes: o.notes.clone() })
        }
        None => None,
    };
    let catalog = Catalog {
        schema: island_catalog::SCHEMA,
        published: o.published.clone().unwrap_or_else(now_rfc3339),
        base,
        base_min: o.version.clone(),
        api: island_host::install::supported_api(),
        modules,
    };
    island_catalog::check(&catalog)?;
    let data = serde_json::to_vec_pretty(&catalog).map_err(|e| e.to_string())?;
    std::fs::write(o.out.join("modules.json"), &data).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(o.out.join("modules.json.minisig"));
    match &o.key {
        Some((key, pw)) => {
            std::fs::write(o.out.join("modules.json.minisig"), sign(&data, key, pw.clone())?).map_err(|e| e.to_string())?;
            summary.push("modules.json: signed".into());
        }
        None => summary.push("modules.json: NOT SIGNED (dry run): the island won't accept this catalog".into()),
    }
    Ok(summary.join("\n"))
}

/// Module folders (those with a module.json), in name order.
fn module_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?.flatten().map(|e| e.path()).filter(|p| p.join("module.json").is_file()).collect();
    dirs.sort();
    Ok(dirs)
}

/// A module's manifest (with `files` filled in) and its files: everything in its folder but its
/// Rust sources, plus its backend exe from `bins`.
fn collect(dir: &Path, bins: &Path) -> Result<(Manifest, Vec<u8>, BTreeMap<String, Vec<u8>>), String> {
    let at = |e: String| format!("{}: {e}", dir.display());
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("module.json")).map_err(|e| at(e.to_string()))?).map_err(|e| at(e.to_string()))?;
    let manifest: Manifest = serde_json::from_value(json.clone()).map_err(|e| at(e.to_string()))?;
    manifest.validate().map_err(at)?;
    let mut files = BTreeMap::new();
    walk(dir, dir, &mut files).map_err(|e| at(e.to_string()))?;
    if let Some(b) = &manifest.backend {
        // Cargo names the binary like the exe, without .exe off Windows.
        let exe = [bins.join(&b.exe), bins.join(b.exe.trim_end_matches(".exe"))].into_iter().find(|p| p.is_file()).ok_or_else(|| at(format!("its backend {} isn't in {} (build it first)", b.exe, bins.display())))?;
        files.insert(b.exe.clone(), std::fs::read(exe).map_err(|e| at(e.to_string()))?);
    }
    let hashes: serde_json::Map<String, serde_json::Value> = files.iter().map(|(p, b)| (p.clone(), island_host::ui::sri(b).into())).collect();
    json["files"] = hashes.into();
    let manifest: Manifest = serde_json::from_value(json.clone()).map_err(|e| at(e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&json).map_err(|e| at(e.to_string()))?;
    Ok((manifest, bytes, files))
}

/// The setup guide ticks it to start with (the manifest's `contributes.setup.preselect`).
fn preselect(m: &Manifest) -> bool {
    m.contributes.setup.as_ref().is_some_and(|s| s.preselect)
}

/// Not packed: the manifest itself (written separately), Rust sources and build output, dotfiles.
fn skip(rel: &str) -> bool {
    rel == "module.json" || rel == "Cargo.toml" || rel.starts_with("src/") || rel.starts_with("target/") || rel.split('/').any(|p| p.starts_with('.'))
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            if !skip(&format!("{rel}/")) {
                walk(root, &p, out)?;
            }
        } else if !skip(&rel) {
            out.insert(rel, std::fs::read(&p)?);
        }
    }
    Ok(())
}

/// A minisign signature over `data` (prehashed, as minisign and Tauri make them).
fn sign(data: &[u8], key_text: &str, password: Option<String>) -> Result<String, String> {
    let text = unwrap_tauri(key_text);
    // An empty password opens an unencrypted key (None would make minisign ask on the terminal).
    let sk = minisign::SecretKeyBox::from_string(&text).and_then(|b| b.into_secret_key(Some(password.unwrap_or_default()))).map_err(|e| format!("the signing key didn't open: {e}"))?;
    let comment = format!("timestamp:{}\tfile:modules.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    minisign::sign(None, &sk, std::io::Cursor::new(data), Some(&comment), None).map(|s| s.to_string()).map_err(|e| e.to_string())
}

/// `cargo tauri signer generate` writes its keys as base64 of the minisign text; plain text is fine too.
fn unwrap_tauri(text: &str) -> String {
    let t = text.trim();
    if t.starts_with("untrusted comment:") {
        return t.to_string();
    }
    B64.decode(t.replace(['\r', '\n'], "")).ok().and_then(|b| String::from_utf8(b).ok()).filter(|s| s.starts_with("untrusted comment:")).unwrap_or_else(|| t.to_string())
}

/// `stage --out DIR [--modules DIR] [--bins DIR]`
fn stage_cmd(args: &[String]) -> Result<String, String> {
    let (mut out, mut modules, mut bins) = (None, PathBuf::from("modules"), PathBuf::from("target").join("debug"));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(val()?)),
            "--modules" => modules = PathBuf::from(val()?),
            "--bins" => bins = PathBuf::from(val()?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    stage(&out.ok_or("stage needs --out DIR")?, &modules, &bins)
}

/// Every module, unzipped, in out/<id>/: its files, its backend exe and its manifest (with hashes).
fn stage(out: &Path, modules: &Path, bins: &Path) -> Result<String, String> {
    let mut done = vec![];
    for dir in module_dirs(modules)? {
        let (manifest, bytes, files) = collect(&dir, bins)?;
        let to = out.join(&manifest.id);
        let _ = std::fs::remove_dir_all(&to);
        for (rel, data) in &files {
            let p = to.join(rel);
            std::fs::create_dir_all(p.parent().unwrap_or(&to)).map_err(|e| e.to_string())?;
            std::fs::write(&p, data).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        std::fs::write(to.join("module.json"), bytes).map_err(|e| e.to_string())?;
        done.push(manifest.id);
    }
    Ok(format!("staged {} in {}: {}", done.len(), out.display(), done.join(", ")))
}

fn verify_cmd(args: &[String]) -> Result<String, String> {
    let (dir, key) = match args {
        [dir, flag, key] if flag == "--key" => (PathBuf::from(dir), PathBuf::from(key)),
        _ => return Err("usage: pack-modules verify DIR --key FILE".into()),
    };
    let key = std::fs::read_to_string(&key).map_err(|e| format!("{}: {e}", key.display()))?;
    verify(&dir, &key)
}

/// Checks a packed folder (see the crate notes); returns what it checked.
fn verify(dir: &Path, key_text: &str) -> Result<String, String> {
    let key = island_catalog::public_key(key_text).map_err(|e| e.to_string())?;
    let data = std::fs::read(dir.join("modules.json")).map_err(|e| format!("modules.json: {e}"))?;
    let sig = std::fs::read_to_string(dir.join("modules.json.minisig")).map_err(|e| format!("modules.json.minisig: {e}"))?;
    let catalog = island_catalog::parse_verified(&data, &sig, &key).map_err(|e| e.to_string())?;
    let mut out = vec![format!("modules.json: signature good, {} modules", catalog.modules.len())];
    let file_of = |url: &str| url.rsplit('/').next().unwrap_or("").to_string();
    if let Some(b) = &catalog.base {
        match std::fs::read(dir.join(file_of(&b.url))) {
            Ok(bytes) => {
                island_catalog::check_download(&bytes, b.size, &b.sha256).map_err(|e| format!("{INSTALLER}: {e}"))?;
                out.push(format!("{INSTALLER}: {} matches", b.version));
            }
            Err(_) => out.push(format!("{INSTALLER}: not in this folder")),
        }
    }
    for m in &catalog.modules {
        let name = file_of(&m.url);
        let Ok(zip) = std::fs::read(dir.join(&name)) else {
            out.push(format!("{name}: not in this folder (an earlier release's)"));
            continue;
        };
        island_catalog::check_download(&zip, m.size, &m.sha256).map_err(|e| format!("{name}: {e}"))?;
        let p = package::read(&zip).map_err(|e| format!("{name}: {e}"))?;
        if p.manifest.id != m.id || p.manifest.version != m.version {
            return Err(format!("{name} holds {} {}, not {} {}", p.manifest.id, p.manifest.version, m.id, m.version));
        }
        out.push(format!("{name}: matches, {} files", p.files.len()));
    }
    Ok(out.join("\n"))
}

/// Now as RFC 3339 (UTC), without a date library.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = include_str!("../testdata/test-only.key");
    const TEST_PUB: &str = include_str!("../testdata/test-only.pub");
    /// The test key's password, like a real key's (it's only ever used for these tests).
    const TEST_PASSWORD: &str = "test-only";

    /// A modules folder with one module (and its sources, which aren't packed) and a bins folder.
    fn fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("pack-modules-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let m = root.join("modules/sample");
        std::fs::create_dir_all(m.join("ui")).unwrap();
        std::fs::create_dir_all(m.join("src")).unwrap();
        std::fs::create_dir_all(root.join("bins")).unwrap();
        std::fs::write(m.join("module.json"), r#"{ "id": "sample", "name": "Sample", "version": "1.0.0", "api": 1, "description": "A sample.", "icon": "ui/icon.svg",
            "backend": { "exe": "sample-module.exe", "run": "on-demand" }, "ui": { "entry": "ui/sample.js" }, "talksTo": [] }"#).unwrap();
        std::fs::write(m.join("ui/icon.svg"), "<svg/>").unwrap();
        std::fs::write(m.join("ui/sample.js"), "export function activate() {}").unwrap();
        std::fs::write(m.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(m.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("bins/sample-module.exe"), "MZ").unwrap();
        std::fs::write(root.join("installer.exe"), "MZ installer").unwrap();
        (root.join("modules"), root.join("bins"), root)
    }

    fn opts(modules: &Path, bins: &Path, out: PathBuf, root: &Path) -> Opts {
        Opts {
            out,
            version: "0.2.0".into(),
            release_url: "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/".into(),
            installer: Some(root.join("installer.exe")),
            notes: "Modules.".into(),
            modules: modules.into(),
            bins: bins.into(),
            published: Some("2026-11-02T18:00:00Z".into()),
            key: Some((TEST_KEY.into(), Some(TEST_PASSWORD.into()))),
            ..Default::default()
        }
    }

    #[test]
    fn same_files_same_zips_and_verify() {
        let (modules, bins, root) = fixture("same");
        pack(&opts(&modules, &bins, root.join("a"), &root)).unwrap();
        pack(&opts(&modules, &bins, root.join("b"), &root)).unwrap();
        let zip = |d: &str| std::fs::read(root.join(d).join("sample-1.0.0.zip")).unwrap();
        assert_eq!(zip("a"), zip("b"), "byte-identical zips across runs");
        assert_eq!(std::fs::read(root.join("a/modules.json")).unwrap(), std::fs::read(root.join("b/modules.json")).unwrap());
        let report = verify(&root.join("a"), TEST_PUB).unwrap();
        assert!(report.contains("signature good") && report.contains("sample-1.0.0.zip: matches, 3 files") && report.contains("IslandShellSetup.exe: 0.2.0 matches"), "{report}");
        // Sources stay out; the manifest lists everything else.
        let p = package::read(&zip("a")).unwrap();
        assert_eq!(p.files.keys().collect::<Vec<_>>(), ["sample-module.exe", "ui/icon.svg", "ui/sample.js"]);
        let c: Catalog = serde_json::from_slice(&std::fs::read(root.join("a/modules.json")).unwrap()).unwrap();
        assert_eq!(c.modules[0].url, "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/sample-1.0.0.zip");
        assert_eq!(c.base.as_ref().unwrap().url, "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/IslandShellSetup.exe");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_catches_tampering() {
        let (modules, bins, root) = fixture("tamper");
        let out = root.join("out");
        pack(&opts(&modules, &bins, out.clone(), &root)).unwrap();
        let mut zip = std::fs::read(out.join("sample-1.0.0.zip")).unwrap();
        let n = zip.len();
        zip[n / 2] ^= 1;
        std::fs::write(out.join("sample-1.0.0.zip"), &zip).unwrap();
        assert!(verify(&out, TEST_PUB).unwrap_err().contains("doesn't match"));
        // Another key, or an edited catalog.
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap().pk.to_base64();
        assert!(verify(&out, &other).unwrap_err().contains("signature"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unchanged_modules_keep_the_last_release() {
        let (modules, bins, root) = fixture("previous");
        pack(&opts(&modules, &bins, root.join("v1"), &root)).unwrap();
        let mut o = opts(&modules, &bins, root.join("v2"), &root);
        o.version = "0.2.1".into();
        o.release_url = "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.1".into();
        o.previous = Some(root.join("v1/modules.json"));
        pack(&o).unwrap();
        let c: Catalog = serde_json::from_slice(&std::fs::read(root.join("v2/modules.json")).unwrap()).unwrap();
        assert!(c.modules[0].url.contains("/v0.2.0/"), "kept from the last release");
        assert!(!root.join("v2/sample-1.0.0.zip").exists());
        // Changed but not bumped: refused.
        std::fs::write(modules.join("sample/ui/sample.js"), "export function activate() { /* new */ }").unwrap();
        assert!(pack(&o).unwrap_err().contains("bump its version"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dry_run_and_bad_input() {
        let (modules, bins, root) = fixture("dry");
        let mut o = opts(&modules, &bins, root.join("out"), &root);
        o.key = None;
        assert!(pack(&o).unwrap().contains("NOT SIGNED"));
        assert!(!root.join("out/modules.json.minisig").exists());
        assert!(verify(&root.join("out"), TEST_PUB).is_err());
        std::fs::remove_file(bins.join("sample-module.exe")).unwrap();
        assert!(pack(&o).unwrap_err().contains("build it first"));
        assert!(Opts::parse(&["--out".into(), "x".into()]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_repo_modules_pack() {
        // Every real module's manifest and files are packable (with stand-in exes).
        let root = std::env::temp_dir().join(format!("pack-modules-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bins = root.join("bins");
        std::fs::create_dir_all(&bins).unwrap();
        let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let mut zips = vec![];
        for dir in module_dirs(&modules).unwrap() {
            let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join("module.json")).unwrap()).unwrap();
            zips.push(format!("{}-{}.zip", m.id, m.version));
            if let Some(b) = m.backend {
                std::fs::write(bins.join(b.exe), "MZ").unwrap();
            }
        }
        let mut o = opts(&modules, &bins, root.join("out"), &root);
        o.installer = None;
        let summary = pack(&o).unwrap();
        // Each at the version its manifest says.
        for z in &zips {
            assert!(summary.contains(z.as_str()), "{z} missing from:
{summary}");
        }
        verify(&root.join("out"), TEST_PUB).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    /// `cargo test -p pack-modules make_test_key -- --ignored`: writes a fresh test-only key pair
    /// (password "test-only").
    #[test]
    #[ignore = "writes testdata/test-only.key and .pub"]
    fn make_test_key() {
        let kp = minisign::KeyPair::generate_encrypted_keypair(Some(TEST_PASSWORD.into())).unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        std::fs::write(dir.join("test-only.key"), kp.sk.to_box(Some("test-only key: never sign a release with it")).unwrap().to_string()).unwrap();
        std::fs::write(dir.join("test-only.pub"), kp.pk.to_box().unwrap().to_string()).unwrap();
    }
}
