# Building Island Shell

Island Shell is a [Tauri 2](https://tauri.app) app: a Rust backend (`src-tauri/`) and a plain HTML/CSS/JS page with no bundler (`src/`). It runs on Windows 10 and 11 (x64) and uses WebView2. Its integrations (Music, Discord, Canvas, Destiny 2…) are **modules** in `modules/`, each a small backend process plus UI the island loads; [MODULES.md](MODULES.md) is how they work and how to write one.

## What you need

- [Rust](https://rustup.rs) (stable, MSVC toolchain) and the Visual Studio C++ build tools
- The Tauri CLI, version 2.12.4 for the installer (see below): `cargo install tauri-cli --version 2.12.4 --locked`, or run it through npm as `npx @tauri-apps/cli@2.12.4` in place of `cargo tauri`
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

The template is Tauri's from **tauri-cli 2.12.4**, and it only builds with that CLI: Tauri's NSIS helpers change between versions (2.12 needs restart-manager macros 2.11's template doesn't have). The release workflow pins the same version. To move to a newer CLI, merge its template into ours, then change the pin:

```
git merge-file src-tauri/installer/installer.nsi <old CLI's installer.nsi> <new CLI's installer.nsi>
```

(Both are `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` in the Tauri repo, at the tags `tauri-cli-v<old>` and `tauri-cli-v<new>`. Line endings: the repo's copy is CRLF.)

The repo is one Cargo workspace (the root `Cargo.toml`): the app, its crates, the modules and the tools share one `Cargo.lock` and one `target/` folder, and the release profile lives in the root manifest.

Tests: `cargo test --workspace` from the repo root. Many modules have `#[ignore]`d tests that print live readings from your PC (`cargo test --release <module>::tests::live -- --ignored --nocapture`). Each one's `#[ignore = "needs …"]` says what it needs. CI (`.github/workflows/ci.yml`) runs `cargo check --workspace --all-targets` and `cargo test --workspace` on Windows for every push and pull request, plus `node --test` over `dev/tests` once that folder exists.

## Modules while you develop

A build without the release key can't download modules, so load the repo's own in **developer mode**:

```
cargo build --workspace --bins --exclude island-shell --exclude pack-modules
cargo run -p pack-modules -- stage --out target\dev-modules
```

`stage` writes every module, unzipped, with its backend exe beside it, into `target\dev-modules\<id>\`. In the Command Center's **Modules** cube, **Load a folder…** and pick `target\dev-modules` to load them all (or one module's folder for just that one). They're badged *Developer* and aren't checked against a signature. After changing a module, **Turn off** it in the Modules cube (a running backend's exe can't be replaced), build and `stage` again, and turn it back on; its UI reloads with the island. Keeping `target\` out of your antivirus's scans helps: Defender's cloud scan sometimes takes small new unsigned exes (the test helper `echo-module.exe` among them) for malware.

The dev harness (`dev/harness`, see its README) shows the page and every module's UI in an ordinary browser with made-up data, no Windows needed.

## Releases and updates

Island Shell updates itself, and its modules, from a **signed catalog** attached to every GitHub release: `modules.json` (crates/island-catalog) and its minisign signature `modules.json.minisig`. The island checks it at most once a day (Power → *Check daily*) and when asked (Power → *Check now*, or the tray's *Check for updates*). Everything it downloads is kept only if its size and sha256 match what the signed catalog says.

- **Island Shell itself:** Power shows the new version; *Update* downloads the installer, runs it with `/UPDATE` and quits (the installer quits a running island first and opens the new one).
- **Modules:** newer versions of installed modules are swapped in by the island. The old folder is kept aside until the new one is in place, and an update cut short is undone at the next start. With *Check daily* on, they update by themselves; otherwise Power shows *Update modules*.

### One-time setup (the owner)

1. Make the signing key, on your own PC: `cargo tauri signer generate -w %USERPROFILE%\.island\update.key`. Give it a password. Keep the key file and password safe and never commit them: anyone with them can push updates to every install.
2. Put the **public** key it prints into `src-tauri/catalog.pub`, below the note, and commit that. Until it's there the updater stays off in every build.
3. In the repository's Settings → Secrets and variables → Actions, add `ISLAND_SIGNING_KEY` (the whole text of the key file) and `ISLAND_SIGNING_KEY_PASSWORD`.

### Making a release

1. Bump the version in **both** `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (the updater compares the latter), and the version in a module's `module.json` (and its `Cargo.toml`) for every module that changed: anything in its own folder (its manifest, `ui/`, `skins/`, `readers/`, `src/`, `Cargo.toml`). That's what decides it, as a hash in the catalog (`source`), not the module's built program. So a change to the shared crates (`crates/island-module`, `island-module-proto`, `island-win`), a newer Rust or a new linker on the runner doesn't force any bumps: a module nobody touched keeps its last package, and its old program keeps working (the module API is unchanged). Bump a module anyway when it should ship with a shared-crate change, such as a fix it needs. The Release workflow pins Rust (1.99.1) and builds are reproducible (`.cargo/config.toml` links with `/Brepro`), so a release builds the same way each time.
2. Run **Actions → Release → Run workflow**, with a sentence or two of notes for the update prompt.
   - It builds the installer and every module's backend.
   - It runs `pack-modules`, which writes a deterministic zip per module (a module whose version is in the last release's catalog keeps that release's zip, and is refused if its own files changed without a version bump). A catalog from 0.3.1 or before has no source hashes, so for those modules the workflow downloads their zips and compares the files in them instead, all but the backend exe. It also writes the catalog with the installer in it, and signs it.
   - It checks the signed catalog against `src-tauri/catalog.pub`, then creates a **draft** release with `IslandShellSetup.exe`, the zips, `modules.json` and `modules.json.minisig`.
3. Review the draft and publish it. Installs see it on their next check (they read the *latest* release, so drafts are invisible to them).

Without the secrets the workflow is a dry run: everything is built and packed, the catalog is left unsigned (no island accepts it), nothing is released, and the files are attached to the run instead.

### Doing it by hand

```
cargo build --release --workspace --bins --exclude island-shell --exclude pack-modules
cargo run -p pack-modules -- pack --out dist --version 0.2.0 --release-url https://github.com/AbyssLotus/island-shell/releases/download/v0.2.0 --installer "target\release\bundle\nsis\Island Shell_0.2.0_x64-setup.exe" --notes "What's new" [--previous modules.json] [--unsigned]
cargo run -p pack-modules -- verify dist --key src-tauri\catalog.pub
```

With `--previous`, a module the catalog lists without a `source` hash (0.3.1 and before) is compared with its zip from that release, which must sit beside the catalog. `pack` signs with the key in `ISLAND_SIGNING_KEY` (and `ISLAND_SIGNING_KEY_PASSWORD`). `tools/pack-modules/testdata/` holds a key named `test-only` (password `test-only`) for the tool's tests; never sign a release with it.

## Layout

```
Cargo.toml           the workspace: the app, its crates, the modules and the tools
src/                 the island page (no bundler)
  index.html, style.css, kit.css   the page; kit.css is the shared look every module's shadow root adopts
  app.js             island state machine, tabs, polling, the now-playing chrome (from a `media` activity),
                     the resting pill, a call on the pill (a `call` activity), and the services modules get
  registry.js        tabs, sections, tiles, chips, cubes, sites, sheets, search, commands, hotkeys, setup cards,
                     activities; cube slots, activity order, the resting pill's weather, Win + letter table
  modhost.js         the module host on the page: manifests become registry entries, UI loads into shadow roots, ctx API 1
  engine.js          procedural wallpaper renderer
  cc.js, cc.css      the Command Center (Win + Space): eclipse canvas, search, cubes, sheets
  cc-apps.js         the Apps sheet: every Start menu app, pins, folders, open windows, the tray
  cc-calendar.js     the Today sheet's calendar app: month / week / day / agenda, editor, calendars list
  cc-modules.js      the Modules sheet: installed, available and updates; settings, logs, usage, developer mode
  cc-taskmgr.js      the Task Manager window: sidebar, status bar, Processes, Performance, Startup apps
  tm-overview.js     Task Manager pages: Summary, System Info, App history, Users
  tm-system.js       Task Manager pages: Services, Power & Freq, Connections, Installed Apps
  tm-tools.js        Task Manager pages: Disk Space (circle packing), Benchmarks (dials, WebGL test)
  tm-util.js, tm.css Task Manager formats, graphs, meters, dials; its window
  setup.js           the first-run setup guide, with its Pick your modules step
  sounds.js          notification sounds: Web Audio synthesis, per-kind choices, your own files (IndexedDB)
src-tauri/src/
  main.rs            commands, tray, startup, shutdown, the status loop
  modules.rs         runs modules (crates/island-host): serves their UI at mod.localhost, routes calls, events,
                     topics and activities, developer mode, install / remove, the upgrade from 0.1.x
  updater.rs         the signed release catalog: Island Shell's own updates, module updates, the install queue
  island.rs          window placement, click-through, fullscreen hiding, focus hand-back, island.geometry
  taskbar.rs         hide / restore / crash recovery
  winkey.rs          Windows key: solo tap and the Win + letter shortcuts (low-level keyboard hook)
  audio.rs           volume, devices and switching the default (IPolicyConfig), mic, per-app mixer, a program's
                     own volume and level meter (the equalizer, for a module's player)
  embed.rs           docked websites from modules' manifests (Discord, Canvas, DIM): owned windows, navigation guard,
                     idle reaper, each site's skin on its own hosts
  calendar.rs, ics.rs the calendar: your events, iCal feeds (15-minute cache), modules' calendar sources; iCalendar parser
  taskmgr.rs         Task Manager: processes and threads from one NtQuerySystemInformation call, grouping (module
                     backends sit in Island Shell's group), counters, end task, efficiency mode, app history, startup apps
  sysinfo.rs, services.rs, installed.rs, conns.rs, diskscan.rs, bench.rs, sensors.rs, stats.rs   the other Task Manager pages
  apps.rs, running.rs, icons.rs, places.rs   Start menu apps, open windows, icons, Command Center places
  clipboard.rs, radios.rs, bar.rs, wallpaper.rs, power.rs, tray.rs, util.rs
src-tauri/examples/  diagnostics (build with `cargo build --examples`, run them from a normal terminal)
  applemusic_dump.rs   every media session + Apple Music's UI Automation tree; run it if an Apple Music update breaks the Music module's bridge
  sensors_probe.rs, topmost_probe.rs, audio_sessions.rs, tobii_probe.rs, uia_dump.rs, winkey_log.rs
crates/
  island-module-proto  the module protocol: messages, framing, manifests (with validation)
  island-module        the SDK for module backends: run loop, host messages, DPAPI secrets, HTTP to declared hosts
  island-host          the supervisor: run modes, crash backoff, job object, routing, topics, packages, installs
  island-catalog       the signed release catalog
  island-win           Windows helpers module backends share (Steam, processes, registry, windowless launching)
  vd-vm-core           the Voicemeeter Remote API engine from the VoiceDeck project (MIT), for the Voicemeeter module
modules/<id>/        each module: module.json, src/main.rs (its backend), ui/ (its UI), skins/ (docked sites' skins)
  music, discord, canvas, destiny, games, gaze, voicemeeter, vpn, wallpaper-engine
tools/
  pack-modules         module zips, the signed catalog, `verify`, and `stage` for developer mode
dev/
  harness              the page in an ordinary browser with a mocked backend and made-up data
  tests                front-end tests (`node --test "dev/tests/**/*.js"`)
```
