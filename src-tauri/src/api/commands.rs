use std::path::PathBuf;
use std::sync::atomic::Ordering;

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
    state.install_cancel.store(false, Ordering::SeqCst);
    let cancel = state.install_cancel.clone();

    download::install_mod(
        &app,
        &config,
        &mods,
        mod_id,
        file_id,
        replace_folder,
        cancel,
    )
    .await
}

/// Compare installed `.zzmm.json` file IDs with the latest GameBanana files.
#[tauri::command]
pub async fn check_mod_updates(state: State<'_, AppState>) -> AppResult<UpdateCheckResult> {
    let mods = mods_dir(&state)?;
    let cache = cache_dir(&state);

    gamebanana::updates(&cache, &mods).await
}
