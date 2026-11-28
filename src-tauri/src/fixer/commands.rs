//! Tauri commands for the mod fixer. Thin wrappers: all the work happens on a
//! blocking thread so a large Mods folder never stalls the webview.
//!
//! The ignore list and the fix history live in one JSON file in the app config
//! folder (`fixer::store`). Commands read it before a scan/apply and write it
//! after an apply or a restore; the file is the only thing ignoring a mod
//! touches.

use std::path::Path;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::fixer::model::{
    FixerApplyResult, FixerFailure, FixerHistoryRecord, FixerIgnoredEntry, FixerRollbackResult,
    FixerScanResult, FixerStateView,
};
use crate::fixer::store::{self, IgnoreSet};
use crate::fixer::{backup, ops};
use crate::state::AppState;

/// Read-only scan of a Mods folder: nothing is written, not even a backup.
/// Files the ignore list covers are still reported, marked as ignored.
#[tauri::command]
pub async fn fixer_scan(
    state: State<'_, AppState>,
    mods_dir: String,
) -> AppResult<FixerScanResult> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        let ignored = IgnoreSet::from_state(&store::load(&store::path_in(&config_dir)));
        ops::scan(Path::new(&mods_dir), &ignored)
    })
    .await
}

/// Rewrite the named INIs. With `backup`, each original is moved into the
/// loader's backup folder first. Ignored files are left alone, and every file
/// that was written is appended to the history the UI can restore from.
#[tauri::command]
pub async fn fixer_apply(
    state: State<'_, AppState>,
    mods_dir: String,
    ini_paths: Vec<String>,
    backup: bool,
) -> AppResult<FixerApplyResult> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        let state_file = store::path_in(&config_dir);
        let mut persisted = store::load(&state_file);
        let ignored = IgnoreSet::from_state(&persisted);

        let result = ops::apply(Path::new(&mods_dir), &ini_paths, backup, &ignored)?;

        if !result.applied.is_empty() {
            let records: Vec<FixerHistoryRecord> =
                result.applied.iter().map(FixerHistoryRecord::from).collect();
            store::append_history(&mut persisted, records);
            // The INIs are already written; a history file that cannot be saved
            // is worth a log line, not a failed run.
            if let Err(error) = store::save(&state_file, &persisted) {
                log::warn!("Could not record the fixer history: {error}");
            }
        }

        Ok(result)
    })
    .await
}

/// Put the backups back over the INIs they came from. Restores the newest
/// backup per file; the history menu restores one specific recorded fix.
#[tauri::command]
pub async fn fixer_rollback(
    mods_dir: String,
    ini_paths: Vec<String>,
) -> AppResult<FixerRollbackResult> {
    blocking(move || ops::rollback(Path::new(&mods_dir), &ini_paths)).await
}

/// The ignore list and the restore history, for the Mod Fixer page. Reads disk
/// only: whether a backup is still there is answered per call, not cached.
#[tauri::command]
pub async fn fixer_state(state: State<'_, AppState>) -> AppResult<FixerStateView> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || Ok(store::view(&store::load(&store::path_in(&config_dir))))).await
}

/// Add or remove an ignore entry: a whole mod (`ini_path` and `key` absent),
/// one INI (`key` absent), or one recommendation (`key` set).
///
/// This never touches a mod's INI, so ignoring anything is reversible and the
/// file stops being fixed from the next scan on.
#[tauri::command]
pub async fn fixer_ignore(
    state: State<'_, AppState>,
    folder: String,
    ini_path: Option<String>,
    key: Option<String>,
    label: String,
    ignored: bool,
) -> AppResult<FixerStateView> {
    if key.is_some() && ini_path.is_none() {
        return Err(AppError::IniPath(
            "a recommendation ignore needs the ini path it belongs to".to_string(),
        ));
    }
    if folder.trim().is_empty() {
        return Err(AppError::InvalidModFolder(folder));
    }

    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        let state_file = store::path_in(&config_dir);
        let mut persisted = store::load(&state_file);
        let entry = FixerIgnoredEntry {
            folder,
            ini_path,
            key,
            label,
            ignored_at: backup::now_seconds(),
        };

        store::set_ignored(&mut persisted, entry, ignored);
        store::save(&state_file, &persisted)?;
        log::info!("Fixer ignore list updated ({ignored})");

        Ok(store::view(&persisted))
    })
    .await
}

/// Put the original INI back from the backup one recorded fix wrote.
///
/// `fixedAt` with `iniPath` identifies the history entry; its backup path comes
/// from the stored record, so the UI can never point a restore at another file.
#[tauri::command]
pub async fn fixer_restore(
    state: State<'_, AppState>,
    mods_dir: String,
    ini_path: String,
    fixed_at: u64,
) -> AppResult<FixerRollbackResult> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        let state_file = store::path_in(&config_dir);
        let mut persisted = store::load(&state_file);
        let mut result = FixerRollbackResult {
            restored: Vec::new(),
            failed: Vec::new(),
        };

        let Some(record) = store::find_history(&persisted, &ini_path, fixed_at).cloned() else {
            result.failed.push(FixerFailure {
                ini_path,
                error: "No fix with that history entry is recorded.".to_string(),
            });
            return Ok(result);
        };

        match ops::restore_history(Path::new(&mods_dir), &record) {
            Ok(restored_from) => {
                log::info!(
                    "Fixer history: {} restored from {}",
                    record.ini_path,
                    restored_from.display()
                );
                store::mark_restored(&mut persisted, &ini_path, fixed_at, backup::now_seconds());
                if let Err(error) = store::save(&state_file, &persisted) {
                    log::warn!("Could not record the restore in the fixer history: {error}");
                }
                result.restored.push(ini_path);
            }
            Err(error) => result.failed.push(FixerFailure { ini_path, error }),
        }

        Ok(result)
    })
    .await
}

async fn blocking<T, F>(work: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}
