//! Disk Space: what's taking up a drive, for the Task Manager's bubble map.
//!
//! A scan walks every folder it's allowed into (six threads share the work; reparse points such
//! as junctions are skipped so nothing is counted twice) and keeps a compact tree of folders with
//! their sizes and how much of each kind of file they hold (media, documents, code, archives,
//! apps, system, other), plus the largest files. Without administrator rights some folders can't
//! be read (other users', System Volume Information…); they're counted and reported. The tree is
//! only kept while the page uses it: `clear` drops it.

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use serde::Serialize;
use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
    WIN32_FIND_DATAW,
};

/// The kinds of file, in the order the page (tm-tools.js) shows them.
#[allow(dead_code)]
pub const CATS: [&str; 7] = ["Media", "Documents", "Code", "Archives", "Apps & games", "System", "Other"];

fn category(name: &str, in_windows: bool) -> usize {
    if in_windows {
        return 5;
    }
    let ext = match name.rsplit_once('.') {
        Some((_, e)) if e.len() <= 8 => e.to_ascii_lowercase(),
        _ => return 6,
    };
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "heif" | "tif" | "tiff" | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "kra" | "exr" | "hdr"
        | "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" | "flv" | "ts" | "m2ts" | "braw" | "r3d" | "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma"
        | "opus" | "aiff" | "aif" | "mid" | "blend" | "fbx" | "obj" => 0,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt" | "ods" | "odp" | "md" | "csv" | "epub" | "pages" | "numbers" | "key" | "one" => 1,
        "rs" | "js" | "mjs" | "cjs" | "jsx" | "tsx" | "py" | "pyc" | "c" | "cc" | "cpp" | "h" | "hpp" | "cs" | "java" | "class" | "jar" | "go" | "rb" | "php" | "html"
        | "htm" | "css" | "scss" | "json" | "xml" | "yml" | "yaml" | "toml" | "lock" | "sh" | "ps1" | "bat" | "cmd" | "sql" | "ipynb" | "kt" | "swift" | "lua" | "gd"
        | "glsl" | "hlsl" | "rlib" | "rmeta" | "pdb" | "o" | "obj_" | "d" | "map" | "wasm" | "node" => 2,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" | "img" | "cab" | "dmg" | "vhd" | "vhdx" | "vmdk" | "qcow2" | "wim" | "esd" | "mrpack" => 3,
        "exe" | "dll" | "msi" | "msix" | "appx" | "appxbundle" | "msixbundle" | "pak" | "ucas" | "utoc" | "vpk" | "bsa" | "ba2" | "assets" | "bundle" | "resource"
        | "uasset" | "umap" | "wad" | "forge" | "big" | "arc" | "dat" | "bin" | "sys" | "ttf" | "otf" | "apk" | "unity3d" | "rpf" | "cpk" | "acb" | "awb" | "bank" | "wem"
        | "pck" => 4,
        _ => 6,
    }
}

struct Node {
    name: Box<str>,
    parent: u32,
    depth: u16,
    /// Bytes in this folder's own files, then (after the scan) in everything under it.
    size: u64,
    files: u32,
    /// KB of each kind of file, this folder's own (then everything under it).
    cats: [u32; 7],
    first_child: u32,
    next: u32,
}

const NONE: u32 = u32::MAX;

#[derive(Default)]
struct Tree {
    root: String,
    nodes: Vec<Node>,
    biggest: BinaryHeap<Reverse<(u64, u32, Box<str>)>>, // (size, folder, file name)
    denied: u64,
    done: bool,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub root: String,
    pub running: bool,
    pub done: bool,
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub denied: u64,
    pub current: String,
    pub ms: u64,
    pub total: u64,
    pub free: u64,
}

static TREE: Mutex<Option<Tree>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static FILES: AtomicU64 = AtomicU64::new(0);
static FOLDERS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static CURRENT: Mutex<String> = Mutex::new(String::new());
static STARTED: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

fn path_of(nodes: &[Node], mut i: u32, root: &str) -> String {
    let mut parts = vec![];
    while i != 0 && i != NONE {
        parts.push(&*nodes[i as usize].name);
        i = nodes[i as usize].parent;
    }
    let mut p = root.trim_end_matches('\\').to_string();
    for part in parts.iter().rev() {
        p.push('\\');
        p.push_str(part);
    }
    if parts.is_empty() {
        p.push('\\');
    }
    p
}

fn free_space(root: &str) -> (u64, u64) {
    let (mut free, mut total) = (0u64, 0u64);
    unsafe {
        let _ = windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&HSTRING::from(root), None, Some(&mut total), Some(&mut free));
    }
    (total, free)
}

