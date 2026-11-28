use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::mods::loadouts::{self, LoadoutApplyResult, LoadoutSummary};
use crate::mods::model::{
    ApplyModStatesResult, ModDetails, ModScanResult, ModStateUpdate,
};
use crate::mods::{actions, asset_scope, scanner};
use crate::state::AppState;

const SCAN_PROGRESS_EVENT: &str = "mods:scan-progress";

fn mods_dir(state: &AppState) -> AppResult<PathBuf> {
    let dir = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .mods_dir
        .clone();

    dir.map(PathBuf::from).ok_or(AppError::NoModsDir)
}

/// Scan synchronously and forward progress to the webview. Called inside
/// `spawn_blocking` by every command below.
fn scan_with_progress(app: &AppHandle, dir: &Path) -> AppResult<ModScanResult> {
    scanner::scan(dir, |progress| {
        if let Err(error) = app.emit(SCAN_PROGRESS_EVENT, progress) {
            log::warn!("Could not emit mod scan progress: {error}");
        }
    })
}

#[tauri::command]
pub async fn scan_mods(app: AppHandle, state: State<'_, AppState>) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    // Thumbnails are served through the asset protocol; make sure this folder is
    // in scope even when it was restored from disk at startup.
    if let Err(error) = asset_scope::allow(&app, &dir) {
        log::warn!(
            "Could not grant thumbnail access to {}: {error}",
            dir.display()
        );
    }

    tauri::async_runtime::spawn_blocking(move || scan_with_progress(&app, &dir))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn set_mod_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
    enabled: bool,
) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::set_enabled(&dir, &folder_name, enabled)?;
        scan_with_progress(&app, &dir)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn apply_mod_states(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: Vec<ModStateUpdate>,
) -> AppResult<ApplyModStatesResult> {
    let dir = mods_dir(&state)?;
    let config_dir = state.paths.config_dir.clone();

    tauri::async_runtime::spawn_blocking(move || {
        // Only write a safety snapshot when at least one folder will actually
        // be renamed.
        let snapshot = loadouts::snapshot(&dir)?;
        let changes_anything = updates.iter().any(|update| {
            snapshot.iter().any(|current| {
                current
                    .folder_name
                    .eq_ignore_ascii_case(&update.folder_name)
                    && current.enabled != update.enabled
            })
        });

        let backup = if changes_anything {
            loadouts::write_state_backup(&config_dir, &dir)?
        } else {
            None
        };

        let (changed, failed) = actions::apply_states(&dir, &updates);
        let scan = scan_with_progress(&app, &dir)?;

        Ok(ApplyModStatesResult {
            scan,
            changed,
            failed,
            backup_file: backup.map(|path| path.to_string_lossy().to_string()),
        })
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn set_mod_character(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
    character: Option<String>,
) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::set_character(&dir, &folder_name, character.as_deref())?;
        scan_with_progress(&app, &dir)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn delete_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::delete(&dir, &folder_name)?;
        scan_with_progress(&app, &dir)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn open_mod_folder(
    state: State<'_, AppState>,
    folder_name: Option<String>,
) -> AppResult<()> {
    let dir = mods_dir(&state)?;

    let target = match folder_name {
        Some(name) => actions::mod_dir(&dir, &name)?,
        None => {
            if !dir.is_dir() {
                return Err(AppError::ModsDirMissing(dir.display().to_string()));
            }
            dir
        }
    };

    tauri::async_runtime::spawn_blocking(move || actions::open_in_explorer(&target))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn get_mod_details(
    state: State<'_, AppState>,
    folder_name: String,
) -> AppResult<ModDetails> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || actions::details(&dir, &folder_name))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn set_mod_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
    notes: String,
) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::set_notes(&dir, &folder_name, &notes)?;
        scan_with_progress(&app, &dir)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn rename_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
    new_name: String,
) -> AppResult<ModScanResult> {
    let dir = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::rename(&dir, &folder_name, &new_name)?;
        scan_with_progress(&app, &dir)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

/// Pick a new preview image with the native file dialog and copy it into the
/// mod folder. `None` means the user cancelled.
#[tauri::command]
pub async fn change_mod_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_name: String,
) -> AppResult<Option<ModScanResult>> {
    use tauri_plugin_dialog::DialogExt;

    let dir = mods_dir(&state)?;
    let dialog_app = app.clone();

    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("Choose a preview image")
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
            .blocking_pick_file()
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?;

    let Some(picked) = picked else {
        return Ok(None);
    };

    let source = picked
        .into_path()
        .map_err(|error| AppError::Dialog(error.to_string()))?;

    tauri::async_runtime::spawn_blocking(move || {
        actions::set_preview(&dir, &folder_name, &source.to_string_lossy())?;
        scan_with_progress(&app, &dir).map(Some)
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn open_external_url(url: String) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || actions::open_url(&url))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn list_loadouts(state: State<'_, AppState>) -> AppResult<Vec<LoadoutSummary>> {
    let config_dir = state.paths.config_dir.clone();

    tauri::async_runtime::spawn_blocking(move || Ok(loadouts::list(&config_dir)))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn save_loadout(state: State<'_, AppState>, name: String) -> AppResult<LoadoutSummary> {
    let dir = mods_dir(&state)?;
    let config_dir = state.paths.config_dir.clone();

    tauri::async_runtime::spawn_blocking(move || loadouts::save(&config_dir, &dir, &name))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn delete_loadout(state: State<'_, AppState>, name: String) -> AppResult<()> {
    let config_dir = state.paths.config_dir.clone();

    tauri::async_runtime::spawn_blocking(move || loadouts::delete(&config_dir, &name))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

#[tauri::command]
pub async fn apply_loadout(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> AppResult<LoadoutApplyResult> {
    let dir = mods_dir(&state)?;
    let config_dir = state.paths.config_dir.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let loadout = loadouts::get(&config_dir, &name)?;
        let current = loadouts::snapshot(&dir)?;

        let mut updates = Vec::new();
        let mut missing = Vec::new();

        for entry in &loadout.entries {
            // Base names are stable across the DISABLED_ toggle. A renamed mod
            // shows up as missing rather than being disabled by accident.
            match current
                .iter()
                .find(|state| state.base_name.eq_ignore_ascii_case(&entry.base_name))
            {
                Some(state) => {
                    if state.enabled != entry.enabled {
                        updates.push(ModStateUpdate {
                            folder_name: state.folder_name.clone(),
                            enabled: entry.enabled,
                        });
                    }
                }
                None => missing.push(entry.base_name.clone()),
            }
        }

        let backup = if updates.is_empty() {
            None
        } else {
            loadouts::write_state_backup(&config_dir, &dir)?
        };

        let (changed, failed) = actions::apply_states(&dir, &updates);
        let scan = scan_with_progress(&app, &dir)?;

        Ok(LoadoutApplyResult {
            scan,
            changed,
            missing,
            failed,
            backup_file: backup.map(|path| path.to_string_lossy().to_string()),
        })
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))?
}
