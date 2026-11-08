/// ZZZ position strands: the game ships 40-byte vertices; other MI games and
/// merged mods use smaller ones. The first stride that divides the file wins.
const POSITION_STRIDES: &[usize] = &[40, 36, 32, 24, 20, 16, 12];
/// ZZZ texcoord buffers start with a half-float morph header, so the UV pair
/// lives at some offset inside each vertex (stride 36 → +4, stride 20 → +8);
/// other MI games put it at 0. Sentinel mods ship packed texcoords at stride
/// 28; smaller strides cover skinned exports.
const TEXCOORD_STRIDES: &[usize] = &[20, 36, 24, 28, 16, 32, 8, 40, 12];

const MAX_VERTICES: usize = 1_500_000;

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    bytes
        .get(offset..offset + 4)
        .map(|slice| f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// Little-endian IEEE 754 binary16 → f32.
fn read_hf32(bytes: &[u8], offset: usize) -> f32 {
    let Some(&lo) = bytes.get(offset) else {
        return 0.0;
    };
    let Some(&hi) = bytes.get(offset + 1) else {
        return 0.0;
    };
    let bits = u16::from_le_bytes([lo, hi]) as u32;

    let sign = (bits & 0x8000) << 16;
    let exponent = bits & 0x7C00;
    let mantissa = bits & 0x03FF;

    if exponent == 0x7C00 {
        // Infinity / NaN keeps NaN semantics in the f32 mapping.
        return f32::from_bits(sign | 0x7F80_0000 | (mantissa << 13));
    }

    if exponent == 0 {
        if mantissa == 0 {
            return f32::from_bits(sign);
        }
        let value = (mantissa as f32) * 2f32.powi(-24);
        return if bits & 0x8000 != 0 { -value } else { value };
    }

    let biased = ((exponent >> 10) as i32) - 15 + 127;
    f32::from_bits(sign | ((biased as u32) << 23) | (mantissa << 13))
}

/// How a texcoord buffer stores the UV pair: full `f32` pairs are standard,
/// but standalone ZZZ exports pack them as half floats.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UvDecode {
    F32,
    F16,
}

impl UvDecode {
    pub(crate) fn read(self, bytes: &[u8], offset: usize) -> f32 {
        match self {
            UvDecode::F32 => read_f32(bytes, offset),
            UvDecode::F16 => read_hf32(bytes, offset),
        }
    }

    /// Bytes per value: f16 pairs take 4 bytes where f32 pairs take 8.
    pub(crate) fn wide(self) -> bool {
        matches!(self, UvDecode::F32)
    }
}

pub(crate) const UV_DECODES: &[UvDecode] = &[UvDecode::F32, UvDecode::F16];

/// Spread of the middle half of the samples on each axis. A correct stride
/// yields a compact, character-sized point cloud; decoding a buffer at the
/// wrong stride interleaves UV/weight/normal fields and collapses the middle
/// half towards zero, which is what used to show up as a cloud of stray
/// triangles around the model.
fn robust_spans(bytes: &[u8], stride: usize, count: usize) -> Option<[f32; 3]> {
    const SAMPLES: usize = 2048;

    let step = (count / SAMPLES).max(1);
    let mut axes: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut sampled = 0usize;

    for index in (0..count).step_by(step) {
        let base = index * stride;
        if base + 12 > bytes.len() {
            break;
        }
        for (axis, values) in axes.iter_mut().enumerate() {
            values.push(read_f32(bytes, base + axis * 4));
        }
        sampled += 1;
    }

    if sampled < 4 {
        return None;
    }

    let mut spans = [0.0f32; 3];
    for (axis, values) in axes.iter_mut().enumerate() {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let low = values[values.len() / 4];
        let high = values[values.len() * 3 / 4];
        spans[axis] = high - low;
    }

    Some(spans)
}

