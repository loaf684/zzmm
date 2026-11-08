//! Texture decoding for the 3D preview.
//!
//! 3DMigoto mods ship DDS textures, almost always BC7 (ZZZ) or BC1/BC3. The
//! decoder is pure Rust, and previews are downscaled and PNG-encoded so the
//! webview never has to deal with block-compressed data.

pub mod dds;

use std::fs;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::error::{AppError, AppResult};
use crate::texture::dds::{DdsFormat, DdsHeader};

const MAX_TEXTURE_BYTES: u64 = 96 * 1024 * 1024;
/// Preview textures are capped so a 4K BC7 texture does not cost several MB of
/// base64 over the IPC bridge.
const PREVIEW_MAX_SIDE: usize = 1024;

#[derive(Debug, Clone)]
pub struct DecodedTexture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Decode the base mip of a DDS file to RGBA8.
pub fn decode_dds(bytes: &[u8]) -> AppResult<DecodedTexture> {
    let header = dds::parse(bytes).map_err(AppError::Texture)?;
    let DdsHeader {
        width,
        height,
        format,
        data_offset,
        ..
    } = header;

    let data = bytes
        .get(data_offset..)
        .ok_or_else(|| AppError::Texture("DDS file contains no pixel data.".to_string()))?;

    if format.is_compressed() {
        let blocks_x = width.div_ceil(4);
        let blocks_y = height.div_ceil(4);
        let required = blocks_x * blocks_y * format.block_bytes();

        if data.len() < required {
            return Err(AppError::Texture(format!(
                "DDS data is truncated ({} of {required} bytes).",
                data.len()
            )));
        }

        let mut pixels = vec![0u32; width * height];
        let source = &data[..required];

        let result = match format {
            DdsFormat::Bc1 => texture2ddecoder::decode_bc1(source, width, height, &mut pixels),
            DdsFormat::Bc2 => texture2ddecoder::decode_bc2(source, width, height, &mut pixels),
            DdsFormat::Bc3 => texture2ddecoder::decode_bc3(source, width, height, &mut pixels),
            DdsFormat::Bc4 => texture2ddecoder::decode_bc4(source, width, height, &mut pixels),
            DdsFormat::Bc5 => texture2ddecoder::decode_bc5(source, width, height, &mut pixels),
            DdsFormat::Bc6 => {
                texture2ddecoder::decode_bc6_unsigned(source, width, height, &mut pixels)
            }
            DdsFormat::Bc7 => texture2ddecoder::decode_bc7(source, width, height, &mut pixels),
            _ => {
                return Err(AppError::Texture(
                    "Unsupported compressed DDS format.".to_string(),
                ))
            }
        };

        result.map_err(|error| AppError::Texture(error.to_string()))?;

        // texture2ddecoder stores pixels as 0xAARRGGBB.
        let mut rgba = Vec::with_capacity(width * height * 4);
        for pixel in pixels {
            rgba.push(((pixel >> 16) & 0xFF) as u8);
            rgba.push(((pixel >> 8) & 0xFF) as u8);
            rgba.push((pixel & 0xFF) as u8);
            rgba.push(((pixel >> 24) & 0xFF) as u8);
        }

        return Ok(DecodedTexture {
            width,
            height,
            rgba,
        });
    }

    let pixel_bytes = match format {
        DdsFormat::Rgba8 | DdsFormat::Bgra8 => 4,
        DdsFormat::Masked { bits, .. } => (bits as usize / 8).max(1),
        _ => 4,
    };
    let required = width * height * pixel_bytes;

    if data.len() < required {
        return Err(AppError::Texture(format!(
            "DDS data is truncated ({} of {required} bytes).",
            data.len()
        )));
    }

    let mut rgba = vec![0u8; width * height * 4];

    for index in 0..width * height {
        let offset = index * pixel_bytes;
        let pixel = &data[offset..offset + pixel_bytes];

        let (red, green, blue, alpha) = match format {
            DdsFormat::Rgba8 => (pixel[0], pixel[1], pixel[2], pixel[3]),
            DdsFormat::Bgra8 => (pixel[2], pixel[1], pixel[0], 0xFF),
            DdsFormat::Masked {
                bits,
                red,
                green,
                blue,
                alpha,
            } => {
                let value = if bits == 24 {
                    u32::from_le_bytes([pixel[0], pixel[1], pixel[2], 0])
                } else {
                    u32::from_le_bytes([
                        *pixel.first().unwrap_or(&0),
                        *pixel.get(1).unwrap_or(&0),
                        *pixel.get(2).unwrap_or(&0),
                        *pixel.get(3).unwrap_or(&0),
                    ])
                };

                (
                    extract_channel(value, red),
                    extract_channel(value, green),
                    extract_channel(value, blue),
                    if alpha == 0 {
                        0xFF
                    } else {
                        extract_channel(value, alpha)
                    },
                )
            }
            _ => (0, 0, 0, 0xFF),
        };

        rgba[index * 4] = red;
        rgba[index * 4 + 1] = green;
        rgba[index * 4 + 2] = blue;
        rgba[index * 4 + 3] = alpha;
    }

    Ok(DecodedTexture {
        width,
        height,
        rgba,
    })
}

