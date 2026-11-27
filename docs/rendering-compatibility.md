# Mod rendering compatibility

The preview builds a mesh from whatever a mod actually declares; it never
requires a particular folder layout, model format, or material naming scheme.

## Pipeline

1. **Collect INIs.** Every `.ini` in the mod folder is parsed (`collect_ini_files`),
   not just the first one. Bufferless/texture-only INIs still contribute
   textures, `this = ...` swaps, and texture overrides. Manager backups and stale
   copies (`DISABLED_*`, `old *`, `backup*`, `~*`, `*.bak`, `* - copy`,
   `*_backup`) are skipped so they cannot stack geometry or redeclare resources.
2. **Index resources.** Each `[Resource...]` is classified as a position buffer,
   another vertex buffer, an index buffer, or a texture (`resources.rs`).
   Name-based suffixes (`Position`, `Texcoord`, `IB`) are hints only; the INI
   `stride`/`format` remain authoritative for decoding.
3. **Harvest draws (`variants.rs`).** The INI is replayed against the saved
   `d3dx_user.ini` state:
   - `[Present]` is replayed first. Menu-driven mods derive the draw toggles
     (`$yifu0`, `$hair1`, `$shenti0`, …) from persistent selector variables
     there every frame, and those derived values are never saved. Without the
     replay they stay at their `[Constants]` default of 0 and the preview draws
     only the always-on props. Literal and variable-copy assignments in an
     active branch are applied; time/input arithmetic is left untouched.
   - `vb0`/`ib` bindings are tracked, so each `drawindexed` is attributed to the
     buffers bound at that moment rather than to the whole section.
   - `match_first_index` values split a shared position buffer into consecutive
     mesh slices; equal values are treated as alternative variants.
   - `this = ...` and `ps-t0`/`Diffuse` bindings are recorded per index buffer,
     together with the section that made the binding.
   - Every active `drawindexed` also records the colour map that was bound at
     that exact moment, whatever slot carried it (`Resource\ZZMI\Diffuse`,
     `ps-t0`, or a plain `ps-t3` assignment). A section that rebinds the diffuse
     between draws (a merged third-party hair + captured game cape, or Seed's
     scooter + body) therefore produces one render part per material instead of
     painting every range with the last texture.
   - `ib = null` clears the running index binding, so a later draw never inherits
     the previous buffer.
   - A buffer with explicit `drawindexed` lines records that fact even when every
     line sits in a disabled branch. All-disabled means the part is omitted; only
     a buffer with no `drawindexed` line at all (the game's `auto` draw) falls
     back to the whole index buffer.
   - A whole-line commented `; drawindexed` / `# drawindexed` line is the mod switching
     that sub-draw off. It is recorded against the index buffer bound at that
     point, but the part is only omitted when the section's hash group also sets
     `handling = skip` and submits no `drawindexed = auto`; otherwise the game
     still draws the buffer and the preview keeps the whole-buffer fallback.
   - The hash group's `handling = skip` is tracked with the hash (evaluated
     against the saved toggle state), so a conditional skip the state does not
     take leaves the original draw in place.
   - Toggle branches apply the saved user values first, then `draw_type`/`active`
     system defaults. Only drawn variants are kept.
4. **Scan the folder (`resources::scan_folder`).** Shader-dump mods keep `.buf`,
   `.ib`, and `.dds` files without declaring every one in an INI. The scan adds
   missing textures and is the fallback buffer source when no INI describes any.
   `.ib` files are always buffers, never textures.
5. **Build parts (`commands.rs`).** For each position buffer, index buffers are
   selected by identity, decoded, and split into one render part per slice.
   Faces, vertices, UVs, and the chosen colour map are emitted per part.
   Position ranges that no index buffer references are pruned with a warning.

## Texture resolution order

Each `drawindexed` range records the colour map bound when it was submitted.
When a section rebinds the diffuse between draws, that per-range binding is
authoritative and the index buffer is split into one render part per material
(labelled `[tag]`). The order below applies to ranges that recorded no binding
of their own.

