# Island Shell modules

Island Shell is the island, the Command Center, the Task Manager and the calendar. Everything else is a **module** the user installs only if they want it: a folder with a manifest, an optional backend (a small windowless program of its own) and UI the island loads into its page. This is how modules work and how to write one. The exact formats and rules are in [design/modules-handoff.md §4](design/modules-handoff.md#4-definitions-normative); the reasons are in [design/modules.md](design/modules.md).

The nine first-party modules in [`modules/`](../modules) are the best examples:

| Module | Shows how to |
| --- | --- |
| `vpn` | A section in a base tab, Controls tiles, a badge on the bar's network chip; a backend that runs on demand |
| `wallpaper-engine` | A *view* of a base tab (picked in its header), images from folders the backend shares |
| `voicemeeter` | A view of the Audio tab, a bar chip, a backend that runs while another app does and streams live data |
| `destiny`, `canvas` | A tab, a cube with a docked website and its skin, a calendar source, settings-templated hosts, notifications |
| `games` | A tab, a full Command Center sheet of its own, search results, a Win + letter hotkey |
| `discord` | A `call` activity (faces on the pill), a split docked site, topics it follows, an always-there UI |
| `music` | A `media` activity the island draws, island services (app volume, bringing an app forward), a topic it publishes |
| `gaze` | A backend-only feature: a DLL loaded in its own process, topics in and out, a Controls tile |

## What's in a module

```
modules/<id>/
  module.json        the manifest: what it is, what it adds, what it runs, what it talks to
  Cargo.toml         its backend crate (in the repo's workspace)
  src/main.rs        its backend
  ui/<id>.js         its UI's entry: export activate(ctx) and deactivate()
  ui/<id>.css        its UI's styles (its shadow roots only)
  ui/icon.svg        its icon, a 24 × 24 stroke icon like the island's
  skins/…            CSS for websites it docks in the Command Center
```

Everything in the folder except `src/`, `Cargo.toml` and dotfiles goes into the module's package. The id matches `^[a-z][a-z0-9-]{1,31}$`.

### module.json

```jsonc
{
  "id": "vpn", "name": "VPN", "version": "1.0.0", "api": 1,
  "description": "One or two sentences: the setup guide and the Modules cube show it.",
  "icon": "ui/icon.svg",
  "backend": { "exe": "vpn-module.exe", "run": "on-demand", "idle": "90s" },
  "ui": { "entry": "ui/vpn.js", "css": ["ui/vpn.css"], "eager": true },
  "contributes": { "sections": [{ "tab": "network", "id": "vpn", "order": 10 }] },
  "talksTo": [],                     // plain words, shown before it's installed; [] = nothing leaves the PC
  "hosts": [],                       // what its backend's HTTP may reach (`{settings.<key>}` for one the user enters)
  "publishes": [], "subscribes": [], // topics
  "keywords": ["vpn", "tailscale"],  // what a search finds it by before it's installed
  "legacy": []                       // 0.1.x files it takes over
}
```

`contributes` can hold `tabs`, `sections` (in a base tab: `network`, `audio` and `wallpaper`, where `"view": true` makes it an alternative the tab's header switches to), `cubes` (with `slot`, `size` and an optional docked `site`), `sites` (`url`, `hosts`, `skin`, `default`, `split`, and a `reader` with its `readPage`: see [Reading a docked site](#reading-a-docked-site)), `calendar` sources, `hotkeys` (`{ "key": "G", "opens": "tab:games" }`) and `setup` (`{ "card": true, "preselect": false }`). A test in each module (`manifest_is_valid`) runs the island's own validation over its manifest.

## The backend

A windowless exe built on the SDK, [`crates/island-module`](../crates/island-module):

```rust
#![cfg_attr(windows, windows_subsystem = "windows")]
use island_module::{json, CallResult, Host, Init, Module, RpcError, Value};

struct Hello;
impl Module for Hello {
    fn init(&self, host: &Host, init: &Init) -> Result<(), RpcError> {
        host.info("started");                 // island.log, as [mod:<id>]
        Ok(())
    }
    fn call(&self, host: &Host, method: &str, params: Value) -> CallResult {
        match method {
            "hello" => Ok(json!({ "text": "Hi" })),
            _ => Err(RpcError::unknown_method(method)),
        }
    }
}
fn main() { island_module::run(Hello); }
```

- **It talks JSON-RPC over its stdin and stdout.** The island starts it and nothing else can reach it. Its stderr is its log, which the Modules cube shows.
- **Each UI call runs on its own thread.** `event` (a topic changed) runs in order on one thread, so keep it quick. `tick` is the scheduled wake-up, and `shutdown` comes before it exits.
- **What it can send the island, from any thread:**
  - `host.emit(name, data)` sends an event to its own UI.
  - `host.notify(..)` shows a notification.
  - `host.activity(..)` sets something live on the pill.
  - `host.calendar(source, events)` fills its calendar source.
  - `host.publish(topic, data)` updates a topic.
  - `host.share_images(dirs)` lets its UI show pictures from those folders.
- **Where it keeps things:**
  - `host.data_dir()` for settings and DPAPI-encrypted keys (`island_module::secrets`).
  - `host.cache_dir()` for what can be fetched again; its UI loads images from here as `ctx.cacheUrl(name)`.
  - `host.take_legacy(name, to)` moves a 0.1.x file over.
- **Its network is `island_module::http`**, which only reaches the manifest's `hosts`.
- **When it runs** (`backend.run`):
  - `always`.
  - `while-process`, while one of `backend.process` runs. A UI call also starts it, so it can open its app or be set up, and it then stops after `idle`.
  - `scheduled`, every `every` for a `tick`.
  - `on-demand`, started by a call and stopped after `idle`.
- **If it crashes,** the island restarts it after 1 s, then 5 s, then 30 s. After three crashes in five minutes it stays stopped until the user restarts it.

Run it alone to try it: start the exe and type JSON-RPC lines. First `{"jsonrpc":"2.0","id":1,"method":"island.init","params":{…}}` (see `Init`), then your own methods.

## The UI

`ui/<id>.js` is an ES module the island imports from `http://mod.localhost/<id>@<version>/…`. Import other files of yours by relative path.

```js
let ctx;
export function activate(c) {
  ctx = c;
  ctx.tabs.register({ id: 'hello', mount(root) { root.innerHTML = '<div class="card">…</div>'; }, show() {}, hide() {} });
  ctx.on('status', s => { /* the backend's host.emit('status', …) */ });
  if (ctx.backend.running()) ctx.call('hello').then(r => { /* … */ });
}
export function deactivate() {}
```

- **Every mount point is a shadow root.** It has the island's kit stylesheet (`src/kit.css`: cards, buttons, chips, fields, switches, `.empty`, `.note`, the `k-` service and setup cards) and your CSS. The theme's custom properties (`--accent`, `--glow`, `--ink-*`, `--fg`, `--muted`…) come through. Nothing of yours leaks out and nothing of the island's leaks in, except through `:host-context(…)`.
- **What `ctx` gives you** (API 1, listed at the top of `src/modhost.js`):
  - `call` and `on` reach your backend, and `backend.running()` and `backend.on()` follow it without starting it.
  - `store` is localStorage under `mod.<id>.`.
  - `ui` has the island's helpers: `icon`, `esc`, `setHtml`, `txt`, `note`, `ago`…
  - Registering things: `tabs`, `sections`, `cubes` (a stat line and an optional full sheet), `tiles`, `chips`, `search`, `commands`, `activity`, `notify`, `settings.mount` (your page in the Modules cube) and `setup.card`.
  - Getting around: `open('tab:…' | 'cube:…' | 'cube:…:web')` (`:web` opens the cube on its docked site) and `openWeb(url)`, which never opens in Edge.
  - `sites.read(site, args)` runs the site's reader (below).
  - The island's own services through `island`: `layout()`, `showing(tab)`, `closePanel()`, `state()`, `on('panel' | 'tab' | 'skin' | 'activities' | 'level')`, `apps`, `focusApp({ id, exe, hwnd })` (the island is in front when the user asks, so it can bring an app forward), and `audio.appVolume(exe)` / `setAppVolume(exe, level)`.
- **Activities the island draws itself:**
  - A `media` activity (title, artist, art, position, `exe`, `lyric`, `run(action)`) becomes the pill's cover and equalizer, the peek, the Controls card and the bar chip.
  - A `call` activity with `avatars` becomes faces on the pill.
- **Module UI isn't a security boundary.** It runs in the island's page; trust comes from the signed catalog.

## Reading a docked site

A service with no API a module can use (Canvas at a school that turns access tokens off) can still be read the way its own website reads it. A site in the manifest can name a **reader**, a script of the module's:

```jsonc
"sites": [{ "id": "web", "label": "Canvas website", "url": "https://{settings.domain}/", "hosts": ["{settings.domain}"],
            "reader": "readers/canvas-reader.js", "readPage": "/api/v1/users/self" }]
```

- `ctx.sites.read('web', args)` opens a hidden copy of the site at `readPage` (a path; the start page if there's none). It has the docked site's sign-in, since they share one browser profile. Once a page of the site's own `hosts` has loaded, the reader runs in it as `(reader)(args)`, and the call resolves to what it resolves to (anything JSON can hold). Then the copy is closed. If the site sends the copy to a sign-in page elsewhere, the call fails with "Sign in to … first" and the reader never runs there.
- The reader's file is one function expression, `async function name(args) { … }`, with the page's `fetch`, `document` and cookies. Its sha256 is checked against the manifest like a skin's. A light `readPage` loads faster: Canvas's reader uses a small JSON page.
- The copy has no access to the island, like a docked site. The island asks, and takes the answer back by evaluating a script in the page, so treat the answer as the site's data.
- Reads are a few seconds each and at most three run at once. The backend can't start one (it can't reach the site's sign-in), so a backend that wants regular reads emits an event its UI answers. Canvas's tick emits `read`, and its UI (`ui.eager`) reads and hands the result to the backend's `ingest`.

## Topics

Shared state, published by the island or a module. A subscriber gets the last value as soon as it subscribes.

| Topic | From | Data |
| --- | --- | --- |
| `island.geometry` | the island | `{ rect, monitor, scale, visible }`, physical pixels |
| `fullscreen` | the island | `{ active }` |
| `theme` | the island | `{ dark, accent }` |
| `media.state` | Music | what's playing: `{ playing, app, source, title, artist, album, duration, position, sampled_at, key }` |
| `presence` | the eye tracker | `{ state: "looking" \| "present" \| "away" \| "off" }` |

A module may only publish and subscribe to what its manifest says. The island follows `presence` itself: looking opens the peek, and notifications wait until they've been seen.

## Trying it

- **Without Windows:** the dev harness ([`dev/harness`](../dev/harness/README.md)) runs the page with every module's UI. A fixture `"mod:<id>.<method>"` answers a call, and `"mod_states": { "<id>": "running" }` marks a backend running.
- **In the island:** see [BUILDING.md, Modules while you develop](BUILDING.md#modules-while-you-develop). In short: `pack-modules stage`, then **Load a folder…** in the Modules cube.
- **Tests:** `cargo test -p <your-module>` for the backend, `node --test "dev/tests/**/*.js"` for the page's own logic.

## Shipping it

First-party modules ship with Island Shell's releases. `pack-modules pack` zips each module (deterministically, with every file's sha256 in its manifest) and writes the signed catalog the island installs and updates from. Bump a module's `version` whenever it changes; a changed module with an old version is refused. See [BUILDING.md, Releases and updates](BUILDING.md#releases-and-updates).

Modules are part of Island Shell and under its [GPL-3.0](../LICENSE): their UI runs inside the island's page. Third-party modules aren't distributed yet; developer mode is how to build and try one.
