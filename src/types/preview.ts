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
}
