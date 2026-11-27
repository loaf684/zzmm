//! Tauri commands for the mod fixer. Thin wrappers: all the work happens on a
//! blocking thread so a large Mods folder never stalls the webview.

use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::fixer::model::{FixerApplyResult, FixerRollbackResult, FixerScanResult};
use crate::fixer::ops;

/// Read-only scan of a Mods folder: nothing is written, not even a backup.
#[tauri::command]
pub async fn fixer_scan(mods_dir: String) -> AppResult<FixerScanResult> {
    blocking(move || ops::scan(Path::new(&mods_dir))).await
}

/// Rewrite the named INIs. With `backup`, each original is moved into the
/// loader's backup folder first.
#[tauri::command]
pub async fn fixer_apply(
    mods_dir: String,
    ini_paths: Vec<String>,
    backup: bool,
) -> AppResult<FixerApplyResult> {
    blocking(move || ops::apply(Path::new(&mods_dir), &ini_paths, backup)).await
}

/// Put the backups back over the INIs they came from.
#[tauri::command]
pub async fn fixer_rollback(
    mods_dir: String,
    ini_paths: Vec<String>,
) -> AppResult<FixerRollbackResult> {
    blocking(move || ops::rollback(Path::new(&mods_dir), &ini_paths)).await
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