/// Starts scanning `root` ("C:\\" or any folder), dropping any earlier scan.
pub fn start(root: &str) -> Result<(), String> {
    let root = root.trim().to_string();
    if root.len() < 2 || !std::path::Path::new(&root).is_dir() {
        return Err("Choose a drive or folder to scan".into());
    }
    CANCEL.store(true, Ordering::SeqCst);
    while RUNNING.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    CANCEL.store(false, Ordering::SeqCst);
    for c in [&FILES, &FOLDERS, &BYTES] {
        c.store(0, Ordering::SeqCst);
    }
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    let mut t = Tree { root: root.clone(), ..Default::default() };
    t.nodes.push(Node { name: "".into(), parent: NONE, depth: 0, size: 0, files: 0, cats: [0; 7], first_child: NONE, next: NONE });
    *TREE.lock().unwrap() = Some(t);
    *STARTED.lock().unwrap() = Some((Instant::now(), 0));
    RUNNING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        scan(&root, &windir);
        RUNNING.store(false, Ordering::SeqCst);
    });
    Ok(())
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

pub fn clear() {
    CANCEL.store(true, Ordering::SeqCst);
    *TREE.lock().unwrap() = None;
}

struct Work {
    queue: Mutex<(Vec<(u32, String, bool)>, usize)>, // (folder node, path, inside Windows), workers busy
    cv: Condvar,
}

fn scan(root: &str, windir: &str) {
    let work = Arc::new(Work { queue: Mutex::new((vec![(0, root.trim_end_matches('\\').to_string(), false)], 0)), cv: Condvar::new() });
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let work = work.clone();
            let windir = windir.to_string();
            std::thread::spawn(move || worker(&work, &windir))
        })
        .collect();
    for t in threads {
        let _ = t.join();
    }
    // Sizes and kinds roll up into each parent: children always come after their parent.
    let mut g = TREE.lock().unwrap();
    if let Some(t) = g.as_mut() {
        for i in (1..t.nodes.len()).rev() {
            let (p, size, cats) = (t.nodes[i].parent as usize, t.nodes[i].size, t.nodes[i].cats);
            t.nodes[p].size += size;
            for k in 0..7 {
                t.nodes[p].cats[k] = t.nodes[p].cats[k].saturating_add(cats[k]);
            }
        }
        t.done = !CANCEL.load(Ordering::SeqCst);
    }
    drop(g);
    let mut started = STARTED.lock().unwrap();
    if let Some((t0, _)) = *started {
        *started = Some((t0, t0.elapsed().as_millis() as u64));
    }
    drop(started);
    crate::util::log_line(&format!("Disk space: scanned {root} ({} files, {} folders)", FILES.load(Ordering::Relaxed), FOLDERS.load(Ordering::Relaxed)));
}

