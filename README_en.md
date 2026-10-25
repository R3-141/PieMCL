# PieMCL

PieMCL is a free, open-source launcher for **Minecraft: Java Edition** on Windows, built with Tauri v2 (Rust + React).

> Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

[中文](./README.md) | English

## Features

### Instance Management
- Multiple isolated game instances, each with its own game directory, version and loader
- One-click installation of official Mojang versions (releases & snapshots) and mod loaders (Fabric / Quilt / Forge / NeoForge, with optional OptiFine for Forge)
- **Automatic import of existing instances**: place the launcher next to a `.minecraft` folder and instances created by other launchers (HMCL, PCL, the official launcher) are detected and imported automatically — zero intrusion, original files are never modified, and those launchers keep working
- Instance duplication, rename, deletion to the recycle bin, and per-instance settings (memory, JVM optimization presets, extra JVM arguments, window size / fullscreen, custom Java)

### Mod Marketplace & Resource Management
- Built-in browsing and one-click installation of **Mods** (Modrinth and CurseForge), **Data Packs**, **Shader Packs** and **Resource Packs** (Modrinth), sorted by downloads / updates / relevance with version and loader filters
- Full dependency resolution before installation, per-file progress, update checking, and drag-and-drop installation from anywhere in the window
- Mod enable/disable (rename-based, no file moves), a per-instance trash with retention and restore, and missing-dependency warnings
- Modpack import from `.mrpack` (Modrinth) and CurseForge `manifest.json` zips: loader installation, all pack files, and `overrides/` (configs, resource packs, shader packs) are deployed automatically; instances can be exported back to `.mrpack`

### Game Launch
- Strictly follows Mojang's official command-line specification: conditional JVM arguments, natives extraction, and the complete placeholder set for both legacy (`minecraftArguments`) and modern (`arguments`) version formats
- Automatic resource completion before launch (client jar, libraries, assets) with hash verification
- Live process log viewer with crash summarization, kill / relaunch controls, and an exit guard when games are still running

### Download Engine
- Concurrent, chunked downloads (large files are split into parallel ranges) with resumable breakpoints and SHA-1 / SHA-256 integrity verification
- Automatic candidate rotation between official sources and community mirrors (BMCLAPI, MCIMirror), with per-source concurrency tuned to avoid server-side rate limiting
- Configurable global / per-host concurrency and optional proxy

### Java Management
- Automatic detection of installed Java runtimes: registry, `PATH`, `JAVA_HOME`, common vendor directories, and official-launcher bundled runtimes (robust against non-ASCII install paths)
- One-click download of the correct Java major version (Eclipse Temurin) required by each game version, plus custom Java path registration

### Accounts & Skins
- **Offline accounts** with custom player IDs and local skin injection (built-in local Yggdrasil session server)
- **Microsoft accounts** via the official OAuth 2.0 device code flow, with skin / cape management through the official Minecraft Services API
- 3D skin preview with cape toggle on the home page

### Multiplayer & Servers
- Server browser with live status (MOTD, online players, player list, SRV record resolution), one-click import of servers into any instance
- Built-in tunnel management for SakuraFrp (natfrp): download `frpc`, manage tunnels, and start port forwarding without leaving the launcher

### Media & Utilities
- Screenshot viewer per instance (grid preview, full-size view, clipboard copy, reveal in explorer, delete)
- World (save) management with auto-generated thumbnails, zip backup to an arbitrary location, and timed-delete confirmation
- Download panel with pause / resume / cancel / retry for every task; launcher data directory kept fully portable

### Appearance
- Light / dark / system theme, custom accent color, Acrylic window blur, and a custom background image with in-app cropping, blur and dimming controls

## Security & Privacy

- Microsoft sign-in uses the official device code flow in the user's system browser — the launcher never sees or stores the Microsoft password
- Tokens (OAuth refresh token and Minecraft access token) are stored **only** on the user's local machine, inside the launcher's own application-data folder, and are transmitted nowhere except directly to official Microsoft / Mojang endpoints
- No telemetry, no analytics, no data upload — all traffic goes directly to official Mojang / Microsoft / Modrinth / CurseForge endpoints (and user-configured mirrors)

## Tech Stack

- **Backend:** Rust (Tokio async runtime), Tauri v2
- **Frontend:** React 19, TypeScript, Tailwind CSS

## Build from Source

Prerequisites: [Node.js](https://nodejs.org/) 20+, [pnpm](https://pnpm.io/), [Rust](https://rustup.rs/) (MSVC toolchain on Windows).

```bash
pnpm install
pnpm tauri dev     # development build
pnpm tauri build   # release build → src-tauri/target/release/piemcl.exe
```

The launcher is a single portable executable; place it next to a `.minecraft` folder (or run it anywhere — existing instances from other launchers are detected and imported automatically).

## License

This project is licensed under the [GNU General Public License v3.0](./LICENSE).

Copyright © 2026 R3_141 and contributors.
