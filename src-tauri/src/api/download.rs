use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::api::client::send_download_request;
use crate::api::gamebanana;
use crate::api::model::{DownloadOutcome, DownloadProgress, ModDetail, ModFile};
use crate::error::{AppError, AppResult};
use crate::installer::install;
use crate::installer::model::InstallProgress;
use crate::mods::{actions, metadata};
use crate::util::iso_utc_now;

const DOWNLOAD_EVENT: &str = "download:progress";
const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "7z", "rar"];
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

/// Download one GameBanana file and install it through the archive installer.
///
/// `replace_folder` is the folder of the version being updated: after a
/// successful install it is disabled (not deleted) so the user can roll back and
/// nothing is silently destroyed.
pub async fn install_mod(
    app: &AppHandle,
    config_dir: &Path,
    mods_dir: &Path,
    mod_id: u64,
    file_id: Option<u64>,
    replace_folder: Option<String>,
    cancel: Arc<AtomicBool>,
) -> AppResult<DownloadOutcome> {
    if !mods_dir.is_dir() {
        return Err(AppError::NoModsDir);
    }

    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }

    let cache_dir = config_dir.join("cache").join("gamebanana");
    let detail: ModDetail = gamebanana::detail_for_browse(&cache_dir, mod_id).await?;
    let file = pick_file(&detail, file_id)?;
    let file_name = safe_archive_name(&file.name)?;

    let downloads_dir = config_dir.join("downloads");
    fs::create_dir_all(&downloads_dir).map_err(|error| {
        AppError::Download(format!(
            "Could not create the downloads folder ({}): {error}",
            downloads_dir.display()
        ))
    })?;

    let archive_path = unique_path(&downloads_dir, &file_name);

    emit(
        app,
        &detail,
        "downloading",
        0,
        0,
        0,
        format!("Downloading {}", file.name),
    );

    if let Err(error) = download_file(app, &detail, &file, &archive_path, cancel.as_ref()).await {
        remove_archive(&archive_path);
        return Err(error);
    }

    let installed = install_download(
        app,
        mods_dir,
        &archive_path,
        &detail,
        &file,
        replace_folder,
        cancel,
    )
    .await;

    // The archive is only needed for the install; keep the disk tidy on success,
    // failure, and cancellation.
    remove_archive(&archive_path);

    installed
}

fn remove_archive(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "Could not remove downloaded archive {}: {error}",
            path.display()
        ),
    }
}

fn pick_file(detail: &ModDetail, file_id: Option<u64>) -> AppResult<ModFile> {
    if detail.files.is_empty() {
        return Err(AppError::GameBanana(format!(
            "{} has no downloadable files.",
            detail.name
        )));
    }

    if let Some(wanted) = file_id {
        if let Some(file) = detail.files.iter().find(|file| file.id == wanted) {
            return Ok(file.clone());
        }
        return Err(AppError::GameBanana(format!(
            "File {wanted} is no longer listed for {}. Refresh the mod page.",
            detail.name
        )));
    }

    Ok(detail.files[0].clone())
}

fn safe_archive_name(raw: &str) -> AppResult<String> {
    let name = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string();

    let extension = Path::new(&name)
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if !ARCHIVE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(AppError::ArchiveUnsupported(if name.is_empty() {
            "unknown file".to_string()
        } else {
            name
        }));
    }

    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            {
                '_'
            } else {
                character
            }
        })
        .collect();

    Ok(cleaned)
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();

    for index in 2..=999u32 {
        let name = if extension.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{extension}")
        };

        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(format!("{stem}-{}.{extension}", std::process::id()))
}

async fn download_file(
    app: &AppHandle,
    detail: &ModDetail,
    file: &ModFile,
    target: &Path,
    cancel: &AtomicBool,
) -> AppResult<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }

    let response = send_download_request(&file.download_url).await?;

    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Download(format!(
            "GameBanana answered with HTTP {} for {}.",
            status.as_u16(),
            file.download_url
        )));
    }

    let total = response.content_length().unwrap_or(0);
    let mut response = response;
    let mut handle = File::create(target).map_err(|error| {
        AppError::Download(format!("Could not create the download file: {error}"))
    })?;

    let mut received: u64 = 0;
    let mut last_emit = Instant::now();

    while let Some(chunk) = response.chunk().await.map_err(|error| {
        AppError::Download(format!(
            "The download stopped after {} ({}): {error}",
            format_bytes(received),
            if total > 0 {
                format!("of {}", format_bytes(total))
            } else {
                "of an unknown total".to_string()
            }
        ))
    })? {
        if cancel.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled);
        }

        handle
            .write_all(&chunk)
            .map_err(|error| AppError::Download(error.to_string()))?;

        received += chunk.len() as u64;

        if last_emit.elapsed() >= PROGRESS_INTERVAL {
            let percent = percent_of(received, total);
            emit(
                app,
                detail,
                "downloading",
                percent,
                received,
                total,
                format!("{} of {}", format_bytes(received), format_bytes(total)),
            );
            last_emit = Instant::now();
        }
    }

    handle
        .flush()
        .map_err(|error| AppError::Download(error.to_string()))?;

    if received == 0 {
        return Err(AppError::Download(
            "GameBanana returned an empty file. Try another file or try again later.".to_string(),
        ));
    }

    Ok(())
}