fn worker(work: &Work, windir: &str) {
    loop {
        let job = {
            let mut q = work.queue.lock().unwrap();
            loop {
                if CANCEL.load(Ordering::SeqCst) {
                    q.0.clear();
                }
                if let Some(j) = q.0.pop() {
                    q.1 += 1;
                    break Some(j);
                }
                if q.1 == 0 {
                    break None;
                }
                q = work.cv.wait(q).unwrap();
            }
        };
        let Some((node, path, in_win)) = job else {
            work.cv.notify_all();
            return;
        };
        let found = list_dir(&path);
        let mut subdirs = vec![];
        {
            let mut g = TREE.lock().unwrap();
            if let Some(t) = g.as_mut() {
                match found {
                    None => t.denied += 1,
                    Some(entries) => {
                        let depth = t.nodes[node as usize].depth + 1;
                        for (name, is_dir, size) in entries {
                            if is_dir {
                                let idx = t.nodes.len() as u32;
                                let next = t.nodes[node as usize].first_child;
                                t.nodes.push(Node { name: name.clone().into(), parent: node, depth, size: 0, files: 0, cats: [0; 7], first_child: NONE, next });
                                t.nodes[node as usize].first_child = idx;
                                let child = format!("{path}\\{name}");
                                let win = in_win || child.to_ascii_lowercase() == windir;
                                subdirs.push((idx, child, win));
                            } else {
                                let n = &mut t.nodes[node as usize];
                                n.size += size;
                                n.files += 1;
                                let c = category(&name, in_win);
                                n.cats[c] = n.cats[c].saturating_add((size / 1024) as u32);
                                if size >= 32 << 20 {
                                    t.biggest.push(Reverse((size, node, name.into())));
                                    if t.biggest.len() > 300 {
                                        t.biggest.pop();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        FOLDERS.fetch_add(1, Ordering::Relaxed);
        if FOLDERS.load(Ordering::Relaxed) % 64 == 0 {
            *CURRENT.lock().unwrap() = path.clone();
        }
        let mut q = work.queue.lock().unwrap();
        q.0.extend(subdirs);
        q.1 -= 1;
        work.cv.notify_all();
    }
}

/// (name, is a folder, size) for everything in a folder; None when it can't be read.
fn list_dir(path: &str) -> Option<Vec<(String, bool, u64)>> {
    let mut out = vec![];
    unsafe {
        let mut fd = WIN32_FIND_DATAW::default();
        let pat = HSTRING::from(format!("{path}\\*"));
        let h = match FindFirstFileExW(&pat, FindExInfoBasic, &mut fd as *mut _ as *mut _, FindExSearchNameMatch, None, FIND_FIRST_EX_LARGE_FETCH) {
            Ok(h) => h,
            Err(e) => return if e.code() == ERROR_ACCESS_DENIED.to_hresult() { None } else { Some(out) },
        };
        loop {
            let n = fd.cFileName.iter().position(|&c| c == 0).unwrap_or(fd.cFileName.len());
            let name = String::from_utf16_lossy(&fd.cFileName[..n]);
            if name != "." && name != ".." {
                let attr = fd.dwFileAttributes;
                let is_dir = attr & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                if is_dir {
                    if attr & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
                        out.push((name, true, 0));
                    }
                } else {
                    let size = ((fd.nFileSizeHigh as u64) << 32) | fd.nFileSizeLow as u64;
                    FILES.fetch_add(1, Ordering::Relaxed);
                    BYTES.fetch_add(size, Ordering::Relaxed);
                    out.push((name, false, size));
                }
            }
            if FindNextFileW(h, &mut fd).is_err() || CANCEL.load(Ordering::Relaxed) {
                break;
            }
        }
        let _ = FindClose(h);
    }
    Some(out)
}

pub fn status() -> Status {
    let g = TREE.lock().unwrap();
    let (root, denied, done) = g.as_ref().map(|t| (t.root.clone(), t.denied, t.done)).unwrap_or_default();
    let ms = STARTED.lock().unwrap().map(|(t0, fin)| if RUNNING.load(Ordering::SeqCst) { t0.elapsed().as_millis() as u64 } else { fin }).unwrap_or(0);
    let (total, free) = if root.is_empty() { (0, 0) } else { free_space(&root) };
    Status {
        running: RUNNING.load(Ordering::SeqCst),
        done,
        files: FILES.load(Ordering::Relaxed),
        folders: FOLDERS.load(Ordering::Relaxed),
        bytes: BYTES.load(Ordering::Relaxed),
        denied,
        current: CURRENT.lock().unwrap().clone(),
        ms,
        total,
        free,
        root,
    }
}

#[derive(Serialize, Clone, Default)]
pub struct Item {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// "dir", "files" (this folder's own files together) or "rest" (smaller folders together).
    pub kind: &'static str,
    pub files: u64,
    /// The kind of file most of its space goes to (index into CATS).
    pub cat: usize,
    /// Has folders under it worth opening.
    pub deeper: bool,
    /// Its own biggest folders (and its files together), for drawing inside it.
    pub kids: Vec<Kid>,
}

#[derive(Serialize, Clone, Default)]
pub struct Kid {
    pub name: String,
    pub size: u64,
    pub cat: usize,
}

#[derive(Serialize, Clone, Default)]
pub struct View {
    pub path: String,
    /// From the scanned root down to this folder: (name, path).
    pub crumbs: Vec<(String, String)>,
    pub size: u64,
    pub files: u64,
    pub items: Vec<Item>,
    /// Bytes of each kind of file under this folder.
    pub cats: Vec<u64>,
    pub folders: Vec<Item>,
    pub largest: Vec<Item>,
}

fn dominant(c: &[u32; 7]) -> usize {
    (0..7).max_by_key(|&i| c[i]).unwrap_or(6)
}

fn find(t: &Tree, path: &str) -> Option<u32> {
    let root = t.root.trim_end_matches('\\').to_ascii_lowercase();
    let p = path.trim_end_matches('\\').to_ascii_lowercase();
    let rest = p.strip_prefix(&root)?;
    let mut node = 0u32;
    for part in rest.split('\\').filter(|s| !s.is_empty()) {
        let mut c = t.nodes[node as usize].first_child;
        let mut hit = NONE;
        while c != NONE {
            if t.nodes[c as usize].name.eq_ignore_ascii_case(part) {
                hit = c;
                break;
            }
            c = t.nodes[c as usize].next;
        }
        if hit == NONE {
            return None;
        }
        node = hit;
    }
    Some(node)
}

fn children(t: &Tree, node: u32) -> Vec<u32> {
    let mut v = vec![];
    let mut c = t.nodes[node as usize].first_child;
    while c != NONE {
        v.push(c);
        c = t.nodes[c as usize].next;
    }
    v
}

/// What's in a folder of the scan (the root when `path` is empty): its biggest folders and its own
/// files as items for the map, the kinds of file in it, its weightiest folders and largest files.
pub fn view(path: &str) -> Option<View> {
    let g = TREE.lock().unwrap();
    let t = g.as_ref()?;
    let node = if path.is_empty() { 0 } else { find(t, path)? };
    let n = &t.nodes[node as usize];
    let here = path_of(&t.nodes, node, &t.root);
    // While scanning, sizes haven't rolled up yet: add up what's under each folder on the fly.
    let rolled = t.done || !RUNNING.load(Ordering::SeqCst);
    let mut subtree_cache: HashMap<u32, (u64, [u32; 7], u64)> = HashMap::new();
    let mut measure = |i: u32| -> (u64, [u32; 7], u64) {
        if rolled {
            let x = &t.nodes[i as usize];
            return (x.size, x.cats, x.files as u64);
        }
        if let Some(v) = subtree_cache.get(&i) {
            return *v;
        }
        let (mut size, mut cats, mut files) = (0u64, [0u32; 7], 0u64);
        let mut stack = vec![i];
        while let Some(j) = stack.pop() {
            let x = &t.nodes[j as usize];
            size += x.size;
            files += x.files as u64;
            for k in 0..7 {
                cats[k] = cats[k].saturating_add(x.cats[k]);
            }
            stack.extend(children(t, j));
        }
        subtree_cache.insert(i, (size, cats, files));
        (size, cats, files)
    };
    let (total, cats, _) = measure(node);
    let mut own_files = n.files as u64;
    let mut own = n.size;
    if rolled {
        own = n.size - children(t, node).iter().map(|&c| t.nodes[c as usize].size).sum::<u64>();
        own_files = n.files as u64;
    }
    let mut dirs: Vec<(u32, u64, [u32; 7], u64)> = children(t, node).into_iter().map(|c| { let (s, k, f) = measure(c); (c, s, k, f) }).collect();
    dirs.sort_by(|a, b| b.1.cmp(&a.1));
    let mut items: Vec<Item> = vec![];
    let keep = 90;
    for &(c, size, k, files) in dirs.iter().take(keep) {
        if size == 0 {
            continue;
        }
        // Once sizes have rolled up, each folder brings its biggest folders and its own files.
        let mut kids = vec![];
        if rolled {
            // Look through folders that are nearly all one subfolder (Program Files (x86) → Steam →
            // steamapps → common), so what's drawn inside is a real spread.
            let mut at = c;
            for _ in 0..5 {
                let size = t.nodes[at as usize].size.max(1);
                match children(t, at).into_iter().max_by_key(|&g| t.nodes[g as usize].size) {
                    Some(big) if t.nodes[big as usize].size as f64 >= size as f64 * 0.75 => at = big,
                    _ => break,
                }
            }
            let c = at;
            let sub = children(t, c);
            let mut v: Vec<Kid> = sub.iter().map(|&g| { let x = &t.nodes[g as usize]; Kid { name: x.name.to_string(), size: x.size, cat: dominant(&x.cats) } }).filter(|q| q.size > 0).collect();
            let own = t.nodes[c as usize].size.saturating_sub(v.iter().map(|q| q.size).sum::<u64>());
            v.sort_by(|a, b| b.size.cmp(&a.size));
            v.truncate(24);
            if own > 0 {
                v.push(Kid { name: "files".into(), size: own, cat: dominant(&t.nodes[c as usize].cats) });
            }
            kids = v;
        }
        items.push(Item { name: t.nodes[c as usize].name.to_string(), path: path_of(&t.nodes, c, &t.root), size, kind: "dir", files, cat: dominant(&k), deeper: t.nodes[c as usize].first_child != NONE, kids });
    }
    let rest: u64 = dirs.iter().skip(keep).map(|d| d.1).sum();
    if rest > 0 {
        items.push(Item { name: format!("{} smaller folders", dirs.len() - keep), path: here.clone(), size: rest, kind: "rest", files: 0, cat: 6, deeper: false, kids: vec![] });
    }
    if own > 0 {
        let own_cats = if rolled {
            let mut c = n.cats;
            for &d in &children(t, node) {
                for k in 0..7 {
                    c[k] = c[k].saturating_sub(t.nodes[d as usize].cats[k]);
                }
            }
            c
        } else {
            n.cats
        };
        items.push(Item { name: format!("{own_files} files here"), path: here.clone(), size: own, kind: "files", files: own_files, cat: dominant(&own_cats), deeper: false, kids: vec![] });
    }
    items.sort_by(|a, b| b.size.cmp(&a.size));
    // Weightiest folders below: ones holding at least 1% where no single subfolder holds 70%+.
    let mut folders = vec![];
    if rolled {
        let mut stack = vec![node];
        let floor = total / 100;
        while let Some(i) = stack.pop() {
            let kids = children(t, i);
            let x = &t.nodes[i as usize];
            let top = kids.iter().map(|&c| t.nodes[c as usize].size).max().unwrap_or(0);
            if i != node && x.size >= floor && (top as f64) < x.size as f64 * 0.7 {
                folders.push(Item { name: x.name.to_string(), path: path_of(&t.nodes, i, &t.root), size: x.size, kind: "dir", files: x.files as u64, cat: dominant(&x.cats), deeper: !kids.is_empty(), kids: vec![] });
            }
            for c in kids {
                if t.nodes[c as usize].size >= floor {
                    stack.push(c);
                }
            }
        }
        folders.sort_by(|a, b| b.size.cmp(&a.size));
        folders.truncate(14);
    }
    // Largest files under this folder.
    let prefix = format!("{}\\", here.trim_end_matches('\\').to_ascii_lowercase());
    let mut largest: Vec<Item> = t
        .biggest
        .iter()
        .map(|Reverse((size, dir, name))| {
            let dpath = path_of(&t.nodes, *dir, &t.root);
            (size, format!("{}\\{name}", dpath.trim_end_matches('\\')), name)
        })
        .filter(|(_, p, _)| node == 0 || p.to_ascii_lowercase().starts_with(&prefix))
        .map(|(size, p, name)| Item { name: name.to_string(), path: p, size: *size, kind: "file", files: 1, cat: category(name, false), deeper: false, kids: vec![] })
        .collect();
    largest.sort_by(|a, b| b.size.cmp(&a.size));
    largest.truncate(20);
    // Crumbs from the root.
    let mut chain = vec![];
    let mut i = node;
    while i != NONE {
        chain.push(i);
        i = t.nodes[i as usize].parent;
    }
    let crumbs = chain.iter().rev().map(|&i| (if i == 0 { t.root.trim_end_matches('\\').to_string() } else { t.nodes[i as usize].name.to_string() }, path_of(&t.nodes, i, &t.root))).collect();
    Some(View { path: here, crumbs, size: total, files: measure(node).2, items, cats: cats.iter().map(|&k| k as u64 * 1024).collect(), folders, largest })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(CATS[category("Movie.MKV", false)], "Media");
        assert_eq!(CATS[category("report.pdf", false)], "Documents");
        assert_eq!(CATS[category("main.rs", false)], "Code");
        assert_eq!(CATS[category("backup.7z", false)], "Archives");
        assert_eq!(CATS[category("pakchunk0-Windows.pak", false)], "Apps & games");
        assert_eq!(CATS[category("anything.dll", true)], "System");
        assert_eq!(CATS[category("README", false)], "Other");
    }

    #[test]
    #[ignore]
    fn live() {
        let root = std::env::var("SCAN").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
        start(&root).unwrap();
        while status().running {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let s = status();
            println!("{} files {} folders {} MB denied {} {}", s.files, s.folders, s.bytes >> 20, s.denied, s.current);
        }
        let v = view("").unwrap();
        println!("{} total {} MB, {} files; cats {:?}", v.path, v.size >> 20, v.files, v.cats.iter().map(|c| c >> 20).collect::<Vec<_>>());
        for i in v.items.iter().take(12) {
            println!("  {:<40} {:>8} MB {:<6} {}", i.name, i.size >> 20, i.kind, CATS[i.cat]);
        }
        for f in v.folders.iter().take(6) {
            println!("  folder {} {} MB", f.path, f.size >> 20);
        }
        for f in v.largest.iter().take(6) {
            println!("  file {} {} MB", f.path, f.size >> 20);
        }
        println!("{:?}", v.crumbs);
        let s = status();
        println!("done {} in {} ms", s.done, s.ms);
    }
}