/// True when a decoded stride looks like vertex positions: finite values, a
/// believable size, and movement on at least two axes.
fn plausible_positions(spans: &[f32; 3]) -> bool {
    let dominant = spans.iter().copied().fold(0.0f32, f32::max);
    let moving = spans.iter().filter(|span| **span > 0.01).count();

    dominant > 0.05 && dominant < 1000.0 && moving >= 2
}

/// Pick the position stride from the INI hint or the ZZZ defaults, preferring
/// the densest plausible decoding when the hint is missing or wrong.
pub fn position_layout(
    bytes: &[u8],
    hint: Option<usize>,
    warnings: &mut Vec<String>,
) -> Option<(usize, usize)> {
    let mut candidates: Vec<(usize, bool)> = Vec::new();

    if let Some(stride) = hint {
        candidates.push((stride, true));
    }
    for stride in POSITION_STRIDES {
        if Some(*stride) != hint {
            candidates.push((*stride, false));
        }
    }

    let mut first_divisor: Option<(usize, usize)> = None;
    let mut best: Option<(usize, usize, f32)> = None;

    for (stride, from_ini) in candidates {
        if stride < 12 || bytes.len() % stride != 0 {
            continue;
        }

        let count = bytes.len() / stride;
        if count < 3 || count > MAX_VERTICES {
            continue;
        }

        // The INI stride is what the game itself uses, so a hint that fits the
        // buffer is authoritative. Small props can span only a few
        // centimetres, and a plausibility threshold would wrongly reject them
        // and decode the padding as a giant point cloud.
        if from_ini {
            return Some((stride, count));
        }

        first_divisor.get_or_insert((stride, count));

        let Some(spans) = robust_spans(bytes, stride, count) else {
            continue;
        };
        if !plausible_positions(&spans) {
            continue;
        }

        let dominant = spans.iter().copied().fold(0.0f32, f32::max);
        if best.map_or(true, |(_, _, best_span)| dominant > best_span * 1.15) {
            best = Some((stride, count, dominant));
        }
    }

    if let Some((stride, count, _)) = best {
        match hint {
            Some(hint) => warnings.push(format!(
                "Position stride {hint} does not look like vertex data; assumed {stride}."
            )),
            None => warnings.push(format!(
                "No position stride in the INI; assumed ZZZ default {stride}."
            )),
        }
        return Some((stride, count));
    }

    if let Some((stride, count)) = first_divisor {
        match hint {
            Some(hint) => warnings.push(format!(
                "Position stride {hint} does not fit the buffer; assumed {stride}."
            )),
            None => warnings.push(format!(
                "No position stride in the INI; assumed ZZZ default {stride}."
            )),
        }
        return Some((stride, count));
    }

    None
}

pub fn read_positions(bytes: &[u8], stride: usize, count: usize) -> Vec<[f32; 3]> {
    (0..count)
        .map(|index| {
            let offset = index * stride;
            [
                read_f32(bytes, offset),
                read_f32(bytes, offset + 4),
                read_f32(bytes, offset + 8),
            ]
        })
        .collect()
}

/// UVs are accepted only when the stride yields exactly the vertex count of the
/// position buffer *and* the decoded values look like texture coordinates. The
/// plausibility check is what makes the stride/offset/format guess safe: morph
/// headers, normals or padding decode to denormals or out-of-range junk.
pub fn read_uvs(
    bytes: &[u8],
    hint: Option<usize>,
    vertex_count: usize,
) -> Option<(usize, Vec<[f32; 2]>)> {
    let mut strides: Vec<usize> = Vec::new();
    if let Some(stride) = hint {
        strides.push(stride);
    }
    for stride in TEXCOORD_STRIDES {
        if !strides.contains(stride) {
            strides.push(*stride);
        }
    }

    // Prefer a decode whose UVs clearly sweep the atlas. A small prop (a
    // headband, a button) legitimately occupies a tiny corner of the texture, so
    // if nothing sweeps on the first, strict pass retry with a much lower floor
    // before giving up and rendering the part without UVs.
    for &min_span in &[0.05f32, 0.001f32] {
        if let Some(found) = scan_uvs(bytes, &strides, vertex_count, min_span) {
            return Some(found);
        }
    }

    None
}

