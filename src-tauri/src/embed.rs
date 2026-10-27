//! Websites docked inside the Command Center: the sites modules declare (`contributes.sites`, like
//! the Discord module's web app, the Destiny 2 module's DIM and the user's own Canvas). Each lives in a
//! small borderless window owned by the island window
//! and placed exactly over the slot the page leaves for it, so it reads as part of the Command
//! Center while being the real site (you sign in to it once; the session is kept).
//!
//! - No access to the island: these windows have no capability, so the site can't call any of the
//!   island's commands.
//! - Links that open a new window (Discord's chat links, Canvas files…) go to the default browser
//!   instead (never Edge, see util::open_web); only http(s) pages load inside.
//! - Hidden when the Command Center closes. A hidden site is told to trim its memory at once and is
//!   closed 45 seconds later (a docked web app is the heaviest thing the island runs: DIM alone holds
//!   ~800 MB), so coming straight back is instant and anything longer reopens on its start page.
//! - Each site wears the island skin (the Taken look), CSS only, injected as a <style> before the page
//!   loads and only on that site's own pages (not the sign-in pages it sends you through):
//!   - A module's site (`<module>/<site>`) takes its start page, hosts and skin file from the module's
//!     manifest; the skin is used only if its sha256 matches the manifest (developer mode excepted).
//!     `{settings.<key>}` in its URL or hosts comes from the module's settings.json (Canvas's address).
//! - A site may also have a reader (`sites[].reader`, checked the same way): the module's own script,
//!   run in a hidden copy of the site (`read`), which shares the docked site's sign-in. It reads what
//!   the site's own pages read, for a module whose service has no API it can use (Canvas when the
//!   school turns access tokens off). It runs only on the site's own hosts, and the copy has no more
//!   access to the island than the docked site does: the island asks, and takes what it resolves to.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};

use crate::util::log_line;

/// When each hidden site was hidden (to close the ones left unused).
static HIDDEN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

const KEEP_HIDDEN: Duration = Duration::from_secs(45);

/// A module's site, from its manifest.
struct SiteDef {
    /// The start page.
    url: String,
    /// Where the skin and the reader apply (`*.` dropped: subdomains match too).
    hosts: Vec<String>,
    skin: Option<String>,
    label: String,
    reader: Option<String>,
    read_page: Option<String>,
}

/// A file of the module's, if its sha256 matches the manifest (developer mode excepted).
fn module_file(info: &island_host::ModuleInfo, site: &str, path: &str) -> Option<String> {
    let bytes = std::fs::read(Path::new(&info.dir).join(path)).ok()?;
    let ok = info.dev || info.manifest.files.get(path).is_some_and(|h| *h == island_host::ui::sri(&bytes));
    if !ok {
        log_line(&format!("embed: {site}'s {path} doesn't match its manifest, so it's left off"));
    }
    ok.then(|| String::from_utf8_lossy(&bytes).into_owned())
}