async fn install_download(
    app: &AppHandle,
    mods_dir: &Path,
    archive_path: &Path,
    detail: &ModDetail,
    file: &ModFile,
    replace_folder: Option<String>,
    cancel: Arc<AtomicBool>,
) -> AppResult<DownloadOutcome> {
    if cancel.load(Ordering::SeqCst) {
        return Err(AppError::Cancelled);
    }

    emit(
        app,
        detail,
        "installing",
        96,
        0,
        0,
        "Extracting and installing…".to_string(),
    );

    let metadata = metadata::ModMetadata {
        source: Some("gamebanana".to_string()),
        game_banana_mod_id: Some(detail.id),
        game_banana_file_id: Some(file.id),
        name: Some(detail.name.clone()),
        character: detail.sub_category.clone(),
        version: file.version.clone().or_else(|| detail.version.clone()),
        author: detail.author.clone(),
        installed_at: Some(iso_utc_now()),
        thumbnail_url: detail.thumbnail_url.clone(),
        notes: None,
    };

    let app_handle = app.clone();
    let mods_dir_owned = mods_dir.to_path_buf();
    let archive_owned = archive_path.to_path_buf();
    let mod_name = detail.name.clone();
    let mod_id = detail.id;
    let file_id = file.id;
    let file_name = file.name.clone();
    let detail_for_progress = detail.clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || -> AppResult<DownloadOutcome> {
        if cancel.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled);
        }

        let mut emit_install = |progress: InstallProgress| {
            emit(
                &app_handle,
                &detail_for_progress,
                "installing",
                // 96..99 while the installer works; the final 100 is emitted by
                // the caller once metadata is written.
                (96 + (progress.percent as u16 * 3 / 100) as u8).min(99),
                0,
                0,
                progress.message,
            );
        };

        let result = install::install_archive_single(
            &mods_dir_owned,
            &archive_owned,
            &detail_for_progress.name,
            cancel.as_ref(),
            &mut emit_install,
        );

        if result.cancelled {
            return Err(AppError::Cancelled);
        }

        if let Some(error) = result.error {
            return Err(AppError::InstallFailed(error));
        }

        for folder in &result.installed {
            let folder_path = mods_dir_owned.join(folder);
            if let Err(error) = metadata::write(&folder_path, &metadata) {
                log::warn!("Could not write .zzmm.json for {folder}: {error}");
            }
        }

        // Disable the previous version so two copies are never loaded at once.
        let replaced = match replace_folder {
            Some(folder) if !result.installed.iter().any(|name| name == &folder) => {
                match actions::set_enabled(&mods_dir_owned, &folder, false) {
                    Ok(()) => Some(folder),
                    Err(error) => {
                        log::warn!("Could not disable the previous version {folder}: {error}");
                        None
                    }
                }
            }
            _ => None,
        };

        Ok(DownloadOutcome {
            mod_id,
            mod_name,
            file_id,
            file_name,
            installed: result.installed,
            warnings: result.warnings,
            replaced_folder: replaced,
        })
    })
    .await
    .map_err(|error| AppError::Task(error.to_string()))??;

    Ok(outcome)
}

fn percent_of(received: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((received.min(total) * 100) / total).min(100) as u8
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn emit(
    app: &AppHandle,
    detail: &ModDetail,
    stage: &str,
    percent: u8,
    received: u64,
    total: u64,
    message: String,
) {
    let payload = DownloadProgress {
        mod_id: detail.id,
        name: detail.name.clone(),
        stage: stage.to_string(),
        percent,
        received,
        total,
        message,
    };

    if let Err(error) = app.emit(DOWNLOAD_EVENT, payload) {
        log::warn!("Could not emit download progress: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_archive_downloads() {
        assert!(safe_archive_name("pulchra.zip").is_ok());
        assert!(safe_archive_name("pack.7z").is_ok());
        assert!(safe_archive_name("setup.exe").is_err());
        assert!(safe_archive_name("no-extension").is_err());
    }

    #[test]
    fn strips_directories_from_download_names() {
        assert_eq!(
            safe_archive_name("../../evil/mod.zip").expect("valid"),
            "mod.zip"
        );
    }

    #[test]
    fn formats_progress_percentages() {
        assert_eq!(percent_of(0, 0), 0);
        assert_eq!(percent_of(50, 100), 50);
        assert_eq!(percent_of(200, 100), 100);
    }

    #[test]
    fn deduplicates_download_paths() {
        let dir = std::env::temp_dir().join(format!("zzmm-download-test-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        File::create(dir.join("mod.zip")).expect("seed file");

        assert_eq!(unique_path(&dir, "mod.zip"), dir.join("mod (2).zip"));

        fs::remove_dir_all(&dir).ok();
    }
}