For each part the first non-data candidate wins:

1. A binding made by the index buffer's **own section** (this also covers
   `this = ...` replacements and split body variants). A redeclared resource name
   is resolved inside the draw's own INI folder first, then globally, so a body
   INI and a sibling `Funnel/` INI that both define `Resource_Body_Diffuse` do not
   paint each other's geometry.
2. A colour map whose file name starts with the index buffer's component core
   (`Body1B...` -> `Body1BDiffuse`, `d8003df3...` -> `d8003df3...DiffuseMap`).
   This prevents a section that draws two meshes from leaking its diffuse onto
   the sibling index buffer. The candidate must exist on disk: a mod may declare
   `YuzuhaOutfitADiffuse.dds` without shipping it while the draw is really bound
   to the shared body atlas.
3. The explicit binding resolved for the position/index (`texture_for`,
   `texture_for_index`).
4. A named `TextureOverride...Diffuse` section matching the component core. The
   target may live in a different INI; a same-folder definition wins over a
   global one.
5. The full index-buffer stem, then the resolved `ib -> diffuse` link table.
6. A colour map whose name mentions the component key.

`Resource\ZZMI\Diffuse` is the colour slot; `Resource\ZZMI\D` is the same slot
through the `SetTex`/`Set.FromAlias` short aliases and is treated identically.
Draws that rebind a colour on several `ps-tN` slots use the last active
non-data binding (ZZZ shaders put the diffuse on `ps-t3`). A `ResourceNullTexture`
(1x1 placeholder) is never a colour map: it only disables a slot and must not
overwrite a diffuse another INI supplied for the same section name.

Normal, light, material, glow, mask, opacity/alpha, specular, roughness, and UI
maps are never used as colour. Face/makeup maps are excluded for non-head parts.

### Cross-mod texture library

When a part has UVs but *no* colour map in its own mod, the preview may borrow a
diffuse from another installed mod for the **same character**. The library is
built from the file names of every other mod folder; a name is split into a
character token and a component token (`NekomataBodyADiffuse` → `nekomata` /
`body`). Matching is conservative:

- only an exact character + component match, or
- a body/limb part (`body`, `legs`, `arm`, `hand`, `skin`, …) borrowing that
  character's `body` atlas.

Accessories with their own UV island (`coat`, `gourd`, `hair`, `glass`) never
borrow a body atlas, because that paints them with the wrong island. Files under
the current mod are never considered, and the borrowed file is only read, never
modified. Every borrow is reported:
`"<part>: no diffuse is shipped in this mod; using <file> from the mod library for this character."`.

If nothing matches, the part is left neutral and warned; the preview never
invents a tint or borrows another part's atlas.

## Alpha cutout

Preview textures whose alpha channel contains values below 245 are flagged
(`PreviewTexture.has_alpha`, `MeshPart.has_alpha`) and rendered with
`alphaTest = 0.5` when the part has UVs. This keeps hair, lace, and cutout
clothing from rendering as opaque cards without tinting the opaque parts.

Uncompressed `B8G8R8A8` colour maps keep their fourth byte as alpha for exactly
this reason; the `B8G8R8X8` forms, whose fourth byte is padding, and any mask
set without an alpha channel decode as opaque.

The preview downscale averages colour weighted by alpha coverage, so the fully
transparent black texels around a cutout island no longer drag the island's edge
colour towards black (which read as a dark fringe on hair and lace).

## Decode cost

One colour map is decoded and PNG-encoded once per preview, however many
sub-draws bind it, and a source taller or wider than 4096 px is decoded from a
sampled block grid (every second 4x4 block) before the box filter. Both matter
in practice: an 8192x8192 BC7 atlas costs about 29 s per full decode in the
pure-Rust decoder, and one installed mod binds three of them plus 2048 px maps
to twenty-one parts — 428 s per preview before, 23 s after. The preview itself
is capped at 1024 px, so the extra source resolution could never be shown.

