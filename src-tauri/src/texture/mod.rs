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
/// Sources above this are decoded from a sampled block grid instead of the whole
/// base mip. The pure-Rust decoders cost roughly a minute per 8192x8192 BC7
/// atlas, and some installed mods ship three of them, which makes the preview
/// look hung. Keeping every second block at this size still leaves the box
/// filter below averaging real pixels, and a 1024 px thumbnail cannot show the
/// rest.
const MAX_DECODE_SIDE: usize = 4096;

#[derive(Debug, Clone)]
pub struct DecodedTexture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// A decoded base mip plus how coarsely it had to be sampled.
#[derive(Debug, Clone)]
pub struct DecodedDds {
    pub texture: DecodedTexture,
    /// Blocks (compressed) or pixels (uncompressed) skipped per axis. 1 means
    /// the whole base mip was decoded.
    pub stride: usize,
}

/// Decode the base mip, sampling it when it is larger than `max_side` on either
/// axis. Colour maps that large exist (an 8192x8192 BC7 atlas is 64 MB) and
/// decoding them in full costs far more than the thumbnail they feed.
pub fn decode_dds_capped(bytes: &[u8], max_side: usize) -> AppResult<DecodedDds> {
    let header = dds::parse(bytes).map_err(AppError::Texture)?;
    let DdsHeader {
        mut width,
        mut height,
        format,
        data_offset,
        ..
    } = header;

    let data = bytes
        .get(data_offset..)
        .ok_or_else(|| AppError::Texture("DDS file contains no pixel data.".to_string()))?;

    let stride = width.max(height).div_ceil(max_side.max(1));

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

        // Sampling keeps the decoded image whole: the gathered blocks are
        // contiguous, so the decoder sees a smaller image of the same shape.
        let sampled;
        let source: &[u8] = if stride > 1 {
            let (blocks, kept_x, kept_y) =
                subsample(&data[..required], blocks_x, blocks_y, format.block_bytes(), stride);
            sampled = blocks;
            width = kept_x * 4;
            height = kept_y * 4;
            &sampled
        } else {
            &data[..required]
        };

        let mut pixels = vec![0u32; width * height];

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

        return Ok(DecodedDds {
            texture: DecodedTexture {
                width,
                height,
                rgba,
            },
            stride,
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

    let sampled_pixels;
    let source: &[u8] = if stride > 1 {
        let (pixels, kept_x, kept_y) = subsample(data, width, height, pixel_bytes, stride);
        sampled_pixels = pixels;
        width = kept_x;
        height = kept_y;
        &sampled_pixels
    } else {
        data
    };

    let mut rgba = vec![0u8; width * height * 4];

    for index in 0..width * height {
        let offset = index * pixel_bytes;
        let pixel = &source[offset..offset + pixel_bytes];

        let (red, green, blue, alpha) = match format {
            DdsFormat::Rgba8 => (pixel[0], pixel[1], pixel[2], pixel[3]),
            DdsFormat::Bgra8 => (pixel[2], pixel[1], pixel[0], pixel[3]),
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

    Ok(DecodedDds {
        texture: DecodedTexture {
            width,
            height,
            rgba,
        },
        stride,
    })
}

/// Copy every `stride`-th block (compressed) or pixel (uncompressed) out of a
/// level, so an oversized source decodes into a smaller image of the same
/// shape. `element` is the block or pixel size in bytes; `count_x`/`count_y`
/// are the level's block or pixel counts.
fn subsample(
    data: &[u8],
    count_x: usize,
    count_y: usize,
    element: usize,
    stride: usize,
) -> (Vec<u8>, usize, usize) {
    let kept_x = count_x.div_ceil(stride);
    let kept_y = count_y.div_ceil(stride);
    let mut out = Vec::with_capacity(kept_x * kept_y * element);

    for y in (0..count_y).step_by(stride) {
        for x in (0..count_x).step_by(stride) {
            let start = (y * count_x + x) * element;
            out.extend_from_slice(&data[start..start + element]);
        }
    }

    (out, kept_x, kept_y)
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

            // Coverage weighting: sum rgb * alpha, then divide by the summed
            // alpha. Cutout atlases (hair, lace) store fully transparent black
            // in the texels around their islands; a plain channel average would
            // pull those black texels into the edge colour and paint a dark
            // fringe over every cutout. Weighting by alpha keeps the edge texel
            // the colour of the geometry that actually covers it, while a fully
            // opaque block averages exactly as before.
            let mut sums = [0u64; 3];
            let mut sum_alpha = 0u64;
            let mut samples = 0u64;

            for y in y_start..y_end.max(y_start + 1) {
                for x in x_start..x_end.max(x_start + 1) {
                    let offset = (y.min(height - 1) * width + x.min(width - 1)) * 4;
                    let alpha = u64::from(rgba[offset + 3]);
                    for (channel, sum) in sums.iter_mut().enumerate() {
                        *sum += u64::from(rgba[offset + channel]) * alpha;
                    }
                    sum_alpha += alpha;
                    samples += 1;
                }
            }

            let samples = samples.max(1);
            let target = (target_y * target_width + target_x) * 4;
            out[target + 3] = (sum_alpha / samples) as u8;
            for (channel, sum) in sums.iter().enumerate() {
                out[target + channel] = if sum_alpha == 0 {
                    0
                } else {
                    (sum / sum_alpha) as u8
                };
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
    let decoded = decode_dds_capped(&bytes, MAX_DECODE_SIDE)?;
    if decoded.stride > 1 {
        // Say so in the log: a subsampled atlas explains a softer preview than
        // the file itself would give, without pretending the file was broken.
        log::info!(
            "{}: decoded every {}th block for the preview ({})",
            path.display(),
            decoded.stride,
            "source above the 4096 px decode cap"
        );
    }
    let (width, height, rgba) = downscale(
        decoded.texture.width,
        decoded.texture.height,
        &decoded.texture.rgba,
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

    fn decode_dds(bytes: &[u8]) -> AppResult<DecodedTexture> {
        decode_dds_capped(bytes, MAX_DECODE_SIDE).map(|decoded| decoded.texture)
    }

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
    fn subsamples_sources_above_the_decode_cap() {
        // A 32x32 DXT1 is 8x8 blocks; each block gets a colour that names its
        // own position, so the kept blocks are identifiable in the output.
        let mut body = Vec::new();
        for by in 0..8u16 {
            for bx in 0..8u16 {
                let colour = (bx << 11) | (by << 5);
                body.extend(bc1_block(colour, colour));
            }
        }
        let dds = dds_with(*b"DXT1", 32, 32, body);

        let full = decode_dds_capped(&dds, 64).expect("decode");
        assert_eq!(full.stride, 1);
        assert_eq!((full.texture.width, full.texture.height), (32, 32));

        // Cap of 8: every fourth block survives, so a 2x2 block grid.
        let sampled = decode_dds_capped(&dds, 8).expect("decode");
        assert_eq!(sampled.stride, 4);
        assert_eq!((sampled.texture.width, sampled.texture.height), (8, 8));

        let channel = |decoded: &DecodedTexture, x: usize, y: usize| {
            let offset = (y * decoded.width + x) * 4;
            [decoded.rgba[offset], decoded.rgba[offset + 1]]
        };
        // Kept block (0,0) is source block (0,0); kept block (1,0) is source
        // block (4,0), which starts at source x = 16.
        assert_eq!(
            channel(&sampled.texture, 0, 0),
            channel(&full.texture, 0, 0)
        );
        assert_eq!(
            channel(&sampled.texture, 4, 0),
            channel(&full.texture, 16, 0)
        );
        assert_eq!(
            channel(&sampled.texture, 0, 4),
            channel(&full.texture, 0, 16)
        );
        // Not a nearest-pixel grab: the sampled block still decodes as a block.
        assert_ne!(
            channel(&sampled.texture, 0, 0),
            channel(&sampled.texture, 4, 0)
        );
    }

    #[test]
    fn keeps_bgra_alpha_and_x8_opaque() {
        let masks = |alpha: u32| {
            let mut bytes = dds_with([0, 0, 0, 0], 1, 1, Vec::new());
            bytes[88..92].copy_from_slice(&32u32.to_le_bytes());
            bytes[92..96].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
            bytes[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
            bytes[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
            bytes[104..108].copy_from_slice(&alpha.to_le_bytes());
            bytes
        };

        // B8G8R8A8: byte order B,G,R,A — the alpha byte is the cutout mask that
        // keeps hair and lace from rendering as solid cards.
        let mut bgra = masks(0xFF00_0000);
        bgra.extend_from_slice(&[0, 0, 255, 64]);
        assert_eq!(
            decode_dds(&bgra).expect("decode").rgba,
            vec![255, 0, 0, 64]
        );

        // B8G8R8X8: no alpha mask, so the fourth byte is padding. A zero there
        // must not punch a hole through the part.
        let mut x8 = masks(0);
        x8.extend_from_slice(&[0, 0, 255, 0]);
        assert_eq!(decode_dds(&x8).expect("decode").rgba, vec![255, 0, 0, 255]);
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
    fn does_not_darken_colour_with_transparent_texels() {
        // One opaque red texel, three transparent black ones — the layout of a
        // cutout island edge. The colour must stay red instead of being dragged
        // to a dark fringe by the invisible texels.
        let rgba = vec![255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let (width, height, scaled) = downscale(2, 2, &rgba, 1);

        assert_eq!((width, height), (1, 1));
        // Coverage is a quarter, so alpha is 63, but the colour is unweighted.
        assert_eq!(scaled, vec![255, 0, 0, 63]);
    }

    #[test]
    fn a_fully_transparent_block_stays_black() {
        let rgba = vec![255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0];
        let (_, _, scaled) = downscale(2, 2, &rgba, 1);

        // No division by zero, and no colour invented where nothing covers.
        assert_eq!(scaled, vec![0, 0, 0, 0]);
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