/// Pull one channel out of a packed pixel using its DDS mask.
fn extract_channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }

    let shift = mask.trailing_zeros();
    let bits = mask.count_ones();
    let raw = (value & mask) >> shift;

    if bits >= 8 {
        return (raw >> (bits - 8)) as u8;
    }

    let max = (1u32 << bits) - 1;
    ((raw * 255 + max / 2) / max) as u8
}

/// Box-filter downscale; texture previews do not need full resolution.
fn downscale(width: usize, height: usize, rgba: &[u8], max_side: usize) -> (usize, usize, Vec<u8>) {
    if width <= max_side && height <= max_side {
        return (width, height, rgba.to_vec());
    }

    let scale = max_side as f32 / width.max(height) as f32;
    let target_width = ((width as f32 * scale).round() as usize).max(1);
    let target_height = ((height as f32 * scale).round() as usize).max(1);
    let x_ratio = width as f32 / target_width as f32;
    let y_ratio = height as f32 / target_height as f32;

    let mut out = vec![0u8; target_width * target_height * 4];

    for target_y in 0..target_height {
        let y_start = (target_y as f32 * y_ratio).floor() as usize;
        let y_end = (((target_y + 1) as f32 * y_ratio).ceil() as usize).min(height);

        for target_x in 0..target_width {
            let x_start = (target_x as f32 * x_ratio).floor() as usize;
            let x_end = (((target_x + 1) as f32 * x_ratio).ceil() as usize).min(width);

            let mut sums = [0u32; 4];
            let mut samples = 0u32;

            for y in y_start..y_end.max(y_start + 1) {
                for x in x_start..x_end.max(x_start + 1) {
                    let offset = (y.min(height - 1) * width + x.min(width - 1)) * 4;
                    for channel in 0..4 {
                        sums[channel] += rgba[offset + channel] as u32;
                    }
                    samples += 1;
                }
            }

            let samples = samples.max(1);
            let target = (target_y * target_width + target_x) * 4;
            for channel in 0..4 {
                out[target + channel] = (sums[channel] / samples) as u8;
            }
        }
    }

    (target_width, target_height, out)
}