fn scan_uvs(
    bytes: &[u8],
    strides: &[usize],
    vertex_count: usize,
    min_span: f32,
) -> Option<(usize, Vec<[f32; 2]>)> {
    const UV_OFFSETS: &[usize] = &[4, 0, 8, 12];
    const SAMPLES: usize = 2048;

    for &stride in strides {
        if stride < 4 || bytes.len() % stride != 0 {
            continue;
        }
        if bytes.len() / stride != vertex_count {
            continue;
        }

        for &offset in UV_OFFSETS {
            for &decode in UV_DECODES {
                // Word size of one value: f16 halves take 2 bytes, f32 takes 4.
                let value_bytes = if decode.wide() { 4 } else { 2 };
                // Distance from u to v: f32 pairs sit 4 bytes apart, f16 halves
                // are adjacent.
                let v_gap = if decode.wide() { 4 } else { 2 };
                if offset + v_gap + value_bytes > stride {
                    continue;
                }

                let step = (vertex_count / SAMPLES).max(1);
                let scheduled = (vertex_count + step - 1) / step;
                let mut min_u = f32::INFINITY;
                let mut max_u = f32::NEG_INFINITY;
                let mut min_v = f32::INFINITY;
                let mut max_v = f32::NEG_INFINITY;
                let mut sampled = 0usize;

                for index in (0..vertex_count).step_by(step) {
                    let base = index * stride;
                    let u = decode.read(bytes, base + offset);
                    let v = decode.read(bytes, base + offset + v_gap);
                    // Textures can repeat a little, but real UVs stay in a
                    // small band; morph headers decode to ~1e-41 denormals and
                    // buffer positioning mistakes hit huge values.
                    if !u.is_finite() || !v.is_finite() || u.abs() > 8.0 || v.abs() > 8.0 {
                        break;
                    }
                    min_u = min_u.min(u);
                    max_u = max_u.max(u);
                    min_v = min_v.min(v);
                    max_v = max_v.max(v);
                    sampled += 1;
                }

                if sampled < scheduled {
                    // Some sample was rejected; try the next format or offset.
                    continue;
                }

                let span_u = max_u - min_u;
                let span_v = max_v - min_v;

                // Both axes must move for the data to be UVs; a lone spread is
                // a normal or padding that happens to land in range.
                if span_u >= min_span && span_v >= min_span {
                    let uvs = (0..vertex_count)
                        .map(|index| {
                            let base = index * stride;
                            [
                                decode.read(bytes, base + offset),
                                decode.read(bytes, base + offset + v_gap),
                            ]
                        })
                        .collect();
                    return Some((stride, uvs));
                }
            }
        }
    }

    None
}

pub enum IndexLayout {
    U16,
    U32,
}

impl IndexLayout {
    pub fn from_format(format: Option<&str>) -> Option<Self> {
        let format = format?.to_ascii_uppercase();
        if format.contains("R32_UINT") {
            Some(IndexLayout::U32)
        } else if format.contains("R16_UINT") {
            Some(IndexLayout::U16)
        } else {
            None
        }
    }

    pub fn width(&self) -> usize {
        match self {
            IndexLayout::U16 => 2,
            IndexLayout::U32 => 4,
        }
    }
}

fn parse_indices(bytes: &[u8], layout: &IndexLayout, vertex_count: usize) -> Option<Vec<u32>> {
    let stride = layout.width();
    if bytes.is_empty() || bytes.len() % stride != 0 {
        return None;
    }

    let indices: Vec<u32> = match layout {
        IndexLayout::U16 => bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]) as u32)
            .collect(),
        IndexLayout::U32 => bytes
            .chunks_exact(4)
            .map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
            .collect(),
    };

    if indices.len() < 3 || indices.len() % 3 != 0 {
        return None;
    }
    if indices.iter().any(|index| *index as usize >= vertex_count) {
        return None;
    }

    Some(indices)
}

