/// Texture layouts ZZMM understands. 3DMigoto mods are almost always BC7,
/// BC1 or BC3; uncompressed DDS files come from hand-made texture swaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdsFormat {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6,
    Bc7,
    Rgba8,
    Bgra8,
    /// Uncompressed with explicit channel masks (e.g. BGRX).
    Masked {
        bits: u32,
        red: u32,
        green: u32,
        blue: u32,
        alpha: u32,
    },
}

impl DdsFormat {
    /// Bytes per 4x4 block, or per pixel for uncompressed formats.
    pub fn block_bytes(self) -> usize {
        match self {
            DdsFormat::Bc1 | DdsFormat::Bc4 => 8,
            DdsFormat::Bc2 | DdsFormat::Bc3 | DdsFormat::Bc5 | DdsFormat::Bc6 | DdsFormat::Bc7 => {
                16
            }
            DdsFormat::Rgba8 | DdsFormat::Bgra8 => 16,
            DdsFormat::Masked { bits, .. } => (bits as usize / 8) * 4,
        }
    }

    pub fn is_compressed(self) -> bool {
        !matches!(
            self,
            DdsFormat::Rgba8 | DdsFormat::Bgra8 | DdsFormat::Masked { .. }
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DdsHeader {
    pub width: usize,
    pub height: usize,
    pub format: DdsFormat,
    pub data_offset: usize,
}

const DDS_MAGIC: &[u8; 4] = b"DDS ";
const HEADER_SIZE: usize = 124;
const DX10_HEADER_SIZE: usize = 20;

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn dxgi_format(value: u32) -> Option<DdsFormat> {
    match value {
        // BC1
        71 | 72 => Some(DdsFormat::Bc1),
        // BC2
        74 | 75 => Some(DdsFormat::Bc2),
        // BC3
        77 | 78 => Some(DdsFormat::Bc3),
        // BC4
        80 | 81 => Some(DdsFormat::Bc4),
        // BC5
        83 | 84 => Some(DdsFormat::Bc5),
        // BC6H
        95 | 96 => Some(DdsFormat::Bc6),
        // BC7
        98 | 99 => Some(DdsFormat::Bc7),
        // R8G8B8A8 / B8G8R8A8 unordered and sRGB variants
        27 | 28 | 29 => Some(DdsFormat::Rgba8),
        87 | 88 | 91 => Some(DdsFormat::Bgra8),
        _ => None,
    }
}

fn fourcc_format(fourcc: &[u8]) -> Option<DdsFormat> {
    match fourcc {
        b"DXT1" => Some(DdsFormat::Bc1),
        b"DXT2" | b"DXT3" => Some(DdsFormat::Bc2),
        b"DXT4" | b"DXT5" => Some(DdsFormat::Bc3),
        b"ATI1" | b"BC4U" => Some(DdsFormat::Bc4),
        b"ATI2" | b"BC5U" => Some(DdsFormat::Bc5),
        _ => None,
    }
}

/// Parse the 128-byte DDS header (plus DX10 header when present).
pub fn parse(bytes: &[u8]) -> Result<DdsHeader, String> {
    if bytes.len() < 128 {
        return Err("File is too small to be a DDS texture.".to_string());
    }
    if &bytes[0..4] != DDS_MAGIC {
        return Err("Not a DDS file (missing DDS signature).".to_string());
    }

    let header_size = u32_at(bytes, 4).unwrap_or(0);
    if header_size != HEADER_SIZE as u32 {
        return Err(format!("Unsupported DDS header size ({header_size})."));
    }

    let height = u32_at(bytes, 12).unwrap_or(0) as usize;
    let width = u32_at(bytes, 16).unwrap_or(0) as usize;

    if width == 0 || height == 0 {
        return Err("DDS texture has no pixels.".to_string());
    }
    if width > 8192 || height > 8192 {
        return Err(format!("DDS texture is too large ({width}x{height})."));
    }

    let fourcc = bytes.get(84..88).unwrap_or(b"");
    let pixel_bits = u32_at(bytes, 88).unwrap_or(0);
    let red_mask = u32_at(bytes, 92).unwrap_or(0);
    let green_mask = u32_at(bytes, 96).unwrap_or(0);
    let blue_mask = u32_at(bytes, 100).unwrap_or(0);
    let alpha_mask = u32_at(bytes, 104).unwrap_or(0);

    let (format, data_offset) = if fourcc == b"DX10" {
        let dxgi = u32_at(bytes, 128).ok_or_else(|| "DDS DX10 header is truncated.".to_string())?;
        let format = dxgi_format(dxgi).ok_or_else(|| {
            format!("Unsupported DXGI format {dxgi} (BC1/BC3/BC5/BC7 and RGBA8 are supported).")
        })?;
        (format, 128 + DX10_HEADER_SIZE)
    } else if let Some(format) = fourcc_format(fourcc) {
        (format, 128)
    } else if pixel_bits == 32 || pixel_bits == 24 {
        let format = if red_mask == 0x0000_00FF && blue_mask == 0x00FF_0000 {
            if alpha_mask == 0xFF00_0000 {
                DdsFormat::Rgba8
            } else {
                DdsFormat::Masked {
                    bits: pixel_bits,
                    red: red_mask,
                    green: green_mask,
                    blue: blue_mask,
                    alpha: alpha_mask,
                }
            }
        } else if blue_mask == 0x0000_00FF && red_mask == 0x00FF_0000 {
            DdsFormat::Bgra8
        } else {
            DdsFormat::Masked {
                bits: pixel_bits,
                red: red_mask,
                green: green_mask,
                blue: blue_mask,
                alpha: alpha_mask,
            }
        };
        (format, 128)
    } else {
        return Err(format!(
            "Unsupported DDS pixel format (fourCC {:?}, {pixel_bits}-bit).",
            String::from_utf8_lossy(fourcc)
        ));
    };

    if bytes.len() <= data_offset {
        return Err("DDS file contains no pixel data.".to_string());
    }

    Ok(DdsHeader {
        width,
        height,
        format,
        data_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(fourcc: &[u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(DDS_MAGIC);
        bytes[4..8].copy_from_slice(&124u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&64u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&128u32.to_le_bytes());
        bytes[84..88].copy_from_slice(fourcc);
        bytes
    }

    #[test]
    fn parses_legacy_dxt_fourcc() {
        let mut bytes = header(b"DXT5");
        bytes.extend(vec![0u8; 16]);

        let parsed = parse(&bytes).expect("valid dds");
        assert_eq!(parsed.format, DdsFormat::Bc3);
        assert_eq!((parsed.width, parsed.height), (128, 64));
        assert_eq!(parsed.data_offset, 128);
    }

    #[test]
    fn parses_dx10_bc7() {
        let mut bytes = header(b"DX10");
        bytes.extend(98u32.to_le_bytes()); // DXGI_FORMAT_BC7_UNORM
        bytes.extend(vec![0u8; DX10_HEADER_SIZE - 4]);
        bytes.extend(vec![0u8; 16]);

        let parsed = parse(&bytes).expect("valid dds");
        assert_eq!(parsed.format, DdsFormat::Bc7);
        assert_eq!(parsed.data_offset, 148);
    }

    #[test]
    fn rejects_non_dds_and_unknown_formats() {
        assert!(parse(b"not a dds file at all").is_err());

        let mut bytes = header(b"ABCD");
        bytes.extend(vec![0u8; 16]);
        assert!(parse(&bytes).is_err());
    }

    #[test]
    fn detects_uncompressed_layouts() {
        let mut bytes = header(&[0, 0, 0, 0]);
        bytes[88..92].copy_from_slice(&32u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
        bytes[104..108].copy_from_slice(&0xFF00_0000u32.to_le_bytes());
        bytes.extend(vec![0u8; 64]);

        let parsed = parse(&bytes).expect("valid dds");
        assert_eq!(parsed.format, DdsFormat::Bgra8);
    }
}
