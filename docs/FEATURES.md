# Island Shell: everything it does

The full reference. For installing and first steps, see the [setup guide](SETUP.md); for building it yourself, [BUILDING.md](BUILDING.md).

## Built in, and modules

Island Shell itself is the island, the Command Center, the Task Manager and stats, the calendar and Today, apps, the clipboard, sound, Wi-Fi and Bluetooth, procedural wallpapers and power. Everything marked *(module)* below is a module you add only if you want it: **Music**, **Discord**, **Canvas**, **Destiny 2** (with DIM), **Steam & games**, the **Tobii eye tracker**, **Voicemeeter**, **Wallpaper Engine** and **VPN**. A module you haven't added has no files, no process and no tab.

- **Modules cube** (Command Center): *Installed* (each one's state, memory and CPU, what it talks to, its settings and its recent log; Turn off, Restart, Update, Remove, keeping or forgetting its keys and data), *Available* (from the signed release catalog, ready to install) and *Updates*. **Load a folder…** loads an unpacked module in developer mode.
- **Search** offers **Install …** for a module you haven't added, by its name or what it does ("lyrics" finds Music).
- **Without Music**, the pill at rest shows the weather's icon and temperature, the date and the clock, and Controls has no now-playing card.
- How modules work, and how to write one: [MODULES.md](MODULES.md).

## Using the island

| Action | How |
| --- | --- |
| Peek at what's playing | Hover the pill |
| Open the panel | Tap the **Windows key** (opens Apps with search ready), press **Alt + `**, click the pill, or scroll down on it |
| Close it | Esc, scroll up, click outside it, or click into another app |
| Switch tabs | ← / →, keys 1–9 (the first nine tabs), or scroll sideways |
| Clipboard | ↑ ↓ to move, Enter to copy, P to pin, Del to remove |
| Power | Lock is instant; hold Sleep, Restart or Shut down for a moment to confirm |

The panel resizes to fit each tab, so the tab strip adapts with it: the open tab always shows its name, the tabs next to it keep theirs while there's room, and the rest shrink to icons (hover one to see its name).

At rest the island sits on the desktop layer: above the wallpaper (including Wallpaper Engine and Lively) and below every app window, so open apps cover it. Opening the panel (Windows key, Alt + `, or clicking the pill when it's visible) brings it in front of everything, and notifications pop in front briefly; when they close it settles back onto the desktop. A Windows-key tap on its own replaces the Start menu; Windows-key shortcuts (Win + E, Win + Shift + S, …) still work. The island slides out of the way while a fullscreen game or video is in front (maximized windows don't count), and a hotkey still brings it up over one. Diagnostics go to `island.log` in the data folder.

### Windows key shortcuts

Island Shell takes over the Windows shortcuts whose panels it replaces:

| Keys | Windows would open | Island Shell opens |
|---|---|---|
| Win (tap on its own) | Start | Apps |
| Win + A | Quick Settings | Audio |
| Win + G | Xbox Game Bar | Games |
| Win + V | Clipboard history | Clipboard |
| Win + N | Notifications and calendar | Today |
| Win + W | Widgets | Today |
| Win + Space | Switch keyboard language | Command Center (full screen) |

Pressing a shortcut while the island is open on another tab switches to its tab; pressing it again closes the island. Every other Win shortcut (Win+E, Win+L, Win+D, Win+Shift+S…) works as usual, and Win+Shift/Ctrl/Alt with those letters stays Windows' own. The keys are caught by a low-level keyboard hook, so Windows never sees them (see `winkey.rs`); quit Island Shell and they're Windows' again. Win itself is held back until another key joins it, and is then sent just ahead of that key, so a Win tap on its own never reaches Windows (Start can't open) while every other Win shortcut still works. Win with only the mouse doesn't count as Win held.

Windows silently drops a hook like this the first time it answers too slowly, and a hook another program installs later (PowerToys, for one) gets keys first. Either one let Win+G and Win+Space through to Windows. So the hook runs on a high-priority thread and is replaced with a fresh one every 3 seconds. Each shortcut it keeps from Windows is written to `island.log` ("Win shortcut kept from Windows → games"). If Windows still reacts to a shortcut and that line isn't in the log, the hook didn't see the key. That happens while a program running as administrator is in front, because Windows doesn't pass its keys to non-admin programs.

## Command Center

**Win + Space** turns the island into a full-screen hub; **Esc** (or Win + Space again) puts it back. The pill grows into a dark void with a black eclipse in the middle: white-blue light streams off its rim, warm plasma loops arc off it, and embers and dark specks drift away. A digital clock and the search box sit inside the disc.

