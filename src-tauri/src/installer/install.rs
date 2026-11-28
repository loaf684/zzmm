use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};
use crate::installer::extract;
use crate::installer::model::{ArchiveInstallOutcome, InstallProgress};
use crate::installer::root;
use crate::mods::scanner::{self, DISABLED_PREFIX};

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Install every mod found in one archive. A folder lands enabled, unless
/// another mod for the same confidently identified character is already
/// installed — then it gets the ZZMI `DISABLED_` prefix so two outfits for one
/// character are never active at once. Failures are reported in the outcome
/// instead of aborting a multi-archive drop.
pub fn install_archive(
    mods_dir: &Path,
    archive_path: &Path,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(InstallProgress),
) -> ArchiveInstallOutcome {
    let archive_name = archive_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| archive_path.to_string_lossy().to_string());

    let mut outcome = ArchiveInstallOutcome {
        archive: archive_name.clone(),
        installed: Vec::new(),
        warnings: Vec::new(),
        error: None,
        cancelled: false,
    };

    let result = install_inner(
        mods_dir,
        archive_path,
        &archive_name,
        None,
        None,
        cancel,
        &mut outcome,
        emit,
    );

    let cancelled = matches!(&result, Err(AppError::Cancelled));
    let failed = result.is_err() && !cancelled;
    if let Err(error) = &result {
        if !cancelled {
            outcome.error = Some(error.to_string());
        }
    }
    outcome.cancelled = cancelled;

    emit(InstallProgress {
        archive: archive_name,
        stage: if cancelled {
            "cancelled"
        } else if failed {
            "failed"
        } else {
            "done"
        }
        .to_string(),
        percent: 100,
        message: if cancelled {
            "Installation cancelled".to_string()
        } else if failed {
            "Installation failed".to_string()
        } else {
            format!("Installed {} mod folder(s)", outcome.installed.len())
        },
    });

    outcome
}

/// Install one downloaded mod as a single folder, even when the archive splits
/// its INIs across sibling folders (`body/`, `face/`, `hair/`). GameBanana
/// files are one mod, so flattening every INI root into the Mods folder would
/// show the same download as several installed entries. Like the drag-and-drop
/// installer, the folder lands enabled unless another mod for the same
/// confidently identified character is already installed. `character_hint` is a
/// character the download source already knows (GameBanana's character
/// subcategory) before any `.zzmm.json` has been written.
pub fn install_archive_single(
    mods_dir: &Path,
    archive_path: &Path,
    preferred_name: &str,
    character_hint: Option<&str>,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(InstallProgress),
) -> ArchiveInstallOutcome {
    let archive_name = archive_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| archive_path.to_string_lossy().to_string());

    let mut outcome = ArchiveInstallOutcome {
        archive: archive_name.clone(),
        installed: Vec::new(),
        warnings: Vec::new(),
        error: None,
        cancelled: false,
    };

    let result = install_inner(
        mods_dir,
        archive_path,
        &archive_name,
        Some(preferred_name),
        character_hint,
        cancel,
        &mut outcome,
        emit,
    );

    let cancelled = matches!(&result, Err(AppError::Cancelled));
    let failed = result.is_err() && !cancelled;
    if let Err(error) = &result {
        if !cancelled {
            outcome.error = Some(error.to_string());
        }
    }
    outcome.cancelled = cancelled;

    emit(InstallProgress {
        archive: archive_name,
        stage: if cancelled {
            "cancelled"
        } else if failed {
            "failed"
        } else {
            "done"
        }
        .to_string(),
        percent: 100,
        message: if cancelled {
            "Installation cancelled".to_string()
        } else if failed {
            "Installation failed".to_string()
        } else {
            format!("Installed {} mod folder(s)", outcome.installed.len())
        },
    });

    outcome
}

