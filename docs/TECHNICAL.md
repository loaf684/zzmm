# ZZMM technical reference

> This is the detailed implementation, release, and API reference. For the
> project overview and quick start, see the [README](../README.md).

A desktop mod manager for **Zenless Zone Zero**, built for the **ZZMI / XXMI**
(3DMigoto) mod loader.

> **Status: complete (Phases 1–7).** Settings, installed-mod management, the
> mod detail panel (previews, embedded 3D preview, notes, rename and hotkey
> editing), local archive installation (zip/7z/rar), mod-root detection,
> drag-and-drop, hash conflict detection, GameBanana browsing/installing/update
> checks, the lossless INI parser, the CSP/error-handling pass and the packaging
> documentation are all implemented.

---

## Stack

| Layer      | Choice                                                                 |
| ---------- | ---------------------------------------------------------------------- |
| Shell      | Tauri v2 (Rust), v2 plugins + capabilities                             |
| UI         | React 18 + TypeScript + Vite 6                                         |
| Styling    | TailwindCSS 3.4 (dark, ZZZ-inspired theme)                             |
| State      | Zustand                                                                |
| Routing    | React Router 7 (`HashRouter`, safe under the Tauri protocol)           |
| Icons      | lucide-react                                                           |
| 3D         | three.js + @react-three/fiber + @react-three/drei                       |
| Logging    | `tauri-plugin-log` (rotating file + console)                           |
| Dialog     | `tauri-plugin-dialog` (native folder picker, called from Rust)         |
| Thumbnails | Tauri asset protocol, scope granted at runtime                         |
| Archives   | `zip` (pure-Rust codecs), `sevenz-rust2` (pure Rust), `unrar` (vendored C++) |
| HTTP       | `reqwest` (SChannel TLS on Windows, no OpenSSL) + TTL file cache       |
| Package mgr| pnpm 12                                                                |


## Requirements

- **Windows 10 or 11** (x64)
- **Node.js 20+** (tested on 26)
- **pnpm 10+** (tested on 12.16)
- **Rust stable 1.93+** with the MSVC toolchain (tested on 1.99)
- **Microsoft C++ Build Tools** with the "Desktop development with C++" workload
  (also required to build the vendored UnRAR extractor)
- **WebView2 runtime** — preinstalled on Windows 11 and most Windows 10
  machines. If missing, `pnpm tauri dev` fails to open a window; install the
  Evergreen WebView2 Runtime, or build with the bundled bootstrapper (already
  configured for installers).

## Run

```bash
pnpm install
pnpm tauri dev
```

`pnpm dev` alone serves the UI in a browser, but every Tauri command fails with a
"desktop backend is not available" error — use `pnpm tauri dev` for the real app.

## Build and package

```bash
pnpm tauri build
```

Artifacts (release profile: LTO, `opt-level = "s"`, symbols stripped):

| Artifact | Path |
| -------- | ---- |
| NSIS installer | `src-tauri/target/release/bundle/nsis/ZZMM_0.1.0_x64-setup.exe` |
| MSI installer | `src-tauri/target/release/bundle/msi/ZZMM_0.1.0_x64_en-US.msi` |
| Portable exe | `src-tauri/target/release/zzmm.exe` (needs WebView2 on the machine) |

The installers are per-user (`nsis.installMode: currentUser`), so they install
to `%LOCALAPPDATA%\Programs\ZZMM` without prompting for administrator rights.
That matters here: the app only ever writes to user-owned folders (its config
folder and the user-chosen Mods folder).

WebView2 is downloaded during installation when missing
(`webviewInstallMode: downloadBootstrapper`). Machines without internet access
need the Evergreen Runtime installed beforehand.

### Release checklist

1. `pnpm typecheck && pnpm build` — type and bundle check.
2. `cargo fmt --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path
   src-tauri/Cargo.toml` — Rust format and tests. Lints are optional and need
   the component first: `rustup component add clippy`, then `cargo clippy
   --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.
3. Bump `version` in `package.json`, `src-tauri/Cargo.toml` and
   `src-tauri/tauri.conf.json` (all three must agree).
4. `pnpm tauri build`, then install the NSIS artifact and run through: link a
   Mods folder, install an archive, install from GameBanana, edit a hotkey,
   preview a model.
5. Check the log at `%LOCALAPPDATA%\app.zzmm.desktop\logs\zzmm.log` for `ERROR`
   or `WARN` lines produced during that pass.

Unsigned builds trigger Windows SmartScreen ("unknown publisher"). Signing needs
a code-signing certificate wired into `bundle.windows.certificateThumbprint` or
`signCommand`; none is configured in this repository.

## Verify

```bash
pnpm typecheck
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