- **Search everything**: start typing. The eclipse steps aside and results fill the right side, grouped with a top hit:
  - Start apps, open windows (switch to or close), Steam games (play, show in Steam, install folder).
  - Drives with free space, your folders (Desktop, Documents, Downloads, Pictures, Videos, Music, Home), and recently opened files.
  - Windows settings pages, and commands. Commands include Discord mute/deafen/leave, play/pause/next, the Voicemeeter mic and engine restart, Wi-Fi and Bluetooth, show/hide the taskbar, a new desktop, Task Manager, Terminal, File Explorer, the Recycle Bin, a screenshot, Control Panel and Lock.
  - Start with `>` for commands only, or type math (`2*(3+4)`, `=15%4`) for the calculator. Anything else can be searched on the web.
- **Keys**: ↑ ↓ to move, Enter for the main action, Ctrl+Enter for the second one, Tab to switch between kinds.
- **Remembers you**: what you open from here ranks higher next time (kept in the page's local storage). Recent files come from Windows' own Recent list, which is empty when *Show recently opened items* is switched off in Settings.
- **Cubes**: Apps, Discord, Audio, Today, Canvas, Games, Network, Clipboard, Destiny and Stats float around the eclipse, each with a live line ("4 in Late night lobby", "CPU 3% · 31°"). Hovering one turns it toward you and the eclipse's light reaches for it. Today's cube shows what's next on your calendar.
- **Sheets**: clicking a cube opens that section as a full-size glass sheet, with every section along the top (← → or 1–9 to switch, Esc back to the eclipse). Most sections show the island's own view, made bigger. Some have more:
  - **Discord**: the real Discord web app, docked in beside the island's call controls (sign in to Discord inside it once). Links in it open in your default browser.
  - **Canvas** and **Destiny**: switch between the island view and your Canvas website or DIM, docked in.
  - **Today**: a full calendar app (below).
  - **Apps**: your whole Start menu, full size (below).
  - The docked sites are separate windows owned by the island with no access to the island's own commands. A hidden site is told to use as little memory as possible straight away and is closed 45 seconds after you leave it (it reopens where you were, still signed in).

### Apps (the Apps cube)

Every app in your Start menu: Pinned (click switches to an open window, like a taskbar button), Opened lately, your folders, then everything from A to Z with a letter rail down the side to jump. Views for All, Pinned, Recent (how many times and when you opened each from the island) and Folders (rename, delete, make a new one). Type anywhere to search apps and windows; ↑ ↓ ← → move, Enter opens, Ctrl+Enter runs as administrator.

- **The side** shows the app under the pointer: its big icon, publisher, version and program file (click the path to copy it), whether it's running, which folder it's in, and Open, Run as administrator, Open file location, Pin and Uninstall (Uninstall opens Settings' page for it; you finish there). Run as administrator and the file location come from the app's own Start menu entry, so they're only offered where Start offers them.
- **Open now**: every open window (click to switch, minimize or close), and the tray icons below (click, double-click and right-click work as on the taskbar).
- **Right-click** any app for the same actions plus moving it into or out of a folder; right-click a window to switch, minimize, close or pin its app.

### Games (the Games cube) *(module)*

Your Steam library, full size. The chosen game fills the top with its hero banner and logo (your custom artwork from Steam first, with the logo where you placed it there), a big Play button (*Playing* while it runs, *Update* when Steam has one waiting), when you last played, how many hours you've played (and how many in the last two weeks), its size on disk, and buttons for Show in Steam, the install folder, Screenshots and the Store page.

- Below, every installed game's cover, with search, *All / Played lately / Most played / Needs an update*, and sorting by last played, name, hours or size. Click a cover to show it at the top, double-click (or Enter) to play. Arrow keys move around the covers.
- Your other launchers (Epic, EA, Battle.net…) sit at the bottom.
- Hours played come from Steam's own count in your account's `localconfig.vdf`; sizes from each game's manifest. Nothing is sent anywhere.

### Music (the Music cube, under the eclipse) *(module)*

What's playing, full size: the cover (Apple's full-size art when the catalog has the song, otherwise the player's own), glowing with the music; the title, artist and album, the year, genre and track number, and the Lossless / Dolby Atmos badge. Below it a progress bar you can drag, shuffle / previous / play / next / repeat, Favorite (Apple Music), the player's own volume, a button that copies the song's Apple Music link, and *Open Apple Music* (which closes the Command Center). The sheet is washed in the album's colour.

- **Lyrics**: the time-synced lyrics from LRCLIB fill the right side, the line being sung lit and kept in the middle. Click a line to jump there; scroll to look ahead (it follows again after a few seconds). *Lyrics on / off* is the same switch as the Music tab's.
- **Keys**: Space plays or pauses, ← / → jump 10 seconds, N / P next and previous, L lyrics.
- The cover, year and genre come from Apple's public iTunes catalog: only the title, artist and album are sent, once per song, and kept until the island quits. The up-next queue isn't shown: players don't share it with Windows.

### Task Manager (the Stats cube)

The Stats cube, the **Task Manager** command in search, or **Task Manager** in the island's Stats tab opens a Task Manager window over the eclipse, laid out like TMOG: a sidebar of pages, a status bar (live, processes, threads, sample count, up time) and an update speed (every second, every 3 seconds, paused). Drag it by its title bar, resize it from the corner, double-click the title bar (or use the square button) to fill the screen, Esc to close. Nothing is sampled while it's closed. Windows' own Task Manager is still there (Ctrl + Shift + Esc).

- **Summary**: LED meters for CPU, clock, temperature and GPU; the CPU overview (utilization, temperature and kernel time on one graph); the busiest processes (click one to find it in Processes); memory; tiles for disks, network, graphics power and the hottest sensor.
- **Performance**: CPU (a graph for each logical processor with its kernel time, or one overall; base speed, cores, caches, virtualization, power plan), memory (speed, slots, each module), each graphics adapter (engines, temperature, fan, power, clocks, voltage, driver, displays), each disk (model, type, volumes, temperature) and each network adapter (addresses, gateway, DNS).
- **Processes**: grouped the way Windows groups them (Apps, Background, Windows), or every process on its own. Columns to choose from: PID, status (suspended, efficiency mode), user, CPU, memory, GPU, disk read and write, threads, handles, command line. Choosing one shows its details (parent, user, start time, command line, copied with a click) with **Open file location**, **Efficiency mode** and **End task** (Del, asks first). **Run new task** runs anything the Run box does, optionally as administrator.
- **System Info**: Windows edition, version, build and install date; maker, board, BIOS, firmware, Secure Boot and TPM; the processor; each memory module (from the firmware's tables); graphics with driver versions; displays; disks and volumes; network adapters. **Copy as text** copies it all.
- **App history**: CPU time, reads, writes and launches for each program since the island started counting (Windows' own history needs administrator rights to read).
- **Startup apps**: as before.
- **Users**: everyone signed in, with their session's CPU, memory and busiest apps, and Windows' services together.
- **Services**: every service with its state, PID, description, startup type and group; start, stop and restart (Windows asks for permission when it's needed, which is most of the time).
- **Power & Freq**: every sensor Windows shares without a driver, with its lowest and highest since the page opened: CPU temperature, per-core utilization and clocks; each graphics card's temperature, clocks, fan, power, voltage and memory; each SSD's temperature, activity, speed and its S.M.A.R.T. health log (spare, wear, data written, power-on hours, unsafe shutdowns, errors); memory speed. CPU watts, board fans and voltages need a hardware driver and are marked as such.
- **Connections**: every TCP and UDP endpoint with the program that owns it, filtered by connected, listening, public, UDP or this PC, with remote host names (reverse DNS) and a map of where they go.
- **Installed Apps**: everything Settings > Apps lists, programs and Store apps, with publisher, version, date, size (measured in the background when the installer didn't say) and scope; uninstall (asks first), modify, open location, web page.
- **Disk Space**: scans a drive (all of C: takes about ten seconds) into a bubble map: each folder's size, colored by what's mostly in it, its biggest folders drawn inside it; double-click to go in. Beside it: the drive's space, the kinds of file, the weightiest folders and the largest files.
- **Benchmarks**: CPU (one thread and all of them), memory (copy speed, latency), disk (a 1 GB file written and read uncached, then random 4 KB reads; deleted afterwards), graphics (WebGL in the window) and internet (Cloudflare's speed test), on dials, with an overall Island score and a history. Nothing runs until you press Start.

### Calendar (the Today sheet)

- **Views**: Month, Week, Day and Agenda (the next 60 days). Week and Day have an all-day row, side-by-side overlapping events and a line for now. Keys: ← → to move, T for today, M / W / D / A for the views, N for a new event.
- **What's on it**:
  - Your own events, saved on this PC.
  - Canvas due dates, from the Canvas connection.
  - Destiny's weekly reset (Tuesdays, 17:00 UTC).
  - Any calendar you add by its iCal link.
- **Your events**: New event, or click an empty slot or day. Each has a name, time or all day, place, notes and color. Click an event for details; your own can be edited or deleted.
- **Adding a calendar** (Add a calendar, in the right column): paste its iCal link (https:// or webcal://). The link is stored DPAPI-encrypted and the calendar is re-read every 15 minutes. Repeating events, skipped dates and moved or cancelled single dates are handled.
  - **Google Calendar**: Settings → your calendar → Integrate calendar → *Secret address in iCal format*.
  - **iCloud**: Calendar → the share button next to the calendar → Public Calendar → copy the link.
  - **Outlook**: Settings → Calendar → Shared calendars → Publish a calendar → the ICS link.
- **Calendars list**: click one to show or hide it. A red **!** means the last read failed (hover it for why). Feeds you added can be removed (click × twice).
- Times are shown in this PC's time zone. An event stamped with a different named time zone is read as if it were in yours, which is right for calendars kept in your own zone (UTC-stamped events are always right).
- While it's open the island's window covers the whole monitor and stays on top (the same "lift, then always on top" trick as @mention notifications); closing puts it back where it rests.

## The look: Taken

Everything is in one look, the Taken from Destiny 2: a void-black body with cold white-blue light (Taken light, #c9e9ff) leaking from its edges, the same palette as the Command Center's eclipse.

- The island has a cold rim of light and a faint corona, and breathes slowly while it rests. A notification arrives with a flare and a flicker. Motes drift up through the open panel.
- Switched-on tiles stay dark and are lit from the edge; sliders fill with light toward a glowing edge; section titles are in Destiny's small spaced capitals.
- The light skin (Dark mode off) is Taken inverted: a pale ground with black ink.
- The accent is Taken light. *Accent follows wallpaper* (Wallpaper tab) brings back the wallpaper's colour; it starts off.
- The docked Discord app wears the same look: deep void backgrounds, cold mentions and selections instead of yellow and grey, Taken-light buttons and links, and light along the window's edges.
- A seventh generated wallpaper, **Ascendant**: a black eclipse leaking cold light over rim-lit peaks, with dark shards hanging in the void.
- Docked DIM wears a **Taken** theme written in DIM's own theme variables (over whichever DIM theme you picked; masterwork gold and DIM's other item signals keep their meaning). Docked Canvas, which has no dark theme, gets a filter skin: the page inverted with its hues kept, pictures, video and documents turned back, and Canvas's brand colours set so they land on the void and Taken light. Each skin stays off the sign-in pages a site sends you through (your school's, Bungie's). The brush button in a docked site's header switches its skin off and on.
- Every tab has its own touches: a diamond marker before each section title (and the resting pill's dot is a diamond), the sigil in empty states, a thin edge of light on the list row under the pointer, the Today clock in thin luminous mono, glowing rings and graphs, a cold corona round the album art that swells with the music, Taken-light audio meters, and Destiny's corner reticle on the game cover under the pointer, the chosen Guardian, the chosen wallpaper, the chosen game in the Games sheet, today in the calendar, and whatever has keyboard focus.

## Two layouts

Controls → **Layout** switches between them (Island Shell restarts in the new layout, about a second). From a script: `island-shell.exe --style bar`, `--style tall` or `--style island`.

- **Island**: the floating pill at the top center, resting on the desktop under your apps.
- **Top bar**: a full-width bar like a Linux desktop panel. It stays in front and reserves its 40px strip the way the taskbar does, so maximized windows fit underneath.
  - *Left*: the Apps button, Controls (gear), a dock of pinned and running apps (dot = running, accent line = the window you were in), and File Explorer.
  - *Middle*: your virtual desktops. Click a number to switch, + for a new one, or scroll over them.
  - *Right*: now playing (hidden on narrow screens), volume (scroll to change), Bluetooth (click toggles), Wi-Fi name and signal, battery (laptops only), CPU, your tray icons, the Windows notification center, power, and the clock.
  - Clicking a chip drops its panel down underneath it; clicking again closes it. The Windows key drops Apps down from the Apps button.
  - The bar slides away while a fullscreen game or video is in front.
- **Tall bar**: the same top bar at 48px instead of 40, with bigger chips and icons.

## What's real in this version

- **Controls**: Wi-Fi and Bluetooth radios, volume and mute on the default output device, a compact now-playing card while something plays (click it for the Music tab), dark/light island, and a taskbar show/hide switch. Sound output and Night light open the matching Settings page for now.
- **Music** *(module)*: built around Apple Music, with the basics for Spotify and anything else that reports to Windows media controls.
  - *Now playing*: big artwork that tints the whole card, title, artist and album, a seek bar (drag it), time left, shuffle, previous / play / next, repeat (off → all → one) and the player's own volume (just that app, like the Volume mixer). Open Apple Music brings its window forward.
  - *Apple Music extras*: Apple Music only tells Windows the title, artwork and play/pause/next, and squeezes "Artist — Album" into one field. Island Shell splits the album back out, and reads and presses Apple Music's own player bar through UI Automation (the accessibility API) for seeking, shuffle, repeat, Favorite (the star) and the Lossless / Dolby Atmos badge. It works while Apple Music is minimized or in its mini player.
  - *Lyrics*: time-synced lyrics from [LRCLIB](https://lrclib.net), a free open lyrics database. The line being sung stays in the middle; click any line to jump there; scroll to look ahead (it follows again a few seconds later). The current line also shows when you hover the pill. Only the song's title, artist, album and length are sent, and results are cached. The Lyrics button turns lookups off.
  - *Equalizer*: the bars in the pill and top bar move with the player's real output level.
  - *Which player*: whatever is playing, Apple Music first, then Spotify, then anything else. When more than one app has a media session, chips in the corner let you pin one (Auto goes back to choosing).
  - Spotify gets whatever its media session offers: seeking, shuffle and repeat when it reports them, plus volume, equalizer and lyrics. There's no Favorite outside Apple Music.
- **Audio** (Win+A, or the volume chip in the top bar, or the Sound output tile): master volume and mute; every output and input device, click one to make it the default for everything (it uses the same Windows interface the Sound settings page does); the default microphone's level and mute; and a mixer with each app's own volume, mute and a live level meter (apps playing to any device, merged per program; Apple Music shows as Apple Music even though its sound comes from its background agent).
  - *Voicemeeter* *(module)*: a **Windows / Voicemeeter / Sounds** switch at the top of the tab. The Voicemeeter view shows your inputs (the strip names you gave them, faders over live meters, mute, and A1–B3 routing) and outputs (A1–B3 with their devices, faders, mute), plus **Restart engine**. Drag or scroll a fader (Shift = fine), double-click for 0 dB. It runs on VoiceDeck's own Voicemeeter engine (`audio-app/crates/vm-core`, linked rather than copied), and Voicemeeter takes several remote clients at once, so the island and VoiceDeck stay in step. Meters are only read while that view is on screen.
  - *VoiceDeck*: **Open VoiceDeck** brings your VoiceDeck window up (it lives in the tray). In the top bar, a **mic chip** mutes strip 1, the same strip VoiceDeck's mic-mute key and red tray icon follow.
- **Canvas** *(module)* (your school's Canvas), two ways to connect:
  - *Calendar feed* (for schools that block access tokens): in Canvas, Calendar → *Calendar Feed* (bottom right); paste the `.ics` link. It gives every due date and course event, with the course and a link, but no grades, submission status or announcements. Done ticks are kept on this PC (`canvas-done.json`), and a **Courses** card shows each class's load for the coming week in place of grades. Notifications: due within 3 hours (on top of everything) and newly posted assignments. The link is DPAPI-encrypted, since anyone with it can see the calendar.
  - *Access token*, where the school allows them. Everything below:
  - *Coming up*: Canvas's planner for the next three weeks, grouped by day, colored by course. Each item shows its kind, course, time and points, plus *Submitted* / *Graded* / *Missing* / time-left chips. The checkbox ticks it off in Canvas itself, like the Canvas app's. Click an item to open it in your browser (never Edge).
  - *Missing* work on top, *Grades* (current score and letter per course), recent *Announcements*.
  - *Notifications*, checked every 10 minutes: due within 3 hours and not submitted (on top of everything, fullscreen games included), newly graded work with the score, new announcements. Clicking one opens the Canvas tab. The first check after connecting only takes note of what's already there.
  - *Setup, once*: your Canvas address (e.g. `school.instructure.com`) and a token from Canvas → Account → Settings → *+ New Access Token*. It's DPAPI-encrypted in `canvas.json`. Some schools turn tokens off.
- **Network**: your current Wi-Fi or Ethernet connection, plus a card for each VPN you have installed (the network chip in the top bar opens it; Controls has a tile for each).
  - *Tailscale* *(the VPN module)*: connected or not, your tailnet and this PC's address, Connect / Disconnect (it starts the Tailscale app when needed), an exit-node picker when your tailnet has exit nodes, and every device with its address, OS and whether the link is direct or relayed (offline ones show when they were last seen). Click an address to copy it.
  - *Radmin VPN*: on or off, your 26.x address, your networks, and who's online right now, connected directly or through Radmin's relay, and since when (or when they were last seen). Radmin has no command line or API, so this comes from its service log (`C:\ProgramData\Famatech\Radmin VPN\service.log`) and the network list its window keeps in the registry. Its power switch only exists in its own window; Open brings that window out of the tray.
  - The top bar's network chip shows a small shield while either VPN is connected.
- **Discord** *(module)* (through the Discord app's local connection, so Discord has to be running):
  - *Your call*: the voice channel you're in and its server, everyone in it with their avatar, a green ring on whoever is talking, and who's muted or deafened. Mute, Deafen and Leave buttons. While you're in a call the pill (or a chip in the top bar) shows up to three avatars with the same talking rings, and a muted-mic icon when you're muted.
  - *@mentions*: when someone @mentions you (or replies to you with the ping on), the island shows it with their avatar, on top of everything, fullscreen games included, for 6 seconds, without taking focus. Plain messages, DMs without an @, `@everyone`, `@here` and role pings are left to Discord. Switch: *Show when someone @mentions me*. You may want to turn off Discord's own Windows notifications so you don't get both.
  - *Listening to Apple Music*: while Apple Music plays, your Discord status shows the song, artist and album, the album art, a time bar, and a *Listen on Apple Music* button for others (art and link come from Apple's public iTunes catalog). It clears when the music stops. Switch: *Show what I'm listening to*.
  - *Setup, once*: Discord only lets an app its owner made read your calls and notifications, so the island runs as your own Discord application. In the [Discord Developer Portal](https://discord.com/developers/applications) click **New Application** and name it **Apple Music** (the status says "Listening to" + the app's name). In its **OAuth2** page add the redirect `http://localhost`, then paste the **Client ID** and **Client Secret** into the Discord tab and click Connect. Discord asks you to authorize it once; after that the island signs in by itself. The status alone needs only the Client ID.
  - The secret and Discord's sign-in tokens are encrypted with Windows' DPAPI (only your Windows account can read them) in `discord.json`. *App settings → Forget this app* deletes them.
- **Games** *(module)*: your installed Steam games as a shelf of their portrait covers (no titles needed, the logo is on the art): your own custom covers from Steam first (*Manage → Set custom artwork*, including animated WebP/APNG ones, which play), otherwise Steam's default art, most recently played first and a little bigger. A game that's running gets a green *Playing* badge, one waiting on an update says *Update*. Click to play (through `steam://rungameid/`), right-click for *Show in Steam* and *Open install folder*, scroll the mouse wheel to move along the shelf.
  - Under a divider, your other launchers as app icons (Steam, Epic, EA, Ubisoft Connect, Riot Client, Rockstar, RSI, Hytale, Battle.net, GOG, Xbox, Minecraft/Prism/Modrinth, Kyber, Roblox, whichever are installed). Clicking one switches to it if it's already open.
  - Read from Steam's own files: `libraryfolders.vdf`, each library's `appmanifest_*.acf` (name, install state, last played), your custom art in `userdata\<id>\config\grid\<appid>p.*`, and `appcache\librarycache`. A custom cover bigger than 1800×2700 is shrunk once into `covers\` in the data folder. Tools and non-games (redistributables, dedicated servers, Wallpaper Engine…) are left out. The gamepad button in the top bar opens the tab.
- **Destiny** *(module)*: your Guardians from Bungie's API, with your own API key.
  - *Characters* as their emblem banners: class, Power, race and when you last played, or a green line with what you're doing right now ("Crucible · Control"). Click one for its loadout.
  - *Loadout*: subclass, the three weapons with names and Power, and the five armor pieces with Power; exotics get a gold border.
  - *Xûr*: his exotic weapons and armor as icons (names on hover) and when his stock refreshes. Bungie currently lists him all week, without a location; if that changes back, it shows a countdown to his arrival. Daily and weekly reset countdowns sit at the top (17:00 UTC; weekly on Tuesdays).
  - *Setup, once*: paste the API key from [bungie.net/en/Application](https://www.bungie.net/en/Application) and your Bungie Name (`Name#1234`). Everything shown is public, so there's no Bungie sign-in. The key is DPAPI-encrypted in `destiny.json`. Item and activity names and icons are fetched from Bungie's manifest one at a time and cached in `destiny-defs.json`. The profile is re-read every minute while the tab is open, Xûr every 20 minutes. If the profile is private on bungie.net, the tab says so.
- **Eye tracking** *(module)* (Tobii trackers, through the Stream Engine library that Tobii Experience installs; nothing to set up):
  - Looking at the pill opens its preview, like hovering it with the mouse; look away and it closes. (Island layout; the bars have no pill.)
  - Notifications stay until you've looked at them (up to 20 seconds), instead of vanishing while you were looking elsewhere.
  - Optional: *Pause music when I step away* (Controls, under Layout). After 10 seconds away from the screen the music pauses; it plays again when you're back, unless you'd paused it yourself.
  - The Eye tracking tile in Controls turns all of it off. Gaze is only used live inside Island Shell; nothing is stored or sent.
- **Notification sounds** (Audio → **Sounds**, or the **Sounds** tile in Controls to switch them off and on): the island's own sounds, never Windows' default ding. They're made on the spot with Web Audio: Glass, Chime, Pulse, Drop, Eclipse (a slow dark swell), Taken (two voices a semitone apart, sinking), Ember, Tick and Low.
  - Each kind of notification has its own: Discord @mentions (Glass), due soon on Canvas (Chime), other Canvas updates (Pulse), the focus timer (Eclipse), apps asking for attention (Drop). Calls, small confirmations (copied, opening an app) and failed actions start silent. ▶ plays one.
  - *Your own file…* uses any sound file you pick (mp3, wav, ogg, m4a, flac; up to 5 MB; the first 8 seconds play). It's kept in the island page's own storage, not copied anywhere on disk.
  - A volume for notification sounds only, and *Quiet over fullscreen* for no sounds while a game or video fills the screen.
  - A sound still plays when the island's panel is open and its banner is hidden.
- **Today**: clock, this week, a focus timer that survives restarts, to-dos, and weather from Open-Meteo (click the city to change it, click the temperature to switch °F/°C).
- **Stats**: CPU, GPU and memory from Windows performance counters, top apps by CPU, disks, network speed and uptime.
  - *Temperatures, without HWiNFO*: everything comes from Windows itself, with no admin rights and no extra driver. The CPU chip's temperature (the Ryzen 7 8700G reports it through its built-in Radeon 780M, which is on the same chip), the graphics card's temperature, and each SSD's temperature with its drive letters. Each has a bar showing how close it is to its warning point, turning amber and then red. The CPU and GPU rings carry a small temperature tag, and the top bar's CPU chip shows it too (`12% · 41°`).
  - *Graphics card*: fan speed (or *Stopped* while it idles quietly), power as a share of its limit, core and memory clocks, voltage and VRAM, from the same source Task Manager uses. The CPU's real current clock comes from Windows' performance counters.
  - Not available without a driver: motherboard fan speeds and CPU power in watts.
- **Clipboard**: every text, link, color and image you copy, kept across restarts (150 items, 40 images). Content that password managers mark as private is never recorded.
- **Apps**: works like Start and the taskbar in one place.
  - *Running*: every open window (same rules as taskbar buttons). Click to switch, × or middle-click to close, right-click to minimize or pin.
  - *Tray*: the notification-area icons. Click, double-click and right-click behave like the real tray. Island Shell collects them by acting as a second tray host and forwards everything to Explorer (`"tray_host": false` in settings.json turns this off).
  - *Pinned*: right-click any app or open window → Pin, or drag an app into the row. Pinned apps switch to their open window if there is one, otherwise launch. Drag to reorder, drag out to unpin.
  - *Folders*: right-click an app → New folder… or a folder name, or drag an app onto a folder. Apps in folders leave the main grid. Click a folder to open, rename or delete it (its apps go back to All apps).
  - Typing searches open windows and apps; Enter switches to the first window or launches the first app.
- **Wallpaper**: with the Wallpaper Engine module, a **Wallpaper Engine / Island scenes** switch.
  - *Wallpaper Engine*: your whole library (Workshop subscriptions, your own projects, the bundled ones) as a grid, the one that's on first (marked ON), then recently used, then newest. Search titles and tags, filter Scenes / Videos / Web, hover a tile to play its animated preview, click to put it on your screen (through Wallpaper Engine's own `-control openWallpaper`). Pause/play and mute buttons control the running wallpaper. Mature-rated wallpapers stay blurred until you hover them.
  - Previews are ~1 MB GIFs or JPGs, so each gets a small still thumbnail, made once with Windows' image decoder and cached in `wethumbs\` (600 took about 8 seconds); only the hovered tile plays its real preview, and the grid is built a page at a time and dropped when you leave the tab.
  - *Island scenes*: six generated scenes rendered at your monitor's resolution and set on every monitor.
  - The island's accent color follows whichever wallpaper you pick, unless you turn that off.
- **Power**: lock, sleep, restart, shut down, start with Windows, quit.

Planned for the next pass: real Night light and Do not disturb toggles, monitor brightness (DDC/CI), switching the audio output device, and the desktop clock widget.

## Memory

At rest Island Shell uses about 70–130 MB of RAM in total (its own process plus every WebView2 process under it). With the Command Center open on Games and Discord docked it can reach 1–1.4 GB, almost all of it the docked web app and Chromium's working caches; that falls back to about 120 MB within a minute of closing it. What keeps it there:

- **WebView2 low-memory mode while resting.** A few seconds after the island goes back to a pill (or the bar has nothing dropped down), the WebView is told to use as little memory as possible (`MemoryUsageTargetLevel = Low`): it drops decoded images, raster tiles and other caches it would otherwise keep because the PC isn't short on memory. It's back to normal the moment the island is hovered or opened.
- **Memory handed back at rest.** 30 seconds after the island goes back to a pill, and every 10 minutes while it stays there, every process in its tree (the island, WebView2's browser, GPU, renderers and utilities) is trimmed with `EmptyWorkingSet`, the same trim Windows makes when it's short of memory. What Chromium kept around after use (decoded pictures, raster scratch, the GPU process's pools) goes back to Windows instead of sitting there for minutes; anything needed again comes back the moment the island opens. The log notes each trim (`memory: at rest, 9 processes 707 MB -> 95 MB`).
- **Docked sites don't linger.** Discord, DIM and Canvas run in their own renderers (DIM alone is ~800 MB while open). Hidden, they're switched to low-memory mode; 45 seconds later they're closed.
- **The Command Center sleeps when it's covered.** The eclipse and cubes stop drawing and the canvas shrinks to 1×1 while a sheet is open or the Command Center is closed, and it draws at 75% resolution otherwise (it's soft glow, so the difference doesn't show).
- **Small copies of game art.** A picture costs its decoded size, not its file size: a 3840-wide hero is 19 MB of pixels. Covers wider than 300 px and heroes wider than 2560 px get a small copy (made once, in the background, in `gamethumbs`); animated covers are left alone so they keep playing.
- **The equalizer only touches its own bars.** It updates 30 times a second; doing that through CSS variables on `<body>` restyled the whole page each time and piled up hundreds of MB of garbage between collections.
- **Heavy views are only built while they show.** The Games covers (some animated) are removed a moment after you leave the tab.
- **Small images where small is enough.** Clipboard images get 320 px thumbnails for the list (the preview still loads the original); album art travels as raw bytes into one blob URL, not as base64 text; the full-resolution wallpaper canvas is freed as soon as it's encoded.
- **Nothing is rewritten that didn't change.** The once-a-second refresh only touches text, icons and artwork that actually changed.

To look inside the WebView yourself, start Island Shell with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` set and open `http://127.0.0.1:9333` in a Chromium browser (not needed for normal use).

## The taskbar

While Island Shell runs, the Windows taskbar is hidden and set to auto-hide so maximized windows use the full screen. Quitting puts your original taskbar setting back. Your original setting is saved in `taskbar-restore.txt` before anything changes, so even after a crash the taskbar can be restored:

```
island-shell.exe --restore-taskbar
```

Starting Island Shell again and quitting it does the same thing. You can also flip the **Taskbar** tile in Controls, or the tray menu item when the taskbar is showing.

Other command-line switches:

- `island-shell.exe --quit` closes a running shell cleanly.
- Launching it a second time toggles the panel of the running one.

## Files

Everything lives in `%APPDATA%\league.seaweed.islandshell\`:

- `settings.json`: `hide_taskbar`, `tray_host`, `style`, `media_source` (the pinned player, empty for automatic), `gaze` (eye tracking on), `gaze_pause` (pause music when away)
- `canvas.json` / `canvas-seen.json` / `canvas-done.json`: your Canvas address and token or feed link (DPAPI-encrypted); what's already been notified; Done ticks with the feed
- `calendar.json`: your own events, the calendars you added (links DPAPI-encrypted), hidden calendars and colors
- `destiny.json` / `destiny-defs.json`: your Bungie API key (DPAPI-encrypted) and Bungie Name; cached names and icons from Bungie's manifest
- `discord.json`: your Discord application's Client ID, its secret and Discord's sign-in tokens (both DPAPI-encrypted), and the two Discord switches
- `lyrics\`: cached lyrics (misses are retried after 12 hours)
- `clipboard.json` and `clips\`: clipboard history and copied images
- `icons\`: cached app icons
- `wallpapers\`: rendered wallpaper PNGs
- `taskbar-restore.txt`: only exists while the taskbar is hidden

Page settings (accent, to-dos, timer, weather city) are kept in the WebView's local storage.