fn install_inner(
    mods_dir: &Path,
    archive_path: &Path,
    archive_name: &str,
    single: Option<&str>,
    character_hint: Option<&str>,
    cancel: &AtomicBool,
    outcome: &mut ArchiveInstallOutcome,
    emit: &mut dyn FnMut(InstallProgress),
) -> AppResult<()> {
    if !mods_dir.is_dir() {
        return Err(AppError::NoModsDir);
    }

    ensure_not_cancelled(cancel)?;
    let kind = extract::detect_kind(archive_path)?;
    ensure_not_cancelled(cancel)?;

    // Stage inside the Mods folder: moves stay on one volume and the dot-prefixed
    // folder is ignored by the scanner. The guard removes it on every exit path.
    let staging = create_staging(mods_dir)?;
    let _guard = StagingGuard(staging.clone());
    let content = staging.join("content");
    fs::create_dir_all(&content)?;

    emit(InstallProgress {
        archive: archive_name.to_string(),
        stage: "extracting".to_string(),
        percent: 5,
        message: format!("Extracting {} archive…", kind.label()),
    });

    extract::extract_archive(
        archive_path,
        &content,
        kind,
        cancel,
        &mut |percent, message| {
            emit(InstallProgress {
                archive: archive_name.to_string(),
                stage: "extracting".to_string(),
                percent,
                message,
            });
        },
    )?;

    ensure_not_cancelled(cancel)?;
    root::clean_junk(&content);

    emit(InstallProgress {
        archive: archive_name.to_string(),
        stage: "detecting".to_string(),
        percent: 65,
        message: "Detecting mod folders…".to_string(),
    });

    let roots = root::find_mod_roots(&content);

    if roots.is_empty() {
        return Err(AppError::ArchiveExtract(format!(
            "No .ini file was found in {archive_name}. This does not look like a ZZMI mod archive."
        )));
    }

    // Characters already installed decide which new folders land disabled. The
    // set grows as this install places folders, so a pack holding several mods
    // for one character keeps only the first enabled.
    let mut installed = installed_characters(mods_dir);

    if let Some(preferred_name) = single {
        let preferred = if preferred_name.trim().is_empty() {
            archive_path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            preferred_name.to_string()
        };
        let base = sanitize_folder_name(&preferred);
        let character = scanner::staged_character(&base, &roots, character_hint);
        let disabled = character.map_or(false, |character| installed.contains(character));
        let destination = unique_destination(mods_dir, &base, disabled);
        let installed_name = destination
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or(base);

        emit(InstallProgress {
            archive: archive_name.to_string(),
            stage: "installing".to_string(),
            percent: 70,
            message: format!("Installing {installed_name}…"),
        });

        ensure_not_cancelled(cancel)?;
        let moved = if roots.len() == 1 {
            move_folder(&roots[0], &destination, cancel)
        } else {
            place_roots(&roots, &destination, cancel)
        };

        if let Err(error) = moved {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }

        log::info!("Installed {} as {}", archive_name, installed_name);
        outcome.installed.push(installed_name);

        emit(InstallProgress {
            archive: archive_name.to_string(),
            stage: "installing".to_string(),
            percent: 96,
            message: "Finishing up…".to_string(),
        });

        return Ok(());
    }

    let total = roots.len().max(1);

    for (index, source) in roots.iter().enumerate() {
        if let Err(error) = ensure_not_cancelled(cancel) {
            discard_installed(mods_dir, outcome);
            return Err(error);
        }

        let preferred = if source == &content {
            archive_path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            source
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default()
        };

        let base = sanitize_folder_name(&preferred);
        let character = scanner::staged_character(&base, std::slice::from_ref(source), None);
        let disabled = character.map_or(false, |character| installed.contains(character));
        let destination = unique_destination(mods_dir, &base, disabled);
        let installed_name = destination
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| base.clone());

        let percent = 70 + (((index as u32 * 25) / total as u32) as u8);

        emit(InstallProgress {
            archive: archive_name.to_string(),
            stage: "installing".to_string(),
            percent,
            message: format!("Installing {installed_name}..."),
        });

        match move_folder(source, &destination, cancel) {
            Ok(()) => {
                if let Some(character) = character {
                    installed.insert(character.to_string());
                }
                log::info!("Installed {} as {}", archive_name, installed_name);
                outcome.installed.push(installed_name);
            }
            Err(AppError::Cancelled) => {
                discard_installed(mods_dir, outcome);
                return Err(AppError::Cancelled);
            }
            Err(error) => {
                log::warn!("Could not install {preferred} from {archive_name}: {error}");
                outcome
                    .warnings
                    .push(format!("Could not install \"{preferred}\": {error}"));
            }
        }
    }

    if outcome.installed.is_empty() {
        return Err(AppError::InstallFailed(
            "No mod folder could be moved into the Mods directory.".to_string(),
        ));
    }

    emit(InstallProgress {
        archive: archive_name.to_string(),
        stage: "installing".to_string(),
        percent: 96,
        message: "Finishing up…".to_string(),
    });

    Ok(())
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

