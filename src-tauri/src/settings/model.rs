use serde::{Deserialize, Serialize};

/// Bump when the on-disk shape changes in a breaking way.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Absolute path to the ZZMI `Mods` folder. `None` until the user links one.
    #[serde(default)]
    pub mods_dir: Option<String>,

    /// When true, enabling a mod automatically disables every other enabled mod
    /// that belongs to the same character. Mods without a character are never
    /// touched.
    #[serde(default)]
    pub one_active_per_character: bool,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            mods_dir: None,
            one_active_per_character: false,
        }
    }
}

/// Result of a cheap structural check on a candidate Mods folder. This is not a
/// full mod scan (that arrives with the installed-mods feature); it only checks
/// existence and counts immediate entries.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsDirState {
    pub exists: bool,
    pub is_dir: bool,
    pub valid: bool,
    pub mod_count: u32,
    pub disabled_count: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsView {
    pub config_dir: String,
    pub settings_file: String,
    pub log_dir: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub paths: PathsView,
    pub mods_dir_state: ModsDirState,
}

/// A candidate Mods folder found by the installer auto-detection scan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModsDir {
    pub path: String,
    pub label: String,
    /// One of `xxmi-launcher`, `xxmi`, `zzmi`, `mods-folder`.
    pub source: String,
    pub mod_count: u32,
    pub disabled_count: u32,
}
