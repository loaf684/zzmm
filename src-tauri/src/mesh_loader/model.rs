use serde::Serialize;

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
}