/// A cancelled multi-mod archive must not leave the folders it already moved
/// behind. These entries were created by this install, never pre-existing, so
/// removing them restores the Mods folder to its previous state.
fn discard_installed(mods_dir: &Path, outcome: &mut ArchiveInstallOutcome) {
    for name in outcome.installed.drain(..) {
        let path = mods_dir.join(&name);
        match fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "Could not remove cancelled install {}: {error}",
                path.display()
            ),
        }
    }
}

fn create_staging(mods_dir: &Path) -> AppResult<PathBuf> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for attempt in 0..20 {
        let path = mods_dir.join(format!(
            ".zzmm-install-{}-{unique}-{attempt}",
            std::process::id()
        ));

        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err(AppError::InstallFailed(
        "Could not create a staging folder in the Mods directory.".to_string(),
    ))
}

struct StagingGuard(PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "Could not clean installer staging {}: {error}",
                self.0.display()
            ),
        }
    }
}

/// Windows-safe folder name for the installed mod.
fn sanitize_folder_name(raw: &str) -> String {
    let replaced: String = raw
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect();

    let cleaned = replaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|character| character == '.' || character == ' ')
        .to_string();

    let mut limited: String = cleaned.chars().take(80).collect();
    limited = limited
        .trim_end_matches(|character| character == '.' || character == ' ')
        .to_string();

    if limited.is_empty() {
        return "Installed Mod".to_string();
    }

    let stem = limited
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();

    if RESERVED_NAMES.contains(&stem.as_str()) {
        return format!("_{limited}");
    }

    limited
}

/// Confident characters already present in the Mods folder, enabled or not. A
/// new mod for one of them lands disabled so two outfits for the same character
/// are never active at once. A mod whose character cannot be pinned to a known
/// ZZZ character never collides (see `characters::confident_character`).
fn installed_characters(mods_dir: &Path) -> HashSet<String> {
    let mut characters = HashSet::new();

    let Ok(entries) = fs::read_dir(mods_dir) else {
        return characters;
    };

    for entry in entries.flatten() {
        let folder_name = entry.file_name().to_string_lossy().to_string();
        if folder_name.starts_with('.') {
            continue;
        }

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let (_, base_name) = scanner::split_disabled(&folder_name);
        if base_name.trim().is_empty() {
            continue;
        }

        if let Some(character) = scanner::installed_character(&entry.path(), &base_name) {
            characters.insert(character.to_string());
        }
    }

    characters
}

/// Unique destination for an installed mod. `disabled` mirrors the caller's
/// collision decision: a colliding mod keeps the ZZMI `DISABLED_` prefix, every
/// other install lands enabled.
///
/// A name is skipped while its twin is on disk too: a folder and its
/// `DISABLED_` counterpart cannot be renamed into each other, so installing
/// `Mod` beside an existing `DISABLED_Mod` would leave that mod impossible to
/// enable later (the toggle would find the target name taken). This is what
/// keeps the GameBanana update path able to disable the replaced version.
fn unique_destination(mods_dir: &Path, preferred: &str, disabled: bool) -> PathBuf {
    let base = sanitize_folder_name(preferred);
    let free = |name: &str| !mods_dir.join(name).exists();
    let taken = |name: &str| !free(name) || !free(&format!("{DISABLED_PREFIX}{name}"));
    let leaf = |name: &str| {
        if disabled {
            format!("{DISABLED_PREFIX}{name}")
        } else {
            name.to_string()
        }
    };

    if !taken(&base) {
        return mods_dir.join(leaf(&base));
    }

    for index in 2..=999u32 {
        let suffixed = format!("{base} ({index})");
        if !taken(&suffixed) {
            return mods_dir.join(leaf(&suffixed));
        }
    }

    let fallback = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    mods_dir.join(leaf(&format!("{base} ({fallback})")))
}

/// Put every detected root under one mod folder, keeping each root's name.
fn place_roots(roots: &[PathBuf], destination: &Path, cancel: &AtomicBool) -> AppResult<()> {
    fs::create_dir_all(destination)?;

    for source in roots {
        ensure_not_cancelled(cancel)?;
        let leaf = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if leaf.is_empty() {
            continue;
        }

        let mut target = destination.join(&leaf);
        let mut index = 2u32;
        while target.exists() {
            target = destination.join(format!("{leaf} ({index})"));
            index += 1;
        }
        move_folder(source, &target, cancel)?;
    }

    Ok(())
}

