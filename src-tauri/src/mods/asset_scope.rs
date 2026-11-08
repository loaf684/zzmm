use std::path::Path;

use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};

/// Allow the webview to load preview images from the user's Mods folder.
///
/// The scope is granted at runtime (not in tauri.conf.json) because the folder
/// is chosen by the user. It is re-granted whenever settings are saved or a scan
/// starts, which also covers folders restored from the previous session.
pub fn allow(app: &AppHandle, dir: &Path) -> AppResult<()> {
    app.asset_protocol_scope()
        .allow_directory(dir, true)
        .map_err(|error| AppError::AssetScope(error.to_string()))
}
