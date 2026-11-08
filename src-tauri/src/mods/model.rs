use serde::{Deserialize, Serialize};

use crate::mods::metadata::ModMetadata;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    /// Actual folder name, including the `DISABLED_` prefix when disabled.
    pub folder_name: String,
    /// Folder name without the `DISABLED_` prefix.
    pub base_name: String,
    /// Friendly name from `.zzmm.json`, falling back to the folder name.
    pub display_name: String,
    /// Best-effort character used for grouping.
    pub character: String,
    pub enabled: bool,
    /// True when the mod is linked to a GameBanana entry. A notes-only
    /// sidecar does not count, so saving a note never hides "Unlinked".
    pub linked: bool,
    /// Absolute path to a local preview image, if one was found.
    pub thumbnail_path: Option<String>,
    /// Unique texture hashes declared in `[TextureOverride*]` sections.
    pub hashes: Vec<String>,
    /// Active `[Key*]` bindings found in non-disabled INI files.
    pub hotkey_count: u32,
    pub has_conflict: bool,
    pub metadata: Option<ModMetadata>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashConflictPart {
    pub folder_name: String,
    pub base_name: String,
    pub display_name: String,
    pub file: String,
    pub section: String,
    pub line: u32,
}

/// Enabled mods that override the same texture hash.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashConflict {
    pub hash: String,
    pub mods: Vec<HashConflictPart>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModScanResult {
    pub mods_dir: String,
    pub mods: Vec<InstalledMod>,
    pub conflicts: Vec<HashConflict>,
    pub enabled_count: u32,
    pub disabled_count: u32,
    pub unlinked_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scanned: u32,
    pub total: u32,
    pub folder_name: String,
}

/// One requested enable/disable change for a batch operation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStateUpdate {
    /// Actual folder name, including any `DISABLED_` prefix.
    pub folder_name: String,
    pub enabled: bool,
}

/// A single mod that could not be changed during a batch operation. Other mods
/// in the same batch are still processed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStateFailure {
    pub folder_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyModStatesResult {
    pub scan: ModScanResult,
    /// Folder names that actually changed state.
    pub changed: Vec<String>,
    pub failed: Vec<ModStateFailure>,
    /// Absolute path of the safety snapshot written before the batch, if any.
    pub backup_file: Option<String>,
}

/// Result of tagging several freshly installed folders with one character.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterAssignResult {
    pub scan: ModScanResult,
    pub failed: Vec<ModStateFailure>,
}

/// Extra per-mod data loaded only when the detail panel opens: local preview
/// images, the total folder size, and the fallback install date.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetails {
    pub folder_name: String,
    /// Absolute paths to root-level `preview*` images, ready for the asset protocol.
    pub previews: Vec<String>,
    pub folder_size: u64,
    /// `.zzmm.json` install time, falling back to the folder creation time.
    pub installed_at: Option<String>,
    pub notes: Option<String>,
}
