use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::error::AppResult;
use crate::settings::model::ModsDirState;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub settings_file: PathBuf,
    pub log_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        let resolver = app.path();
        let config_dir = resolver.app_config_dir()?;
        let log_dir = resolver.app_log_dir()?;

        Ok(Self {
            settings_file: config_dir.join("settings.json"),
            config_dir,
            log_dir,
        })
    }
}

/// Inspect a candidate folder cheaply. The resulting state is what the settings
/// UI displays; it intentionally avoids walking nested mod trees.
pub fn inspect_mods_dir(path: &Path) -> ModsDirState {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            let mut mod_count = 0u32;
            let mut disabled_count = 0u32;

            // Entry counting is only used as a signal ("is this folder usable?").
            // Ignore unreadable entries rather than failing validation.
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let is_dir = entry
                        .file_type()
                        .map(|file_type| file_type.is_dir())
                        .unwrap_or(false);
                    if !is_dir {
                        continue;
                    }

                    mod_count += 1;
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_uppercase()
                        .starts_with("DISABLED_")
                    {
                        disabled_count += 1;
                    }
                }
            }

            ModsDirState {
                exists: true,
                is_dir: true,
                valid: true,
                mod_count,
                disabled_count,
                message: None,
            }
        }
        Ok(_) => ModsDirState {
            exists: true,
            is_dir: false,
            valid: false,
            message: Some("That path is a file, not a folder.".into()),
            ..ModsDirState::default()
        },
        Err(error) => {
            let message = match error.kind() {
                std::io::ErrorKind::NotFound => {
                    "Folder not found. Check the path or use Browse.".to_string()
                }
                std::io::ErrorKind::PermissionDenied => {
                    "Windows denied access to this folder. Try running ZZMM normally (not elevated) or pick another folder."
                        .to_string()
                }
                _ => format!("Could not read folder: {error}"),
            };

            ModsDirState {
                exists: false,
                is_dir: false,
                valid: false,
                message: Some(message),
                ..ModsDirState::default()
            }
        }
    }
}