## Where data lives

Both paths are shown in **Settings → Storage & logs**.

| Data     | Location                                                   |
| -------- | ---------------------------------------------------------- |
| Settings | `%APPDATA%\app.zzmm.desktop\settings.json`                  |
| Loadouts | `%APPDATA%\app.zzmm.desktop\loadouts.json`                  |
| State backups | `%APPDATA%\app.zzmm.desktop\backups\mod-state-*.json`  |
| Logs     | `%LOCALAPPDATA%\app.zzmm.desktop\logs\zzmm.log`             |
| API cache| `%APPDATA%\app.zzmm.desktop\cache\gamebanana\*.json`        |
| Downloads| `%APPDATA%\app.zzmm.desktop\downloads\` (deleted after install) |

Settings are written atomically (temp file + sync + rename). A corrupt settings
file is moved to `settings.json.corrupt-<unix-seconds>` instead of being
overwritten. Relative folder paths are resolved to absolute paths on save.

`settings.json` shape:

```json
{
  "schemaVersion": 1,
  "modsDir": "C:\\XXMI\\ZZMI\\Mods",
  "oneActivePerCharacter": false
}
```

### Per-mod metadata

A mod can contain `.zzmm.json`. Manually added mods without one still appear and
are flagged **Unlinked**. Unknown or missing fields are tolerated.

```json
{
  "source": "gamebanana",
  "gameBananaModId": 252471,
  "gameBananaFileId": 1046460,
  "name": "Ellen Joe — Streetwear",
  "character": "Ellen Joe",
  "version": "1.3",
  "installedAt": "2026-11-03T12:34:56Z",
  "thumbnailUrl": "https://images.gamebanana.com/img/ss/mods/....jpg"
}
```

## Features

### Settings

- Native folder picker (Rust-side dialog), manual path editing, and debounced
  validation.
- Saving rejects paths that do not exist or are not directories.
- Bounded auto-detection of common XXMI Launcher / XXMI / ZZMI install folders.
- Shows local settings/log locations and the app version.
- **One active mod per character** optionally disables the other enabled mods for
  the same character when one is enabled. `Uncategorized` mods are never touched.

### Installed mods

- Scans immediate subfolders of the Mods folder, including nested INIs up to
  four levels deep, and tolerates unreadable entries.
- Detects the ZZMI `DISABLED_` prefix case-insensitively; enabling removes it.
- Enable/disable, delete (with confirmation), and open-in-Explorer per mod, plus
  a button to open the Mods folder itself.
- Groups mods by character: `.zzmm.json` first, then a meaningful
  `[Character]` folder prefix, then a best-effort ZZZ roster, otherwise
  **Uncategorized**. The character can be overridden from the detail panel.
- The sidebar owns the primary library filters: search (`q`), enabled state
  (`state=all|enabled|disabled`) and character (`character`). The page mirrors
  them as URL params, so the sidebar, the compact toolbar and deep links stay in
  sync. The library defaults to a dense flat grid; an optional **Group** toggle
  restores per-character sections. Hotkey/conflict filters, Updates, Loadouts
  and the hash-conflict panel live in one toolbar row, so conflicts no longer
  push the grid down. Grid/list views, card size and grouping are remembered
  locally.
- **Loadouts** snapshot the enabled state of every current mod. Applying one
  only switches mods that still exist, reports missing entries, and never
  touches mods installed after the loadout was saved.
- Batch operations write `backups/mod-state-*.json` before renaming folders, and
  report per-mod failures instead of aborting the whole batch.
- After any change that 3DMigoto only notices on reload (enable/disable, delete,
  rename, install, hotkey save) a throttled toast reminds you to press **F10**
  in-game.
- Local preview images are served through Tauri's asset protocol; remote
  `thumbnailUrl` is used as a fallback.
- Emits `mods:scan-progress` events during scans.
- Delete, rename, and open commands accept only a single folder name and verify
  it resolves to a direct child of the configured Mods folder.
- Clicking a card opens a slide-over detail panel with an **Image / 3D** viewer,
  character, author, version, source, install date, folder size, folder name,
  editable character and personal notes. Actions cover enable/disable, open
  folder, rename, delete, change preview and open the GameBanana page.
- The panel finds INIs in subfolders (up to four levels), but only parses files
  that actually contain a `[Key…]` header. `DISABLED*.ini` and `desktop.ini` are
  skipped. Hotkeys are grouped by file and show keyboard badges, action type,
  option count and `condition`. Mods without bindings show an explicit empty
  state, and keyboard indicators on cards come from the same scan (no extra pass
  over the Mods folder).
- Hotkey reports are cached per mod with each file's relative path, size and
  modified time, so reopening the panel reuses the result unless an INI changed.
  The scan runs on a blocking worker, never the UI thread.
- Hotkeys are edited inline in the panel: capture a new key combination, change
  the action type (`activate`, `hold`, `toggle`, `cycle`) and save. Only the
  `key` and `type` settings are written — there is no raw/text INI editor. After
  a save the panel reloads that mod's bindings and refreshes conflict badges.
- Hotkey conflict checks run when the panel opens, comparing only enabled mods
  and warning when another mod can fire on the same key press. The conflict scan
  applies the same file rules as the panel, so bindings inside `DISABLED*.ini` /
  `desktop.ini` never produce a warning.
- The old dedicated Hotkeys and Preview pages are gone; `/hotkeys` and
  `/preview` redirect to `/installed`, and the sidebar only lists Installed,
  Browse and Settings. The sidebar uses the real app icon and puts search,
  status and the character list at the top-left.

### Installing archives

- Drop `.zip`, `.7z`, or `.rar` files **anywhere in the window**. Multiple files
  can be dropped at once.
- Archive type is detected from magic bytes, not just the extension.
- Extraction happens in a hidden staging folder inside the Mods directory
  (`.zzmm-install-*`), which is always cleaned up. Same-volume moves make the
  final install fast, and failed downloads never leave half-mods behind.
- **Mod-root detection**: the extracted tree is walked until folders that
  directly contain `.ini` files are found. Wrapper folders are transparent, and
  nested folders below a root stay part of that mod. This supports both a single
  mod wrapped in folders and packs containing several mods.
- Multiple mods in one archive are installed as separate folders.
- Folder names are sanitized for Windows (invalid characters, reserved names,
  leading dots) and collisions get a ` (2)`, ` (3)` suffix instead of
  overwriting an existing mod.
- If the archive contains the INI at its root, the archive filename becomes the
  mod folder name.
- Drag-and-drop shows a full-window overlay; progress is shown in a floating
  panel and streamed via `install:progress`.
- `__MACOSX`, `.DS_Store`, `Thumbs.db`, `desktop.ini` and similar packaging noise
  are removed before installation.

### Browsing GameBanana

- **Browse** (`/browse`) lists Zenless Zone Zero mods (game ID `19567`) with a
  card grid: thumbnail, name, author, likes and version.
- Search, category filter (GameBanana root categories), character filter and
  sorting, with infinite scroll and a manual *Load more* fallback.
- **Detail page** (`/browse/:modId`): gallery with thumbnails, stats, file list
  with sizes/dates/versions, and an install button.
- **Install** downloads the chosen file into the app data `downloads` folder
  (with a byte-accurate progress bar), then hands it to the same archive
  installer used for drag-and-drop. Each installed folder gets a `.zzmm.json`
  with the mod ID, file ID, version, install date and thumbnail URL.
- **Update checks** on the Installed page compare the stored file ID (or version
  when the ID is missing) against the newest GameBanana file. Updating installs
  the new file as a new folder and **disables** the old one instead of deleting
  it, so nothing is lost.

#### API details and constraints

- Endpoints used: `Mod/Index` (browse, newest first), `Game/{id}/Subfeed`
  (trending / recently updated), `Util/Search/Results` (search),
  `Mod/{id}/ProfilePage` (detail + files), `Game/{id}/ProfilePage` (root
  categories).
- **Sorting**: GameBanana rejects `_sSort` on `Mod/Index` and only accepts
  `default`, `new` and `updated` on the game subfeed, and no endpoint exposes a
  download-count ordering. So the sorts are *Newest* (index order), *Trending*
  (GameBanana's engagement-ordered subfeed) and *Recently updated*.
- **Category filter** works server-side only on `Mod/Index`. For search and the
  sorted subfeed the category is matched against each record's root category,
  which is why one request may walk a few pages to fill a screen.
- **Character filter** matches GameBanana's subcategory name (e.g. "Ellen Joe")
  by substring. There is no working apiv11 endpoint for the subcategory tree, so
  the field offers roster suggestions but accepts any text.
- **Descriptions** are HTML fragments in GameBanana's `_sText` field (with
  `<br>`, `<b>`, `<font>`, `&nbsp;`, and occasional BBCode). ZZMM parses them
  into readable plain text — preserving paragraph and line breaks, list bullets
  and link labels/URLs — and renders that as text, so no raw markup or script is
  injected.
- Responses are cached on disk: browse/search 30 minutes, detail 2 hours,
  update checks 30 minutes, categories 12 hours. Cached pages are labelled
  `cached`.
- Requests use a stable `ZZMM/<version> (ZZMI mod manager; +https://github.com/loaf684/zzmm)`
  User-Agent, a 300 ms gap between request starts, and exponential backoff with
  `Retry-After` support on HTTP 429. Update checks remain capped at 25 mods and
  are serialized by the shared client.

### Archive support and the RAR trade-off

- **ZIP** uses the `zip` crate with only pure-Rust codecs (`deflate` via
  zlib-rs, deflate64, LZMA/XZ, PPMd) and rejects unsafe paths via
  `enclosed_name`. Encrypted ZIPs are not supported.
- **7z** uses `sevenz-rust2`, a pure-Rust implementation. LZMA/LZMA2 and the
  common BCJ/delta filters work; optional C-backed codecs are disabled. The
  high-level extractor performs traversal checks.
- **RAR** uses the `unrar` crate, which statically links RARLab's UnRAR
  extractor. Trade-offs:
  - It requires compiling C++ (the MSVC Build Tools are already required by
    Tauri), but there is **no DLL and no system dependency at runtime**.
  - It carries the UnRAR license terms (extraction/listing only; it cannot
    create archives).
  - RAR entry paths are validated against absolute/`..` components before
    extraction.
  - The pure-Rust `rar` crate was not chosen because it is older and less
    complete for solid/current RAR archives.

GameBanana downloads reuse this same installer pipeline: the archive is
downloaded to a temporary file first and then installed exactly like a dropped
archive.

### 3D preview

- Parses `[Resource*]` sections for `filename`, `stride` and `format`, matching
  ZZZ's split buffers (Position / Blend / Texcoord / IB) by component name. A
  relative `filename` resolves against the folder of the INI that declares it
  (3DMigoto semantics, also accepted as the same path from the mod root); a
  path-qualified name never resolves by bare name, and every resolved file must
  be a real file inside the mod folder. `IniResource::base_dir` keeps the
  caller's INI path for ranking and texture-scope comparisons.
- When the INI is incomplete, ZZZ's default layout is used and **validated
  against the file sizes**: position stride 40 (falling back to
  36/32/24/20/16/12), texcoord stride 20 (or whichever stride yields exactly the
  position vertex count), and 16-bit indices unless the vertex count needs
  32-bit.
- `DXGI_FORMAT_R16_UINT` and `DXGI_FORMAT_R32_UINT` are honoured. A missing
  index format is inferred and the guess is rejected when any index exceeds the
  vertex count.
- Normals are computed in Rust (area-weighted) because the position buffer does
  not carry usable ones.
- DDS textures are decoded in Rust: BC1/BC2/BC3/BC4/BC5/BC6/BC7 plus
  uncompressed RGBA/BGRA/masked layouts. Only the base mip is read; previews are
  box-downscaled to 1024 px and sent as PNG data URLs.
- Vertex data crosses the IPC bridge as base64 little-endian buffers and is
  rehydrated into typed arrays, so a 50k-vertex mesh stays around 600 KB rather
  than megabytes of JSON numbers.
- Viewer: embedded in the mod detail panel behind an **Image / 3D** tab, with
  orbit / zoom / pan, ambient + hemisphere + key/fill lights, an ink outline,
  wireframe toggle, per-component visibility, an expand control that fills the
  window with the stage (Escape or the restore button docks it back), and
  automatic camera framing from the mesh bounds.
- **Toggle controls**: the viewer lists the mod's `$variable` toggles that can
  change the rendered mesh and lets you switch them (e.g. show/hide a part)
  without editing the INI. A variable is offered only when a `[Key*]` section
  binds it with a `$var = ...` assignment — the mod's actual in-game controls —
  so `[Constants]` defaults, `[Present]`-derived guards, and condition-only
  internal flags are never exposed even when they change geometry. It must also
  be used in an `if` / `elif` / `else if` condition, have at least two known
  numeric values (from `$var = n` and comma-separated `$var = a, b` assignments
  plus `$var == n`/`$var != n` comparisons; a lone `0`/`1` is completed to the
  boolean pair), and have at least one alternative that changes the model
  fingerprint:
  renderable resources bound on an active path, the colour map resolved for each
  part and index buffer, and the `drawindexed` ranges with their colour maps.
  Normal/light/glow/mask maps and null placeholders are excluded from the
  fingerprint because the viewer never renders them, so a toggle that only
  rebinds one earns no control. `currentValue` is always one of `options`, and
  numeric options are ordered by value. Overrides are applied before `[Present]`
  replay (so a selector propagates to its derived guards) and again after it (so
  a direct override of a derived guard wins); only names the mod gates on and
  numeric values are accepted. Each change rebuilds the preview, keeping the
  last mesh visible until the new one lands and pruning hidden parts that the
  new mesh no longer contains.
- The mesh is built only when the 3D tab is opened. Loading and failure states
  are shown in place with a retry button; warnings (missing UVs, inferred
  strides, undecodable textures) are listed separately instead of failing the
  whole preview.
- Blend buffers and skinning are not applied: the preview is the bind pose.

### Hotkeys

- **Lossless, line-based parser**: sections, comments, blank lines, ordering,
  inline comments and CRLF/LF endings survive every edit. A save rewrites only
  the characters of the key/type value being changed.
- `[Key*]` sections are flattened per mod for the detail panel: file, section,
  binding, action type (`activate` default, `hold`, `toggle`, `cycle`), option
  count, `condition`, `$variables` and settings. Only sections with a real key
  are offered for editing.
- **Key capture**: click *Rebind* and press a combination. Modifiers are written
  in `CTRL ALT SHIFT` order, browser keys map to 3DMigoto tokens (`VK_F1`,
  `VK_NUMPAD0`, `VK_OEM_4`, `VK_PRIOR`, …), and *Require no modifiers* writes
  `NO_MODIFIERS`.
- **Conflict warnings** across all enabled mods. Bindings are grouped when they
  can fire on the same key press: positive modifiers are additive in 3DMigoto,
  so `F1` and `CTRL F1` conflict, while `NO_CTRL F1` and `CTRL F1` do not.
- **Backups**: the first save of a file creates `<name>.ini.bak` (never
  overwritten) before an atomic temp-file + rename write.
- Encodings are preserved: UTF-8, UTF-8 with BOM, UTF-16LE and a byte-exact
  single-byte fallback for legacy ANSI files.
- Path safety: only relative `.ini` paths inside the selected mod folder are
  accepted (no `..`, drive letters, ADS names or control characters).

### Hash conflicts

- Reads enabled mods' `*.ini` files and extracts `hash = ...` lines from
  `[TextureOverride*]` sections, handling inline comments, `0x` prefixes and
  UTF-16LE INIs.
- Flags hashes shared by two or more **enabled** mods, with file/section/line
  details.

## Architecture

All filesystem, archive, INI, mesh, and texture work happens in Rust as Tauri
commands. React owns presentation and state only.

```
src/
  lib/api/            Typed wrappers around invoke() + ApiError normalisation
  store/              Zustand stores (settings, mods, hotkeys, browse, preview)
  components/         Reusable UI primitives, app shell, drag-drop overlay
  features/installed/ Scan results, grouping, cards, detail panel, loadouts
  features/browse/    GameBanana grid and mod detail page
  features/hotkeys/   KeyCapture control used by the mod detail panel
  features/preview/   Shared three.js ModelViewer
  features/settings/  Settings page
src-tauri/src/
  error.rs            thiserror enum serialised as { code, message }
  state.rs            App state (settings cache + paths)
  settings/           model, atomic store, path resolution, detection, commands
  mods/               scanner, metadata, characters, actions, asset scope, commands
  installer/          archive detection/extraction, root detection, install, commands
  ini_parser/         lossless INI parser, encodings, bindings, conflicts, commands
  mesh_loader/        resource scanning, buffer decoding, preview command
  texture/            DDS header parsing, BCn decoding, PNG previews
  api/                GameBanana client, TTL cache, browse/detail/updates, download, commands
  util.rs             dependency-free RFC 3339 timestamps
```

IPC commands:

| Command              | Purpose                                                |
| -------------------- | ------------------------------------------------------ |
| `get_settings`       | Persisted settings + resolved paths + folder state     |
| `save_settings`      | Validate, normalise, persist, refresh thumbnail scope  |
| `validate_mods_dir`  | Cheap existence/shape check for the edited path        |
| `choose_mods_folder` | Native folder picker (returns `null` on cancel)        |
| `detect_mods_dirs`   | Ranked list of detected Mods folders                   |
| `scan_mods`          | Scan installed mods, metadata, hashes and conflicts    |
| `set_mod_enabled`    | Toggle `DISABLED_`, then rescan                         |
| `delete_mod`         | Permanently delete a mod folder, then rescan           |
| `open_mod_folder`    | Open a mod (or the Mods root) in Explorer              |
| `get_mod_details`    | Preview images, folder size, install date and notes    |
| `set_mod_notes`      | Persist per-mod notes in `.zzmm.json`, then rescan      |
| `rename_mod`         | Rename a folder while preserving its enabled state      |
| `change_mod_preview` | Pick and copy a new `preview.*` image                   |
| `open_external_url`  | Open an http(s) URL in the default browser              |
| `apply_mod_states`   | Apply many enable/disable changes with a safety backup |
| `set_mod_character`  | Persist a character override in `.zzmm.json`            |
| `list_loadouts`      | Saved loadout summaries                                |
| `save_loadout`       | Snapshot the current enabled/disabled state            |
| `delete_loadout`     | Remove a saved loadout                                 |
| `apply_loadout`      | Switch to a saved state, reporting missing/failed mods |
| `install_archives`   | Extract/install local archives, then report outcomes   |
| `browse_gamebanana`  | One page of GameBanana mods (cursor-based)             |
| `get_gamebanana_mod` | Mod detail, gallery and file list                      |
| `get_gamebanana_categories` | GameBanana root mod categories                 |
| `install_gamebanana_mod` | Download a file and install it, with progress      |
| `check_mod_updates`  | Compare installed file IDs with GameBanana             |
| `list_mod_ini_files` | INI files inside one mod folder                       |
| `get_mod_hotkeys`    | Active bindings flattened across one mod's INI files  |
| `read_mod_ini`       | Parsed document + sections + hotkeys (backend only)    |
| `save_mod_ini_raw`   | Raw save endpoint, not exposed in the UI              |
| `update_ini_setting` | Change/insert one setting, losslessly (hotkey saves)   |
| `scan_hotkey_conflicts` | Bindings from all enabled mods that overlap        |
| `load_mod_preview`   | Buffers + textures for one mod; accepts toggle overrides |

## Thumbnail access (asset protocol)

`app.security.assetProtocol` is enabled with an empty static scope. When settings
are saved, at startup, and before each scan, Rust grants the asset protocol
recursive access to the configured Mods folder. `convertFileSrc` then serves
`preview.png`/`preview.jpg` without copying files or base64-encoding them.

## Security

### Webview CSP

Release builds use a strict policy (`app.security.csp` in `tauri.conf.json`):

```
default-src 'self';
connect-src 'self' ipc: http://ipc.localhost;
img-src 'self' asset: http://asset.localhost data: https://*.gamebanana.com;
style-src 'self' 'unsafe-inline';
script-src 'self';
font-src 'self' data:;
worker-src 'none';
media-src 'self';
object-src 'none'; frame-src 'none'; base-uri 'self';
form-action 'none'; frame-ancestors 'none'
```

Each relaxation is there for a specific feature:

| Source                        | Needed by                                             |
| ----------------------------- | ----------------------------------------------------- |
| `ipc:`, `http://ipc.localhost` | Tauri's command and event bridge                     |
| `asset:`, `http://asset.localhost` | Local thumbnails via the asset protocol          |
| `data:` on `img-src`          | DDS texture previews, sent as PNG data URLs           |
| `https://*.gamebanana.com`    | Remote thumbnails, galleries and avatars              |
| `'unsafe-inline'` on `style-src` | React `style` props (card sizes, drag overlay)    |

`script-src` stays `'self'`: the production bundle loads no CDN script, uses no
`eval`, and contains no inline script. Tauri appends its own nonces and hashes
to the policy at build time for the code it injects.

`app.security.devCsp` is a separate, slightly relaxed policy used only by
`pnpm tauri dev` — it adds `http://localhost:1420` and `ws://localhost:1420` so
Vite's inline React-refresh preamble and HMR socket work. It is never shipped.

### Error handling

- Rust commands return `AppError` (`thiserror`), serialised as
  `{ code, message }`. `src/lib/api/client.ts` normalises every `invoke` result
  into an `ApiError`, and `toUserMessage` turns anything thrown — including raw
  strings and non-`Error` values — into toast text.
- Feature stores keep failures in state (`scanError`, `detailError`,
  `hotkeyError`, `conflictError`, `loadError`, …) and render the shared
  dismissible `Notice` component with a retry action. Dismissing an error keeps
  the page usable while a background request stays failed. Actions that change
  the disk (enable, delete, install, save hotkey) raise toasts.
- A render crash is caught by `src/components/ErrorBoundary.tsx`, which shows
  the message, the component stack, and a reload button instead of leaving a
  blank window.
- Unhandled promise rejections are logged and surfaced as a toast in
  `main.tsx`, so a missed `catch` is visible rather than silent.
- Backend panics are avoided in command paths: `unwrap`/`expect` appear only in
  `#[cfg(test)]` code, HTTP client construction failure becomes a network
  error, and a failed event loop is logged before exiting non-zero (the release
  profile sets `panic = "abort"`, so a panic would otherwise close the app with
  no message).

## Regenerating the app icon

The icon source is `src-tauri/app-icon.png` (generated by
`scripts/generate-icon.mjs`), and the platform icon set is produced with:

```bash
node scripts/generate-icon.mjs
pnpm tauri icon src-tauri/app-icon.png -o src-tauri/icons
```

The sidebar uses the shipped app icon (`src-tauri/icons/icon.png`, copied to
`src/assets/app-icon.png`) rather than a separate logo.

## Known limitations / notes

- Character grouping for unlinked mods is heuristic; ambiguous names fall into
  **Uncategorized**.
- Delete is permanent (no Recycle Bin integration) and is confirmed in the UI.
- Installing a mod with an existing name creates a suffixed copy rather than
  replacing files in place. Updates install a new folder and disable the old
  one; the old folder is not deleted automatically.
- The character filter is a substring match on GameBanana subcategory names, so
  unusual names ("Jane", "Pulchra") may also match unrelated mods.
- GameBanana has no download-count sort in apiv11, and encrypted archives can
  never be installed automatically.
- Encrypted ZIP/RAR archives and unusual 7z codecs are not supported.
- Hash conflict detection covers `[TextureOverride*]` `hash` lines only.
- The INI parser treats every physical line independently, so 3DMigoto line
  continuations are preserved but not understood as one value.
- The UI exposes only hotkey key/type editing; raw INI editing is not available.
- GameBanana descriptions are converted to plain text, so author-chosen colours,
  font sizes and inline images are not reproduced.
- Hotkey conflicts compare bindings only, not `condition` expressions, so two
  mods with mutually exclusive conditions are still reported.
- The 3D preview ignores the blend buffer, so models are shown in bind pose;
  vertex colours and morphs are not applied.
- Texture previews are capped at 1024 px and only the base mip is decoded.
- Merged or custom vertex layouts that fit no known stride are skipped with a
  warning instead of guessed further.
- The CSP still allows inline **styles** (`'unsafe-inline'` on `style-src`),
  because React `style` props (card sizes, drag overlay) cannot be nonced from
  this codebase. Scripts are restricted to `'self'` and no web workers are
  allowed (`worker-src 'none'`).
- `devCsp` is deliberately looser than the shipped policy (Vite HMR needs
  inline scripts and a websocket); treat only release builds as hardened.
- No code signing is configured; Windows SmartScreen will warn on unsigned
  builds.
