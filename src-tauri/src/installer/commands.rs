use std::path::PathBuf;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::installer::install;
use crate::installer::model::{InstallBatchResult, InstallProgress};
use crate::state::AppState;

const INSTALL_PROGRESS_EVENT: &str = "install:progress";

fn current_mods_dir(state: &AppState) -> AppResult<PathBuf> {
    let dir = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .mods_dir
        .clone();

    dir.map(PathBuf::from).ok_or(AppError::NoModsDir)
}

/// Install one or more local archives. Each archive is processed independently,
/// so one corrupt file does not stop the rest of a drag-and-drop batch.
#[tauri::command]
pub async fn install_archives(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> AppResult<InstallBatchResult> {
    let mods_dir = current_mods_dir(&state)?;
    state.install_cancel.store(false, Ordering::SeqCst);
    let cancel = state.install_cancel.clone();
    let install_lock = state.install_lock.clone();

    // Folder extraction and moves touch the shared Mods directory, so a local
    // batch takes the same lock as a GameBanana download's install phase.
    let _install_guard = install_lock.lock().await;

    tauri::async_runtime::spawn_blocking(move || {
        let mut outcomes = Vec::with_capacity(paths.len());

        for path in paths {
            let archive = PathBuf::from(path);

            let mut emit = |progress: InstallProgress| {
                if let Err(error) = app.emit(INSTALL_PROGRESS_EVENT, progress) {
                    log::warn!("Could not emit install progress: {error}");
                }
            };

            let outcome = install::install_archive(&mods_dir, &archive, cancel.as_ref(), &mut emit);
            let cancelled = outcome.cancelled;
            outcomes.push(outcome);

            if cancelled {
                break;
            }
        }

        InstallBatchResult { outcomes }
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))
}

/// Ask the running install to stop at its next safe checkpoint. Extraction and
/// copying clean up their staging folders, so cancelling never leaves a partial
/// mod behind.
#[tauri::command]
pub fn cancel_install(state: State<'_, AppState>) -> bool {
    state.install_cancel.store(true, Ordering::SeqCst);
    true
}
