use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::error::AppResult;
use crate::settings::model::Settings;
use crate::settings::paths::AppPaths;

pub fn ensure_config_dir(paths: &AppPaths) -> AppResult<()> {
    fs::create_dir_all(&paths.config_dir)?;
    Ok(())
}

/// Read settings, falling back to defaults. A file that cannot be parsed is
/// moved aside instead of overwritten, so user data is never silently lost.
pub fn load(path: &Path) -> Settings {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::info!("No settings file at {}; using defaults", path.display());
            return Settings::default();
        }
        Err(error) => {
            log::warn!("Could not read settings at {}: {error}", path.display());
            return Settings::default();
        }
    };

    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) => settings,
        Err(error) => {
            let backup = path.with_extension(format!("json.corrupt-{}", unix_timestamp()));
            log::error!(
                "Settings file is invalid JSON ({error}); backing up to {}",
                backup.display()
            );
            if let Err(rename_error) = fs::rename(path, &backup) {
                log::error!("Could not back up corrupt settings: {rename_error}");
            }
            Settings::default()
        }
    }
}

/// Write via a temporary file + rename so a crash can never leave a half-written
/// settings.json behind.
pub fn save(path: &Path, settings: &Settings) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(settings)?;
    let temp_path = path.with_extension("json.tmp");

    {
        let mut file = File::create(&temp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }

    // On Windows, std::fs::rename replaces an existing destination atomically.
    fs::rename(&temp_path, path)?;
    log::debug!("Wrote settings to {}", path.display());
    Ok(())
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
