use std::fs::{self, File, OpenOptions};
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
///
/// Downloads are independent per call; `install_lock` is only taken for the
/// extract/move phase, which is the part that shares the Mods directory.
pub async fn install_mod(
    app: &AppHandle,
    config_dir: &Path,
    mods_dir: &Path,
    mod_id: u64,
    file_id: Option<u64>,
    replace_folder: Option<String>,
    cancel: Arc<AtomicBool>,
    install_lock: Arc<tokio::sync::Mutex<()>>,
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

    // Reserve the archive path atomically: concurrent downloads may pick the
    // same friendly name, and only one of them may own that file.
    let (archive_path, archive) = create_archive(&downloads_dir, &file_name)?;

    emit(
        app,
        &detail,
        "downloading",
        0,
        0,
        0,
        format!("Downloading {}", file.name),
    );

    if let Err(error) = download_file(app, &detail, &file, archive, cancel.as_ref()).await {
        remove_archive(&archive_path);
        return Err(error);
    }

    // Extracting and moving folders must not overlap another install (local or
    // GameBanana): both decide destinations and per-character enable state from
    // the same Mods folder. The download above stayed parallel.
    let install_guard = match install_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            emit(
                app,
                &detail,
                "installing",
                95,
                0,
                0,
                "Waiting for another install to finish...".to_string(),
            );
            install_lock.lock().await
        }
    };

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

    drop(install_guard);

    // The archive is only needed for the install; keep the disk tidy on success,
    // failure, and cancellation.
    remove_archive(&archive_path);

    installed
}

/// Map GameBanana's root category to the app's grouping label.
///
/// `Character Skins` keeps the leaf category (the character); `UI` and
/// `Bangboo Skins` are the two non-character targets and use their root names
/// exactly as the Browse category filter displays them. Any other root returns
/// `None` so the scanner's own name-based guess decides.
fn category_character(super_category: Option<&str>, leaf: Option<&str>) -> Option<String> {
    let root = super_category?.trim();

    if root.eq_ignore_ascii_case("ui") {
        return Some("UI".to_string());
    }

    if root.to_ascii_lowercase().contains("bangboo") {
        return Some("Bangboo Skins".to_string());
    }

    if root.eq_ignore_ascii_case("character skins") {
        return leaf
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }

    None
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

/// Reserve a fresh archive path and file handle. `create_new` closes the race
/// that a plain `exists()` check leaves between two parallel downloads.
fn create_archive(dir: &Path, name: &str) -> AppResult<(PathBuf, File)> {
    for _ in 0..64 {
        let candidate = unique_path(dir, name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(AppError::Download(format!(
                    "Could not create the download file ({}): {error}",
                    candidate.display()
                )))
            }
        }
    }

    Err(AppError::Download(
        "Could not reserve a download file name. Try again.".to_string(),
    ))
}

async fn download_file(
    app: &AppHandle,
    detail: &ModDetail,
    file: &ModFile,
    mut handle: File,
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

    // GameBanana's root category is the safest automatic classification: UI
    // and Bangboo submissions have no character of their own, while a character
    // skin's leaf category is the roster name. Unknown roots stay `None` so the
    // scanner's name-based guess is not overridden by an invented category.
    let character =
        category_character(detail.super_category.as_deref(), detail.category.as_deref())
            .or_else(|| detail.sub_category.clone());

    let metadata = metadata::ModMetadata {
        source: Some("gamebanana".to_string()),
        game_banana_mod_id: Some(detail.id),
        game_banana_file_id: Some(file.id),
        name: Some(detail.name.clone()),
        character: character.clone(),
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
    let character_hint = character.clone();

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
            character_hint.as_deref(),
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
        let replaced = disable_replaced(
            &mods_dir_owned,
            &result.installed,
            replace_folder.as_deref(),
        );

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

/// Disable the version a download replaces now that the new folder is on disk.
///
/// Returns the folder that was disabled. The new install is never touched: a
/// `replace_folder` naming it is ignored, and a failed rename is logged and
/// reported as nothing replaced rather than failing the whole download.
fn disable_replaced(
    mods_dir: &Path,
    installed: &[String],
    replace_folder: Option<&str>,
) -> Option<String> {
    let folder = replace_folder?;

    if installed.iter().any(|name| name == folder) {
        return None;
    }

    match actions::set_enabled(mods_dir, folder, false) {
        Ok(()) => Some(folder.to_string()),
        Err(error) => {
            log::warn!("Could not disable the previous version {folder}: {error}");
            None
        }
    }
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
    fn replacing_an_enabled_mod_disables_it_beside_the_new_copy() {
        let root = std::env::temp_dir().join(format!("zzmm-replace-test-{}", std::process::id()));
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        // The user's current version is enabled, so it has no `DISABLED_` prefix.
        fs::create_dir_all(mods_dir.join("My Skin")).expect("existing mod");
        fs::write(mods_dir.join("My Skin/mod.ini"), b"[TextureOverride]\n").expect("ini");

        let archive_path = root.join("My Skin.zip");
        {
            let file = File::create(&archive_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("mod.ini", options).expect("zip entry");
            writer
                .write_all(b"[TextureOverride]\nhash = bbbb\n")
                .expect("write");
            writer.finish().expect("finish zip");
        }

        let outcome = install::install_archive_single(
            &mods_dir,
            &archive_path,
            "My Skin",
            None,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(outcome.error, None);

        // "My Skin" names no character, so the copy lands enabled beside the old
        // name; the replaced version is the one disabled afterwards.
        assert_eq!(outcome.installed, vec!["My Skin (2)".to_string()]);

        let replaced = disable_replaced(&mods_dir, &outcome.installed, Some("My Skin"));
        assert_eq!(replaced, Some("My Skin".to_string()));

        assert!(mods_dir.join("My Skin (2)/mod.ini").is_file());
        assert!(mods_dir.join("DISABLED_My Skin/mod.ini").is_file());
        assert!(!mods_dir.join("My Skin").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn classifies_gamebanana_root_categories() {
        assert_eq!(
            category_character(Some("UI"), Some("Character UI")).as_deref(),
            Some("UI")
        );
        assert_eq!(
            category_character(Some("Bangboo Skins"), Some("Eous")).as_deref(),
            Some("Bangboo Skins")
        );
        assert_eq!(
            category_character(Some("Character Skins"), Some("Ellen Joe")).as_deref(),
            Some("Ellen Joe")
        );
        // An unfamiliar root or a character skin without a leaf name stays
        // unknown instead of inventing a category.
        assert_eq!(category_character(Some("Other/Misc"), Some("Tools")), None);
        assert_eq!(category_character(None, Some("Ellen Joe")), None);
        assert_eq!(category_character(Some("Character Skins"), None), None);
    }

    #[test]
    fn reserves_distinct_archives_for_parallel_downloads() {
        let dir =
            std::env::temp_dir().join(format!("zzmm-parallel-download-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");

        let (first, _first_file) = create_archive(&dir, "mod.zip").expect("first");
        let (second, _second_file) = create_archive(&dir, "mod.zip").expect("second");

        assert_ne!(first, second);
        assert!(first.is_file());
        assert!(second.is_file());

        fs::remove_dir_all(&dir).ok();
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
