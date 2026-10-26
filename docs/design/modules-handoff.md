# Modules: handoff path

This is the build plan for [modules.md](modules.md), written so that a fresh session (on a cloud machine, without the owner's notes) can pick up any step and carry it out. **modules.md** is the *why*. **This file** is the *what, exactly*: the rules, the definitions, the formats, and the work packages with their definitions of done.

**How to use it**

1. Read [modules.md](modules.md), then this file.
2. Find the first work package marked **todo** in the [progress tracker](#progress-tracker) whose dependencies are **done**.
3. Do that **one** package on a branch named `modules/<ID>-<short-name>`, for example `modules/M0.0-ci`.
4. Open a pull request to `main` titled `<ID>: <title>`.
   - The description lists what changed, how it was verified, and the package's **PC check** list for the owner.
   - Update the tracker row in the same PR.
5. Stop. Don't merge, don't release, and don't start the next package.

---

## 1. Ground rules (from the owner, all binding)

1. **Never open anything in Microsoft Edge.** Links go through `util::open_web`, which copies the link instead when Edge is the default browser.
2. **Every process the app or a module starts is windowless.** That means `CREATE_NO_WINDOW` and `windows_subsystem = "windows"`: no console flashes, ever.
3. **No Discord self-botting.** Discord means only the local RPC pipe with the user's own Discord application, plus the official web app docked in the Command Center.
4. **Never create accounts or handle the user's credentials.** The user types keys into the island's UI, and they're stored DPAPI-encrypted.
5. **Don't change Windows system settings.** Hiding the taskbar is the app's job and is restored on exit; nothing else.
6. **One integration at a time.** One module per PR, and each PR leaves the app working.
7. **No generic CSS class names in anything layered over the island.** Module UI lives in shadow roots (§4.10). Base overlays prefix every class (the setup guide uses `su-`), because generic names like `.ring`, `.pill` and `.win` already collided once.
8. **Keep the "Taken" look:** cold light leaking out of black. The sigil and art are original; never use Destiny 2 or Bungie assets.
9. **Memory budget:** the base stays at about 130 MB or less at rest, and the total with every module running stays under 200 MB.
10. **Outward-facing actions need the owner.** Pushing to `main`, merging, creating releases, posting anywhere and adding secrets are the owner's job. Work on branches and open PRs; release workflows create **draft** releases only.
11. **No personal data in the repo:** no names, user paths, school names, keys, tokens, process dumps or screenshots with personal content. Test fixtures are synthetic.
12. **Write code like the surrounding code.**
    - The front end is plain ES modules: no bundler, no framework, no npm runtime dependencies.
    - The backend is Rust 2021 with the `windows` crate.
    - Match the existing comment density and naming.

## 2. Environment: what can be checked where

Island Shell only runs on Windows 10/11. A cloud session is usually Linux.

| Check | Where | How |
| --- | --- | --- |
| Platform-neutral Rust (protocol, SDK, manifest, catalog, pack tool) | Cloud | `cargo test -p <crate>`. Keep these crates free of `cfg(windows)` except their transport modules. |
| The whole workspace compiles and its tests pass | GitHub Actions, `windows-latest` | `.github/workflows/ci.yml` (M0.0). Push the branch and read the run. Windows runners are free for public repos. |
| Front-end logic (registries, cube layout, weather mapping) | Cloud | `node --test "dev/tests/**/*.js"` on DOM-free modules (Node 22 doesn't search a directory given to `--test`; CI runs the same glob). Keep logic out of DOM code so it can be imported in Node. |
| How the UI looks | Cloud | The dev harness (M0.2) serves `src/` with a mocked Tauri bridge. Screenshot it with headless Chromium (Playwright, if it's available). |
| Running the real app: Tobii, Apple Music, Discord, Steam, the installer, Defender | **The owner's PC only** | Each package ends with a **PC check** list the owner runs and ticks off in the PR. |

## 3. Glossary

| Term | Definition |
| --- | --- |
| **Base** | `island-shell.exe` and the `src/` UI. It always ships: island and bar, Command Center and search, the Task Manager and stats, calendar/Today (iCal, weather, to-dos), apps, clipboard, audio devices, Wi-Fi and Bluetooth, procedural wallpapers, power, the Windows key, the setup guide, the docking engine and the module host. |
| **Module** | An optional, separately installed feature folder: a manifest, an optional backend, a UI bundle and assets. |
| **Module id** | Matches `^[a-z][a-z0-9-]{1,31}$`, unique, and never reused: `music`, `discord`, `canvas`, `destiny`, `gaze`, `games`, `voicemeeter`, `wallpaper-engine`, `vpn`. |
| **Manifest** | `module.json` in the module's root (§4.2). It's the only thing the base reads before the module runs. |
| **Backend** | The module's exe. A separate, windowless process started by the base. It's optional, because a module can be UI-only. |
| **Supervisor** | The base code (`src-tauri/src/modules/`) that starts, stops, restarts and talks to backends. |
| **Run mode** | When a backend runs: `always`, `while-process`, `scheduled` or `on-demand` (§4.5). |
| **Contribution** | Something a module adds to the base: a tab, cube, site, calendar source, hotkey, search provider, command, setup card or settings page. |
| **Registry** | The base UI's list for each kind of contribution. The base's own sections register through the same registries (M0.3). |
| **Built-in module** | During Phase 0 and Phase 2: an integration whose UI already lives in `src/modules/<id>/` and registers through the registries, but is still compiled into the base. |
| **Topic** | A named stream of shared state with a JSON payload (§4.7). It's published by the base or a module, and modules subscribe to it. |
| **Activity** | A live item on the island's pill: now playing, in a call, a timer (§4.8). The highest priority wins. |
| **Rest state** | The pill when there's no activity: clock, date, temperature and a weather icon (§4.11). |
| **Site** | A real website docked into a Command Center sheet by `embed.rs` (Discord, DIM, Canvas). |
| **Skin** | CSS injected into a site so it wears the Taken look (`*-skin.css`). |
| **Calendar source** | A named feed of events the calendar shows, with its own color and on/off switch. Modules push events to it (§4.9). |
| **Catalog** | `modules.json` attached to every GitHub release, with its signature `modules.json.minisig` (§4.4). |
| **Module API version** | The integer `api`. The base supports N and N−1. Today N = 1. |
| **Installed set** | `installed.json` (§4.3): which modules are installed, at which version, and whether each is enabled. |
| **Developer mode** | Loading an unpacked module folder without a signature, shown with a warning badge. It's for building modules. |
| **Legacy data** | 0.1.x files in the app data root that a module takes over on its first run (§6, M3.5). |

## 4. Definitions (normative)

### 4.1 Paths

| What | Path |
| --- | --- |
| Module code | `%LOCALAPPDATA%\league.seaweed.islandshell\modules\<id>\` |
| Installed set | `%LOCALAPPDATA%\league.seaweed.islandshell\modules\installed.json` |
| Staging and trash | `%LOCALAPPDATA%\league.seaweed.islandshell\modules\.staging\`, `\.trash\` |
| Module data (settings, DPAPI secrets) | `%APPDATA%\league.seaweed.islandshell\modules\<id>\` |
| Module cache (images, big JSON) | `%LOCALAPPDATA%\league.seaweed.islandshell\modules-cache\<id>\` |
| Legacy (0.1.x) data | `%APPDATA%\league.seaweed.islandshell\` (`canvas.json`, `destiny.json`, `discord.json`, `settings.json`, `data\voicedeck_path.txt`, …) |

DPAPI in 0.1.x uses `CryptProtectData` with no entropy and no flags (`util.rs`). The SDK's `secrets` must do exactly the same, so modules can read the legacy files.

### 4.2 `module.json`

| Field | Type | Req. | Meaning |
| --- | --- | :-: | --- |
| `id` | string | ✓ | Module id (§3). |
| `name` | string | ✓ | Display name. |
| `version` | semver string | ✓ | The module's version. |
| `api` | integer | ✓ | The module API version it was built for. |
| `description` | string | ✓ | One sentence, shown in the setup guide and the Modules sheet. |
| `icon` | path | ✓ | An SVG inside the module, 24×24, stroke style like the island's icons. |
| `backend` | object | | `{ "exe": "<file>.exe", "run": "always" \| "while-process" \| "scheduled" \| "on-demand", "process": ["Discord.exe"], "every": "15m", "idle": "60s" }`. `process` is for `while-process`, `every` for `scheduled`, and `idle` for `on-demand` (default `60s`). |
| `ui` | object | | `{ "entry": "ui/<id>.js", "css": ["ui/<id>.css"] }`. |
| `contributes.tabs` | array | | `{ id, title, icon, order }` — an island tab. `order` is a number; base tabs use multiples of 10. |
| `contributes.cubes` | array | | `{ id, title, icon, size: "S"\|"M"\|"L"\|"W", slot?, site? }` — a Command Center cube (§4.12). |
| `contributes.sites` | array | | `{ id, label, url, hosts: [..], skin?, default? }`. `url` may hold `{settings.<key>}`; `hosts` are where the skin applies. |
| `contributes.calendar` | array | | `{ source, name, color }`. |
| `contributes.hotkeys` | array | | `{ key: "G", opens: "tab:games" }` — the Win + letter. Letters the base uses are taken (§4.13). |
| `contributes.setup` | object | | `{ "card": true, "preselect": false }` — a card in the setup guide's module step. |
| `publishes` / `subscribes` | string[] | | Topics (§4.7). The supervisor enforces both lists. |
| `talksTo` | string[] | ✓ | Plain-language list of everything it contacts. An empty list means nothing. |
| `legacy` | string[] | | Legacy file names, relative to the legacy root, that it takes over. |
| `files` | object | ✓ in releases | `{ "<path>": "sha256-<base64>" }` for every file except `module.json`. `pack-modules` writes it. |

### 4.3 `installed.json`

```json
{ "schema": 1,
  "modules": { "destiny": { "version": "1.0.0", "enabled": true, "auto_update": true, "dev": false,
                             "installed": "2026-11-02T18:00:00Z", "source": "catalog" } } }
```

### 4.4 The catalog: `modules.json` and `modules.json.minisig`

```json
{ "schema": 1, "published": "2026-11-02T18:00:00Z", "base_min": "0.2.0", "api": [1],
  "modules": [ { "id": "destiny", "name": "Destiny 2", "version": "1.0.0", "api": 1,
                 "description": "…", "talksTo": ["bungie.net"], "size": 812345,
                 "url": "https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0/destiny-1.0.0.zip",
                 "sha256": "<hex>" } ] }
```

- **Signature:** minisign (ed25519) over the exact bytes of `modules.json`. Keys are made with `cargo tauri signer generate`.
  - The base embeds the public key and verifies with the `minisign-verify` crate.
  - The private key never enters the repo. It lives on the owner's PC and in the GitHub secrets `ISLAND_SIGNING_KEY` and `ISLAND_SIGNING_KEY_PASSWORD`.
- **Where:** every release carries `IslandShellSetup.exe`, `modules.json`, `modules.json.minisig` and every module zip, unchanged modules included at their old version. The app reads `https://github.com/AbyssLotus/island-shell/releases/latest/download/modules.json`. Releases must not be marked as prereleases, or `latest` stops pointing at them.
- **Zip rules:**
  - The entries are exactly `module.json` plus the manifest's `files`.
  - No absolute paths, no `..`, no symlinks, nothing else.
  - The zip's sha256 matches the catalog, and every file matches its manifest hash.

### 4.5 Run modes

| Mode | Starts | Stops |
| --- | --- | --- |
| `always` | When the island starts, if enabled | When the island exits |
| `while-process` | When one of `backend.process` is running (from the Task Manager snapshot, checked every 2 s), and on a UI call (to open the app, or set it up, while it isn't running) | 5 s after the last one exits; started by a call, after `idle` without calls |
| `scheduled` | Every `every` for an `island.tick`, and also on a UI call | After the tick or call is answered and 10 s have passed idle |
| `on-demand` | On the first `mod_call` | After `idle` with no calls |

**Every backend:**

- Started with `CREATE_NO_WINDOW` and put in one Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. This needs the `Win32_System_JobObjects` feature.
- On a crash, restarted after 1 s, then 5 s, then 30 s. After 3 crashes in 5 minutes it's marked `stopped` until the user presses Restart.

### 4.6 Wire protocol

- **Transport:** the base creates `\\.\pipe\island-shell-<session-id>-<module-id>`.
  - The pipe's DACL allows only the current user, and the pipe sets `PIPE_REJECT_REMOTE_CLIENTS`.
  - After connecting, the base checks `GetNamedPipeClientProcessId` against the PID it spawned.
  - The backend's arguments are `--pipe <name> --data <dir> --cache <dir> --legacy <dir> --api 1`.
  - `--stdio` uses stdin and stdout instead, for tests and debugging on any OS.
- **Framing:** UTF-8 JSON, one object per line ending in `\n`, at most 4 MiB per message. Anything bigger goes in the cache folder (§4.10).
- **Messages:** JSON-RPC 2.0. A request has an `id`; a notification doesn't.
- **Timeouts:** `island.init` 5 s (a miss counts as a crash), UI calls 15 s, `island.tick` 60 s, and 2 s after `island.shutdown` before the job kills the process.

| Direction | Method | Kind | Params → result |
| --- | --- | --- | --- |
| base → module | `island.init` | request | `{ api, id, base_version, data_dir, cache_dir, legacy_dir, locale, theme: { dark, accent }, settings }` → `{}` |
| base → module | `island.tick` | request | `{}` → `{}` |
| base → module | `island.event` | notification | `{ topic, data }` (only subscribed topics) |
| base → module | `island.shutdown` | notification | `{}` |
| base → module | *anything else* | request | A UI call from `mod_call(id, method, params)`, forwarded as is. Names must not start with `island.` or `host.`. |
| module → base | `host.emit` | notification | `{ name, data }` → the Tauri event `mod:<id>:<name>` to the UI |
| module → base | `host.notify` | notification | `{ title, body, icon?, urgent?, action?: "tab:<id>" \| "cube:<id>" }` |
| module → base | `host.activity` | notification | an Activity (§4.8), or `{ id, clear: true }` |
| module → base | `host.calendar` | notification | `{ source, events: [CalendarEvent] }`, replacing all of that source's events |
| module → base | `host.publish` | notification | `{ topic, data }` (only declared topics) |
| module → base | `host.log` | notification | `{ level: "error" \| "warn" \| "info", msg }`, written to `island.log` as `[mod:<id>] …` |

**Error codes** (JSON-RPC `error.code`):

| Code | Meaning |
| --- | --- |
| `-32601` | Unknown method |
| `-32602` | Bad params |
| `-32000` | Failed; `message` is shown to the user |
| `-32001` | Not set up (the UI shows its setup state) |
| `-32002` | Offline or the service is unreachable |
| `-32003` | The module is stopped or crashed (sent by the supervisor) |

### 4.7 Topics

| Topic | Published by | Payload |
| --- | --- | --- |
| `island.geometry` | base | `{ rect: {x,y,w,h}, monitor: {x,y,w,h}, visible }` — physical pixels, virtual-screen coordinates |
| `fullscreen` | base | `{ active }` — a fullscreen app has hidden the island |
| `session` | base | `{ locked }` |
| `power` | base | `{ state: "suspend" \| "resume" }` |
| `theme` | base | `{ dark, accent }` |
| `media.state` | music | `{ playing, app, source, title, artist, album, duration, position }` |
| `presence` | gaze | `{ state: "looking" \| "present" \| "away" \| "off" }`: "away" once nobody has been in front of the tracker for 10 s, "off" without a tracker (or with it turned off). The island follows it too (looking opens the peek; while someone's there, notifications wait to be seen) |

Late subscribers get the last value immediately. Topics are only state, never commands. For example, Music reacts to `presence: away` by pausing itself, if its "pause when I step away" setting is on. That setting moves from the Tobii settings into Music's.

### 4.8 Activities

`{ id, kind: "media" | "call" | "timer" | "custom", priority, icon?, art?, text, sub?, progress?: 0..1, action?: "tab:<id>", avatars?: [{ id, src, talk }] }`

- Priority defaults by kind: call 30, timer 20, media 10, custom 5.
- A `media` activity is what's playing: `{ text: title, sub: artist, app, art, playing, position, sampled_at, duration, can_next, can_prev, key, exe, lyric, action, run(action) }`. The island draws its pill cover and equalizer, peek, Controls card and top-bar chip from the highest one; their buttons call `run('toggle' | 'next' | 'prev')`, the equalizer meters `exe` (image names, `|`-separated), and `lyric` is the line being sung for the peek. Without one, the pill is at rest (§4.11) and Controls has no now-playing card.
- A `call` activity with `avatars` draws on the pill beside the clock: up to three faces, a green ring on each `talk`, and `icon` (your own mute or deafen). Only the rings change while the faces stay the same, so talking doesn't redraw anything. Bar chips take the same `avatars`.
- The pill shows the highest priority, and on a tie the most recent. The base's own Pomodoro timer becomes a base activity.

### 4.9 Calendar events

`{ id, title, start, end?, all_day, url?, color?, kind?: "due" | "event" | "reset" }`

- Times are RFC 3339.
- The base caches the last set per source in `calendar.json`, so events still show while the module isn't running.
- On/off and color per source stay in `calendar.json`, keyed by `source`.

### 4.10 Module UI

- **Served from** `http://mod.localhost/<id>/<path>`, a Tauri `register_asynchronous_uri_scheme_protocol("mod", …)` handler.
  - Only installed and enabled modules are served.
  - Only paths in `files` are served, after their sha256 matches (checked once per session).
  - Responses carry the right MIME type and `Access-Control-Allow-Origin: http://tauri.localhost`.
  - `http://mod.localhost/<id>/~cache/<path>` serves `image/*` files from the module's cache dir, with no hash check.
- **CSP** (`tauri.conf.json`):
  - Add `http://mod.localhost` to `script-src`, `style-src` and `img-src`.
  - `connect-src` doesn't change: modules reach the network only through their backend.
  - The first-party image hosts already in `img-src` stay.
- **Entry point:** `export async function activate(ctx)` and `export function deactivate()`. The base imports it lazily, the first time one of the module's tabs or cubes opens. Tabs and cubes appear from the manifest straight away.
- **Mounting:**
  - Every mount point is a shadow root. The base adopts its UI-kit stylesheet (M0.4) into it.
  - Theme custom properties (`--glow`, the accent and the rest) inherit through it.
  - Module CSS is the module's own business, but it never targets anything outside its root.
- **`ctx` (API 1):**

```ts
interface Ctx {
  id: string;
  call(method: string, params?: object): Promise<any>;   // rejects with { code, message }
  on(name: string, fn: (data: any) => void): () => void; // host.emit events; returns unsubscribe
  backend: { running(): boolean; state(): string; on(fn: (state: string) => void): () => void }; // without starting it, unlike call()
  store: { get<T>(key: string, dflt: T): T; set(key: string, v: unknown): void }; // localStorage "mod.<id>.<key>"
  ui: UiKit;                                             // icon(name), toggle(), row(), sparkline(), fmt.*
  tabs:     { register(def: { id: string; mount(root: ShadowRoot): void; show?(): void; hide?(): void }): void };
  cubes:    { register(def: { id: string; sheet: { mount(root: ShadowRoot): void; close?(): void } }): void };
  search:   { register(p: { kind: string; query(q: string): Promise<Result[]> }): void };
  commands: { register(c: { id: string; title: string; run(): void }): void };
  activity: { set(a: Activity): void; clear(id: string): void };
  notify(n: Notification): void;
  settings: { mount(fn: (root: ShadowRoot) => void): void };  // its page in the Modules sheet
  setup:    { card(fn: (el: HTMLElement) => void): void };    // its card in the setup guide
  open(target: `tab:${string}` | `cube:${string}`): void;
}
```

### 4.11 The rest state of the pill

This shows whenever there's no activity.

- **Island layout:** `[weather icon] 72°   Fri 24   9:41`.
  - The temperature is a whole number with `°` and no letter, in the Today card's unit (`S.unit`, °F or °C).
  - The date is the short weekday plus the day of the month, in the user's locale.
  - The time uses the existing `#pillTime` format.
  - The pill may widen at rest to fit. Measure it in the harness, and keep the change in `island.rs` geometry if the window size has to change.
- **Bar layout:** the icon and temperature sit just left of the clock.
- **Weather data:** the cache Today already keeps (`store 'weather'` from Open-Meteo). With no city set, or a cache older than 3 h, leave out the icon and temperature; the clock and date stay.
- **Four icons only** (owner's choice), using existing glyphs from the icon map in `app.js`:

| Icon | WMO weather codes |
| --- | --- |
| `sun` (sunny) | 0, 1 |
| `cloud` (cloudy) | 2, 3, 45, 48 |
| `rain` (rainy) | 51–57, 61–67, 80–82, 95–99 |
| `snow` | 71–77, 85, 86 |

### 4.12 Cube layout

`cc.js` has 11 fixed cube positions. They become **12 named slots**; the 12th is new, for the Modules cube.

| Slot | x, y | Size | Owner |
| --- | --- | :-: | --- |
| `apps` | −1.8, −1.12 | L | base |
| `audio` | −2, 1.1 | L | base |
| `today` | −3.4, .82 | S | base |
| `network` | 2.48, .04 | W | base |
| `clipboard` | 1.92, 1.14 | M | base |
| `stats` | 3.42, 1.02 | S | base |
| `discord` | −2.52, −.02 | M | preferred by discord |
| `canvas` | −3.3, −.84 | S | preferred by canvas |
| `games` | 1.72, −1.1 | M | preferred by games |
| `destiny` | 3.3, −.84 | S | preferred by destiny |
| `music` | 0, 1.62 | W | preferred by music |
| `modules` | 0, −1.62 | S | base (the Modules cube); confirm placement in the harness |

- A module cube takes its `slot` if that slot is free. Otherwise it takes the first free one in the order `discord, canvas, games, destiny, music`.
- Empty slots draw nothing.
- **PC check:** look at the layout with no modules and with every module installed.

### 4.13 Hotkeys

- The base owns the keyboard hook (`winkey.rs`).
- `SHORTCUTS` becomes a runtime table: the base's entries plus the enabled modules' `hotkeys`. The table is rebuilt when modules change.
- The base keeps `A` (audio), `V` (clipboard), `N` and `W` (today) and `Space` (Command Center). `G` moves to the games module.
- If two modules claim the same letter, the one installed first keeps it.

## 5. Module map (today's code → module)

The command lists come from `main.rs`. Before moving a module, confirm the full list with `grep "<prefix>_" src-tauri/src/main.rs`.

| Module | Backend files | Tauri commands → RPC methods | UI in `src/` today | Base state it uses today | Legacy data | Run mode | Topics | Contributes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `destiny` (**includes DIM**) | `destiny.rs` | `destiny_configure, _forget, _profile, _status, _xur` | Destiny tab (app.js, search `destiny`), `SITES.destiny` → `dim` | `calendar.rs` calls `destiny::status` | `destiny.json` | on-demand | — | tab, cube `destiny`, site `dim` with `dim-skin.css` (works without a Bungie key), calendar source `destiny` (weekly reset) |
| `canvas` | `canvas.rs` | `canvas_configure, _configure_feed, _done, _forget, _open, _overview, _status` | `renderCv`, `SITES.canvas` | `calendar.rs` calls `canvas::fetch/status/overview/parse_iso`; `canvas::spawn` in `main.rs` | `canvas.json` | scheduled, 15m | — | tab, cube, site with `canvas-skin.css`, calendar source `canvas`, notifications |
| `wallpaper-engine` | `wpe.rs` | `we_apply, we_control, we_library` | `renderWeGrid`, the Wallpaper tab's library | — | check `settings.json` | on-demand | — | a section in the Wallpaper tab (§7 Q1) |
| `vpn` | `vpn.rs` | `vpn_status, vpn_tailscale, vpn_radmin_open` | `renderVpn`, `renderVpnTiles` | — | — | on-demand | — | tiles in Network and Controls (§7 Q1) |
| `games` | `games.rs` | `games_list, game_action` | `renderGames`, `cc-games.js` | `winkey.rs` `G` | game caches | on-demand | — | tab, cube, hotkey `G`, search kind `games` |
| `voicemeeter` | `voicemeeter.rs`, `crates/vd-vm-core` | `vm_levels, vm_mic, vm_restart, vm_set, vm_state, mixer_apps, voicedeck_open, voicedeck_installed` | `renderVm`, `renderVmChip` | `voicemeeter::spawn` | `data\voicedeck_path.txt` | while-process `voicemeeter*.exe` | — | a section in the Audio tab (§7 Q1) |
| `discord` | `discord.rs` | `discord_action, _open, _options, _status` | `renderDc`, `renderDcCall`, `SITES.discord` | reads `shared.status.media`; `discord::spawn` | `discord.json` | while-process `Discord.exe` | subscribes `media.state` | tab, cube, site with `discord-skin.css` (and `tools/discord-skin/`), call activity, notifications |
| `music` | `media.rs`, `applemusic.rs`, `lyrics.rs`, `music.rs` | `media_cmd, media_thumb, media_lyrics, music_info` (and the other `media_*`) | `renderMedia`, `renderMusic`, `renderLyrics`, `cc-music.js`, the Controls now-playing card | `shared.media_source`, `media_exe`, `media_playing`, `status.media`; `applemusic::spawn` | music prefs in localStorage | always | publishes `media.state`, subscribes `presence` | tab, cube, media activity, setup card **pre-selected** |
| `gaze` (Tobii) | `gaze.rs` | `gaze_set, gaze_status` | `renderGaze` | `shared.geom`, `place`, `island_hwnd`, `fullscreen_hidden`, `media_playing`, `media_source`; `settings.gaze`, `settings.gaze_pause` | gaze fields in `settings.json` | always (while on) | subscribes `island.geometry`, `fullscreen`; publishes `presence` | its settings page; the Tobii DLL loads in the module process |

The docking engine (`embed.rs`, `cc_embed`, `cc_embed_hide`) stays in the base. Skins stop being `include_str!` constants: `skin_for` reads the skin from the owning module's folder (§4.2 `sites.skin`).

## 6. Work packages

Each package lists **Goal**, **Changes**, **Done when** (the definition of done), **Verify** (what the session checks) and **PC check** (what the owner checks on Windows). Every package also requires that CI is green and that the app behaves the same except for what the package changes.

### Phase 0: groundwork. Still one exe; users see nothing new except the rest state.

**M0.0 CI on Windows**

- **Goal:** sessions without Windows get real compile and test feedback.
- **Changes:** `.github/workflows/ci.yml`, run on push and PR, on `windows-latest`:
  - The Rust toolchain plus a cache.
  - `cargo check` and `cargo test` for the app.
  - `node --test dev/tests` once that folder exists.
  - Tests that need hardware or the owner's machine get `#[ignore = "needs …"]` with the reason.
- **Done when:** CI passes on the PR.
- **Verify:** the Actions run.
- **PC check:** none.

**M0.1 Cargo workspace** (depends on M0.0)

- **Goal:** one lockfile and one target dir for the base, crates, modules and tools.
- **Changes:**
  - A root `Cargo.toml` `[workspace]` with members `src-tauri`, `crates/*` (later also `modules/*` and `tools/*`).
  - Move `Cargo.lock` to the root.
  - Release profile settings move to the root.
  - Update `docs/BUILDING.md`.
- **Done when:** `cargo tauri build` from `src-tauri` still makes the same installer, and CI is green.
- **PC check:** `cargo tauri build` works, and the installer installs and runs.

**M0.2 Dev harness** (depends on M0.0)

- **Goal:** the UI can be seen without Windows.
- **Changes:** `dev/harness/`:
  - `index.html` loads the island page and Command Center from `../../src`.
  - `mock-tauri.js` defines `window.__TAURI__` (`core.invoke` and `event.listen`), answering from `fixtures/*.json`.
  - A tiny static server script, plus the README.
  - Fixtures are synthetic: made-up processes, songs, events.
- **Done when:** the island's tabs and the Command Center's cubes render in headless Chromium, with no console errors.
- **Verify:** screenshots attached to the PR.
- **PC check:** none.

**M0.3 Front-end registries** (depends on M0.2)

- **Goal:** tabs, cubes, sites, search, commands, hotkeys, setup cards and activities come from registries.
- **Changes:**
  - New `src/registry.js`, DOM-free with tests in `dev/tests`.
  - `index.html` stops hardcoding the 14 `data-tab`s; `app.js` builds the tab strip and panels from the registry.
  - `cc.js` `CUBES` becomes the slot table in §4.12, and `SITES` and `NATIVE` become registries.
  - The base's own sections register first.
- **Done when:** harness screenshots match the pre-change ones, and every tab, cube and search kind still works.
- **PC check:** click through every tab and cube; Win + A, G, V, N and Space still work.

**M0.4 UI-kit stylesheet** (depends on M0.3)

- **Goal:** one stylesheet holding the island's shared look (tokens, buttons, toggles, rows, cards, icons) that shadow roots can adopt.
- **Changes:**
  - Split `style.css` into `kit.css` (shared) and the base layout.
  - Load `kit.css` as a constructable `CSSStyleSheet`, adopted by the document **and** by every module shadow root.
  - Write it up as the kit's README.
- **Done when:** nothing looks different, and a test shadow root in the harness renders kit components correctly.
- **PC check:** a visual pass in dark and light.

**M0.5a–i Built-in modules, one per PR** (depends on M0.4), in this order:

| Step | Module |
| --- | --- |
| a | destiny |
| b | canvas |
| c | wallpaper-engine |
| d | vpn |
| e | games |
| f | voicemeeter |
| g | discord |
| h | music |
| i | gaze |

- **Goal:** each integration's UI moves into `src/modules/<id>/` (`index.js` exporting `activate` and `deactivate`, plus its CSS) and mounts in a shadow root through `ctx` (§4.10). `ctx.call` still maps to the old `invoke` command for now.
- **Changes:** `src/modules/builtin.js` lists the built-ins, and the integration's code leaves `app.js`, `cc-*.js` and `style.css`.
- **Done when:**
  - The integration works as before.
  - No code outside `src/modules/<id>/` mentions it, other than `builtin.js` and the CSP.
  - Its harness screenshots match.
- **PC check:** that integration's features, end to end.

**M0.6 Backend event bus** (can run in parallel with M0.3–M0.5)

- **Goal:** cut the Rust couplings using the topic names in §4.7.
- **Changes:**
  - New `src-tauri/src/bus.rs`: publish, subscribe, and last value per topic.
  - `gaze.rs` stops reading `shared.media_*` (Music pauses on `presence: away`).
  - `discord.rs` subscribes to `media.state`.
  - `calendar.rs` gets a source registry, and Canvas and Destiny push into it.
  - `winkey.rs` gets the runtime shortcut table (§4.13).
- **Done when:**
  - `grep -n "crate::canvas\|crate::destiny" src-tauri/src/calendar.rs` and `grep -n "media_playing\|media_source" src-tauri/src/gaze.rs src-tauri/src/discord.rs` find nothing.
  - Bus unit tests pass.
- **PC check:**
  - With Tobii on, looking away still pauses music.
  - Canvas due dates and the Destiny reset are still in the calendar.

**M0.7 Rest state** (depends on M0.3)

- **Goal:** the pill at rest exactly as in §4.11.
- **Changes:** the pill markup, its CSS, and a DOM-free WMO → icon mapping with tests.
- **Done when:** harness screenshots show sunny, cloudy, rainy, snow, no city, playing (unchanged) and timer (unchanged), for both the island and the bar.
- **PC check:** it looks right, and the pill's rest width doesn't clip.

### Phase 1: the module host and the Destiny 2 pilot

**M1.1 Protocol and SDK crates** (depends on M0.1)

- **Goal:** the wire format and the module-side runtime, platform-neutral.
- **Changes:**
  - `crates/island-module-proto`: serde types for §4.2–§4.9, a framed reader and writer (4 MiB cap), and error codes.
  - `crates/island-module`, for module authors:
    - A `Module` trait (`init`, `call`, `tick`, `event`) and the run loop.
    - Transports: `--stdio` everywhere, and the pipe on `cfg(windows)`.
    - Helpers for `emit`, `notify`, `activity`, `calendar`, `publish` and `log`.
    - `secrets` (DPAPI exactly as `util.rs`, under `cfg(windows)`), plus an HTTP helper that only allows the hosts the module declares.
- **Done when:** round-trip tests for every message, plus tests for oversize and broken frames, pass on Linux and on Windows CI.
- **PC check:** none.

**M1.2 Supervisor** (depends on M1.1)

- **Goal:** the base runs backends as in §4.5 and §4.6.
- **Changes:**
  - `src-tauri/src/modules/`: `mod.rs`, `manifest.rs` (validation), `installed.rs`, `supervisor.rs` (spawn, job object, run modes, backoff), `pipe.rs` (DACL, PID check) and `router.rs` (calls, events, topic permissions).
  - Commands `mod_call`, `mod_list`, `mod_set_enabled`, `mod_restart` and `mod_logs`.
  - A test-only `modules/echo` backend.
- **Done when:** Windows CI integration tests pass, using `echo`:
  - A call round trip.
  - A crash, then a restart with backoff, then `stopped`.
  - Killing the base leaves no `echo` process behind.
  - Idle exit.
  - An undeclared topic is rejected.
- **PC check:** none yet.

**M1.3 Module UI loading** (depends on M1.2, M0.4)

- **Goal:** §4.10 end to end.
- **Changes:**
  - The `mod` protocol handler, written as a pure function (request path → response) with unit tests for traversal, unlisted files, a bad hash and `~cache`.
  - The CSP change.
  - A loader that turns manifests into registry entries and imports the entry point lazily.
- **Done when:** unit tests pass and a fixture module loads in the harness.
- **PC check:** none yet.

**M1.4 Modules sheet** (depends on M1.3)

- **Goal:** a Modules cube (slot `modules`) and a native sheet.
- **Changes:**
  - The sheet lists installed modules with enable, restart, logs, running state, and RAM and CPU per backend process (from `taskmgr`).
  - Each module's settings page.
  - Developer mode: "Load a module folder…", with an unsigned badge.
  - The search commands "Modules" and "Load module folder".
- **Done when:** harness screenshots and CI are green.
- **PC check:** the sheet opens and looks right.

**M1.5 Destiny 2 module, the pilot** (depends on M1.4, M0.5a, M0.6)

- **Goal:** the first real module, including DIM.
- **Changes:**
  - `modules/destiny/`: `Cargo.toml`, `src/main.rs` (the `destiny.rs` logic, its commands now RPC methods), `ui/` (from `src/modules/destiny/`), `skins/dim-skin.css` and `module.json`.
  - `embed.rs` reads skins from module folders.
  - Destiny is removed from the base: its commands, `destiny.rs`, its built-in entry, and the calendar's direct call.
  - Migration of `destiny.json` via `legacy`.
- **Done when:**
  - CI builds the module.
  - The base has no Destiny code left.
  - Harness screenshots of the tab are attached.
- **PC check:**
  1. Load the folder in developer mode; the Destiny tab and cube appear without a restart.
  2. The existing key works.
  3. DIM docks with its skin, without a key.
  4. The reset is in the calendar.
  5. Disable, then enable again.
  6. Remove (keeping data), then load again: the key is still there.
  7. Killing the module process from the Task Manager brings it back.

### Phase 2: the rest, one module per PR

**M2.1–M2.8:** canvas, wallpaper-engine, vpn, games, voicemeeter, discord, music, gaze. Each depends on the previous one and on its own M0.5 step. Each follows the M1.5 template, using its row in §5:

- Create `modules/<id>/` with the backend, UI, manifest and legacy list.
- Delete its code, commands, `spawn` call and `Shared` fields from the base.
- Write a PC check list covering the module's features, plus install, disable and enable, remove and reinstall, and kill-and-restart.

Extra conditions:

| Package | Extra done-when |
| --- | --- |
| M2.4 games | Win + G opens Games only while the module is installed. |
| M2.5 voicemeeter | `crates/vd-vm-core` leaves the base's dependencies. |
| M2.6 discord | It runs only while Discord does. Call activity and notifications work. |
| M2.7 music | Without the module the rest state shows and Controls has no now-playing card. With it, `media.state` reaches Discord. |
| M2.8 gaze | Looking at the island and stepping away work through `island.geometry`, `fullscreen` and `presence`. Without Music, stepping away does nothing harmful. |

### Phase 3: distribution

**M3.1 `tools/pack-modules`** (depends on M1.5)

- **Goal:** build module zips, write the catalog, sign it.
- **Changes:** a workspace binary that, for every module:
  - Builds it in release mode.
  - Fills in `files` hashes.
  - Writes a deterministic zip.
  - Writes `modules.json` from the previous catalog plus the changed modules.
  - Signs it with `ISLAND_SIGNING_KEY`.
  - It also has `verify <dir>`.
- **Done when:** byte-identical zips across two runs, and `verify` passes. Its tests use a test key named `test-only` that's committed under `tools/pack-modules/testdata/`.
- **PC check:** none.

**M3.2 Install, update, remove** (depends on M3.1)

- **Goal:** §4.4, plus install, update, remove and rollback from modules.md, inside the base.
- **Changes:**
  - Fetch the catalog at most daily and when the Modules sheet opens.
  - Verify the signature with the embedded public key.
  - Staging, verify, swap, `.trash` rollback, and `installed.json`.
  - Per-module auto-update.
  - API compatibility: N and N−1, otherwise "Update needed".
  - Remove, with "Also forget its keys and data?".
  - The sheet's Available and Updates lists.
- **Done when:** tests against a local catalog signed with the test key cover a good install, a bad signature, a bad hash, a zip with an extra file, an interrupted swap that rolls back, and an API mismatch.
- **PC check:** install, update and remove a module from a test catalog the owner hosts locally.

**M3.3 Release workflow** (depends on M3.1)

- **Goal:** releases built on GitHub.
- **Changes:** `.github/workflows/release.yml`, run manually, on `windows-latest`:
  - Builds the installer and every module, then runs `pack-modules` with the secrets.
  - Creates a **draft** release with `IslandShellSetup.exe`, the zips, `modules.json` and `modules.json.minisig`.
- **Done when:** a dry run without secrets builds everything and skips signing, clearly marked.
- **PC check (owner):**
  1. Generate the key.
  2. Add the secrets.
  3. Run the workflow.
  4. Review the draft and publish it.

**M3.4 Picking modules** (depends on M3.2)

- **Goal:** users meet modules in the right places.
- **Changes:**
  - The setup guide's `connect` step becomes "Pick your modules": a card per catalog module with its description, `talksTo`, size and an install toggle. **Music is pre-selected.** Installs run in the background while the guide continues, and the "Set up" buttons remain.
  - Search: "Install <name>", plus missing-feature hints (searching "lyrics" with Music not installed offers to install it).
  - Module processes are grouped under Island Shell in the Task Manager.
- **Done when:** harness screenshots of the new step and the hints are attached.
- **PC check:** a fresh install (after backing up app data) from the guide.

**M3.5 Migration from 0.1.x** (depends on M3.2, M2.8)

- **Goal:** nobody loses anything when upgrading.
- **Changes:**
  - On 0.2's first start, queue installs for the modules in use, by this table:

    | Found | Install |
    | --- | --- |
    | `canvas.json` | canvas |
    | `destiny.json` | destiny |
    | `discord.json` | discord |
    | `settings.json` with gaze on | gaze |
    | `data\voicedeck_path.txt` | voicemeeter |
    | Steam installed | games |
    | Always | music |

  - Show "Restoring your modules…" and retry when back online.
  - Legacy files stay where they are until each module has copied them and verified the copy.
  - Module-owned localStorage keys are renamed to `mod.<id>.*`.
- **Done when:** unit tests cover detection and the localStorage rename.
- **PC check:** upgrade the owner's real 0.1.0 install. Keep a copy of the app data first.

**M3.6 Docs** (depends on M3.4)

- **Goal:** the docs match the app.
- **Changes:**
  - README: mark module features in the features table, add the GitHub catalog to the privacy table, and explain the install flow.
  - `docs/SETUP.md`: picking modules.
  - New `docs/MODULES.md`: a guide for module authors, built from §4.
  - `docs/BUILDING.md`: the workspace and `pack-modules`.
- **Done when:** every link works and the screenshots are current, with nothing personal in them.

### Phase 4: later (optional, ask the owner first)

- An offline `IslandShellSetup-Full.exe` that carries every module zip.
- Self-update for the base through the same signed catalog.
- Code signing for the installer and module exes, which fixes "Windows protected your PC" and Defender false positives.

## 7. Open questions (ask the owner when you get there; don't guess)

1. **Sections in base tabs.** Voicemeeter (Audio), VPN (Network and Controls) and Wallpaper Engine (Wallpaper) add *sections* to base tabs, not tabs of their own. The plan is a `ctx.sections.register({ tab, id, order, mount })` extension point, added in M0.3. Confirm the visual placement in the harness before M0.5c.
2. **Slot for the Modules cube.** Is slot `modules` at (0, −1.62) the right spot? Show a harness screenshot in M1.4.

## Decisions log

| # | Decision | Source |
| --- | --- | --- |
| D1 | Modules are separate processes, plus UI bundles in shadow roots | modules.md |
| D2 | One Destiny 2 module holds the API features **and** DIM; DIM works without a key | owner, 2026-10-24 |
| D3 | Music is pre-selected in the setup guide | owner, 2026-10-24 |
| D4 | Rest state: clock, date, temperature and a weather icon (sunny, cloudy, rainy, snow) | owner, 2026-10-24 |
| D5 | Modules ship on the same GitHub release as the installer, with a signed catalog | modules.md |
| D6 | First-party modules only in v1; developer mode for unsigned folders | modules.md |
| D7 | Port order: destiny → canvas → wallpaper-engine → vpn → games → voicemeeter → discord → music → gaze | modules.md |
| D8 | The base updates itself through the same signed catalog (a `base` entry: version, installer link, sha256, size, notes); its update waits for a click, modules update by themselves while daily checks are on | owner, 2026-10-25 |
| D9 | One branch for the whole modularization instead of one PR per package; the built-in "M0.5" steps are skipped, each integration goes straight to its module | owner, 2026-10-25 |
| D10 | Backends speak JSON-RPC over their stdin/stdout (anonymous pipes, made non-inheritable) rather than a named pipe: nothing else on the PC can reach them, with no DACL or PID check needed | implementation, 2026-10-25 |
| D11 | A `while-process` module also starts for a UI call, so it can open its app or be set up while the app isn't running; started that way it stops after its idle time. The Audio tab takes module views like Wallpaper (Voicemeeter's mixer) | implementation, 2026-10-26 |
| D12 | Until the Music module exists, the base publishes `media.state` from its status loop (when the song, play state or position jumps), so Discord's status keeps working. A docked site can be `split` (its module's island view stays beside it), a notification can carry an `img` (`round` for a person), and a module's setup card can report its state (`ctx.setup.card(() => ({ state }))`) | implementation, 2026-10-26 |
| D13 | The island keeps the now-playing chrome (pill, peek, Controls card, bar chip) and draws it from a `media` activity; the Music module owns the players, lyrics, the Music tab and sheet. Modules get island services for what the island must do itself: `ctx.island.focusApp` (bring an app forward), `ctx.island.audio.appVolume/setAppVolume`, the equalizer meter (the activity's `exe`) and the `level` event. "Pause when I step away" is the Music module's setting; eye tracking only publishes `presence`. With nothing playing, Controls shows no now-playing card | implementation, 2026-10-26 |
| D14 | The base hears the topics modules publish (the host's `Sink::topic`; the page gets `topic:<name>`, and `topic_last` for the latest), so the island follows `presence` itself. The base publishes `island.geometry` (`{ rect, monitor, scale, visible }`, physical pixels) from its cursor thread when it changes | implementation, 2026-10-26 |
| D15 | Installs the guide, search and migration ask for go through one background queue (`mod_queue`, modules/queue.json): one at a time, kept until each is in place, retried at the next start or check when offline. Migration runs when the page has `setupDone` from 0.1.x but not `modulesMigrated` (settings.json can't tell, the base writes it every start): it queues Music, plus each module whose 0.1.x file is there or whose app is on the PC. The catalog carries each module's `keywords` and `preselect` | implementation, 2026-10-26 |

## Progress tracker

Update your row in your PR: **todo** → **in review** (with a PR link). The owner sets **done** when merging.

| ID | Title | Depends on | Status | PR |
| --- | --- | --- | --- | --- |
| M0.0 | CI on Windows | — | in review | [#1](https://github.com/AbyssLotus/island-shell/pull/1) |
| M0.1 | Cargo workspace | M0.0 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.2 | Dev harness | M0.0 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.3 | Front-end registries | M0.2 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.4 | UI-kit stylesheet | M0.3 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5a | Built-in: destiny | M0.4 | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5b | Built-in: canvas | M0.5a | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5c | Built-in: wallpaper-engine | M0.5b | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5d | Built-in: vpn | M0.5c | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5e | Built-in: games | M0.5d | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5f | Built-in: voicemeeter | M0.5e | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5g | Built-in: discord | M0.5f | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5h | Built-in: music | M0.5g | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.5i | Built-in: gaze | M0.5h | skipped: ported straight to a module | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.6 | Backend event bus | M0.0 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M0.7 | Rest state | M0.3 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M1.1 | Protocol and SDK crates | M0.1 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M1.2 | Supervisor | M1.1 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M1.3 | Module UI loading | M1.2, M0.4 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M1.4 | Modules sheet | M1.3 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M1.5 | Destiny 2 module (pilot) | M1.4, M0.5a, M0.6 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.1 | Module: canvas | M1.5, M0.5b | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.2 | Module: wallpaper-engine | M2.1, M0.5c | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.3 | Module: vpn | M2.2, M0.5d | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.4 | Module: games | M2.3, M0.5e | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.5 | Module: voicemeeter | M2.4, M0.5f | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.6 | Module: discord | M2.5, M0.5g | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.7 | Module: music | M2.6, M0.5h | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M2.8 | Module: gaze | M2.7, M0.5i | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.1 | pack-modules | M1.5 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.2 | Install, update, remove | M3.1 | in review (no Modules sheet yet) | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.3 | Release workflow | M3.1 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.4 | Picking modules | M3.2 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.5 | Migration from 0.1.x | M3.2, M2.8 | in review | [#2](https://github.com/AbyssLotus/island-shell/pull/2) |
| M3.6 | Docs | M3.4 | todo | |

## Starting a session

Paste this into a new cloud session for the repo:

> Continue Island Shell's modularization. Read `docs/design/modules.md` and `docs/design/modules-handoff.md` (on `main`, or on the `design/modules` branch if it isn't merged yet). Do the next work package from the progress tracker whose dependencies are done, and only that one. Follow §1's rules. Open a PR to `main` with the package's PC check list in the description. Don't merge or release.
