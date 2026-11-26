use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshPart {
    /// Component name derived from the buffer file name (e.g. `Component1`).
    pub name: String,
    pub vertex_count: u32,
    pub triangle_count: u32,
    /// base64 of little-endian f32 xyz triples.
    pub positions: String,
    /// base64 of little-endian f32 xyz triples (computed, area-weighted).
    pub normals: String,
    /// base64 of little-endian f32 uv pairs (zeros when the part has no UVs).
    pub uvs: String,
    /// base64 of little-endian u32 indices.
    pub indices: String,
    /// `data:image/png;base64,…` for the matched DDS, when one decoded.
    pub texture: Option<String>,
    pub texture_error: Option<String>,
    /// File name of the diffuse texture that was matched, if any.
    pub material: Option<String>,
    /// True when the matched texture carries a non-opaque alpha channel. The
    /// viewer turns this into an alpha test so cutout hair/cloth renders the
    /// way the game's masked shader draws it instead of as opaque cards.
    pub has_alpha: bool,
    pub source_files: Vec<String>,
}

/// One mod toggle the preview can change: a `[Key*]`-bound `$variable` that
/// gates a resource binding, and the values it is known to take. Only variables
/// whose value actually changes the rendered mesh are reported, so the viewer
/// never shows a control that does nothing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewToggle {
    /// Lower-cased variable name without the `$` (e.g. `swapkey`).
    pub name: String,
    /// Human-readable label derived from the variable name.
    pub label: String,
    /// Value the preview is currently rendered with (a user override when one
    /// was sent, otherwise the INI default / saved `d3dx_user.ini` value).
    pub value: String,
    /// Every value the INI is known to compare or assign this variable,
    /// numeric and sorted. Always at least two entries when a toggle is shown.
    pub options: Vec<String>,
}

/// A user-chosen toggle value sent from the viewer. Overrides the INI default
/// and the saved `d3dx_user.ini` state for the lifetime of one preview build.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewToggleValue {
    /// Variable name without the `$`; compared case-insensitively.
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshPreview {
    pub mod_folder: String,
    pub mod_name: String,
    /// INI that supplied the `[Resource*]` layout, if one was used.
    pub ini_file: Option<String>,
    pub parts: Vec<MeshPart>,
    pub warnings: Vec<String>,
    /// True when ZZZ's default strides/index format had to fill in gaps.
    pub used_default_layout: bool,
    /// True when the preview was filtered by the in-game toggle state (INI
    /// defaults or the values saved in 3DMigoto's `d3dx_user.ini`).
    pub toggles_applied: bool,
    /// Mod toggles the viewer may change, each already proven to alter the
    /// rendered mesh. Empty when no `$variable` gates a binding.
    pub toggles: Vec<PreviewToggle>,
}
