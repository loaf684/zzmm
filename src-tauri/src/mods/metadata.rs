use std::path::Path;

use serde::{Deserialize, Serialize};

pub const METADATA_FILE: &str = ".zzmm.json";

/// Sidecar metadata written by the installer/browser. Every field is optional
/// so older or hand-written files never break scanning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadata {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub game_banana_mod_id: Option<u64>,
    #[serde(default)]
    pub game_banana_file_id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub character: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Original creator on GameBanana, when the installer knows it.
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    /// Free-form user notes, edited from the mod detail panel.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Write the sidecar file atomically so an interrupted install cannot leave a
/// half-written `.zzmm.json` behind.
pub fn write(mod_dir: &Path, metadata: &ModMetadata) -> std::io::Result<()> {
    use std::io::Write;

    let path = mod_dir.join(METADATA_FILE);
    let temp = mod_dir.join(format!("{METADATA_FILE}.tmp"));

    let json = serde_json::to_string_pretty(metadata).map_err(std::io::Error::other)?;

    {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }

    std::fs::rename(&temp, &path)
}

/// Returns `None` when the file is absent or invalid. Invalid metadata is logged
/// and treated as an unlinked manual mod rather than failing the whole scan.
pub fn read(mod_dir: &Path) -> Option<ModMetadata> {
    let path = mod_dir.join(METADATA_FILE);
    let raw = std::fs::read_to_string(&path).ok()?;

    match serde_json::from_str::<ModMetadata>(&raw) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            log::warn!("Ignoring invalid {}: {error}", path.display());
            None
        }
    }
}
