//! Mesh buffer decoding for the 3D preview.
//!
//! ZZMI mods ship split buffers (Position / Blend / Texcoord / IB) that the INI
//! describes with `[Resource*]` sections. When that description is missing or
//! incomplete we fall back to ZZZ's default layout and validate the guess
//! against the file sizes.

pub mod commands;
pub mod decode;
pub mod model;
pub mod resources;
pub mod variants;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// Little-endian f32 payload as base64. Binary keeps the IPC payload small:
/// a 50k-vertex mesh is ~600 KB instead of megabytes of JSON numbers.
pub fn encode_f32(values: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

pub fn encode_u32(values: &[u32]) -> String {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_little_endian_buffers() {
        let encoded = encode_f32(&[1.0, -2.0]);
        let bytes = STANDARD.decode(encoded).expect("valid base64");

        assert_eq!(bytes.len(), 8);
        assert_eq!(
            f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            1.0
        );
        assert_eq!(
            f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            -2.0
        );
    }
}
