# Building Island Shell

Island Shell is a [Tauri 2](https://tauri.app) app: a Rust backend (`src-tauri/`) and a plain HTML/CSS/JS page with no bundler (`src/`). It runs on Windows 10 and 11 (x64) and uses WebView2.

## What you need

- [Rust](https://rustup.rs) (stable, MSVC toolchain) and the Visual Studio C++ build tools
- The Tauri CLI: `cargo install tauri-cli --version "^2"`
- WebView2 (built into Windows 11 and current Windows 10)

## Run and build

```
target\release\island-shell.exe
```

Development (live reload of the page, rebuild on Rust changes):

```
cd src-tauri
cargo tauri dev
```

Release build: `cargo tauri build --no-bundle` (or drop `--no-bundle` for an NSIS installer).

`cargo tauri build` makes the one-click installer at `target/release/bundle/nsis/`. It uses the NSIS template in `src-tauri/installer/` (Tauri's own, changed to install without wizard pages; `/WIZARD` brings them back) and hooks that quit a running Island Shell first so the taskbar always comes back. `src-tauri/installer/make_art.py` redraws the installer's images.

The repo is one Cargo workspace (the root `Cargo.toml`): the app, its crates, the modules and the tools share one `Cargo.lock` and one `target/` folder, and the release profile lives in the root manifest.

Tests: `cargo test --workspace` from the repo root. Many modules have `#[ignore]`d tests that print live readings from your PC (`cargo test --release <module>::tests::live -- --ignored --nocapture`). Each one's `#[ignore = "needs …"]` says what it needs. CI (`.github/workflows/ci.yml`) runs `cargo check --workspace --all-targets` and `cargo test --workspace` on Windows for every push and pull request, plus `node --test` over `dev/tests` once that folder exists.

## Releases and updates

Island Shell updates itself, and its modules, from a **signed catalog** attached to every GitHub release: `modules.json` (crates/island-catalog) and its minisign signature `modules.json.minisig`. The island checks it at most once a day (Power → *Check daily*) and when asked (Power → *Check now*, or the tray's *Check for updates*). Everything it downloads is kept only if its size and sha256 match what the signed catalog says.

- **Island Shell itself:** Power shows the new version; *Update* downloads the installer, runs it with `/UPDATE` and quits (the installer quits a running island first and opens the new one).
- **Modules:** newer versions of installed modules are swapped in by the island. The old folder is kept aside until the new one is in place, and an update cut short is undone at the next start. With *Check daily* on, they update by themselves; otherwise Power shows *Update modules*.

### One-time setup (the owner)

1. Make the signing key, on your own PC: `cargo tauri signer generate -w %USERPROFILE%\.island\update.key`. Give it a password. Keep the key file and password safe and never commit them: anyone with them can push updates to every install.
2. Put the **public** key it prints into `src-tauri/catalog.pub`, below the note, and commit that. Until it's there the updater stays off in every build.
3. In the repository's Settings → Secrets and variables → Actions, add `ISLAND_SIGNING_KEY` (the whole text of the key file) and `ISLAND_SIGNING_KEY_PASSWORD`.

### Making a release

1. Bump the version in **both** `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (the updater compares the latter), and the version in a module's `module.json` for every module that changed.
2. Run **Actions → Release → Run workflow**, with a sentence or two of notes for the update prompt.
   - It builds the installer and every module's backend.
   - It runs `pack-modules`, which writes a deterministic zip per module (a module whose version is in the last release's catalog keeps that release's zip, and is refused if it changed without a version bump). It also writes the catalog with the installer in it, and signs it.
   - It checks the signed catalog against `src-tauri/catalog.pub`, then creates a **draft** release with `IslandShellSetup.exe`, the zips, `modules.json` and `modules.json.minisig`.
3. Review the draft and publish it. Installs see it on their next check (they read the *latest* release, so drafts are invisible to them).

Without the secrets the workflow is a dry run: everything is built and packed, the catalog is left unsigned (no island accepts it), nothing is released, and the files are attached to the run instead.

### Doing it by hand

```
cargo build --release --workspace --bins --exclude island-shell --exclude pack-modules
cargo run -p pack-modules -- pack --out dist --version 0.2.0 --release-url https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0 --installer "target\release\bundle\nsis\Island Shell_0.2.0_x64-setup.exe" --notes "What's new" [--previous modules.json] [--unsigned]
cargo run -p pack-modules -- verify dist --key src-tauri\catalog.pub
```

`pack` signs with the key in `ISLAND_SIGNING_KEY` (and `ISLAND_SIGNING_KEY_PASSWORD`). `tools/pack-modules/testdata/` holds a key named `test-only` (password `test-only`) for the tool's tests; never sign a release with it.

## Layout

```
src/                 the island page (no bundler)
  index.html
  style.css
  app.js             island state machine, tabs, polling
  engine.js          procedural wallpaper renderer
  cc.js, cc.css      the Command Center (Win + Space): eclipse canvas, search, cubes, sheets
  cc-calendar.js     the Today sheet's calendar app: month / week / day / agenda, editor, calendars list
  cc-taskmgr.js      the Task Manager window: sidebar, status bar, Processes, Performance, Startup apps
  tm-overview.js     Task Manager pages: Summary, System Info, App history, Users
  tm-system.js       Task Manager pages: Services, Power & Freq, Connections, Installed Apps
  tm-tools.js        Task Manager pages: Disk Space (circle packing), Benchmarks (dials, WebGL test)
  tm-util.js         Task Manager formats and SVG: graphs, sparklines, LED meters, dials
  tm.css             the Task Manager window
  cc-games.js        the Games sheet: hero banner and logo, Play, playtime, the cover library
  cc-music.js        the Music sheet: big cover, controls, synced lyrics
  sounds.js          notification sounds: Web Audio synthesis, per-kind choices, your own files (IndexedDB)
src-tauri/src/
  main.rs            commands, tray, startup, shutdown
  island.rs          window placement, click-through, fullscreen hiding, focus hand-back
  taskbar.rs         hide / restore / crash recovery
  media.rs           Windows media controls (GSMTC): picks the player, seek / shuffle / repeat
  applemusic.rs      Apple Music's player bar through UI Automation (seek, shuffle, repeat, favorite, badge)
  lyrics.rs          LRCLIB lookup, LRC parsing, cache
  audio.rs           volume, output/input devices and switching the default (IPolicyConfig), mic, per-app mixer and level meter
  vpn.rs             Tailscale (its CLI) and Radmin VPN (service log + registry) for the Network tab
  music.rs           the Music sheet's full-size cover, year, genre and link from the iTunes catalog (best match by title, artist and album)
  taskmgr.rs         Task Manager: every process and thread state from one NtQuerySystemInformation call, per-CPU kernel time, owner and command line, grouping, GPU / disk / network counters (volumes when PhysicalDisk has no instances), end task, efficiency mode, run, app history, startup apps
  sysinfo.rs         System Info: Windows, board / BIOS, CPU caches, SMBIOS memory modules, DXGI adapters with driver versions, displays (QueryDisplayConfig), disks, volumes, adapters, TPM; NVMe health log
  services.rs        services (SCM, configs cached; start/stop/restart, elevated through PowerShell when refused) and signed-in sessions (WTS)
  installed.rs       installed apps: Uninstall keys and Store packages, background folder sizes, uninstall / modify
  conns.rs           TCP/UDP owner-PID tables, scopes, well-known ports, reverse DNS on six threads
  diskscan.rs        Disk Space scanner: six threads over FindFirstFileEx, compact folder tree with kinds of file, views for the bubble map
  bench.rs           benchmarks: CPU, memory, disk (unbuffered), internet (speed.cloudflare.com)
  embed.rs           docked websites in Command Center sheets (Discord, Canvas, DIM): owned windows, navigation guard, idle reaper, per-site skins (discord-skin.css, dim-skin.css, canvas-skin.css), each only on that site's own hosts
  calendar.rs        the calendar: your events, added iCal feeds (15-minute cache), Canvas due dates, Destiny's reset
  ics.rs             iCalendar parser: RRULE (daily / weekly / monthly / yearly, BYDAY, BYMONTHDAY, COUNT, UNTIL), EXDATE, RECURRENCE-ID; wall-clock times through Windows' time-zone APIs
  places.rs          Command Center places: drives, known folders, Windows' Recent list (shortcuts resolved), open / reveal, tool allowlist
  canvas.rs          Canvas LMS: calendar feed (.ics parser) or REST API: planner items, missing work, grades, announcements, planner overrides, 10-minute notification check
  destiny.rs         Bungie API: characters, loadouts, current activity, Xûr; manifest definitions fetched one by one and cached
  discord.rs         Discord local RPC pipe: voice call, speaking, mute/deafen/leave, notifications, Apple Music Rich Presence, OAuth with DPAPI-stored tokens
  winkey.rs          Windows key: solo tap and the Win+A/G/V/N/W shortcuts (low-level keyboard hook)
  voicemeeter.rs     Voicemeeter mixer + VoiceDeck, through VoiceDeck's vd-vm-core engine crate
  wpe.rs             Wallpaper Engine: library scan, current/recent from its config, WIC thumbnails, CLI control
  gaze.rs            Tobii eye tracking: eyes-on-island dwell, presence, pause music when away
  games.rs           installed Steam games, covers, heroes and logos (with Steam's logo placement), sizes, playtime from localconfig.vdf, launch through steam://
  radios.rs          Wi-Fi / Bluetooth radios, current network
  stats.rs           PDH counters, DXGI, disks
  sensors.rs         temperatures, fans, clocks without HWiNFO: GPUs via D3DKMT perf data (CPU via its built-in GPU), SSDs via IOCTL_STORAGE_QUERY_PROPERTY
  clipboard.rs       clipboard listener + history
  apps.rs            AppsFolder scan, icon extraction, launch
  wallpaper.rs       IDesktopWallpaper
  power.rs           lock / sleep / restart / shut down
src-tauri/examples/  diagnostics (build with `cargo build --examples`, run them from a normal terminal)
  applemusic_dump.rs   every media session + Apple Music's UI Automation tree; run it if an Apple Music update breaks the bridge
  sensors_probe.rs     every GPU's and drive's sensor readings, as Windows reports them without admin
  topmost_probe.rs     how a background process can raise the resting island (writes %USERPROFILE%\island-topmost-probe.txt; `read` only reports)
  audio_sessions.rs    every audio session on every output device, with owner process and live levels
  tobii_probe.rs       a few seconds of gaze and presence from the eye tracker (Tobii Stream Engine)
  uia_dump.rs          UI Automation tree of any program's windows: `uia_dump.exe <part of exe name>`
```

`crates/vd-vm-core/` is the Voicemeeter Remote API engine from the VoiceDeck project (MIT), used by the Audio tab.
