//! Tauri commands for the mod fixer. Thin wrappers: all the work happens on a
//! blocking thread so a large Mods folder never stalls the webview.
//!
//! The ignore list lives in one JSON file in the app config folder
//! (`fixer::store`), the per-mod undo snapshots in `fixer-undo` beside it
//! (`fixer::undo`). Commands read them before a scan/apply and write them after
//! an apply, an ignore toggle or an undo; nothing outside those folders is
//! touched by ignoring a mod.

use std::path::Path;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::fixer::model::{
    FixerApplyResult, FixerIgnoredEntry, FixerScanResult, FixerStateView, FixerUndoResult,
};
use crate::fixer::store::{self, IgnoreSet};
use crate::fixer::ops;
use crate::state::AppState;
use crate::util::unix_now;

/// Read-only scan of a Mods folder: nothing is written, not even a snapshot.
/// INIs the ignore list covers whole are counted, not listed.
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

/// Rewrite the named INIs. With `snapshot`, the pre-fix bytes of every file
/// written are stored as its mod's single undo snapshot, replacing the previous
/// one; without it, a stale snapshot for a written mod is dropped.
/// Ignored files are left alone.
#[tauri::command]
pub async fn fixer_apply(
    state: State<'_, AppState>,
    mods_dir: String,
    ini_paths: Vec<String>,
    snapshot: bool,
) -> AppResult<FixerApplyResult> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        let ignored = IgnoreSet::from_state(&store::load(&store::path_in(&config_dir)));
        ops::apply(
            Path::new(&mods_dir),
            &config_dir,
            &ini_paths,
            snapshot,
            &ignored,
        )
    })
    .await
}

/// Put one mod back from its undo snapshot - the state right before its most
/// recent fix. Files that come back are consumed; the rest stay restorable.
#[tauri::command]
pub async fn fixer_undo(
    state: State<'_, AppState>,
    mods_dir: String,
    folder: String,
) -> AppResult<FixerUndoResult> {
    if folder.trim().is_empty() {
        return Err(AppError::InvalidModFolder(folder));
    }

    let config_dir = state.paths.config_dir.clone();
    blocking(move || ops::undo(Path::new(&mods_dir), &config_dir, &folder)).await
}

/// The ignore list and the mods an undo can still put back, for the Mod Fixer
/// page. Reads disk only: whether a snapshot is still there is answered per
/// call, not cached.
#[tauri::command]
pub async fn fixer_state(state: State<'_, AppState>) -> AppResult<FixerStateView> {
    let config_dir = state.paths.config_dir.clone();
    blocking(move || {
        Ok(store::view(
            &config_dir,
            &store::load(&store::path_in(&config_dir)),
        ))
    })
    .await
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
            ignored_at: unix_now(),
        };

        store::set_ignored(&mut persisted, entry, ignored);
        store::save(&state_file, &persisted)?;
        log::info!("Fixer ignore list updated ({ignored})");

        Ok(store::view(&config_dir, &persisted))
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