fn move_folder(source: &Path, destination: &Path, cancel: &AtomicBool) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;

    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-volume / transient lock fallback. Same-volume moves normally
            // take the rename path above.
            if let Err(error) = copy_dir(source, destination, cancel) {
                let _ = fs::remove_dir_all(destination);
                return Err(error);
            }
            fs::remove_dir_all(source)?;
            Ok(())
        }
    }
}

fn copy_dir(source: &Path, destination: &Path, cancel: &AtomicBool) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        ensure_not_cancelled(cancel)?;
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            copy_dir(&entry.path(), &target, cancel)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }

    ensure_not_cancelled(cancel)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Build a zip with one INI entry per path, so a test only states the
    /// folder layout it cares about.
    fn write_archive(path: &Path, entries: &[&str]) {
        let file = fs::File::create(path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for entry in entries {
            writer.start_file(*entry, options).expect("zip entry");
            writer
                .write_all(b"[TextureOverride]\nhash = aaaa\n")
                .expect("write");
        }

        writer.finish().expect("finish zip");
    }

    #[test]
    fn sanitizes_windows_names_and_deduplicates() {
        assert_eq!(sanitize_folder_name("Ellen: Joe?"), "Ellen Joe");
        assert_eq!(sanitize_folder_name("CON"), "_CON");
        assert_eq!(sanitize_folder_name("  ..  "), "Installed Mod");

        let mods_dir = temp_dir("zzmm-name-test");

        // A plain install keeps its name; a colliding one carries the prefix.
        assert_eq!(
            unique_destination(&mods_dir, "ModA", false),
            mods_dir.join("ModA")
        );
        assert_eq!(
            unique_destination(&mods_dir, "ModC", true),
            mods_dir.join("DISABLED_ModC")
        );

        // A pre-existing disabled twin forces a suffix: the pair cannot be
        // renamed into each other, so the enabled name is already taken.
        fs::create_dir_all(mods_dir.join("DISABLED_ModA")).expect("existing disabled mod");
        assert_eq!(
            unique_destination(&mods_dir, "ModA", false),
            mods_dir.join("ModA (2)")
        );
        assert_eq!(
            unique_destination(&mods_dir, "ModA", true),
            mods_dir.join("DISABLED_ModA (2)")
        );

        // An enabled twin is skipped too: taking its disabled name would make it
        // impossible to disable later (the rename target would be taken).
        fs::create_dir_all(mods_dir.join("ModB")).expect("existing enabled mod");
        assert_eq!(
            unique_destination(&mods_dir, "ModB", false),
            mods_dir.join("ModB (2)")
        );
        assert_eq!(
            unique_destination(&mods_dir, "ModB", true),
            mods_dir.join("DISABLED_ModB (2)")
        );

        fs::remove_dir_all(&mods_dir).ok();
    }

    #[test]
    fn a_download_with_split_ini_folders_becomes_one_mod_folder() {
        let root = temp_dir("zzmm-install-single-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        let archive_path = root.join("Seed.zip");
        {
            let file = fs::File::create(&archive_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            for entry in ["body/body.ini", "face/face.ini", "hair/hair.ini"] {
                writer.start_file(entry, options).expect("zip entry");
                writer.write_all(b"[TextureOverrideX]\n").expect("write");
            }
            writer.finish().expect("finish zip");
        }

        let outcome = install_archive_single(
            &mods_dir,
            &archive_path,
            "Seed Thicc",
            None,
            &AtomicBool::new(false),
            &mut |_| {},
        );

        assert_eq!(outcome.error, None);
        // A GameBanana download lands enabled, like a dropped archive.
        assert_eq!(outcome.installed, vec!["Seed Thicc".to_string()]);
        assert!(mods_dir.join("Seed Thicc/body/body.ini").is_file());
        assert!(mods_dir.join("Seed Thicc/face/face.ini").is_file());
        assert!(mods_dir.join("Seed Thicc/hair/hair.ini").is_file());
        assert!(!mods_dir.join("DISABLED_Seed Thicc").exists());
        assert!(!mods_dir.join("body").exists());
        assert!(!mods_dir.join("face").exists());
        assert!(!mods_dir.join("hair").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cancellation_before_extraction_leaves_no_mod_folder_or_staging() {
        let root = temp_dir("zzmm-install-cancel-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        let archive_path = root.join("Cancel Me.zip");
        {
            let file = fs::File::create(&archive_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("mod.ini", options).expect("zip entry");
            writer.write_all(b"[TextureOverrideX]\n").expect("write");
            writer.finish().expect("finish zip");
        }

        let cancel = AtomicBool::new(true);
        let outcome = install_archive(&mods_dir, &archive_path, &cancel, &mut |_| {});

        assert!(outcome.cancelled);
        assert!(outcome.error.is_none());
        assert!(outcome.installed.is_empty());
        assert!(!mods_dir.join("Cancel Me").exists());

        let staging_leftovers = fs::read_dir(&mods_dir)
            .expect("read mods dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".zzmm-install-")
            })
            .count();
        assert_eq!(staging_leftovers, 0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cancellation_during_extraction_cleans_staging() {
        let root = temp_dir("zzmm-install-cancel-extract-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        let archive_path = root.join("Cancel During Extract.zip");
        {
            let file = fs::File::create(&archive_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for entry in ["one.ini", "two.ini"] {
                writer.start_file(entry, options).expect("zip entry");
                writer.write_all(b"[TextureOverrideX]\n").expect("write");
            }
            writer.finish().expect("finish zip");
        }

        let cancel = AtomicBool::new(false);
        let outcome = install_archive(&mods_dir, &archive_path, &cancel, &mut |progress| {
            // Let the first entry finish, then stop before the second one.
            if progress.stage == "extracting" && progress.percent > 5 {
                cancel.store(true, Ordering::SeqCst);
            }
        });

        assert!(outcome.cancelled);
        assert!(outcome.error.is_none());
        assert!(outcome.installed.is_empty());
        assert!(!mods_dir.join("Cancel During Extract").exists());

        let staging_leftovers = fs::read_dir(&mods_dir)
            .expect("read mods dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".zzmm-install-")
            })
            .count();
        assert_eq!(staging_leftovers, 0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn installs_every_mod_from_a_multi_mod_zip_and_avoids_collisions() {
        let root = temp_dir("zzmm-install-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        let archive_path = root.join("Pack.zip");
        write_archive(&archive_path, &["Pack/ModA/mod.ini", "Pack/ModB/mod.ini"]);

        let mut stages = Vec::new();

        let first = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |progress| {
                stages.push(progress.stage);
            },
        );

        assert_eq!(first.error, None);
        assert_eq!(first.installed.len(), 2);
        // Neither folder names a known character, so both land enabled.
        assert!(mods_dir.join("ModA/mod.ini").is_file());
        assert!(mods_dir.join("ModB/mod.ini").is_file());
        assert!(!mods_dir.join("DISABLED_ModA").exists());
        assert!(!mods_dir.join("DISABLED_ModB").exists());
        assert!(stages.iter().any(|stage| stage == "done"));

        let second = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(second.error, None);
        assert!(second.installed.contains(&"ModA (2)".to_string()));
        assert!(second.installed.contains(&"ModB (2)".to_string()));

        // Staging is always cleaned up.
        let staging_leftovers = fs::read_dir(&mods_dir)
            .expect("read mods dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".zzmm-install-")
            })
            .count();
        assert_eq!(staging_leftovers, 0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn uses_archive_name_when_the_ini_is_at_the_root() {
        let root = temp_dir("zzmm-root-install-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");
        let archive_path = root.join("My Ellen Skin.zip");

        {
            let file = fs::File::create(&archive_path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("mod.ini", options).expect("zip entry");
            writer
                .write_all(b"[TextureOverride]\nhash = cccc\n")
                .expect("write");
            writer
                .start_file("textures/body.dds", options)
                .expect("zip entry");
            writer.write_all(b"dds").expect("write");
            writer.finish().expect("finish zip");
        }

        let outcome = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["My Ellen Skin".to_string()]);
        assert!(mods_dir.join("My Ellen Skin/mod.ini").is_file());

        fs::remove_dir_all(&root).ok();
    }

    /// The first mod for a character has nothing to collide with, so it lands
    /// enabled even when its folder names a known character.
    #[test]
    fn a_first_mod_for_a_character_lands_enabled() {
        let root = temp_dir("zzmm-install-first-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        let archive_path = root.join("EllenMod.zip");
        write_archive(&archive_path, &["mod.ini"]);

        let outcome = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |_| {},
        );

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["EllenMod".to_string()]);
        assert!(mods_dir.join("EllenMod/mod.ini").is_file());

        fs::remove_dir_all(&root).ok();
    }

    /// Same character as an installed mod: the new folder lands disabled and the
    /// pre-existing mod keeps both its name and its enabled state.
    #[test]
    fn a_second_mod_for_the_same_character_lands_disabled() {
        let root = temp_dir("zzmm-install-collision-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        fs::create_dir_all(mods_dir.join("Ellen Joe Streetwear")).expect("existing mod");
        fs::write(
            mods_dir.join("Ellen Joe Streetwear/mod.ini"),
            b"[TextureOverride]\n",
        )
        .expect("ini");

        let archive_path = root.join("EllenMod.zip");
        write_archive(&archive_path, &["mod.ini"]);

        let outcome = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |_| {},
        );

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["DISABLED_EllenMod".to_string()]);
        assert!(mods_dir.join("DISABLED_EllenMod/mod.ini").is_file());
        // Nothing pre-existing was renamed.
        assert!(mods_dir.join("Ellen Joe Streetwear/mod.ini").is_file());
        assert!(!mods_dir.join("DISABLED_Ellen Joe Streetwear").exists());

        fs::remove_dir_all(&root).ok();
    }

    /// A mod whose only signal resolves to a different character never collides.
    #[test]
    fn mods_for_different_characters_stay_enabled() {
        let root = temp_dir("zzmm-install-distinct-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        fs::create_dir_all(mods_dir.join("Ellen Joe Streetwear")).expect("existing mod");
        fs::write(
            mods_dir.join("Ellen Joe Streetwear/mod.ini"),
            b"[TextureOverride]\n",
        )
        .expect("ini");

        let archive_path = root.join("NicoleMod.zip");
        write_archive(&archive_path, &["mod.ini"]);

        let outcome = install_archive(
            &mods_dir,
            &archive_path,
            &AtomicBool::new(false),
            &mut |_| {},
        );

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["NicoleMod".to_string()]);
        assert!(mods_dir.join("NicoleMod/mod.ini").is_file());

        fs::remove_dir_all(&root).ok();
    }

    /// A `[Tag]` prefix that is not a ZZZ character, and a name with no
    /// character at all, must never look like a collision.
    #[test]
    fn ambiguous_or_uncategorized_mods_do_not_collide() {
        let root = temp_dir("zzmm-install-ambiguous-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        fs::create_dir_all(mods_dir.join("[4K] Outfit A")).expect("existing mod");
        fs::write(
            mods_dir.join("[4K] Outfit A/mod.ini"),
            b"[TextureOverride]\n",
        )
        .expect("ini");

        let tagged = root.join("[4K] Outfit B.zip");
        write_archive(&tagged, &["mod.ini"]);
        let outcome = install_archive(&mods_dir, &tagged, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["[4K] Outfit B".to_string()]);

        let plain = root.join("Some Random Outfit.zip");
        write_archive(&plain, &["mod.ini"]);
        let outcome = install_archive(&mods_dir, &plain, &AtomicBool::new(false), &mut |_| {});
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["Some Random Outfit".to_string()]);

        fs::remove_dir_all(&root).ok();
    }

    /// The GameBanana single-folder path applies the same rule, using the
    /// character category the download source already knows.
    #[test]
    fn a_download_for_an_installed_character_lands_disabled() {
        let root = temp_dir("zzmm-install-single-collision-test");
        let mods_dir = root.join("Mods");
        fs::create_dir_all(&mods_dir).expect("mods dir");

        fs::create_dir_all(mods_dir.join("Ellen Joe Streetwear")).expect("existing mod");
        fs::write(
            mods_dir.join("Ellen Joe Streetwear/mod.ini"),
            b"[TextureOverride]\n",
        )
        .expect("ini");

        let archive_path = root.join("Summer Dress.zip");
        write_archive(&archive_path, &["mod.ini"]);

        let outcome = install_archive_single(
            &mods_dir,
            &archive_path,
            "Summer Dress",
            Some("Ellen Joe"),
            &AtomicBool::new(false),
            &mut |_| {},
        );

        assert_eq!(outcome.error, None);
        assert_eq!(outcome.installed, vec!["DISABLED_Summer Dress".to_string()]);
        assert!(mods_dir.join("DISABLED_Summer Dress/mod.ini").is_file());
        assert!(mods_dir.join("Ellen Joe Streetwear/mod.ini").is_file());

        fs::remove_dir_all(&root).ok();
    }
}
