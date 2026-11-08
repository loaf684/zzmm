use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Editing cap. Bigger files are almost always generated dumps, not mod INIs.
const MAX_INI_BYTES: u64 = 4 * 1024 * 1024;

/// Text encoding of an INI file. 3DMigoto accepts plain ASCII, but mod authors
/// ship UTF-8 (sometimes with a BOM) and, rarely, UTF-16LE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    /// Byte-preserving single-byte fallback for legacy ANSI files.
    Latin1,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf8Bom => "utf-8-bom",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Latin1 => "latin-1",
        }
    }
}

pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return (String::from_utf16_lossy(&units), Encoding::Utf16Le);
    }

    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(&bytes[3..]).into_owned(),
            Encoding::Utf8Bom,
        );
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), Encoding::Utf8),
        Err(_) => (
            // Latin-1 maps every byte to the same code point, so writing the
            // text back reproduces the original bytes exactly.
            bytes.iter().map(|byte| *byte as char).collect(),
            Encoding::Latin1,
        ),
    }
}

pub fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(text.as_bytes());
            bytes
        }
        Encoding::Utf16Le => {
            let mut bytes = vec![0xFF, 0xFE];
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            bytes
        }
        Encoding::Latin1 => text
            .chars()
            .map(|character| {
                let code = character as u32;
                if code < 256 {
                    code as u8
                } else {
                    b'?'
                }
            })
            .collect(),
    }
}

pub fn read(path: &Path) -> AppResult<(String, Encoding)> {
    let metadata =
        fs::metadata(path).map_err(|_| AppError::IniNotFound(path.display().to_string()))?;

    if metadata.len() > MAX_INI_BYTES {
        return Err(AppError::IniPath(format!(
            "{} is larger than {} MB and is not editable in ZZMM.",
            path.display(),
            MAX_INI_BYTES / (1024 * 1024)
        )));
    }

    let bytes = fs::read(path)?;
    Ok(decode(&bytes))
}

/// Copy the file to `<name>.ini.bak` the first time it is saved. An existing
/// backup is never overwritten, so the original survives repeated edits.
pub fn backup_once(path: &Path) -> AppResult<Option<String>> {
    let backup = backup_path(path);

    if backup.exists() {
        return Ok(Some(backup.display().to_string()));
    }

    fs::copy(path, &backup)?;
    log::info!("Created INI backup {}", backup.display());
    Ok(Some(backup.display().to_string()))
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

/// Write via a temporary file in the same folder, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let temp = path.with_extension("ini.tmp");

    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_each_encoding() {
        let text = "[KeyToggle]\r\nkey = ctrl VK_F1\r\n$active = 1\r\n";

        for encoding in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le] {
            let bytes = encode(text, encoding);
            let (decoded, detected) = decode(&bytes);
            assert_eq!(decoded, text, "round trip failed for {encoding:?}");
            assert_eq!(detected, encoding);
        }

        // Pure ASCII is valid UTF-8, so the single-byte fallback is only chosen
        // for files that really contain high bytes — and it round-trips them.
        let latin1_text = "[Key]\nname = Éllen\n";
        let bytes = encode(latin1_text, Encoding::Latin1);
        let (decoded, detected) = decode(&bytes);
        assert_eq!(detected, Encoding::Latin1);
        assert_eq!(decoded, latin1_text);
        assert_eq!(encode(&decoded, Encoding::Latin1), bytes);
    }

    #[test]
    fn backup_is_created_once() {
        let dir = std::env::temp_dir().join(format!("zzmm-ini-backup-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("mod.ini");
        fs::write(&path, "key = a\n").expect("seed");

        let first = backup_once(&path).expect("backup").expect("path");
        assert!(Path::new(&first).is_file());
        assert!(first.ends_with("mod.ini.bak"));

        fs::write(&path, "key = b\n").expect("rewrite");
        backup_once(&path).expect("backup again");
        // The first backup is preserved, not replaced with the newer content.
        assert_eq!(fs::read_to_string(&first).expect("read"), "key = a\n");

        fs::remove_dir_all(&dir).ok();
    }
}