/// Resolve the index buffer. An explicit `DXGI_FORMAT_R16_UINT` / `R32_UINT`
/// wins; otherwise the layout is inferred and validated against the vertex
/// count, preferring 16-bit because that is ZZZ's default.
pub fn decode_indices(
    bytes: &[u8],
    vertex_count: usize,
    format: Option<&str>,
    warnings: &mut Vec<String>,
) -> Option<Vec<u32>> {
    if let Some(layout) = IndexLayout::from_format(format) {
        if let Some(indices) = parse_indices(bytes, &layout, vertex_count) {
            return Some(indices);
        }
        warnings.push(format!(
            "Index buffer does not match {}; trying the other width.",
            match layout {
                IndexLayout::U16 => "DXGI_FORMAT_R16_UINT",
                IndexLayout::U32 => "DXGI_FORMAT_R32_UINT",
            }
        ));
    }

    let mut order = vec![IndexLayout::U16, IndexLayout::U32];
    // Only guess 16-bit first when the mesh fits in 16 bits at all.
    if vertex_count > u16::MAX as usize {
        order = vec![IndexLayout::U32, IndexLayout::U16];
    }

    for layout in order {
        if let Some(indices) = parse_indices(bytes, &layout, vertex_count) {
            if format.is_none() {
                warnings.push(format!(
                    "Index format missing in the INI; inferred {}-bit indices.",
                    layout.width() * 8
                ));
            }
            return Some(indices);
        }
    }

    None
}

/// Sequential triangles, used only when a mod ships no index buffer.
pub fn sequential_indices(vertex_count: usize) -> Vec<u32> {
    (0..vertex_count as u32).collect()
}

