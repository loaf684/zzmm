# ZZMM

**A desktop mod manager for Zenless Zone Zero, built for the ZZMI / XXMI (3DMigoto) loader.**

ZZMM keeps a mod library organized, installs archives safely, checks for updates, and browses GameBanana without leaving the desktop. It is local-first: no account, no telemetry, and no mod files changed unless you ask.

> **Status:** v0.1.0, Windows desktop build.
>
> ZZMM is an independent utility. It is not affiliated with or endorsed by HoYoverse, miHoYo, GameBanana, or the XXMI project. Use mods at your own risk.

## Screenshots

Manage the mod library, grouped by character:

![ZZMM mod library grouped by character](docs/screenshot-library.png)

Browse GameBanana and install mods without leaving the app:

![Browsing and installing GameBanana mods in ZZMM](docs/screenshot-browse.png)

Open a mod for a 3D preview, hotkeys, and a link to its source page:

![ZZMM mod detail page with a 3D preview and hotkey list](docs/screenshot-detail.png)

## Highlights

- **Library management** — scan the Mods folder, group by character, enable/disable with the ZZMI `DISABLED_` prefix, and spot hash conflicts between active mods.
- **GameBanana browser** — search, filter by category or character, sort, open the source page, and install a mod without leaving the app.
- **Safe archive installs** — drop `.zip`, `.7z`, or `.rar` files anywhere in the window. Wrapper folders and multi-mod packs are detected automatically.
- **Update checks** — compare installed file IDs against GameBanana; updates install beside the old version and disable it instead of deleting anything.
- **3D previews** — render a mod's mesh and decoded DDS textures directly from its INI, with per-part visibility and wireframe controls.
- **Hotkey editing** — view bindings, rebind keys inline, and detect conflicting hotkeys across enabled mods. The first save creates an `.ini.bak` backup.
- **Loadouts** — snapshot the enabled state of the library and restore it later without touching mods added afterward.

## Install

ZZMM requires **Windows 10 or 11 (x64)** and the **WebView2 runtime** (preinstalled on Windows 11 and most Windows 10 machines). You also need **XXMI Launcher with ZZMI** installed to load mods into the game.

Prebuilt installers are published on the [Releases](https://github.com/loaf684/zzmm/releases) page when available. If there is no release yet, build from source below.

### Build from source

Requirements: Node.js 20+, pnpm 10+, Rust 1.93+ with the MSVC toolchain, and the Microsoft C++ Build Tools with the “Desktop development with C++” workload.

```bash
pnpm install
pnpm tauri dev
```

To produce the release executable and Windows installers:

```bash
pnpm tauri build
```

| Artifact | Path |
| --- | --- |
| Portable executable | `src-tauri/target/release/zzmm.exe` |
| NSIS installer | `src-tauri/target/release/bundle/nsis/ZZMM_0.1.0_x64-setup.exe` |
| MSI installer | `src-tauri/target/release/bundle/msi/ZZMM_0.1.0_x64_en-US.msi` |

## Quick start

1. Install XXMI Launcher and let it set up ZZMI.
2. Open ZZMM and link your `.../XXMI Launcher/ZZMI/Mods` folder in **Settings**.
3. Browse GameBanana and install a mod, or drag an archive into the window.
4. Enable the mod in the library, then press **F10** in-game to reload ZZMI.

## How it works

All filesystem, archive, INI, mesh, and texture work runs in Rust through Tauri commands. React owns presentation and state.

| Layer | Choice |
| --- | --- |
| Shell | Tauri v2 (Rust) |
| UI | React 18 + TypeScript + Vite |
| State | Zustand |
| Routing | React Router 7 |
| 3D | three.js + React Three Fiber |
| HTTP | reqwest + on-disk TTL cache |

```text
src/                 React UI, stores, API wrappers
src-tauri/src/       Rust backend and Tauri commands
  api/               GameBanana client, cache, installs, updates
  installer/         ZIP / 7z / RAR extraction and mod-root detection
  ini_parser/        Lossless INI editing and hotkey conflicts
  mesh_loader/       3D preview mesh construction
  texture/           DDS / BCn decoding
docs/                Detailed technical and rendering notes
```

## GameBanana and privacy

GameBanana requests and mod downloads go **directly from each user's machine** to GameBanana; there is no ZZMM proxy or central API server. GameBanana receives the usual network information, including the user's IP address.

Responses are cached on disk for 30 minutes (browse), 2 hours (detail), 30 minutes (update checks), and 12 hours (categories). Requests are spaced, identify ZZMM with a contact URL in the User-Agent, and retry HTTP 429 responses with `Retry-After` or capped exponential backoff.

ZZMM collects no telemetry. Settings, loadouts, API cache, and downloaded archives stay in the app's local data folders; downloads are deleted after installation.

## Local safety

- Mods are installed as new folders; an existing folder is never overwritten.
- Updating a mod disables the previous folder instead of deleting it.
- Hotkey saves create a one-time `.ini.bak` before writing.
- Delete is permanent and requires confirmation in the UI.
- Downloaded archives are deleted after installation; archives you drag in stay where they are.

## Known limitations

- Windows only.
- Encrypted ZIP/RAR archives and some unusual 7z codecs are unsupported.
- Character grouping for unlinked mods is heuristic; ambiguous names land in **Uncategorized**.
- The 3D preview shows the bind pose; blend buffers and skinning are not applied.
- Hotkey conflict detection compares keys, not `condition` expressions.

See the [technical reference](docs/TECHNICAL.md) for the full feature list, architecture, release checklist, API constraints, and known limitations. Rendering behavior is documented in [rendering compatibility](docs/rendering-compatibility.md).

## License

[MIT](LICENSE) © 2026 bread