fn encode_png(width: usize, height: usize, rgba: &[u8]) -> AppResult<Vec<u8>> {
    let mut buffer = Vec::new();

    {
        let mut encoder = png::Encoder::new(&mut buffer, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|error| AppError::Texture(error.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| AppError::Texture(error.to_string()))?;
    }

    Ok(buffer)
}

/// A preview texture plus the properties the viewer needs to render it the way
/// the game does.
#[derive(Debug, Clone)]
pub struct PreviewTexture {
    /// `data:image/png;base64,...`, ready for three.js.
    pub data_url: String,
    /// True when any pixel is meaningfully transparent. Cutout hair, leaves and
    /// cloth rely on this; rendering them opaque produces the solid "cards"
    /// that read as overlapping geometry.
    pub has_alpha: bool,
}

/// True when the RGBA buffer has at least one partially transparent pixel.
/// Fully-opaque textures (alpha 255 everywhere) stay in the opaque render pass.
fn uses_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|pixel| pixel[3] < 245)
}

/// Decode a DDS file to a preview texture, keeping the alpha usage flag.
pub fn load_preview_texture(path: &Path) -> AppResult<PreviewTexture> {
    let metadata = fs::metadata(path)
        .map_err(|error| AppError::Texture(format!("{}: {error}", path.display())))?;

    if metadata.len() > MAX_TEXTURE_BYTES {
        return Err(AppError::Texture(format!(
            "{} is larger than {} MB.",
            path.display(),
            MAX_TEXTURE_BYTES / (1024 * 1024)
        )));
    }

    let bytes = fs::read(path)
        .map_err(|error| AppError::Texture(format!("{}: {error}", path.display())))?;
    let decoded = decode_dds(&bytes)?;
    let (width, height, rgba) = downscale(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        PREVIEW_MAX_SIDE,
    );
    let has_alpha = uses_alpha(&rgba);
    let png = encode_png(width, height, &rgba)?;

    Ok(PreviewTexture {
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(png)),
        has_alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bc1_block(color0: u16, color1: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&color0.to_le_bytes());
        bytes.extend_from_slice(&color1.to_le_bytes());
        // All 16 pixels use color0.
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    fn dds_with(format_bytes: [u8; 4], width: usize, height: usize, body: Vec<u8>) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&(height as u32).to_le_bytes());
        bytes[16..20].copy_from_slice(&(width as u32).to_le_bytes());
        bytes[84..88].copy_from_slice(&format_bytes);
        bytes.extend(body);
        bytes
    }

    #[test]
    fn decodes_bc1_blocks_to_rgba() {
        // 0xF800 is pure red in RGB565, 0x001F pure blue.
        let dds = dds_with(*b"DXT1", 4, 4, bc1_block(0xF800, 0x001F));
        let decoded = decode_dds(&dds).expect("decode");

        assert_eq!((decoded.width, decoded.height), (4, 4));
        assert_eq!(&decoded.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_truncated_textures() {
        let dds = dds_with(*b"DXT1", 64, 64, vec![0u8; 8]);
        assert!(decode_dds(&dds).is_err());
    }

    #[test]
    fn downscales_by_averaging() {
        // 2x2: one white pixel, one transparent black, then two more black.
        let rgba = vec![255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let (width, height, scaled) = downscale(2, 2, &rgba, 1);

        assert_eq!((width, height), (1, 1));
        // 255 / 4 samples, truncated.
        assert_eq!(scaled, vec![63, 63, 63, 255]);
    }

    #[test]
    fn extracts_masked_channels() {
        assert_eq!(extract_channel(0x00FF_0000, 0x00FF_0000), 255);
        assert_eq!(extract_channel(0x0000_FF00, 0x0000_FF00), 255);
        assert_eq!(extract_channel(0, 0x00FF_0000), 0);
        // 5-bit channel scales up to 255.
        assert_eq!(extract_channel(0b1_1111, 0b1_1111), 255);
    }

    #[test]
    fn detects_cutout_alpha_channels() {
        // A single transparent pixel marks the texture as alpha-cutout.
        let cutout = vec![255, 0, 0, 255, 0, 255, 0, 0];
        assert!(uses_alpha(&cutout));

        // Fully opaque textures stay in the opaque pass.
        let opaque = vec![255, 0, 0, 255, 0, 255, 0, 255];
        assert!(!uses_alpha(&opaque));
    }
}
