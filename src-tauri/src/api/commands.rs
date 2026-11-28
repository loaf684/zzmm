use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::api::download;
use crate::api::gamebanana;
use crate::api::model::{
    BrowsePage, BrowseParams, CategoryChip, DownloadOutcome, ModDetail, UpdateCheckResult,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn mods_dir(state: &AppState) -> AppResult<PathBuf> {
    let dir = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .mods_dir
        .clone();

    dir.map(PathBuf::from).ok_or(AppError::NoModsDir)
}

fn cache_dir(state: &AppState) -> PathBuf {
    state.paths.config_dir.join("cache").join("gamebanana")
}

/// One page of browse results. `params.cursor` is the opaque page cursor
/// returned by the previous call, which is why the UI never counts pages itself.
#[tauri::command]
pub async fn browse_gamebanana(
    state: State<'_, AppState>,
    params: BrowseParams,
) -> AppResult<BrowsePage> {
    let cache = cache_dir(&state);
    gamebanana::browse(&cache, &params).await
}

#[tauri::command]
pub async fn get_gamebanana_mod(state: State<'_, AppState>, mod_id: u64) -> AppResult<ModDetail> {
    let cache = cache_dir(&state);
    gamebanana::detail_for_browse(&cache, mod_id).await
}

#[tauri::command]
pub async fn get_gamebanana_categories(state: State<'_, AppState>) -> AppResult<Vec<CategoryChip>> {
    let cache = cache_dir(&state);
    gamebanana::categories(&cache).await
}

/// Download and install one file from GameBanana.
///
/// Downloads run independently: each mod owns its cancel flag, so a second mod
/// can start while `activeModId`-style UI state belongs to the first. Installing
/// is serialized through `install_lock` because extraction and destination
/// naming touch one shared Mods folder. The mod ID is reserved while it runs,
/// which is what stops a duplicate click from starting the same download twice.
#[tauri::command]
pub async fn install_gamebanana_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    mod_id: u64,
    file_id: Option<u64>,
    replace_folder: Option<String>,
) -> AppResult<DownloadOutcome> {
    let mods = mods_dir(&state)?;
    let config = state.paths.config_dir.clone();
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let mut active = state
            .gamebanana_cancels
            .lock()
            .map_err(|_| AppError::StateLock)?;

        if active.contains_key(&mod_id) {
            return Err(AppError::Download(
                "That mod is already downloading. Wait for it to finish before starting it again."
                    .to_string(),
            ));
        }

        active.insert(mod_id, cancel.clone());
    }

    let result = download::install_mod(
        &app,
        &config,
        &mods,
        mod_id,
        file_id,
        replace_folder,
        cancel,
        state.install_lock.clone(),
    )
    .await;

    if let Ok(mut active) = state.gamebanana_cancels.lock() {
        active.remove(&mod_id);
    }

    result
}

/// Ask one GameBanana download to stop. Other running downloads are untouched.
#[tauri::command]
pub fn cancel_gamebanana_install(state: State<'_, AppState>, mod_id: u64) -> bool {
    let Ok(active) = state.gamebanana_cancels.lock() else {
        return false;
    };

    match active.get(&mod_id) {
        Some(cancel) => {
            cancel.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Compare installed `.zzmm.json` file IDs with the latest GameBanana files.
#[tauri::command]
pub async fn check_mod_updates(state: State<'_, AppState>) -> AppResult<UpdateCheckResult> {
    let mods = mods_dir(&state)?;
    let cache = cache_dir(&state);

    gamebanana::updates(&cache, &mods).await
}