/// Area-weighted smooth normals. ZZZ meshes do not carry usable normals in the
/// position buffer, so they are derived from the triangles instead.
pub fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<f32> {
    let mut normals = vec![0.0f32; positions.len() * 3];

    for triangle in indices.chunks_exact(3) {
        let (a, b, c) = (
            triangle[0] as usize,
            triangle[1] as usize,
            triangle[2] as usize,
        );

        if a >= positions.len() || b >= positions.len() || c >= positions.len() {
            continue;
        }

        let ab = [
            positions[b][0] - positions[a][0],
            positions[b][1] - positions[a][1],
            positions[b][2] - positions[a][2],
        ];
        let ac = [
            positions[c][0] - positions[a][0],
            positions[c][1] - positions[a][1],
            positions[c][2] - positions[a][2],
        ];

        let normal = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];

        for vertex in [a, b, c] {
            normals[vertex * 3] += normal[0];
            normals[vertex * 3 + 1] += normal[1];
            normals[vertex * 3 + 2] += normal[2];
        }
    }

    for vertex in 0..positions.len() {
        let x = normals[vertex * 3];
        let y = normals[vertex * 3 + 1];
        let z = normals[vertex * 3 + 2];
        let length = (x * x + y * y + z * z).sqrt();

        if length > 1e-8 {
            normals[vertex * 3] = x / length;
            normals[vertex * 3 + 1] = y / length;
            normals[vertex * 3 + 2] = z / length;
        } else {
            normals[vertex * 3] = 0.0;
            normals[vertex * 3 + 1] = 0.0;
            normals[vertex * 3 + 2] = 1.0;
        }
    }

    normals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    #[test]
    fn picks_the_stride_that_divides_the_buffer() {
        let mut warnings = Vec::new();
        // 3 vertices at ZZZ's 40-byte stride.
        let bytes = vec![0u8; 3 * 40];

        let (stride, count) = position_layout(&bytes, None, &mut warnings).expect("layout");
        assert_eq!(stride, 40);
        assert_eq!(count, 3);
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("default 40")));

        // An INI hint that fits is trusted without a warning.
        let mut warnings = Vec::new();
        let (stride, _) = position_layout(&bytes, Some(40), &mut warnings).expect("layout");
        assert_eq!(stride, 40);
        assert!(warnings.is_empty()); // A buffer that only fits 12-byte vertices falls back to stride 12.
        let tight = vec![0u8; 7 * 12];
        let (stride, count) = position_layout(&tight, Some(40), &mut warnings).expect("layout");
        assert_eq!(stride, 12);
        assert_eq!(count, 7);
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("assumed 12")));
    }

    #[test]
    fn trusts_a_fitting_stride_for_a_small_prop() {
        // A 5 cm wrist prop: 8 vertices at the real 40-byte stride. Its tiny
        // extent must not make the decoder prefer the 32-byte interleaving.
        let mut data = Vec::new();
        for index in 0..8 {
            let offset = index as f32 * 0.005;
            data.extend(floats(&[0.10 + offset, -0.12 + offset, 1.05 + offset]));
            data.extend(vec![0u8; 28]);
        }

        let mut warnings = Vec::new();
        let (stride, count) = position_layout(&data, Some(40), &mut warnings).expect("layout");
        assert_eq!((stride, count), (40, 8));
        assert!(warnings.is_empty());
    }

    #[test]
    fn rejects_a_stride_that_only_divides_the_buffer() {
        // 15 vertices at the real 24-byte stride: 360 bytes also divides by 40
        // and 36, but those decodings interleave zero padding with the xyz
        // values and collapse the point cloud.
        let mut data = Vec::new();
        for index in 0..15 {
            let x = -0.4 + index as f32 * 0.05;
            let y = 0.05 + index as f32 * 0.01;
            let z = index as f32 * 0.1;
            data.extend(floats(&[x, y, z]));
            data.extend(vec![0u8; 12]);
        }

        let mut warnings = Vec::new();
        let (stride, count) = position_layout(&data, None, &mut warnings).expect("layout");
        assert_eq!(stride, 24);
        assert_eq!(count, 15);
    }

    #[test]
    fn reads_positions_at_offset_zero() {
        let mut data = Vec::new();
        data.extend(floats(&[1.0, 2.0, 3.0]));
        data.extend(vec![0u8; 28]); // rest of a 40-byte vertex
        data.extend(floats(&[4.0, 5.0, 6.0]));
        data.extend(vec![0u8; 28]);

        let positions = read_positions(&data, 40, 2);
        assert_eq!(positions[0], [1.0, 2.0, 3.0]);
        assert_eq!(positions[1], [4.0, 5.0, 6.0]);
    }

    #[test]
    fn reads_uvs_only_when_the_vertex_count_matches() {
        let mut data = Vec::new();
        for index in 0..4 {
            // Varying UVs on a 20-byte vertex: header + uv + padding.
            let u = 0.1 + index as f32 * 0.2;
            data.extend(floats(&[u, u / 2.0]));
            data.extend(vec![0u8; 12]); // rest of the 20-byte texcoord vertex
        }

        let (stride, uvs) = read_uvs(&data, None, 4).expect("uvs");
        assert_eq!(stride, 20);
        assert_eq!(uvs[0], [0.1, 0.05]);
        // 80 bytes cannot be split into 6 UV vertices at any candidate stride.
        assert!(read_uvs(&data, None, 6).is_none());
    }

    #[test]
    fn reads_uvs_for_a_small_prop_with_a_tiny_atlas_region() {
        // A headband maps to a thin strip of the atlas: both UV spans sit well
        // under the 0.05 "sweeps the atlas" floor, but the 28-byte layout the
        // INI declares is real and must still yield coordinates.
        let mut data = Vec::new();
        for index in 0..8 {
            let u = 0.993 + index as f32 * 0.0008; // span ~0.0056
            let v = 0.010 + index as f32 * 0.005; // span ~0.035
            data.extend(floats(&[0.0, u, v, 0.0]));
            data.extend(vec![0u8; 12]); // rest of the 28-byte texcoord vertex
        }

        let (stride, uvs) = read_uvs(&data, Some(28), 8).expect("small-prop uvs");
        assert_eq!(stride, 28);
        assert!((uvs[0][0] - 0.993).abs() < 1e-5);
        assert!((uvs[0][1] - 0.010).abs() < 1e-5);
    }

    #[test]
    fn reads_uvs_behind_a_morph_header() {
        let mut data = Vec::new();
        // ZZZ stores a half-float morph header before the UVs; as f32 it is a
        // denormal that must be rejected as UV data.
        let header: [u8; 4] = [0x80, 0x80, 0x00, 0x00];
        for index in 0..4 {
            let u = 0.2 + index as f32 * 0.15;
            data.extend_from_slice(&header);
            data.extend(floats(&[u, u / 3.0]));
            data.extend(vec![0u8; 24]); // rest of the 36-byte texcoord vertex
        }

        let (stride, uvs) = read_uvs(&data, Some(36), 4).expect("header uvs");
        assert_eq!(stride, 36);
        assert_eq!(uvs[2], [0.5, 0.5f32 / 3.0]);
    }

    #[test]
    fn reads_half_float_uv_pairs() {
        let mut data = Vec::new();
        // AnbyS0-style ZZZ export: f16 morph marker then f16 UV pairs on a
        // 20-byte vertex.
        let header: [u8; 4] = [0x80, 0x80, 0x00, 0xFF];
        // (0.25, 0.125), (0.5, 0.25), (0.625, 0.3125), (0.375, 0.1875)
        let halves: [u16; 8] = [
            0x3400, 0x3000, 0x3800, 0x3400, 0x3900, 0x3500, 0x3600, 0x3200,
        ];
        for index in 0..4 {
            data.extend_from_slice(&header);
            data.extend_from_slice(&halves[index * 2].to_le_bytes());
            data.extend_from_slice(&halves[index * 2 + 1].to_le_bytes());
            data.extend(vec![0u8; 12]); // rest of the 20-byte texcoord vertex
        }

        let (stride, uvs) = read_uvs(&data, Some(20), 4).expect("f16 uvs");
        assert_eq!(stride, 20);
        assert_eq!(uvs[0], [0.25, 0.125]);
        assert_eq!(uvs[2], [0.625, 0.3125]);
    }

    #[test]
    fn decodes_both_index_widths_and_rejects_out_of_range() {
        let mut warnings = Vec::new();
        let u16_indices: Vec<u8> = [0u16, 1, 2]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();

        let decoded = decode_indices(&u16_indices, 3, Some("DXGI_FORMAT_R16_UINT"), &mut warnings)
            .expect("u16");
        assert_eq!(decoded, vec![0, 1, 2]);

        let u32_indices: Vec<u8> = [0u32, 1, 2]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let decoded = decode_indices(&u32_indices, 3, Some("DXGI_FORMAT_R32_UINT"), &mut warnings)
            .expect("u32");
        assert_eq!(decoded, vec![0, 1, 2]);

        // Index 9 does not exist in a 3-vertex mesh.
        let broken: Vec<u8> = [0u32, 9, 2]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert!(decode_indices(&broken, 3, Some("DXGI_FORMAT_R32_UINT"), &mut warnings).is_none());
    }

    #[test]
    fn infers_16_bit_indices_when_the_format_is_missing() {
        let mut warnings = Vec::new();
        let indices: Vec<u8> = [0u16, 1, 2]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();

        let decoded = decode_indices(&indices, 3, None, &mut warnings).expect("inferred");
        assert_eq!(decoded, vec![0, 1, 2]);
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("inferred 16-bit")));
    }

    #[test]
    fn normals_point_out_of_the_triangle_plane() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals = compute_normals(&positions, &[0, 1, 2]);

        assert_eq!(normals.len(), 9);
        for vertex in 0..3 {
            let z = normals[vertex * 3 + 2];
            assert!((z.abs() - 1.0).abs() < 1e-6, "normal should be ±Z");
        }
    }
}
