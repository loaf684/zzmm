/** Types mirroring `src-tauri/src/mesh_loader/model.rs`. */

export interface MeshPart {
  name: string;
  vertexCount: number;
  triangleCount: number;
  /** base64 of little-endian f32 triples (x, y, z). */
  positions: string;
  /** base64 of little-endian f32 triples (nx, ny, nz). */
  normals: string;
  /** base64 of little-endian f32 pairs (u, v); zeros when the part has no UVs. */
  uvs: string;
  /** base64 of little-endian u32 indices. */
  indices: string;
  /** `data:image/png;base64,…` for the matched DDS, if it decoded. */
  texture: string | null;
  textureError: string | null;
  /** Diffuse texture file name, when one was matched. */
  material: string | null;
  /** True when the texture has a non-opaque alpha channel (cutout hair/cloth). */
  hasAlpha: boolean;
  sourceFiles: string[];
}

export interface MeshPreview {
  modFolder: string;
  modName: string;
  /** INI the resource layout came from, if any. */
  iniFile: string | null;
  parts: MeshPart[];
  warnings: string[];
  /** True when ZZZ's default strides/format had to stand in for the INI. */
  usedDefaultLayout: boolean;
  /** True when alternative variants were hidden from the saved toggle state. */
  togglesApplied: boolean;
  /** Mod toggles the viewer may change; only ones proven to alter the mesh. */
  toggles: PreviewToggle[];
}

/** One `$variable` that gates a binding, with the values the INI gives it. */
export interface PreviewToggle {
  /** Lower-cased variable name without the `$`. */
  name: string;
  /** Display label derived from the variable name. */
  label: string;
  /** Value the preview is currently rendered with. */
  value: string;
  /** Known numeric values, sorted; always at least two entries. */
  options: string[];
}

/**
 * What saving the preview choices wrote. `missing` are toggles this mod does
 * not declare (or whose value is not numeric); `ambiguous` are declared by more
 * than one INI, so the loader namespace cannot be decided without guessing.
 */
export interface PreviewToggleSave {
  /** d3dx_user.ini (or 3DUser.ini) that was written. */
  file: string;
  /** Backup kept on the first write of a session. */
  backup: string | null;
  written: string[];
  missing: string[];
  ambiguous: string[];
}
