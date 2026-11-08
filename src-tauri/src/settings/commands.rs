use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, AppResult};
use crate::settings::model::{
    DetectedModsDir, ModsDirState, PathsView, Settings, SettingsView, SCHEMA_VERSION,
};
use crate::settings::paths::{inspect_mods_dir, AppPaths};
use crate::settings::{detect, store};
use crate::state::AppState;

#[tauri::command]
pub async fn get_settings(app: AppHandle, state: State<'_, AppState>) -> AppResult<SettingsView> {
    let state = state.inner();
    let settings = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .clone();

    Ok(build_view(&app, &state.paths, &settings))
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<SettingsView> {
    let normalized = normalize_settings(settings)?;

    let settings_file = state.paths.settings_file.clone();
    let payload = normalized.clone();

    // Disk I/O must not run on the async runtime's worker threads.
    let write_result =
        tauri::async_runtime::spawn_blocking(move || store::save(&settings_file, &payload))
            .await
            .map_err(|error| AppError::Task(error.to_string()))?;
    write_result?;

    {
        let mut guard = state.settings.write().map_err(|_| AppError::StateLock)?;
        *guard = normalized.clone();
    }

    // The folder may have changed; refresh the asset-protocol scope used for
    // local mod thumbnails.
    if let Some(dir) = normalized.mods_dir.as_deref() {
        if let Err(error) = crate::mods::asset_scope::allow(&app, Path::new(dir)) {
            log::warn!("Could not grant thumbnail access to {dir}: {error}");
        }
    }

    log::info!("Settings saved (mods directory: {:?})", normalized.mods_dir);
    Ok(build_view(&app, &state.paths, &normalized))
}

#[tauri::command]
pub async fn validate_mods_dir(path: String) -> AppResult<ModsDirState> {
    let trimmed = path.trim().trim_matches('"').trim().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        if trimmed.is_empty() {
            ModsDirState {
                message: Some("Enter a folder path.".into()),
                ..ModsDirState::default()
            }
        } else {
            inspect_mods_dir(Path::new(&trimmed))
        }
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))
}

#[tauri::command]
pub async fn choose_mods_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    let start_dir = state
        .settings
        .read()
        .map(|settings| settings.mods_dir.clone())
        .unwrap_or(None);
    let dialog_app = app.clone();

    // Blocking dialog APIs must not run on the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = dialog_app
            .dialog()
            .file()
            .set_title("Select your ZZMI / XXMI Mods folder");

        if let Some(dir) = start_dir.as_deref() {
            if Path::new(dir).is_dir() {
                dialog = dialog.set_directory(dir);
            }
        }

        match dialog.blocking_pick_folder() {
            Some(picked) => picked
                .into_path()
                .map(|path| Some(path.to_string_lossy().to_string()))
                .map_err(|error| AppError::Dialog(error.to_string())),
            None => Ok(None),
        }
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn detect_mods_dirs() -> AppResult<Vec<DetectedModsDir>> {
    tauri::async_runtime::spawn_blocking(detect::find_installs)
        .await
        .map_err(|error| AppError::Task(error.to_string()))
}

fn build_view(app: &AppHandle, paths: &AppPaths, settings: &Settings) -> SettingsView {
    let mods_dir_state = settings
        .mods_dir
        .as_deref()
        .map(|path| inspect_mods_dir(Path::new(path)))
        .unwrap_or_default();

    SettingsView {
        settings: settings.clone(),
        paths: PathsView {
            config_dir: paths.config_dir.to_string_lossy().to_string(),
            settings_file: paths.settings_file.to_string_lossy().to_string(),
            log_dir: paths.log_dir.to_string_lossy().to_string(),
            app_version: app.package_info().version.to_string(),
        },
        mods_dir_state,
    }
}

/// Validate and normalise incoming settings. Saving an invalid path is rejected
/// so the app never boots pointing at a folder that does not exist.
fn normalize_settings(mut settings: Settings) -> AppResult<Settings> {
    settings.schema_version = SCHEMA_VERSION;

    let cleaned = settings
        .mods_dir
        .as_deref()
        .map(str::trim)
        .map(|value| value.trim_matches('"').trim())
        .filter(|value| !value.is_empty())
        .map(clean_path);

    match cleaned {
        Some(path) => {
            let state = inspect_mods_dir(Path::new(&path));
            if !state.exists {
                return Err(AppError::ModsDirMissing(path));
            }
            if !state.is_dir {
                return Err(AppError::ModsDirNotDir(path));
            }
            settings.mods_dir = Some(path);
        }
        None => settings.mods_dir = None,
    }

    Ok(settings)
}

/// Drop trailing separators and normalise separators without adding a `\\?\`
/// prefix (which would look wrong in the UI and in logs). Relative paths are
/// resolved against the current directory because embedded paths (thumbnails)
/// must be absolute for the asset protocol.
fn clean_path(value: &str) -> String {
    let path = Path::new(value).components().collect::<PathBuf>();
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    };

    absolute.to_string_lossy().to_string()
}
