# PieMCL

PieMCL is a free, open-source launcher for **Minecraft: Java Edition** on Windows, built with Tauri v2 (Rust + React).

> Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

<img width="1377" height="877" alt="QQ_1792680241635" src="https://github.com/user-attachments/assets/c0d7ae7e-05a0-4f9f-8b15-f784ad972e73" />


## Features

### Instance Management
- Multiple isolated game instances, each with its own `.minecraft` directory, version and loader
- One-click installation of official Mojang versions (releases & snapshots) and mod loaders (Fabric / Quilt / Forge / NeoForge, with optional OptiFine for Forge)
- Instance duplication, export/import, and per-instance JVM settings (memory, window size, presets)

### Download Engine
- Multi-threaded downloads with automatic retry, resumable breakpoints and SHA-1/SHA-256 integrity verification
- Optional mirror acceleration for Mojang-manifest and Java runtime downloads (useful in regions with poor connectivity)


### Java Management
- Automatic detection of installed Java runtimes (registry + common paths + `JAVA_HOME`)
- One-click automatic download and installation of the correct Java major version (Eclipse Temurin) required by each game version

<img width="1377" height="877" alt="QQ_1792682237563" src="https://github.com/user-attachments/assets/08153333-a1d3-447c-9281-3cfb6fbc33ca" />


### Game Launch
- Strictly follows Mojang's official command-line specification, including conditional JVM arguments, natives extraction and the complete placeholder set for both legacy (`minecraftArguments`) and modern (`arguments`) version formats
- Live process logs, crash summarization, and clean process shutdown handling

<img width="2218" height="1060" alt="QQ_1792680309806" src="https://github.com/user-attachments/assets/ffb88379-f8b2-46cd-abbd-5606759f5806" />


### Community Resources
- Built-in browsing and one-click installation of **Mods** (Modrinth and CurseForge), **Data Packs**, **Shader Packs** and **Resource Packs** (Modrinth)
- Full dependency resolution for mods before installation, update checking, drag-and-drop local installation, and modpack import/export (`.mrpack`)

<img width="1377" height="877" alt="QQ_1792680283483" src="https://github.com/user-attachments/assets/2fe91838-aa83-4889-8b7f-d38cc3ebf00c" />

### Accounts
- **Offline accounts** with custom player IDs
- **Microsoft accounts** via the official OAuth 2.0 device code flow (see below)

## Microsoft Account Integration

PieMCL requests the `XboxLive.signin` delegated permission, together with `offline_access`, **solely** to let players sign in with their own Microsoft account and play Minecraft: Java Edition, exactly as the official launcher does.

**What the launcher does with this permission:**

1. Performs the standard Xbox Live / XSTS / Minecraft Services authentication chain (`user.auth.xboxlive.com` → `xsts.auth.xboxlive.com` → `api.minecraftservices.com`) to obtain a Minecraft access token for the signed-in player.
2. Reads the player's Minecraft profile (`GET /minecraft/profile`) to display their player name, skins and capes inside the launcher.
3. Passes the obtained token to the game process at launch (`--access_token`), so the player joins servers and worlds with their real Minecraft identity.
4. Optionally allows the player to manage their own skin, skin model (Classic/Slim) and capes through the official Minecraft Services API — the same actions available on minecraft.net.

**What the launcher does NOT do:**

- It never sees or stores the user's Microsoft password — sign-in happens in the user's system browser via the device code flow.
- It does not upload, sync or share any user data. There is no telemetry.
- Tokens (OAuth refresh token and Minecraft access token) are stored **only** on the user's local machine, inside the launcher's own application-data folder, and are never transmitted anywhere except directly to official Microsoft / Mojang endpoints listed above.

## Tech Stack

- **Backend:** Rust (Tokio async runtime), Tauri v2
- **Frontend:** React 19, TypeScript, Tailwind CSS
- All network traffic goes directly to official Mojang / Microsoft / Modrinth / CurseForge endpoints; the launcher ships no bundled game files.

## License

This project is open source under the MIT license.