fn module_site(site: &str) -> Result<SiteDef, String> {
    let (id, sid) = site.split_once('/').ok_or_else(|| format!("unknown site {site}"))?;
    let info = crate::modules::sup().and_then(|s| s.info(id)).filter(|m| m.enabled).ok_or("That module isn't installed or is turned off")?;
    let s = info.manifest.contributes.sites.iter().find(|s| s.id == sid).ok_or_else(|| format!("{} has no site {sid}", info.manifest.name))?;
    // `{settings.<key>}` in the URL comes from the module's settings.json.
    let settings: serde_json::Value = std::fs::read(crate::modules::data_root().join(id).join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut url = s.url.clone();
    while let Some(i) = url.find("{settings.") {
        let Some(j) = url[i..].find('}') else { break };
        let key = &url[i + 10..i + j];
        let val = settings[key].as_str().unwrap_or("").to_string();
        if val.is_empty() {
            return Err(format!("Set up {} in its island view first", s.label));
        }
        url.replace_range(i..i + j + 1, &val);
    }
    if !url.starts_with("https://") {
        return Err(format!("{} doesn't start with https://", s.label));
    }
    let hosts = island_host::proto::manifest::resolve_hosts(&s.hosts, &settings).into_iter().map(|h| h.trim_start_matches("*.").to_string()).collect();
    Ok(SiteDef {
        url,
        hosts,
        skin: s.skin.as_ref().and_then(|p| module_file(&info, site, p)),
        label: s.label.clone(),
        reader: s.reader.as_ref().and_then(|p| module_file(&info, site, p)),
        read_page: s.read_page.clone(),
    })
}

/// A site's skin and the host names it belongs on (the skin stays off everywhere else, such as a
/// school's or Bungie's sign-in pages the site sends you through).
fn skin_for(site: &str) -> Option<(String, Vec<String>)> {
    module_site(site).ok().and_then(|d| d.skin.map(|css| (css, d.hosts)))
}

/// Whether `host` is one of the site's hosts or a subdomain of one.
fn on_hosts(hosts: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    hosts.iter().any(|x| host == *x || host.ends_with(&format!(".{x}")))
}

/// Runs in every page of the site before its own scripts: on the site's own hosts (or their
/// subdomains), adds the skin as a <style> as soon as <head> exists, so the page never paints in its
/// own colours first. The window has no IPC, so the switch is `__islandSkin(on)`, called through eval.
fn skin_script(css: &str, hosts: &[String], on: bool) -> String {
    let css = serde_json::to_string(css).unwrap_or_else(|_| "''".into());
    let hosts = serde_json::to_string(hosts).unwrap_or_else(|_| "[]".into());
    let media = if on { "all" } else { "not all" };
    format!(
        "(()=>{{const h=location.hostname.toLowerCase();if(!{hosts}.some(x=>h===x||h.endsWith('.'+x)))return;\
         const el=document.createElement('style');el.id='island-skin';el.media='{media}';el.textContent={css};\
         Object.defineProperty(window,'__islandSkin',{{value:on=>{{el.media=on?'all':'not all';}}}});\
         const put=()=>el.isConnected||(document.head?(document.head.appendChild(el),true):false);\
         if(!put()){{const mo=new MutationObserver(()=>{{if(put())mo.disconnect();}});mo.observe(document,{{childList:true,subtree:true}});}}}})();"
    )
}

/// The site's start page.
fn start_url(site: &str) -> Result<String, String> {
    module_site(site).map(|d| d.url)
}

fn title(site: &str) -> String {
    module_site(site).map(|d| d.label).unwrap_or_else(|_| "Island Shell".into())
}

/// The window's label: `embed-` and the site, with a module site's slash as a dash.
fn label(site: &str) -> String {
    format!("embed-{}", site.replace('/', "-"))
}

/// Shows a site over the given rectangle (CSS pixels in the island window, which covers the monitor).
/// `skin` switches the site's island skin on or off.
pub fn show(app: &AppHandle, site: &str, x: f64, y: f64, w: f64, h: f64, scale: f64, origin: (i32, i32), skin: bool) -> Result<(), String> {
    let island = app.get_webview_window("island").ok_or("no island window")?;
    let pos = tauri::PhysicalPosition::new(origin.0 + (x * scale).round() as i32, origin.1 + (y * scale).round() as i32);
    let size = tauri::PhysicalSize::new((w * scale).round().max(200.0) as u32, (h * scale).round().max(200.0) as u32);
    let win = match app.get_webview_window(&label(site)) {
        Some(win) => win,
        None => {
            let url = start_url(site)?;
            let mut builder = WebviewWindowBuilder::new(app, label(site), WebviewUrl::External(url.parse().map_err(|e| format!("{e}"))?));
            if let Some((css, hosts)) = skin_for(site) {
                builder = builder.initialization_script(skin_script(&css, &hosts, skin));
            }
            let win = builder
                .title(title(site))
                .decorations(false)
                .resizable(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .visible(false)
                .focused(false)
                .shadow(false)
                .parent(&island)
                .map_err(|e| e.to_string())?
                // Pages of any site may load (sign-in pages hop between domains); other schemes don't.
                .on_navigation(|u| matches!(u.scheme(), "https" | "http" | "about" | "data" | "blob"))
                // New windows (chat links, attachments) open in the default browser instead.
                .on_new_window(|u, _| {
                    if matches!(u.scheme(), "https" | "http") {
                        let _ = crate::util::open_web(u.as_str());
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()
                .map_err(|e| e.to_string())?;
            if let Ok(h) = win.hwnd() {
                unsafe {
                    let _ = DwmSetWindowAttribute(HWND(h.0 as _), DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND as *const _ as *const _, 4);
                }
            }
            log_line(&format!("embed: opened {site}"));
            win
        }
    };
    let _ = win.eval(format!("window.__islandSkin&&window.__islandSkin({skin})"));
    crate::island::set_memory_low(&win, false);
    let _ = win.set_size(size);
    let _ = win.set_position(pos);
    let _ = win.show();
    HIDDEN.lock().unwrap().get_or_insert_with(HashMap::new).remove(site);
    Ok(())
}

/// Hides every docked site (the Command Center closed or left that section).
pub fn hide_all(app: &AppHandle) {
    let mut hidden = HIDDEN.lock().unwrap();
    let hidden = hidden.get_or_insert_with(HashMap::new);
    for (lbl, w) in app.webview_windows() {
        let Some(site) = lbl.strip_prefix("embed-") else { continue };
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            crate::island::set_memory_low(&w, true);
            hidden.insert(site.to_string(), Instant::now());
        }
    }
}

/// Closes sites that have been hidden for a while, so they don't hold memory.
pub fn spawn_reaper(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let stale: Vec<String> = {
            let hidden = HIDDEN.lock().unwrap();
            hidden.as_ref().map(|h| h.iter().filter(|(_, t)| t.elapsed() > KEEP_HIDDEN).map(|(s, _)| s.clone()).collect()).unwrap_or_default()
        };
        for site in stale {
            if let Some(w) = app.get_webview_window(&format!("embed-{site}")) {
                if !w.is_visible().unwrap_or(true) {
                    let _ = w.destroy();
                    log_line(&format!("embed: closed {site} (hidden for {} s)", KEEP_HIDDEN.as_secs()));
                }
            }
            if let Some(h) = HIDDEN.lock().unwrap().as_mut() {
                h.remove(&site);
            }
        }
    });
}

/// How long a read may take, from opening the hidden copy to the reader's answer.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Reads running now (each holds a hidden copy of a site open for a few seconds).
static READING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Runs a site's reader in a hidden copy of the site and returns what it resolved to. The copy opens
/// the site's `readPage` (or its start page) with the docked site's sign-in (one WebView2 profile),
/// the reader runs once a page of the site's own hosts has loaded, and the copy closes straight
/// after. If the site sends the copy to a sign-in page elsewhere, that's an error and the reader
/// never runs there. Blocking: call it off the main thread.
pub fn read(app: &AppHandle, site: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    use std::sync::atomic::Ordering;
    use tauri::webview::PageLoadEvent;
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            READING.fetch_sub(1, Ordering::Relaxed);
        }
    }

    let def = module_site(site)?;
    let reader = def.reader.ok_or_else(|| format!("{} has no reader", def.label))?;
    let start: tauri::Url = def.url.parse().map_err(|e| format!("{e}"))?;
    let url = match &def.read_page {
        Some(p) => start.join(p).map_err(|e| format!("{e}"))?,
        None => start,
    };
    if READING.fetch_add(1, Ordering::Relaxed) >= 3 {
        READING.fetch_sub(1, Ordering::Relaxed);
        return Err(format!("{} is already being read", def.label));
    }
    let _running = Running;
    let t0 = Instant::now();
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let (loaded, pages) = std::sync::mpsc::channel::<tauri::Url>();
    let win = WebviewWindowBuilder::new(app, format!("read-{}-{n}", site.replace('/', "-")), WebviewUrl::External(url))
        .title(format!("{} (reading)", def.label))
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .decorations(false)
        .inner_size(1024.0, 768.0)
        .on_navigation(|u| matches!(u.scheme(), "https" | "http" | "about" | "data" | "blob"))
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_page_load(move |_, p| {
            if p.event() == PageLoadEvent::Finished {
                let _ = loaded.send(p.url().clone());
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    let out = wait_for_site(&pages, &def.hosts, &def.label, t0).and_then(|()| run_reader(&win, &reader, args, n, &def.label, t0));
    let _ = win.destroy();
    match &out {
        Ok(_) => log_line(&format!("embed: read {site} in {} ms", t0.elapsed().as_millis())),
        Err(e) => log_line(&format!("embed: reading {site} failed after {} ms: {e}", t0.elapsed().as_millis())),
    }
    out
}

/// Numbers the hidden copies (their window labels, and the reader's answer slot).
static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// Waits for the copy to finish loading a page of the site. Single sign-on may pass through other
/// hosts and come straight back while you're still signed in, so a page elsewhere only counts as
/// "signed out" once nothing has followed it for a few seconds.
fn wait_for_site(pages: &std::sync::mpsc::Receiver<tauri::Url>, hosts: &[String], label: &str, t0: Instant) -> Result<(), String> {
    let mut away = false;
    loop {
        let left = READ_TIMEOUT.saturating_sub(t0.elapsed());
        match pages.recv_timeout(if away { left.min(Duration::from_secs(8)) } else { left }) {
            Ok(u) if u.scheme() == "https" && u.host_str().is_some_and(|h| on_hosts(hosts, h)) => return Ok(()),
            Ok(u) if matches!(u.scheme(), "https" | "http") => away = true,
            Ok(_) => {}
            Err(_) if away => return Err(format!("Sign in to {label} in the Command Center first")),
            Err(_) => return Err(format!("{label} didn't load. Are you online?")),
        }
    }
}

/// Starts the reader in the loaded page, then collects its answer. The window has no IPC, so the
/// answer waits in a property named for this read until the island asks for it (ExecuteScript
/// gives back what a script evaluates to, but doesn't wait for a promise).
fn run_reader(win: &tauri::WebviewWindow, reader: &str, args: &serde_json::Value, n: u32, label: &str, t0: Instant) -> Result<serde_json::Value, String> {
    let key = serde_json::to_string(&format!("__islandRead{n}_{}", now_nanos())).unwrap_or_default();
    win.eval(reader_script(reader, args, &key)).map_err(|e| e.to_string())?;
    let probe = format!("(()=>{{const f=window[{key}];return f?f():null}})()");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    loop {
        if t0.elapsed() > READ_TIMEOUT {
            return Err(format!("{label} took too long to answer"));
        }
        std::thread::sleep(Duration::from_millis(250));
        let tx = tx.clone();
        win.eval_with_callback(probe.clone(), move |r| {
            let _ = tx.send(r);
        })
        .map_err(|e| e.to_string())?;
        // The probe's value as JSON: null while the reader works, then its answer (a JSON string).
        let Ok(r) = rx.recv_timeout(Duration::from_secs(5)) else { continue };
        let Ok(Some(answer)) = serde_json::from_str::<Option<String>>(&r) else { continue };
        let v: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        return if v["ok"] == true { Ok(v["value"].clone()) } else { Err(v["error"].as_str().unwrap_or("The reader failed").to_string()) };
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// The reader (one function expression) called with `args`, its result or error kept as JSON in
/// `window[key]()`. Its code goes on lines of its own so a comment at its end can't swallow the call.
fn reader_script(reader: &str, args: &serde_json::Value, key: &str) -> String {
    let body = reader.trim().trim_end_matches(';').trim_end();
    let args = serde_json::to_string(args).unwrap_or_else(|_| "null".into());
    format!(
        "(()=>{{let out=null;Object.defineProperty(window,{key},{{value:()=>out,configurable:true}});\
         Promise.resolve().then(()=>(\n{body}\n)({args})).then(\
         v=>{{out=JSON.stringify({{ok:true,value:v===undefined?null:v}});}},\
         e=>{{out=JSON.stringify({{ok:false,error:String(e&&e.message||e)}});}});}})();"
    )
}

#[cfg(test)]
mod tests {
    /// The skin rides into the site's page as a JSON string inside this script; the test also writes
    /// each site's script to target/<site>-skin-script.js so it can be checked with `node --check`.
    /// The Discord module's skin, the biggest one.
    const DISCORD_SKIN: &str = include_str!("../../modules/discord/skins/discord-skin.css");

    #[test]
    fn skin_script_shape() {
        let hosts = vec!["discord.com".to_string()];
        let on = super::skin_script(DISCORD_SKIN, &hosts, true);
        assert!(on.starts_with("(()=>{") && on.ends_with("})();"));
        assert!(on.contains("el.media='all'") && super::skin_script(DISCORD_SKIN, &hosts, false).contains("el.media='not all'"));
        assert!(on.contains("--neutral-69-hsl") && on.contains("Rubik"));
        assert!(on.contains(r#"["discord.com"].some"#));
        let js = super::skin_script(DISCORD_SKIN, &["example.edu".to_string()], true);
        let _ = std::fs::write(format!("{}/target/discord-skin-script.js", env!("CARGO_MANIFEST_DIR")), js);
    }

    /// The reader rides into the page wrapped in a call; the test writes one to
    /// target/canvas-reader-script.js so it can be checked with `node --check`.
    #[test]
    fn reader_script_shape() {
        let reader = include_str!("../../modules/canvas/readers/canvas-reader.js");
        let js = super::reader_script(reader, &serde_json::json!({ "action": "read" }), "\"__islandRead1_2\"");
        assert!(js.starts_with("(()=>{let out=null;Object.defineProperty(window,\"__islandRead1_2\""));
        assert!(js.contains("\n)({\"action\":\"read\"})"));
        // A trailing semicolon or line comment can't break the call.
        let js = super::reader_script("async function r(a) { return a } // done\n;", &serde_json::Value::Null, "\"k\"");
        assert!(js.contains("async function r(a) { return a } // done\n)(null)"), "{js}");
        let _ = std::fs::write(format!("{}/target/canvas-reader-script.js", env!("CARGO_MANIFEST_DIR")), super::reader_script(reader, &serde_json::Value::Null, "\"k\""));
    }

    #[test]
    fn hosts_match_subdomains() {
        let hosts = vec!["instructure.com".to_string(), "canvas.school.edu".to_string()];
        assert!(super::on_hosts(&hosts, "school.instructure.com") && super::on_hosts(&hosts, "Canvas.School.edu"));
        assert!(!super::on_hosts(&hosts, "evilinstructure.com") && !super::on_hosts(&hosts, "sso.school.edu"));
    }
}
