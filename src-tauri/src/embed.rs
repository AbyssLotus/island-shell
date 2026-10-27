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

/// A module's site from its manifest: (start page, hosts, skin CSS, title).
fn module_site(site: &str) -> Result<(String, Vec<String>, Option<String>, String), String> {
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
    let skin = s.skin.as_ref().and_then(|p| {
        let bytes = std::fs::read(Path::new(&info.dir).join(p)).ok()?;
        let ok = info.dev || info.manifest.files.get(p).is_some_and(|h| *h == island_host::ui::sri(&bytes));
        if !ok {
            log_line(&format!("embed: {site}'s skin doesn't match its manifest, so it's left off"));
        }
        ok.then(|| String::from_utf8_lossy(&bytes).into_owned())
    });
    let hosts = island_host::proto::manifest::resolve_hosts(&s.hosts, &settings).into_iter().map(|h| h.trim_start_matches("*.").to_string()).collect();
    Ok((url, hosts, skin, s.label.clone()))
}

/// A site's skin and the host names it belongs on (the skin stays off everywhere else, such as a
/// school's or Bungie's sign-in pages the site sends you through).
fn skin_for(site: &str) -> Option<(String, Vec<String>)> {
    module_site(site).ok().and_then(|(_, hosts, skin, _)| skin.map(|css| (css, hosts)))
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
    module_site(site).map(|(url, ..)| url)
}

fn title(site: &str) -> String {
    module_site(site).map(|(.., t)| t).unwrap_or_else(|_| "Island Shell".into())
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
}
