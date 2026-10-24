# Setting up Island Shell

From download to a working island in about two minutes. Everything after step 3 is optional.

> [!NOTE]
> Island Shell is in active development, so you may run into bugs. [Troubleshooting](#troubleshooting) covers the common ones, and [issues](https://github.com/AbyssLotus/island-shell/issues) are welcome.

- [1. Download it](#1-download-it)
- [2. Install it](#2-install-it)
- [3. The setup guide](#3-the-setup-guide)
- [4. Everyday use](#4-everyday-use)
- [5. Connect your stuff](#5-connect-your-stuff)
- [6. Change the look](#6-change-the-look)
- [Troubleshooting](#troubleshooting)
- [Uninstalling](#uninstalling)

## 1. Download it

Get **[IslandShellSetup.exe](https://github.com/AbyssLotus/island-shell/releases/latest/download/IslandShellSetup.exe)** from the [latest release](https://github.com/AbyssLotus/island-shell/releases/latest).

You need Windows 10 or 11, 64-bit. It's about 3 MB.

## 2. Install it

Double-click **IslandShellSetup.exe**. There are no pages to click through: a small window shows the progress for a few seconds, then the island appears at the top of your screen.

- **"Windows protected your PC"**: Island Shell isn't code-signed yet, so SmartScreen may stop it the first time. Click **More info**, then **Run anyway**.
- **No administrator rights needed.** It installs for your account only, in `%LOCALAPPDATA%\Island Shell`, with a Start menu shortcut and a desktop shortcut.
- **Prefer to pick the folder?** Run `IslandShellSetup.exe /WIZARD` for the usual installer pages.
- **Updating** is the same: run the newer installer. It closes the running island properly first and keeps your settings.

## 3. The setup guide

The first time Island Shell starts, it opens the setup guide. It takes about a minute, and every step can be skipped.

| | |
| --- | --- |
| ![Welcome](screenshots/setup-welcome.jpg) | **Welcome.** What the island is. Press **Let's go**, or **Skip setup** to start straight away. |
| ![The island](screenshots/setup-island.jpg) | **The island.** Tap the **Windows key** to open it, hover for a peek, Win + A / G / V / N to jump to a section. Your taskbar is hidden while Island Shell runs and comes back when you quit. |
| ![The Command Center](screenshots/setup-command-center.jpg) | **The Command Center.** **Win + Space** opens a full-screen search for apps, windows, files, games, settings and commands, with every section as a cube. |
| ![Pick your look](screenshots/setup-look.jpg) | **Pick your look.** The **Island** (a pill that grows) or the **Bar** (a strip across the top), dark or light. |
| ![Start with Windows](screenshots/setup-start.jpg) | **Start with Windows.** On by default, so the island is there every time you sign in. |
| ![Connect your stuff](screenshots/setup-connect.jpg) | **Connect your stuff.** Music and Steam work on their own. Discord, Canvas, Destiny 2 and calendars have a **Set up** button that takes you straight to their tab (see [step 5](#5-connect-your-stuff)). |
| ![You're all set](screenshots/setup-done.jpg) | **Done.** You can open the guide again any time: search **Setup guide** in the Command Center. |

## 4. Everyday use

| To | Do |
| --- | --- |
| Open the island | Tap the **Windows key**, click the pill, or scroll down on it |
| Peek at what's playing | Hover the pill |
| Close it | **Esc**, click outside it, or tap the Windows key again |
| Switch sections | ← / →, the number keys, or click the icons along the top |
| Open the Command Center | **Win + Space** |
| Jump to a section | **Win + A** (Audio), **G** (Games), **V** (Clipboard), **N** or **W** (Today) |
| Open the Start menu | **Ctrl + Esc** (the Windows key opens the island now) |
| Quit | The island's **Power** section, or the tray icon |

The Command Center's cubes open each section full size: your Steam library, every app, music with lyrics, a calendar, Discord, and the **Stats** cube's full Task Manager (processes, performance, sensors, disk space, benchmarks and more).

## 5. Connect your stuff

These are all optional, and all stay on your PC: keys and tokens you enter are stored encrypted for your Windows account.

### Music
Nothing to set up. Apple Music, Spotify, your browser and anything else that shows in Windows' media controls appear on the island, with time-synced lyrics from [LRCLIB](https://lrclib.net) when there are some. Apple Music gets extras (album, seek, shuffle, repeat, favorite).

### Steam
Nothing to set up. Installed games show up in the Games section and cube with your custom artwork, playtime and a Play button. Other launchers (Epic, EA, Ubisoft, Riot, Battle.net…) sit underneath.

### Discord
Your call (who's talking, mute, deafen, leave) and your notifications on the island. Discord needs a small "application" of your own, which takes about two minutes:

1. Open the [Discord Developer Portal](https://discord.com/developers/applications) and click **New Application**. Name it **Apple Music** if you'd like your status to read "Listening to Apple Music".
2. On the app's **OAuth2** page, add the redirect `http://localhost` and save.
3. Copy the **Client ID** and **Client Secret** from that page into the island's **Discord** section and press **Connect**.
4. Discord (the desktop app, which must be running) asks you to authorize it. Click **Authorize**.

### Canvas
Due dates, grades and announcements from your school's Canvas, in the **Canvas** section and the calendar. Two ways:

- **Calendar feed** (no token): in Canvas, open **Calendar**, click **Calendar Feed** (bottom right), copy the link that ends in `.ics` and paste it in. You get due dates and course events.
- **Access token** (everything): in Canvas, open **Account → Settings**, click **+ New Access Token**, name it "Island Shell", copy the token and paste it in with your school's Canvas address (like `school.instructure.com`).

### Destiny 2
Your Guardians, loadouts, Xûr and the weekly reset.

1. On [bungie.net/en/Application](https://www.bungie.net/en/Application), click **Create New App**. Any name works; set **OAuth Client Type** to **Not Applicable**.
2. Copy the app's **API Key** into the island's **Destiny** section, with your Bungie Name (like `Guardian#1234`).

### Calendars
In the Command Center's **Today** cube, add any calendar's iCal link: Google Calendar (Settings → your calendar → *Secret address in iCal format*), Outlook (Settings → Calendar → Shared calendars → *Publish a calendar*), iCloud, or your school's.

### Also picked up automatically
Voicemeeter (its mixer in the Audio section), Wallpaper Engine (in Wallpaper), Tailscale and Radmin VPN (in Network), and Tobii eye trackers (pause music when you step away).

## 6. Change the look

- **Layout**: Controls → **Layout**: Island, Top bar or Tall bar. Island Shell restarts in the new layout in about a second.
- **Dark or light**: Controls → **Dark mode**.
- **Accent and wallpapers**: the **Wallpaper** section has procedural wallpapers that set the accent color, and your Wallpaper Engine library.

## Troubleshooting

- **The taskbar didn't come back.** Open the Start menu (Ctrl + Esc) and run **Restore the taskbar (Island Shell)**. Starting Island Shell and quitting it again does the same.
- **The Windows key opens the Start menu instead of the island.** Island Shell isn't running, or another keyboard tool has the key. Start it from the Start menu.
- **Discord says it can't connect.** The Discord desktop app has to be running (not just the website), and the redirect `http://localhost` has to be saved on your app's OAuth2 page.
- **A game or app is missing.** Steam games appear once Steam has installed them; apps come from the Start menu's All apps list.
- **Something else.** Island Shell writes a log to `%APPDATA%\league.seaweed.islandshell\island.log`. [Open an issue](https://github.com/AbyssLotus/island-shell/issues) with what happened and the end of that log.

## Uninstalling

**Settings → Apps → Installed apps → Island Shell → Uninstall.** The uninstaller closes the island properly first, so your taskbar comes back as it was. To also remove your settings and caches, tick **Delete the application data** on the uninstaller's page, or delete `%APPDATA%\league.seaweed.islandshell` afterwards.