## Compatibility behaviors

- **Multiple structures:** resource sections, `CommandList` includes, `ref`/
  `copy` transfers, and `\ZZMI\` texture conventions are all parsed.
- **Split meshes:** body/hair `A`/`B`/`C` IB alternatives are deduplicated per
  `(match_first_index, target component)`, preferring the IB whose own name
  matches the component over one merely bound through `vb0`.
- **Alias vertex buffers:** `ResourceOutfitVB` maps to `outfit` for the outfit
  position, while still allowing the IB's own name to decide.
- **Cross-draw IB leases:** a section may bind another part's IB (Belle's body
  binds the legs IB/VB). The plan keeps the self-named IB rather than the leased
  one, and the leased geometry is still drawn where the INI draws it.
- **Bufferless INIs** are valid; they contribute only textures and overrides.
- **Split downloads:** a GameBanana file that ships several INI roots (`body/`,
  `face/`, `hair/`) is installed under one mod folder, so the library shows one
  entry while every part still loads from its relative path.
- **Resource paths:** a `[Resource*]` `filename` is resolved the way 3DMigoto
  resolves it — against the folder of the INI that declares it (or, for a
  folder-scan file, the folder it was found in) — and a path written from the
  mod root by a nested INI is accepted as the same path. A name that carries a
  folder never resolves by bare name, so a selected variant's missing
  `.\GraceMod.03\GracebodyB.ib` cannot become `GraceMod.01`'s buffer. A bare
  file name is a mod-relative reference and is searched for in the whole mod.
  A resolved file must be a real file inside the mod folder: a `..` step that
  stays inside is accepted (installed mods reach shared files that way), while
  absolute paths, steps above the mod folder, and symlinked files that leave it
  are refused.
- **Missing files** produce a warning naming the resource and leave the part
  untextured rather than failing the whole preview. A missing buffer skips the
  part it would have drawn and is reported the same way; it is never silently
  replaced by another variant's file.
- **Toggle state:** alternatives not drawn by the saved state are skipped and
  counted in the preview warnings.
- **Truncated dumps:** a draw can ask for more indices than its buffer holds
  (`S11Mod` asks for 70221 of a 69273-index IB). The game still draws the
  buffer, so the preview clamps the range, renders the part that exists and
  names the shortfall instead of hiding the whole mesh.
- **Skipped draws:** 3DMigoto merges every override that shares a `hash`, and
  `handling = skip` on any of them suppresses the game's original draw for that
  group. A commented-out `drawindexed` (`;` or `#`) is the mod's explicit "this
  replacement draw is off", and the preview omits that part only when all three
  hold: the draw line is commented out, the section's hash group sets
  `handling = skip` for the current toggle state, and the group submits no
  `drawindexed = auto`. `S11Mod` is the case: all four overrides share
  `hash = e3ee72d9`, `[TextureOverrideS11BodyIB]` sets `handling = skip`, only
  `BodyA` keeps a live `drawindexed` (clamped to the dump's 69,273 indices), and
  `BodyB`/`BodyC` are commented out. The game submits BodyA only, so `BodyB` (the
  hip-height bag, 3,041 triangles) is absent in game; the preview used to paint
  it back in. `SoukakuCowBikini`'s mask (696 triangles) is hidden the same way.
  Everything else keeps the whole-buffer fallback because the game still draws
  it:
  - a commented draw in a hash that is not skipped (the game's original draw
    remains),
  - a skipped group that carries `drawindexed = auto` (Nekomata/Astra/Nicole
    bind their IB beside one, redrawing the whole buffer),
  - a skipped hash with no commented draw at all (fail open),
  - a buffer with no `hash` at all.
- **Per-IB bindings are authoritative:** when a mod binds a diffuse per index
  buffer (`Resource\ZZMI\Diffuse = ref ...` inside a `TextureOverride` that
  matches one draw), a slice with no binding of its own is not painted with a
  sibling's atlas: a part the game still draws renders with the neutral
  material and the "no diffuse texture is shipped for this part" warning.
- **Menu-driven toggles:** a mod needs no `[Key*]` section to expose controls.
  When the mod assigns a flag from itself (`$jacket = 1 - $jacket`,
  `$color = $color + 1`) — the idiom the in-game slot menu uses — that flag is
  user-settable, so the preview offers it. Sunna's, Yuzuha's and Corin's menu
  mods went from 0 controls to 25, 12 and 17 this way. A flag the mod derives
  from game state is assigned a literal or another variable's value instead and
  stays hidden, which is what keeps internal guards (BelleStars' `$bear`) out of
  the panel even though they change geometry.
  Menu art is not a control signal: a `ps-t100` binding to a menu
  `ResourceMenuItem` texture is excluded from the fingerprint, so a flag that
  only swaps the in-game menu graphic earns no control, while one that gates a
  draw or a body colour map still does. The preview cannot show the menu itself.
- **Orientation:** positions are rotated from the game's Z-up to the viewer's
  Y-up when the bounding box shows a vertical Z extent, without touching the
  source buffers.
- **UV layouts:** stride and per-format sampling are probed; a buffer whose
  length does not divide by the vertex count is reported and the part is rendered
  without UVs instead of producing garbage coordinates.
- **Small props:** a real UV set is accepted even when it occupies a tiny strip
  of the atlas (a headband, a button, a cable). Decoding first asks for a set
  that sweeps the atlas (`span >= 0.05` on both axes) and only then retries with
  a much lower floor (`0.001`). This fixes parts such as Ye's `0YE发带` and
  Dialyn's `DialynCable`, whose UVs were previously discarded.

## Saving preview toggles

`save_preview_toggles(folderName, values)` writes the chosen values into the
loader's `d3dx_user.ini` (or `3DUser.ini`) so the game starts with the outfit
the preview shows. Each value goes under the namespace of the INI that declares
it — `$\mods\<folder>\<ini>\<variable> = <value>` — because the loader keys
persisted variables by that path, not by name alone. Only variables the mod
declares in `[Constants]` and only numeric values are written; anything else is
reported back in `missing` rather than invented. A variable two INIs of the same
mod both declare is reported in `ambiguous` and never written, because the
loader's namespace would be a guess. The file keeps its own line endings (the
loader writes CRLF) and every unrelated line, the first write keeps a
`d3dx_user.ini.bak` beside it, and a save that changes nothing leaves the file
untouched.

## Known limits

- Parts whose colour map exists only in the original game, or is produced by a
  runtime custom shader, stay neutral with an explicit warning. Confirmed cases:
  YiXuan coat/gourd, Yanagi hair/glass, Promeia cloak/hair, Pulchra `Body2`,
  Grace hair, Cissia `2A`/`2B`, Belle Stars hair, Belle Summer glasses, Nangong
  wings, and Ye `1YE变身头发` / `0YE透明布料`. Their geometry still renders
  correctly. No diffuse for these exists anywhere on disk; the game's own assets
  are encrypted `.blk` containers and are not read.
- The Trigger cape is a runtime-captured texture (`ResourceOGHairDiffuse`, no
  file on disk). It renders neutral with
  `"this draw uses a texture captured from the running game ... not stored on disk"`
  while the rest of the merged hair IB keeps its shipped third-party atlas.
- Nekomata body and legs ship no diffuse of their own, but another installed mod
  provides a matching `NekomataBodyADiffuse`; the cross-mod library borrows it
  and reports the borrow.
- A resource that is declared but whose file is absent is reported
  (`"<name> was not found on disk"` / `"<name> is missing"`) and never silently
  replaced with an unrelated atlas.
- Duplicate/overlapping source meshes are rendered as the INI draws them; the
  preview does not guess which of two intentionally stacked meshes to hide.
- A part whose position buffer is authored in a different frame than the rest of
  the mod is drawn as authored. `BelleSummer_Toggles`' sunglasses buffer spans
  Y 1.373-1.526 and Z -0.040-0.101 (Y-up) while its body buffer spans Z 0-1.412
  (Z-up), so those glasses sit beside the model instead of on the face. The game
  transforms that draw; the preview renders the bind pose and does not guess a
  rotation for it.
- Texture-only mods (e.g. `JiggleForgeShaderFix`) have no mesh and report that
  explicitly instead of building an empty scene.
- No visual/browser verification is available in the build environment; checks
  are numeric part/material diagnostics plus unit tests.

## Viewer rendering

The React viewer (`ModelViewer.tsx`) is deliberately faithful to the source
data:

- A part with a diffuse texture uses **white** as the material base colour, so
  the texture's own colours are shown unmodified. A part that ships no colour
  map is drawn in the darker neutral grey `#aeb5c0`, and the preview header
  counts them (`n no diffuse`, with the reason on hover) so a grey model does
  not read as a failed texture load.
- Textures use **repeat** wrapping on both axes because game shaders sample with
  wrap addressing; hair, cloth, and scaled surfaces legitimately use UVs outside
  `0..1`. Clamping them would smear the last texel across those triangles.
- Alpha-tested parts (`has_alpha`) use `alphaTest = 0.5` only when UVs exist, so
  opaque textures are not eroded.
- Meshes are rendered double-sided because ZZZ geometry is not consistently
  wound.
- Tone mapping is ACES filmic at exposure `0.72`; the exposure is lowered to
  compensate for removing the old grey material tint, which used to darken
  every textured part by roughly a third.
- Normal maps are intentionally **not** applied. The correct tangent-space
  green-channel convention varies between exports, and guessing it would produce
  visibly inverted relief. The map is not silently treated as a colour either;
  it is ignored and the diffuse is used.
- **Outline.** `PartOutline` draws a second, back-facing copy of every visible
  part pushed out along the vertex normals: the game's inked silhouette without a
  post-processing pass, and without touching the decoded mesh. It is on by
  default, and the preview header's `Outline` pill turns it off.
  - The push is applied in clip space, so the contour is a hairline measured in
    CSS pixels rather than a world-space shell that would swamp a small prop and
    vanish on a large one.
  - The shell samples the part's diffuse with the same `alphaTest` the surface
    uses, so a cutout mask (hair, lace) drops the shell's transparent texels too;
    otherwise the outline would trace each card's quad instead of the hair.
  - The shell is drawn with `depthFunc: LessDepth`, `depthWrite: false` and a
    small polygon offset, so it never z-fights the surface it shares depth with
    (visible through a mask hole or inside an open coat) and never occludes
    another part's rim.
  - Vertices whose normal points at the camera project to zero length; they are
    left in place rather than normalised, which would poison the triangle with
    NaN. These faces are already covered by the surface they back.
  - With `Wire` on, the surface writes depth only along its edges, so the filled
    shell would cover every gap between the wires and read as a solid ink
    silhouette. The shell is therefore **not drawn in wireframe mode**; the
    `Outline` preference keeps its value and the pill shows the shell as off
    while the wires are on.
- **Expanded stage.** The preview header's expand control promotes the preview
  to the whole window (the part chips and `$variable` toggles stay below it);
  the same control, `Escape`, or leaving the 3D tab docks it back. The stage is
  the same `Canvas` in the same DOM node, so expanding only resizes the
  renderer: the camera keeps its orbit, and the mesh is not rebuilt.
  - Expanded, the panel behind the stage is covered: the header, the controls
    below the stage and the pending-hotkey footer are marked `inert`, so they
    leave the tab order and the accessibility tree instead of staying focusable
    under the overlay.
  - The rename dialog is layered above the stage (`z-80` against the stage's
    `z-75`), so it stays usable while the preview fills the window.
  - A preview with no mesh parts cannot be expanded — there is nothing to fill
    the window with — so the docked "No mesh buffers found" placeholder stays.